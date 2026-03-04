use anyhow::Result;

use crate::adapters::apps::terminal_mux::TerminalMuxProvider;
use crate::engine::contract::AdapterCapabilities;
use crate::engine::topology::Direction;

use super::WeztermMux;

pub(super) static WEZTERM_MUX_PROVIDER: WeztermMux = WeztermMux;

impl TerminalMuxProvider for WeztermMux {
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            probe: true,
            focus: true,
            move_internal: true,
            resize_internal: true,
            rearrange: true,
            tear_out: true,
            merge: true,
        }
    }

    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64> {
        WeztermMux::focused_pane_for_pid(pid)
    }

    fn pane_neighbor_for_pid(&self, pid: u32, pane_id: u64, dir: Direction) -> Result<u64> {
        WeztermMux::pane_neighbor_for_pid(pid, pane_id, dir)
    }

    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()> {
        WeztermMux::send_text_to_pane(pid, pane_id, text)
    }

    fn mux_attach_args(&self, _target: String) -> Option<Vec<String>> {
        // Built-in wezterm mux manages windows directly; no external spawn needed.
        None
    }

    fn merge_source_pane_into_focused_target(
        &self,
        source_pid: u32,
        source_pane_id: u64,
        target_pid: u32,
        target_window_id: Option<u64>,
        dir: Direction,
    ) -> Result<()> {
        WeztermMux::merge_source_pane_into_focused_target(
            source_pid,
            source_pane_id,
            target_pid,
            target_window_id,
            dir,
        )
    }

    fn active_foreground_process(&self, pid: u32) -> Option<String> {
        WeztermMux::active_foreground_process(pid)
    }
}
