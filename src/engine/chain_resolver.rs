use crate::adapters::apps::{
    self, emacs, librefox::Librefox, nvim::Nvim, vscode::Vscode, wezterm, AppAdapter, AppKind,
};
use crate::adapters::terminal_multiplexers::tmux::Tmux;
use crate::config::AppSection;
use crate::engine::contract::ChainResolver;
use crate::engine::domain::{EDITOR_DOMAIN_ID, TERMINAL_DOMAIN_ID, WM_DOMAIN_ID};
use crate::engine::runtime::{self, ProcessId};
use crate::engine::topology::DomainId;
use crate::logging;

pub struct RuntimeChainResolver;

static RUNTIME_CHAIN_RESOLVER: RuntimeChainResolver = RuntimeChainResolver;

pub fn runtime_chain_resolver() -> &'static RuntimeChainResolver {
    &RUNTIME_CHAIN_RESOLVER
}

struct DirectAdapterSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    app_ids: &'static [&'static str],
    section: AppSection,
    build: fn() -> Box<dyn AppAdapter>,
}

fn build_editor() -> Box<dyn AppAdapter> {
    apps::bind_policy(Box::new(emacs::EmacsBackend))
}

fn build_librefox() -> Box<dyn AppAdapter> {
    apps::bind_policy(Box::new(Librefox))
}

fn build_vscode() -> Box<dyn AppAdapter> {
    apps::bind_policy(Box::new(Vscode))
}

const DIRECT_ADAPTERS: &[DirectAdapterSpec] = &[
    DirectAdapterSpec {
        name: emacs::ADAPTER_NAME,
        aliases: emacs::ADAPTER_ALIASES,
        app_ids: emacs::APP_IDS,
        section: AppSection::Editor,
        build: build_editor,
    },
    DirectAdapterSpec {
        name: "librefox",
        aliases: &["librefox"],
        app_ids: &["librewolf", "LibreWolf", "firefox", "Firefox"],
        section: AppSection::Browser,
        build: build_librefox,
    },
    DirectAdapterSpec {
        name: "vscode",
        aliases: &["vscode"],
        app_ids: &["code", "code-url-handler", "Code", "code-oss"],
        section: AppSection::Editor,
        build: build_vscode,
    },
];

fn preferred_adapter_name() -> Option<String> {
    crate::config::app_adapter_override().and_then(|raw| {
        let normalized = raw.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    })
}

fn matches_adapter_alias(preferred: &str, aliases: &[&str]) -> bool {
    aliases.iter().any(|candidate| *candidate == preferred)
}

fn resolve_direct_adapter(app_id: &str, preferred: Option<&str>) -> Option<Box<dyn AppAdapter>> {
    for spec in DIRECT_ADAPTERS {
        if !spec.app_ids.iter().any(|candidate| *candidate == app_id) {
            continue;
        }

        if let Some(preferred) = preferred {
            if !matches_adapter_alias(preferred, spec.aliases) {
                logging::debug(format!(
                    "resolve_chain: adapter override '{}' does not match direct adapter '{}'",
                    preferred, spec.name
                ));
                return None;
            }
        }

        if !crate::config::app_integration_enabled(spec.section, spec.aliases) {
            logging::debug(format!(
                "resolve_chain: direct adapter '{}' disabled via config",
                spec.name
            ));
            return None;
        }

        return Some((spec.build)());
    }

    None
}

