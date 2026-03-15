use anyhow::{Context, Result};

use crate::engine::contract::{AppAdapter, TopologyHandler};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::engine::window_manager::{ConfiguredWindowManager, WindowRecord};
use crate::engine::chain_resolver::resolve_app_chain;
use crate::logging;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectionalProbeFocusMode {
    RestoreSource,
    KeepTarget,
}

pub(crate) fn focused_window_record(wm: &mut ConfiguredWindowManager) -> Result<WindowRecord> {
    let window = wm.focused_window()?;
    Ok(WindowRecord {
        id: window.id,
        app_id: window.app_id,
        title: window.title,
        pid: window.pid,
        is_focused: true,
        original_tile_index: window.original_tile_index,
    })
}

pub(crate) fn resolve_adapter_for_window(
    adapter_name: &str,
    window: &WindowRecord,
) -> Option<Box<dyn AppAdapter>> {
    let owner_pid = window.pid.map(ProcessId::get).unwrap_or(0);
    resolve_app_chain(
        window.app_id.as_deref().unwrap_or_default(),
        owner_pid,
        window.title.as_deref().unwrap_or_default(),
    )
    .into_iter()
    .find(|adapter| adapter.adapter_name() == adapter_name)
}

fn window_matches_adapter(adapter_name: &str, window: &WindowRecord) -> bool {
    resolve_adapter_for_window(adapter_name, window).is_some()
}

pub(crate) fn probe_directional_target(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
    source_window_id: u64,
    focus_mode: DirectionalProbeFocusMode,
) -> Result<Option<WindowRecord>> {
    if let Err(err) = wm.focus_direction(dir) {
        logging::debug(format!(
            "orchestrator: directional target probe failed dir={} err={:#}",
            dir, err
        ));
        return Ok(None);
    }

    let target = match focused_window_record(wm) {
        Ok(window) => window,
        Err(err) => {
            let _ = wm.focus_window_by_id(source_window_id);
            return Err(err.context("failed to read target window during directional probe"));
        }
    };

    if target.id == source_window_id {
        return Ok(None);
    }

    if matches!(focus_mode, DirectionalProbeFocusMode::RestoreSource) {
        wm.focus_window_by_id(source_window_id).with_context(|| {
            format!("failed to restore focus to window {}", source_window_id)
        })?;
    }
    Ok(Some(target))
}

pub(crate) fn probe_directional_target_for_adapter(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
    source_window_id: u64,
    adapter_name: &str,
    focus_mode: DirectionalProbeFocusMode,
) -> Result<Option<WindowRecord>> {
    let Some(target_window) =
        probe_directional_target(wm, dir, source_window_id, focus_mode)?
    else {
        return Ok(None);
    };
    if window_matches_adapter(adapter_name, &target_window) {
        return Ok(Some(target_window));
    }
    if matches!(focus_mode, DirectionalProbeFocusMode::KeepTarget) {
        let _ = wm.focus_window_by_id(source_window_id);
    }
    Ok(None)
}

pub(crate) fn probe_in_place_target_for_adapter(
    wm: &mut ConfiguredWindowManager,
    outer_chain: &[Box<dyn AppAdapter>],
    dir: Direction,
    source_window_id: u64,
    owner_pid: u32,
    app_id: &str,
    title: &str,
    adapter_name: &str,
) -> Result<Option<Box<dyn AppAdapter>>> {
    for outer in outer_chain {
        if !outer.capabilities().focus
            || !TopologyHandler::can_focus(outer.as_ref(), dir, owner_pid)?
        {
            continue;
        }
        TopologyHandler::focus(outer.as_ref(), dir, owner_pid)?;
        let focused_window_id = wm.focused_window()?.id;
        if focused_window_id != source_window_id {
            let _ = wm.focus_window_by_id(source_window_id);
            continue;
        }
        let target_app =
            resolve_app_chain(app_id, owner_pid, title)
                .into_iter()
                .find(|candidate| candidate.adapter_name() == adapter_name);
        if target_app.is_some() {
            return Ok(target_app);
        }
        let _ = TopologyHandler::focus(outer.as_ref(), dir.opposite(), owner_pid);
    }
    Ok(None)
}

pub(crate) fn restore_in_place_target_focus(
    outer_chain: &[Box<dyn AppAdapter>],
    dir: Direction,
    owner_pid: u32,
) {
    for outer in outer_chain {
        if outer.capabilities().focus
            && TopologyHandler::can_focus(outer.as_ref(), dir.opposite(), owner_pid)
                .unwrap_or(false)
        {
            let _ = TopologyHandler::focus(outer.as_ref(), dir.opposite(), owner_pid);
            break;
        }
    }
}
