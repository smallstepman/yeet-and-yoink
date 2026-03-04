pub mod emacs;
pub mod librefox;
pub mod nvim;
pub mod vscode;
pub mod wezterm;

use crate::config::AppSection;

pub use crate::engine::contract::{
    unsupported_operation, AdapterCapabilities, AppAdapter, AppCapabilities, AppKind,
    ChainResolver, MergeExecutionMode, MergePreparation, MoveDecision, TearResult, TopologyHandler,
    TopologySnapshot,
};

/// Developer note for adding a new adapter:
/// 1. Implement `AppAdapter` and declare all booleans in `capabilities`.
/// 2. Keep unsupported operations disabled in `capabilities` so the orchestrator
///    classify them as `Unsupported` without runtime probes.
/// 3. Add adapter tests that cover focus/move/resize behavior and precedence.

struct PolicyBoundApp {
    inner: Box<dyn AppAdapter>,
    scope: Option<(AppSection, &'static [&'static str])>,
}

impl PolicyBoundApp {
    fn new(inner: Box<dyn AppAdapter>) -> Self {
        let scope = inner.config_aliases().map(|aliases| {
            let section = match inner.kind() {
                AppKind::Browser => AppSection::Browser,
                AppKind::Editor => AppSection::Editor,
                AppKind::Terminal => AppSection::Terminal,
            };
            (section, aliases)
        });
        Self { inner, scope }
    }

    fn pane_policy(&self) -> Option<crate::config::PanePolicy> {
        let (section, aliases) = self.scope?;
        match section {
            AppSection::Browser => None,
            AppSection::Editor | AppSection::Terminal => {
                Some(crate::config::pane_policy_for(section, aliases))
            }
        }
    }
}

impl AppAdapter for PolicyBoundApp {
    fn adapter_name(&self) -> &'static str {
        self.inner.adapter_name()
    }

    fn config_aliases(&self) -> Option<&'static [&'static str]> {
        self.inner.config_aliases()
    }

    fn kind(&self) -> AppKind {
        self.inner.kind()
    }

    fn capabilities(&self) -> AdapterCapabilities {
        let mut capabilities = self.inner.capabilities();
        if let Some(policy) = self.pane_policy() {
            capabilities.focus &= policy.focus_capability();
            capabilities.move_internal &= policy.move_capability();
            capabilities.resize_internal &= policy.resize_capability();
            capabilities.rearrange &= policy.move_capability();
            capabilities.tear_out &= policy.tear_out_capability();
        }
        capabilities
    }

    fn eval(
        &self,
        expression: &str,
        pid: Option<crate::engine::runtime::ProcessId>,
    ) -> anyhow::Result<String> {
        self.inner.eval(expression, pid)
    }
}

