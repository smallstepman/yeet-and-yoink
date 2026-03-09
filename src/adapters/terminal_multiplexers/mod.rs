use anyhow::Result;

use crate::config::TerminalMuxBackend;
use crate::engine::contract::{TearResult, TerminalMultiplexerProvider};

pub mod kitty;
pub mod tmux;
pub mod wezterm;
pub mod zellij;

pub const WEZTERM_HOST_ALIASES: &[&str] = &["wezterm", "terminal"];

pub fn active_mux_provider(aliases: &[&str]) -> &'static dyn TerminalMultiplexerProvider {
    match crate::config::mux_policy_for(aliases).backend {
        TerminalMuxBackend::Wezterm => &wezterm::WEZTERM_MUX_PROVIDER,
        TerminalMuxBackend::Tmux => &tmux::TMUX_MUX_PROVIDER,
        TerminalMuxBackend::Zellij => &zellij::ZELLIJ_MUX_PROVIDER,
        TerminalMuxBackend::Kitty => &kitty::KITTY_MUX_PROVIDER,
    }
}

pub fn spawn_attach_command(
    aliases: &[&str],
    terminal_launch_prefix: &[&str],
    target: String,
) -> Option<Vec<String>> {
    let mux_args = active_mux_provider(aliases).mux_attach_args(target)?;
    let mut command: Vec<String> = terminal_launch_prefix
        .iter()
        .map(|segment| segment.to_string())
        .collect();
    command.extend(mux_args);
    Some(command)
}

pub fn prepend_terminal_launch_prefix(
    terminal_launch_prefix: &[&str],
    mut tear: TearResult,
) -> TearResult {
    if let Some(mux_args) = tear.spawn_command.take() {
        let mut command: Vec<String> = terminal_launch_prefix
            .iter()
            .map(|segment| segment.to_string())
            .collect();
        command.extend(mux_args);
        tear.spawn_command = Some(command);
    }
    tear
}

pub fn active_foreground_process(aliases: &[&str], pid: u32) -> Option<String> {
    active_mux_provider(aliases).active_foreground_process(pid)
}

pub fn pane_neighbor_for_pid(
    aliases: &[&str],
    pid: u32,
    pane_id: u64,
    dir: crate::engine::topology::Direction,
) -> Result<u64> {
    active_mux_provider(aliases).pane_neighbor_for_pid(pid, pane_id, dir)
}

pub fn send_text_to_pane(aliases: &[&str], pid: u32, pane_id: u64, text: &str) -> Result<()> {
    active_mux_provider(aliases).send_text_to_pane(pid, pane_id, text)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeet-and-yoink-terminal-mux-{prefix}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn set_env(key: &str, value: Option<&str>) -> Option<OsString> {
        let old = std::env::var_os(key);
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
        old
    }

    fn restore_env(key: &str, old: Option<OsString>) {
        if let Some(value) = old {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn active_mux_provider_exposes_default_wezterm_capabilities() {
        let _guard = env_guard();
        let root = unique_temp_dir("wezterm-default-provider");
        let config = root.join("config.toml");
        fs::write(&config, "").expect("config file should be writable");
        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config.to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");
        let provider = super::active_mux_provider(super::WEZTERM_HOST_ALIASES);
        let caps = provider.capabilities();
        assert!(caps.focus);
        assert!(caps.resize_internal);
        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn spawn_attach_command_is_none_for_wezterm_mux_default() {
        let _guard = env_guard();
        let root = unique_temp_dir("wezterm-default-attach");
        let config = root.join("config.toml");
        fs::write(&config, "").expect("config file should be writable");
        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config.to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");
        let command = super::spawn_attach_command(
            super::WEZTERM_HOST_ALIASES,
            &["wezterm", "-e"],
            "dev".to_string(),
        );
        assert_eq!(command, None);
        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = fs::remove_dir_all(root);
    }
}
