use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::engine::contract::{
    AdapterCapabilities, MoveDecision, TearResult, TerminalMultiplexerProvider,
    TerminalPaneSnapshot, TopologyHandler,
};
use crate::engine::runtime;
use crate::engine::topology::{Direction, DirectionalNeighbors};

#[derive(Debug, Deserialize)]
struct KittyOsWindow {
    id: u64,
    #[serde(default)]
    is_focused: bool,
    #[serde(default)]
    tabs: Vec<KittyTab>,
}

#[derive(Debug, Deserialize)]
struct KittyTab {
    id: u64,
    #[serde(default)]
    is_focused: bool,
    #[serde(default)]
    windows: Vec<KittyPane>,
}

#[derive(Debug, Deserialize)]
struct KittyPane {
    id: u64,
    #[serde(default)]
    is_focused: bool,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    cmdline: Vec<String>,
    #[serde(default)]
    foreground_processes: Vec<KittyForegroundProcess>,
}

#[derive(Debug, Deserialize)]
struct KittyForegroundProcess {
    #[serde(default)]
    cmdline: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct KittyMux;

pub(crate) static KITTY_MUX_PROVIDER: KittyMux = KittyMux;

impl KittyPane {
    fn foreground_process_name(&self) -> Option<String> {
        let command = self
            .foreground_processes
            .iter()
            .find_map(|process| process.cmdline.first())
            .map(String::as_str)
            .or_else(|| self.cmdline.first().map(String::as_str))?;
        let normalized = runtime::normalize_process_name(command);
        (!normalized.is_empty()).then_some(normalized)
    }
}

impl KittyMux {
    fn direction_name(dir: Direction) -> &'static str {
        dir.positional()
    }

    fn no_match(stderr: &str) -> bool {
        let value = stderr.to_ascii_lowercase();
        value.contains("no matching window")
            || value.contains("no matching windows")
            || value.contains("no matching tabs")
            || value.contains("matches no windows")
    }

    fn read_environ_var(pid: u32, key: &str) -> Option<String> {
        let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
        let prefix = format!("{key}=");
        for chunk in environ.split(|byte| *byte == 0) {
            let entry = String::from_utf8_lossy(chunk);
            if let Some(value) = entry.strip_prefix(&prefix) {
                if !value.trim().is_empty() {
                    return Some(value.to_string());
                }
            }
        }
        None
    }

    fn socket_for_pid(pid: u32) -> Option<String> {
        let mut candidates = vec![pid];
        for child in runtime::descendant_pids(pid) {
            if !candidates.contains(&child) {
                candidates.push(child);
            }
        }
        for candidate in candidates {
            if let Some(socket) = Self::read_environ_var(candidate, "KITTY_LISTEN_ON") {
                return Some(socket);
            }
        }
        None
    }

    fn active_tab_panes(&self, pid: u32) -> Result<Vec<TerminalPaneSnapshot>> {
        let raw = self.cli_stdout_for_pid(pid, &["ls", "--output-format", "json"])?;
        let windows: Vec<KittyOsWindow> =
            serde_json::from_str(&raw).context("failed to parse kitty ls json")?;
        let window = windows
            .iter()
            .find(|window| window.is_focused)
            .or_else(|| {
                windows.iter().find(|window| {
                    window.tabs.iter().any(|tab| {
                        tab.is_focused
                            || tab
                                .windows
                                .iter()
                                .any(|pane| pane.is_focused || pane.is_active)
                    })
                })
            })
            .or_else(|| windows.first())
            .context("kitty did not report any windows")?;
        let tab = window
            .tabs
            .iter()
            .find(|tab| tab.is_focused)
            .or_else(|| {
                window.tabs.iter().find(|tab| {
                    tab.windows
                        .iter()
                        .any(|pane| pane.is_focused || pane.is_active)
                })
            })
            .or_else(|| window.tabs.first())
            .context("kitty focused window has no tabs")?;
        Ok(tab
            .windows
            .iter()
            .map(|pane| TerminalPaneSnapshot {
                pane_id: pane.id,
                tab_id: Some(tab.id),
                window_id: Some(window.id),
                is_active: pane.is_focused || pane.is_active,
                foreground_process_name: pane.foreground_process_name(),
            })
            .collect())
    }

