use anyhow::Result;

use crate::engine::contract::{AdapterCapabilities, TopologyHandler};
use crate::engine::topology::Direction;

pub trait TerminalMuxProvider: TopologyHandler {
    /// Capabilities this mux backend supports (pane focus, move, resize, etc).
    fn capabilities(&self) -> AdapterCapabilities;
    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64>;
    fn pane_neighbor_for_pid(&self, pid: u32, pane_id: u64, dir: Direction) -> Result<u64>;
    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()>;
    /// Returns the mux-specific attach arguments (e.g. `["tmux", "attach", "-t", target]`),
    /// or `None` if the mux manages windows directly (built-in mux).
    /// The terminal host composes these with its own launch prefix.
    fn mux_attach_args(&self, target: String) -> Option<Vec<String>>;
    fn merge_source_pane_into_focused_target(
        &self,
        source_pid: u32,
        source_pane_id: u64,
        target_pid: u32,
        target_window_id: Option<u64>,
        dir: Direction,
    ) -> Result<()>;
    fn active_foreground_process(&self, pid: u32) -> Option<String>;
}