impl TopologyHandler for PolicyBoundApp {
    fn can_focus(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<bool> {
        if let Some(policy) = self.pane_policy() {
            if !policy.focus_allowed(dir) {
                return Ok(false);
            }
        }
        TopologyHandler::can_focus(self.inner.as_ref(), dir, pid)
    }

    fn move_decision(
        &self,
        dir: crate::engine::topology::Direction,
        pid: u32,
    ) -> anyhow::Result<MoveDecision> {
        if let Some(policy) = self.pane_policy() {
            if !policy.move_allowed(dir) {
                return Ok(MoveDecision::Passthrough);
            }
            let decision = TopologyHandler::move_decision(self.inner.as_ref(), dir, pid)?;
            if matches!(decision, MoveDecision::TearOut) && !policy.tear_out_capability() {
                return Ok(MoveDecision::Passthrough);
            }
            return Ok(decision);
        }
        TopologyHandler::move_decision(self.inner.as_ref(), dir, pid)
    }

    fn can_resize(
        &self,
        dir: crate::engine::topology::Direction,
        grow: bool,
        pid: u32,
    ) -> anyhow::Result<bool> {
        if let Some(policy) = self.pane_policy() {
            if !policy.resize_allowed(dir) {
                return Ok(false);
            }
        }
        TopologyHandler::can_resize(self.inner.as_ref(), dir, grow, pid)
    }

    fn at_side(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<bool> {
        TopologyHandler::at_side(self.inner.as_ref(), dir, pid)
    }

    fn window_count(&self, pid: u32) -> anyhow::Result<u32> {
        TopologyHandler::window_count(self.inner.as_ref(), pid)
    }

    fn focus(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<()> {
        if let Some(policy) = self.pane_policy() {
            if !policy.focus_allowed(dir) {
                return Err(unsupported_operation(self.adapter_name(), "focus"));
            }
        }
        TopologyHandler::focus(self.inner.as_ref(), dir, pid)
    }

    fn move_internal(
        &self,
        dir: crate::engine::topology::Direction,
        pid: u32,
    ) -> anyhow::Result<()> {
        if let Some(policy) = self.pane_policy() {
            if !policy.move_allowed(dir) {
                return Err(unsupported_operation(self.adapter_name(), "move_internal"));
            }
        }
        TopologyHandler::move_internal(self.inner.as_ref(), dir, pid)
    }

    fn resize_internal(
        &self,
        dir: crate::engine::topology::Direction,
        grow: bool,
        step: i32,
        pid: u32,
    ) -> anyhow::Result<()> {
        if let Some(policy) = self.pane_policy() {
            if !policy.resize_allowed(dir) {
                return Err(unsupported_operation(
                    self.adapter_name(),
                    "resize_internal",
                ));
            }
        }
        TopologyHandler::resize_internal(self.inner.as_ref(), dir, grow, step, pid)
    }

    fn rearrange(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<()> {
        if let Some(policy) = self.pane_policy() {
            if !policy.move_allowed(dir) {
                return Err(unsupported_operation(self.adapter_name(), "rearrange"));
            }
        }
        TopologyHandler::rearrange(self.inner.as_ref(), dir, pid)
    }

    fn move_out(
        &self,
        dir: crate::engine::topology::Direction,
        pid: u32,
    ) -> anyhow::Result<TearResult> {
        if let Some(policy) = self.pane_policy() {
            if !policy.move_allowed(dir) || !policy.tear_out_capability() {
                return Err(unsupported_operation(self.adapter_name(), "move_out"));
            }
        }
        TopologyHandler::move_out(self.inner.as_ref(), dir, pid)
    }

    fn merge_into(
        &self,
        dir: crate::engine::topology::Direction,
        source_pid: u32,
    ) -> anyhow::Result<()> {
        TopologyHandler::merge_into(self.inner.as_ref(), dir, source_pid)
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        TopologyHandler::merge_execution_mode(self.inner.as_ref())
    }

    fn prepare_merge(
        &self,
        source_pid: Option<crate::engine::runtime::ProcessId>,
    ) -> anyhow::Result<MergePreparation> {
        TopologyHandler::prepare_merge(self.inner.as_ref(), source_pid)
    }

    fn augment_merge_preparation_for_target(
        &self,
        preparation: MergePreparation,
        target_window_id: Option<u64>,
    ) -> MergePreparation {
        TopologyHandler::augment_merge_preparation_for_target(
            self.inner.as_ref(),
            preparation,
            target_window_id,
        )
    }

    fn merge_into_target(
        &self,
        dir: crate::engine::topology::Direction,
        source_pid: Option<crate::engine::runtime::ProcessId>,
        target_pid: Option<crate::engine::runtime::ProcessId>,
        preparation: MergePreparation,
    ) -> anyhow::Result<()> {
        TopologyHandler::merge_into_target(
            self.inner.as_ref(),
            dir,
            source_pid,
            target_pid,
            preparation,
        )
    }
}

pub(crate) fn bind_policy(app: Box<dyn AppAdapter>) -> Box<dyn AppAdapter> {
    Box::new(PolicyBoundApp::new(app))
}

// ---------------------------------------------------------------------------
// App resolution (delegated to engine ChainResolver)
// ---------------------------------------------------------------------------

/// Baseline adapters used to seed runtime domains even when the focused window
/// does not currently belong to that app kind.
pub fn default_domain_adapters() -> Vec<Box<dyn AppAdapter>> {
    crate::engine::chain_resolver::runtime_chain_resolver().default_domain_adapters()
}

/// Resolve a chain of app handlers for a window, innermost-first.
pub fn resolve_chain(app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn AppAdapter>> {
    crate::engine::chain_resolver::runtime_chain_resolver().resolve_chain(app_id, pid, title)
}

#[cfg(test)]
mod resolve_chain_tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::adapters::apps::{
        emacs, librefox::Librefox, nvim::Nvim, vscode::Vscode, wezterm, TopologyHandler,
    };
    use crate::adapters::terminal_multiplexers::tmux::Tmux;

    use super::resolve_chain;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "niri-deep-app-resolve-{prefix}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn set_env(key: &str, value: Option<&str>) -> Option<std::ffi::OsString> {
        let old = std::env::var_os(key);
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
        old
    }

    fn restore_env(key: &str, old: Option<std::ffi::OsString>) {
        if let Some(old) = old {
            std::env::set_var(key, old);
        } else {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn adapters_implement_topology_traits() {
        fn assert_topology_contracts<T: TopologyHandler>() {}
        assert_topology_contracts::<emacs::EmacsBackend>();
        assert_topology_contracts::<wezterm::WeztermBackend>();
        assert_topology_contracts::<Tmux>();
        assert_topology_contracts::<Nvim>();
        assert_topology_contracts::<Librefox>();
        assert_topology_contracts::<Vscode>();
    }

    #[test]
    fn direct_match_without_override_returns_adapter() {
        let _guard = env_guard();
        let old_override = set_env("NIRI_DEEP_CONFIG", None);
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(emacs::APP_IDS[0], 0, "");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].adapter_name(), emacs::ADAPTER_NAME);

        restore_env("NIRI_DEEP_CONFIG", old_override);
    }

    #[test]
    fn override_filters_non_matching_direct_adapter() {
        let _guard = env_guard();
        let root = unique_temp_dir("override-filter");
        let config_dir = root.join("niri-deep");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.vscode]
enabled = true
"#,
        )
        .expect("config file should be writable");

        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(emacs::APP_IDS[0], 0, "");
        assert!(chain.is_empty());

        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn override_applies_to_terminal_chain_selection() {
        let _guard = env_guard();
        let root = unique_temp_dir("override-terminal");
        let config_dir = root.join("niri-deep");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.editor]
enabled = true
"#,
        )
        .expect("config file should be writable");

        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(wezterm::APP_IDS[0], 0, "");
        assert!(chain.is_empty());

        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolved_editor_capabilities_follow_config_policy() {
        let _guard = env_guard();
        let root = unique_temp_dir("policy-editor");
        let config_dir = root.join("niri-deep");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.emacs]
enabled = true
focus.internal_panes.enabled = false
"#,
        )
        .expect("config file should be writable");

        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(emacs::APP_IDS[0], 0, "");
        assert_eq!(chain.len(), 1);
        assert!(!chain[0].capabilities().focus);

        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolved_terminal_capabilities_follow_config_policy() {
        let _guard = env_guard();
        let root = unique_temp_dir("policy-terminal");
        let config_dir = root.join("niri-deep");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true
resize.internal_panes.enabled = false
"#,
        )
        .expect("config file should be writable");

        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(wezterm::APP_IDS[0], 0, "");
        assert!(!chain.is_empty());
        let wezterm = chain
            .iter()
            .find(|app| app.adapter_name() == wezterm::ADAPTER_NAME)
            .expect("wezterm adapter should be in chain");
        assert!(!wezterm.capabilities().resize_internal);

        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(root);
    }
}