fn resolve_terminal_chain(terminal_pid: u32) -> Vec<Box<dyn AppAdapter>> {
    let mut chain: Vec<Box<dyn AppAdapter>> = Vec::new();

    let fg_hint = crate::adapters::terminal_multiplexers::active_foreground_process(
        wezterm::ADAPTER_ALIASES,
        terminal_pid,
    );
    let fg_base = fg_hint
        .as_deref()
        .map(runtime::normalize_process_name)
        .unwrap_or_default();
    logging::debug(format!(
        "resolve_terminal_chain: pid={} fg_hint={:?} fg_base={}",
        terminal_pid, fg_hint, fg_base
    ));

    let shells: Vec<u32> = runtime::child_pids(terminal_pid)
        .into_iter()
        .filter(|&pid| runtime::is_shell_pid(pid))
        .collect();
    logging::debug(format!(
        "resolve_terminal_chain: shell_candidates={:?}",
        shells
    ));

    let search_pid = if shells.len() <= 1 {
        shells.first().copied()
    } else if !fg_base.is_empty() {
        shells.iter().copied().find(|&shell_pid| {
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{shell_pid}/stat")) else {
                return false;
            };
            let Some(tpgid) = runtime::parse_stat_tpgid(&stat) else {
                return false;
            };
            runtime::process_comm(tpgid)
                .map(|comm| comm == fg_base)
                .unwrap_or(false)
        })
    } else {
        None
    };

    let Some(search_pid) = search_pid else {
        logging::debug("resolve_terminal_chain: no focused shell match; using terminal layer only");
        chain.push(apps::bind_policy(Box::new(wezterm::WeztermBackend)));
        return chain;
    };
    logging::debug(format!(
        "resolve_terminal_chain: selected shell pid={search_pid}"
    ));

    match fg_base.as_str() {
        "tmux" => {
            let tmux_pids = runtime::find_descendants_by_comm(search_pid, "tmux");
            logging::debug(format!(
                "resolve_terminal_chain: tmux descendants under shell {} => {:?}",
                search_pid, tmux_pids
            ));
            let found_tmux = tmux_pids.first().and_then(|tmux_client_pid| {
                Tmux::from_client_pid(
                    *tmux_client_pid,
                    wezterm::TERMINAL_LAUNCH_PREFIX
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                )
            });
            if let Some(tmux) = found_tmux {
                if let Some(nvim_pid) = tmux.nvim_in_current_pane() {
                    if let Some(nvim) = Nvim::for_pid(nvim_pid) {
                        chain.push(apps::bind_policy(Box::new(nvim)));
                    }
                }
                chain.push(apps::bind_policy(Box::new(tmux)));
            }
        }
        "nvim" => {
            let nvim_pids = runtime::find_descendants_by_comm(search_pid, "nvim");
            logging::debug(format!(
                "resolve_terminal_chain: nvim descendants under shell {} => {:?}",
                search_pid, nvim_pids
            ));
            if let Some(&nvim_pid) = nvim_pids.first() {
                if let Some(nvim) = Nvim::for_pid(nvim_pid) {
                    chain.push(apps::bind_policy(Box::new(nvim)));
                }
            }
        }
        _ => {}
    }

    chain.push(apps::bind_policy(Box::new(wezterm::WeztermBackend)));
    logging::debug(format!(
        "resolve_terminal_chain: final depth={}",
        chain.len()
    ));

    chain
}

fn domain_id_for_app_kind(kind: AppKind) -> DomainId {
    match kind {
        AppKind::Terminal => TERMINAL_DOMAIN_ID,
        AppKind::Editor => EDITOR_DOMAIN_ID,
        AppKind::Browser => WM_DOMAIN_ID,
    }
}

impl ChainResolver for RuntimeChainResolver {
    fn resolve_chain(&self, app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn AppAdapter>> {
        logging::debug(format!(
            "resolve_chain: app_id={} pid={} title={}",
            app_id, pid, title
        ));
        let preferred = preferred_adapter_name();

        if wezterm::APP_IDS.contains(&app_id) {
            if let Some(preferred) = preferred.as_deref() {
                if !matches_adapter_alias(preferred, wezterm::ADAPTER_ALIASES) {
                    logging::debug(format!(
                        "resolve_chain: adapter override '{}' disables terminal chain",
                        preferred
                    ));
                    return vec![];
                }
            }
            if !crate::config::app_integration_enabled(
                AppSection::Terminal,
                wezterm::ADAPTER_ALIASES,
            ) {
                logging::debug("resolve_chain: terminal integration disabled via config");
                return vec![];
            }
            let chain = resolve_terminal_chain(pid);
            logging::debug(format!("resolve_chain: terminal depth={}", chain.len()));
            return chain;
        }

        if let Some(app) = resolve_direct_adapter(app_id, preferred.as_deref()) {
            logging::debug("resolve_chain: direct app match depth=1");
            return vec![app];
        }

        logging::debug("resolve_chain: no deep app match depth=0");
        vec![]
    }

    fn default_domain_adapters(&self) -> Vec<Box<dyn AppAdapter>> {
        vec![
            apps::bind_policy(Box::new(wezterm::WeztermBackend)),
            apps::bind_policy(Box::new(emacs::EmacsBackend)),
        ]
    }

    fn domain_id_for_window(
        &self,
        app_id: Option<&str>,
        pid: Option<ProcessId>,
        title: Option<&str>,
    ) -> DomainId {
        let app_id = app_id.unwrap_or_default();
        let title = title.unwrap_or_default();
        let owner_pid = pid.map(ProcessId::get).unwrap_or(0);
        if let Some(kind) = self
            .resolve_chain(app_id, owner_pid, title)
            .into_iter()
            .map(|adapter| adapter.kind())
            .next()
        {
            return domain_id_for_app_kind(kind);
        }
        WM_DOMAIN_ID
    }
}
