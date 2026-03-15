use anyhow::Result;

use crate::engine::contract::{AppAdapter, MergeExecutionMode, TopologyHandler};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::engine::window_manager::ConfiguredWindowManager;
use crate::engine::actions::probe::{
    probe_directional_target_for_adapter, probe_in_place_target_for_adapter,
    resolve_adapter_for_window, restore_in_place_target_focus, DirectionalProbeFocusMode,
};
use crate::logging;

pub(crate) fn attempt_passthrough_merge(
    wm: &mut ConfiguredWindowManager,
    app: &dyn AppAdapter,
    outer_chain: &[Box<dyn AppAdapter>],
    app_id: &str,
    title: &str,
    dir: Direction,
    source_window_id: u64,
    source_pid: Option<ProcessId>,
) -> Result<bool> {
    if !app.capabilities().merge {
        return Ok(false);
    }
    let adapter_name = app.adapter_name();
    let preparation = match TopologyHandler::prepare_merge(app, source_pid) {
        Ok(value) => value,
        Err(err) => {
            logging::debug(format!(
                "orchestrator: app passthrough merge prepare failed adapter={} err={:#}",
                adapter_name, err
            ));
            return Ok(false);
        }
    };

    match TopologyHandler::merge_execution_mode(app) {
        MergeExecutionMode::SourceFocused => {
            let Some(target_window) = probe_directional_target_for_adapter(
                wm,
                dir,
                source_window_id,
                adapter_name,
                DirectionalProbeFocusMode::RestoreSource,
            )?
            else {
                return Ok(false);
            };
            let preparation = TopologyHandler::augment_merge_preparation_for_target(
                app,
                preparation,
                Some(target_window.id),
            );

            match TopologyHandler::merge_into_target(
                app,
                dir,
                source_pid,
                target_window.pid,
                preparation,
            ) {
                Ok(()) => {
                    cleanup_merged_source_window(
                        wm,
                        source_window_id,
                        target_window.id,
                        adapter_name,
                    );
                    logging::debug(format!(
                        "orchestrator: app move handled by {adapter_name} decision=MergeSourceFocused"
                    ));
                    Ok(true)
                }
                Err(err) => {
                    logging::debug(format!(
                        "orchestrator: app passthrough merge failed adapter={} err={:#}",
                        adapter_name, err
                    ));
                    Ok(false)
                }
            }
        }
        MergeExecutionMode::TargetFocused => {
            if let Some(owner_pid) = source_pid.map(ProcessId::get) {
                if let Some(target_app) = probe_in_place_target_for_adapter(
                    wm,
                    outer_chain,
                    dir,
                    source_window_id,
                    owner_pid,
                    app_id,
                    title,
                    adapter_name,
                )? {
                    let preparation = TopologyHandler::augment_merge_preparation_for_target(
                        target_app.as_ref(),
                        preparation,
                        Some(source_window_id),
                    );

                    match TopologyHandler::merge_into_target(
                        target_app.as_ref(),
                        dir,
                        source_pid,
                        source_pid,
                        preparation,
                    ) {
                        Ok(()) => {
                            logging::debug(format!(
                                "orchestrator: app move handled by {adapter_name} decision=MergeTargetFocusedInPlace"
                            ));
                            return Ok(true);
                        }
                        Err(err) => {
                            restore_in_place_target_focus(outer_chain, dir, owner_pid);
                            logging::debug(format!(
                                "orchestrator: app passthrough merge failed adapter={} err={:#}",
                                adapter_name, err
                            ));
                            return Ok(false);
                        }
                    }
                }
            }

            let Some(target_window) = probe_directional_target_for_adapter(
                wm,
                dir,
                source_window_id,
                adapter_name,
                DirectionalProbeFocusMode::KeepTarget,
            )?
            else {
                return Ok(false);
            };
            let Some(target_app) =
                resolve_adapter_for_window(adapter_name, &target_window)
            else {
                let _ = wm.focus_window_by_id(source_window_id);
                return Ok(false);
            };
            let preparation = TopologyHandler::augment_merge_preparation_for_target(
                target_app.as_ref(),
                preparation,
                Some(target_window.id),
            );

            match TopologyHandler::merge_into_target(
                target_app.as_ref(),
                dir,
                source_pid,
                target_window.pid,
                preparation,
            ) {
                Ok(()) => {
                    cleanup_merged_source_window(
                        wm,
                        source_window_id,
                        target_window.id,
                        adapter_name,
                    );
                    logging::debug(format!(
                        "orchestrator: app move handled by {adapter_name} decision=MergeTargetFocused"
                    ));
                    Ok(true)
                }
                Err(err) => {
                    let _ = wm.focus_window_by_id(source_window_id);
                    logging::debug(format!(
                        "orchestrator: app passthrough merge failed adapter={} err={:#}",
                        adapter_name, err
                    ));
                    Ok(false)
                }
            }
        }
    }
}

pub(crate) fn cleanup_merged_source_window(
    wm: &mut ConfiguredWindowManager,
    source_window_id: u64,
    target_window_id: u64,
    adapter_name: &str,
) {
    if source_window_id == target_window_id {
        return;
    }
    if let Err(err) = wm.focus_window_by_id(target_window_id) {
        logging::debug(format!(
            "orchestrator: merge cleanup focus failed adapter={} target_window_id={} err={:#}",
            adapter_name, target_window_id, err
        ));
    }
    if let Err(err) = wm.close_window_by_id(source_window_id) {
        logging::debug(format!(
            "orchestrator: merge cleanup close failed adapter={} source_window_id={} err={:#}",
            adapter_name, source_window_id, err
        ));
    }
}
