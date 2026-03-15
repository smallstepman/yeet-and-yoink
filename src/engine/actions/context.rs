use anyhow::Result;

use crate::engine::contracts::adapter::AppAdapter;
use crate::engine::runtime::ProcessId;
use crate::engine::wm::configured::ConfiguredWindowManager;

/// Captures the focused-window state extracted by the preamble shared across
/// `attempt_focused_app_focus`, `attempt_focused_app_move`, and
/// `attempt_focused_app_resize` in `orchestrator.rs`.
pub struct AppContext {
    pub source_window_id: u64,
    pub source_tile_index: usize,
    pub source_pid: Option<ProcessId>,
    pub owner_pid: u32,
    pub app_id: String,
    pub title: String,
}

impl AppContext {
    /// Build an `AppContext` from the currently-focused window.
    /// Returns `Ok(None)` when there is no focused process ID (same condition
    /// the orchestrator methods use to bail early).
    pub fn from_focused(wm: &mut ConfiguredWindowManager) -> Result<Option<Self>> {
        let focused = wm.focused_window()?;
        let source_window_id = focused.id;
        let source_tile_index = focused.original_tile_index;
        let app_id = focused.app_id.unwrap_or_default();
        let title = focused.title.unwrap_or_default();
        let source_pid = focused.pid;
        let owner_pid = source_pid.map(ProcessId::get);
        let Some(owner_pid) = owner_pid else {
            return Ok(None);
        };

        Ok(Some(Self {
            source_window_id,
            source_tile_index,
            source_pid,
            owner_pid,
            app_id,
            title,
        }))
    }

    /// Resolve the ordered chain of app adapters for this context.
    pub fn resolve_chain(&self) -> Vec<Box<dyn AppAdapter>> {
        crate::engine::chain_resolver::resolve_app_chain(
            &self.app_id,
            self.owner_pid,
            &self.title,
        )
    }
}

// ── chain-walking helpers ────────────────────────────────────────────────────

/// Walk an adapter chain, calling `f` for each adapter and returning the first
/// `Some` value.  Returns `Ok(None)` when no adapter produces a result.
pub fn walk_chain<T, F>(chain: &[Box<dyn AppAdapter>], mut f: F) -> Result<Option<T>>
where
    F: FnMut(&dyn AppAdapter) -> Result<Option<T>>,
{
    walk_chain_iter(chain.len(), |i| f(chain[i].as_ref()))
}

/// Testable inner implementation of the chain-walk loop.
/// `count` is the number of items; `f` receives the index and returns
/// `Ok(Some(v))` to stop, `Ok(None)` to continue, or `Err(e)` to propagate.
pub fn walk_chain_iter<T, F>(count: usize, mut f: F) -> Result<Option<T>>
where
    F: FnMut(usize) -> Result<Option<T>>,
{
    for i in 0..count {
        if let Some(v) = f(i)? {
            return Ok(Some(v));
        }
    }
    Ok(None)
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_chain_returns_first_some() {
        // Simulates [None, Some(42), Some(99)] — should stop at index 1.
        let values: &[Option<i32>] = &[None, Some(42), Some(99)];
        let result = walk_chain_iter(values.len(), |i| Ok(values[i])).unwrap();
        assert_eq!(result, Some(42));
    }

    #[test]
    fn walk_chain_returns_none_if_all_none() {
        let values: &[Option<i32>] = &[None, None, None];
        let result = walk_chain_iter(values.len(), |i| Ok(values[i])).unwrap();
        assert_eq!(result, None);
    }
}
