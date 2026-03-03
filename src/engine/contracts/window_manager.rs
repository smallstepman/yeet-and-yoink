/// Marker module for window-manager-facing contracts.
///
/// App-facing contracts (`DeepApp`, move/merge decisions, capability flags)
/// live in `contracts/common.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowManagerContract;
