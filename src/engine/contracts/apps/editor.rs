use super::super::common::MergePreparation;

pub(crate) fn editor_frame_source(preparation: MergePreparation) -> Option<String> {
    match preparation {
        MergePreparation::EditorFrameSource { frame_id } => Some(frame_id),
        _ => None,
    }
}
