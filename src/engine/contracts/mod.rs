pub mod apps;
pub mod common;
pub mod window_manager;

pub use common::{
    unsupported_operation, AdapterCapabilities, AppCapabilities, AppKind, DeepApp,
    MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
};
