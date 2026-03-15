use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::engine::contract::{AppAdapter, TopologyHandler};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::engine::window_manager::{
    plan_tear_out, CapabilitySupport, ConfiguredWindowManager, WindowRecord,
};
use crate::logging;

pub(crate) fn execute_app_tear_out(
    wm: &mut ConfiguredWindowManager,
    app: &dyn AppAdapter,
    dir: Direction,
    owner_pid: u32,
    source_window_id: u64,
    source_tile_index: usize,
    source_pid: Option<ProcessId>,
    app_id: &str,
    decision_label: &str,
) -> Result<()> {
    let adapter_name = app.adapter_name();
    let pre_window_ids: BTreeSet<u64> = match wm.windows() {
        Ok(windows) => windows.into_iter().map(|window| window.id).collect(),
        Err(err) => {
            logging::debug(format!(
                "orchestrator: unable to snapshot pre-tearout windows err={:#}",
                err
            ));
            BTreeSet::new()
        }
    };
    let tear = TopologyHandler::move_out(app, dir, owner_pid)
        .with_context(|| format!("{adapter_name} move_out failed"))?;
    if let Some(command) = tear.spawn_command {
        wm.spawn(command)
            .with_context(|| format!("{adapter_name} tear-out spawn via wm failed"))?;
    }
    let tearout_window_id = match focus_tearout_window(
        wm,
        &pre_window_ids,
        source_window_id,
        source_pid,
        app_id,
    ) {
        Ok(window_id) => window_id,
        Err(err) => {
            logging::debug(format!(
                "orchestrator: unable to focus tear-out window adapter={} err={:#}",
                adapter_name, err
            ));
            None
        }
    };
    if let Err(err) = place_tearout_window(
        wm,
        dir,
        source_window_id,
        source_tile_index,
        tearout_window_id,
    ) {
        logging::debug(format!(
            "orchestrator: tear-out placement fallback failed adapter={} err={:#}",
            adapter_name, err
        ));
    }
    logging::debug(format!(
        "orchestrator: app move handled by {adapter_name} decision={decision_label}"
    ));
    Ok(())
}

pub(crate) fn focus_tearout_window(
    wm: &mut ConfiguredWindowManager,
    pre_window_ids: &BTreeSet<u64>,
    source_window_id: u64,
    source_pid: Option<ProcessId>,
    source_app_id: &str,
) -> Result<Option<u64>> {
    let target_window_id = wait_for_tearout_window_id(
        wm,
        pre_window_ids,
        source_window_id,
        source_pid,
        source_app_id,
    )?;
    if let Some(target_window_id) = target_window_id {
        if target_window_id != source_window_id {
            wm.focus_window_by_id(target_window_id)?;
            return Ok(Some(target_window_id));
        }
    }
    Ok(None)
}

pub(crate) fn wait_for_tearout_window_id(
    wm: &mut ConfiguredWindowManager,
    pre_window_ids: &BTreeSet<u64>,
    source_window_id: u64,
    source_pid: Option<ProcessId>,
    source_app_id: &str,
) -> Result<Option<u64>> {
    const ATTEMPTS: usize = 25;
    const DELAY: Duration = Duration::from_millis(40);

    for attempt in 0..ATTEMPTS {
        match wm.windows() {
            Ok(windows) => {
                if let Some(target_window_id) = select_tearout_window_id(
                    pre_window_ids,
                    &windows,
                    source_window_id,
                    source_pid,
                    source_app_id,
                ) {
                    if target_window_id != source_window_id {
                        return Ok(Some(target_window_id));
                    }
                }
            }
            Err(err) => {
                logging::debug(format!(
                    "orchestrator: tear-out post-window snapshot failed attempt={} err={:#}",
                    attempt + 1,
                    err
                ));
            }
        }

        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(DELAY);
        }
    }

    Ok(None)
}

pub(crate) fn select_tearout_window_id(
    pre_window_ids: &BTreeSet<u64>,
    windows: &[WindowRecord],
    source_window_id: u64,
    source_pid: Option<ProcessId>,
    source_app_id: &str,
) -> Option<u64> {
    let mut new_windows: Vec<&WindowRecord> = windows
        .iter()
        .filter(|window| !pre_window_ids.contains(&window.id))
        .collect();
    if new_windows.is_empty() {
        return windows
            .iter()
            .find(|window| window.is_focused && window.id != source_window_id)
            .map(|window| window.id);
    }
    new_windows.sort_by_key(|window| window.id);

    new_windows
        .iter()
        .find(|window| {
            window.pid == source_pid && window.app_id.as_deref() == Some(source_app_id)
        })
        .map(|window| window.id)
        .or_else(|| {
            new_windows
                .iter()
                .find(|window| window.pid == source_pid)
                .map(|window| window.id)
        })
        .or_else(|| {
            new_windows
                .iter()
                .find(|window| window.app_id.as_deref() == Some(source_app_id))
                .map(|window| window.id)
        })
        .or_else(|| {
            new_windows
                .iter()
                .find(|window| window.is_focused)
                .map(|window| window.id)
        })
        .or_else(|| new_windows.first().map(|window| window.id))
}

pub(crate) fn place_tearout_window(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
    source_window_id: u64,
    source_tile_index: usize,
    target_window_id: Option<u64>,
) -> Result<()> {
    if let Some(target_window_id) = target_window_id.filter(|id| *id != source_window_id) {
        wm.focus_window_by_id(target_window_id)?;
    }

    let focused_window_id = wm.focused_window()?.id;
    if focused_window_id == source_window_id {
        return Ok(());
    }

    match plan_tear_out(wm.capabilities(), dir) {
        CapabilitySupport::Native => wm.move_direction(dir),
        CapabilitySupport::Unsupported => Ok(()),
        CapabilitySupport::Composed => {
            let adapter_name = wm.adapter_name();
            wm.tear_out_composer_mut()
                .with_context(|| {
                    format!(
                        "configured wm '{}' is missing a tear-out composer for {dir}",
                        adapter_name
                    )
                })?
                .compose_tear_out(dir, source_tile_index)
        }
    }
}
