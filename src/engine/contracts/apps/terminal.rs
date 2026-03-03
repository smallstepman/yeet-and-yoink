use super::super::common::MergePreparation;

pub(crate) fn terminal_mux_source(preparation: MergePreparation) -> Option<(u64, Option<u64>)> {
    match preparation {
        MergePreparation::TerminalMuxSourcePane {
            pane_id,
            target_window_id,
        } => Some((pane_id, target_window_id)),
        _ => None,
    }
}

pub(crate) fn with_target_window_hint(
    preparation: MergePreparation,
    target_window_id: Option<u64>,
) -> MergePreparation {
    match preparation {
        MergePreparation::TerminalMuxSourcePane { pane_id, .. } => {
            MergePreparation::TerminalMuxSourcePane {
                pane_id,
                target_window_id,
            }
        }
        other => other,
    }
}