    fn focus_pane_by_id(&self, pid: u32, pane_id: u64) -> Result<()> {
        let matcher = format!("id:{pane_id}");
        self.cli_stdout_for_pid(pid, &["focus-window", "--match", &matcher])?;
        Ok(())
    }

    fn try_focus_neighbor(&self, pid: u32, dir: Direction) -> Result<bool> {
        let matcher = format!("neighbor:{}", Self::direction_name(dir));
        let output = self.cli_output_for_pid(pid, &["focus-window", "--match", &matcher])?;
        if output.status.success() {
            return Ok(true);
        }
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if Self::no_match(&stderr) {
            return Ok(false);
        }
        bail!(
            "kitty focus-window --match {} failed: {}",
            matcher,
            stderr.trim()
        );
    }
}

impl TerminalMultiplexerProvider for KittyMux {
    fn cli_output_for_pid(&self, pid: u32, args: &[&str]) -> Result<std::process::Output> {
        let mut command = Command::new("kitty");
        command.arg("@");
        if let Some(socket) = Self::socket_for_pid(pid) {
            command.args(["--to", &socket]);
        }
        command.args(args);
        command
            .output()
            .context("failed to run kitty remote-control command")
    }

    fn list_panes_for_pid(&self, pid: u32) -> Result<Vec<TerminalPaneSnapshot>> {
        self.active_tab_panes(pid)
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            probe: true,
            focus: true,
            move_internal: true,
            resize_internal: false,
            rearrange: true,
            tear_out: true,
            merge: false,
        }
    }

    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64> {
        let panes = self.active_tab_panes(pid)?;
        panes
            .iter()
            .find(|pane| pane.is_active)
            .map(|pane| pane.pane_id)
            .or_else(|| panes.first().map(|pane| pane.pane_id))
            .context("unable to determine focused kitty pane")
    }

    fn pane_in_direction_for_pid(
        &self,
        pid: u32,
        pane_id: u64,
        dir: Direction,
    ) -> Result<Option<u64>> {
        let original_focus = self.focused_pane_for_pid(pid).ok();
        if original_focus != Some(pane_id) {
            self.focus_pane_by_id(pid, pane_id)?;
        }
        let result = (|| -> Result<Option<u64>> {
            if !self.try_focus_neighbor(pid, dir)? {
                return Ok(None);
            }
            let focused = self.focused_pane_for_pid(pid)?;
            Ok((focused != pane_id).then_some(focused))
        })();
        if let Some(original_focus) = original_focus {
            let _ = self.focus_pane_by_id(pid, original_focus);
        }
        result
    }

    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()> {
        let matcher = format!("id:{pane_id}");
        let output =
            self.cli_output_for_pid(pid, &["send-text", "--match", &matcher, "--", text])?;
        if !output.status.success() {
            bail!(
                "kitty send-text failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    fn mux_attach_args(&self, _target: String) -> Option<Vec<String>> {
        None
    }

    fn merge_source_pane_into_focused_target(
        &self,
        _source_pid: u32,
        _source_pane_id: u64,
        _target_pid: u32,
        _target_window_id: Option<u64>,
        _dir: Direction,
    ) -> Result<()> {
        Err(crate::engine::contract::unsupported_operation(
            "kitty",
            "merge_source_pane_into_focused_target",
        ))
    }

    fn active_foreground_process(&self, pid: u32) -> Option<String> {
        if pid == 0 {
            return None;
        }
        let pane_id = self.focused_pane_for_pid(pid).ok()?;
        let panes = self.active_tab_panes(pid).ok()?;
        panes
            .into_iter()
            .find(|pane| pane.pane_id == pane_id)
            .and_then(|pane| pane.foreground_process_name)
    }
}

impl TopologyHandler for KittyMux {
    fn directional_neighbors(&self, pid: u32) -> Result<DirectionalNeighbors> {
        let focused_pane = self.focused_pane_for_pid(pid)?;
        let mut neighbors = DirectionalNeighbors::default();
        for direction in Direction::ALL {
            neighbors.set(
                direction,
                self.pane_in_direction_for_pid(pid, focused_pane, direction)?
                    .is_some(),
            );
        }
        Ok(neighbors)
    }

    fn window_count(&self, pid: u32) -> Result<u32> {
        Ok(self.active_tab_panes(pid)?.len() as u32)
    }

    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        let focused_pane = self.focused_pane_for_pid(pid)?;
        Ok(self
            .pane_in_direction_for_pid(pid, focused_pane, dir)?
            .is_some())
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        Ok(self.move_surface(pid)?.decision_for(dir))
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()> {
        if !self.try_focus_neighbor(pid, dir)? {
            bail!("no kitty pane exists in requested direction");
        }
        Ok(())
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        let pane_id = self.focused_pane_for_pid(pid)?;
        let matcher = format!("id:{pane_id}");
        self.cli_stdout_for_pid(
            pid,
            &[
                "move-window",
                "--match",
                &matcher,
                Self::direction_name(dir),
            ],
        )?;
        Ok(())
    }

    fn rearrange(&self, dir: Direction, pid: u32) -> Result<()> {
        self.move_internal(dir, pid)
    }

    fn move_out(&self, _dir: Direction, pid: u32) -> Result<TearResult> {
        let pane_id = self.focused_pane_for_pid(pid)?;
        let matcher = format!("id:{pane_id}");
        self.cli_stdout_for_pid(pid, &["detach-window", "--match", &matcher])?;
        Ok(TearResult {
            spawn_command: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::KittyMux;
    use crate::engine::contract::{TerminalMultiplexerProvider, TopologyHandler};
    use crate::engine::topology::Direction;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    const TEST_RESPONSES_ENV: &str = "KITTY_TEST_RESPONSES_DIR";
    const TEST_LOG_ENV: &str = "KITTY_TEST_LOG";

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    struct KittyHarness {
        base: PathBuf,
        responses_dir: PathBuf,
        log_file: PathBuf,
        old_path: Option<OsString>,
        old_responses_dir: Option<OsString>,
        old_log_file: Option<OsString>,
    }

    impl KittyHarness {
        fn new() -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "yeet-and-yoink-kitty-mux-test-{}-{unique}",
                std::process::id()
            ));
            let bin_dir = base.join("bin");
            let responses_dir = base.join("responses");
            let log_file = base.join("commands.log");
            fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
            fs::create_dir_all(&responses_dir).expect("failed to create fake responses dir");

            let fake_kitty = bin_dir.join("kitty");
            fs::write(
                &fake_kitty,
                r#"#!/bin/sh
set -eu
if [ "$#" -ge 1 ] && [ "$1" = "@" ]; then
  shift
fi
if [ "$#" -ge 2 ] && [ "$1" = "--to" ]; then
  shift 2
fi
key="$*"
printf '%s\n' "$key" >> "${KITTY_TEST_LOG}"
safe_key="$(printf '%s' "$key" | tr -c 'A-Za-z0-9._-' '_')"
status_file="${KITTY_TEST_RESPONSES_DIR}/${safe_key}.status"
stdout_file="${KITTY_TEST_RESPONSES_DIR}/${safe_key}.stdout"
stderr_file="${KITTY_TEST_RESPONSES_DIR}/${safe_key}.stderr"
status=0
if [ -f "$status_file" ]; then
  status="$(cat "$status_file")"
fi
if [ -f "$stdout_file" ]; then
  cat "$stdout_file"
fi
if [ -f "$stderr_file" ]; then
  cat "$stderr_file" >&2
fi
exit "$status"
"#,
            )
            .expect("failed to write fake kitty script");
            let mut permissions = fs::metadata(&fake_kitty)
                .expect("failed to stat fake kitty script")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&fake_kitty, permissions)
                .expect("failed to chmod fake kitty script");

            let old_path = std::env::var_os("PATH");
            let old_responses_dir = std::env::var_os(TEST_RESPONSES_ENV);
            let old_log_file = std::env::var_os(TEST_LOG_ENV);

            let mut path_entries = vec![bin_dir];
            if let Some(ref old) = old_path {
                path_entries.extend(std::env::split_paths(old));
            }
            let path = std::env::join_paths(path_entries).expect("failed to compose PATH");
            std::env::set_var("PATH", path);
            std::env::set_var(TEST_RESPONSES_ENV, &responses_dir);
            std::env::set_var(TEST_LOG_ENV, &log_file);

            Self {
                base,
                responses_dir,
                log_file,
                old_path,
                old_responses_dir,
                old_log_file,
            }
        }

        fn set_response(&self, key: &str, status: i32, stdout: &str, stderr: &str) {
            let safe_key: String = key
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect();
            fs::write(
                self.responses_dir.join(format!("{safe_key}.status")),
                status.to_string(),
            )
            .expect("failed to write fake status");
            fs::write(
                self.responses_dir.join(format!("{safe_key}.stdout")),
                stdout,
            )
            .expect("failed to write fake stdout");
            fs::write(
                self.responses_dir.join(format!("{safe_key}.stderr")),
                stderr,
            )
            .expect("failed to write fake stderr");
        }

        fn command_log(&self) -> String {
            fs::read_to_string(&self.log_file).unwrap_or_default()
        }
    }

    impl Drop for KittyHarness {
        fn drop(&mut self) {
            if let Some(value) = &self.old_path {
                std::env::set_var("PATH", value);
            } else {
                std::env::remove_var("PATH");
            }
            if let Some(value) = &self.old_responses_dir {
                std::env::set_var(TEST_RESPONSES_ENV, value);
            } else {
                std::env::remove_var(TEST_RESPONSES_ENV);
            }
            if let Some(value) = &self.old_log_file {
                std::env::set_var(TEST_LOG_ENV, value);
            } else {
                std::env::remove_var(TEST_LOG_ENV);
            }
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn focused_pane_and_window_count_come_from_active_tab() {
        let _guard = env_guard();
        let harness = KittyHarness::new();
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]},
                  {"id": 101, "is_focused": false, "foreground_processes": [{"cmdline": ["nvim"]}]}
                ]}
              ]}
            ]"#,
            "",
        );

        let provider = KittyMux;
        let focused = provider.focused_pane_for_pid(0).expect("focused pane");
        let count = provider.window_count(0).expect("window_count");
        assert_eq!(focused, 100);
        assert_eq!(count, 2);
    }

    #[test]
    fn move_internal_uses_move_window_command() {
        let _guard = env_guard();
        let harness = KittyHarness::new();
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]}
                ]}
              ]}
            ]"#,
            "",
        );
        harness.set_response("move-window --match id:100 left", 0, "", "");

        let provider = KittyMux;
        provider
            .move_internal(Direction::West, 0)
            .expect("move_internal should succeed");
        assert!(harness
            .command_log()
            .contains("move-window --match id:100 left"));
    }

    #[test]
    fn move_out_uses_detach_window_command() {
        let _guard = env_guard();
        let harness = KittyHarness::new();
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]}
                ]}
              ]}
            ]"#,
            "",
        );
        harness.set_response("detach-window --match id:100", 0, "", "");

        let provider = KittyMux;
        provider
            .move_out(Direction::East, 0)
            .expect("move_out should succeed");
        assert!(harness
            .command_log()
            .contains("detach-window --match id:100"));
    }
}
