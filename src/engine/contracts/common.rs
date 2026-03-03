use std::any::Any;

use anyhow::{anyhow, Result};

use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;

/// What the app wants to do for a move operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDecision {
    /// Swap/move internally within the app.
    Internal,
    /// Rearrange panes: no neighbor in move direction, but panes exist
    /// in other directions. Reorganize layout (e.g. horizontal → vertical).
    Rearrange,
    /// At the edge with multiple splits along the move axis — tear the buffer out.
    TearOut,
    /// Nothing to do internally, fall through to the compositor.
    Passthrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKind {
    Browser,
    Editor,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeExecutionMode {
    SourceFocused,
    TargetFocused,
}

pub struct MergePreparation {
    payload: Option<Box<dyn Any + Send>>,
}

impl MergePreparation {
    pub fn none() -> Self {
        Self { payload: None }
    }

    pub fn with_payload<T>(payload: T) -> Self
    where
        T: Send + 'static,
    {
        Self {
            payload: Some(Box::new(payload)),
        }
    }

    pub fn into_payload<T>(self) -> Option<T>
    where
        T: Send + 'static,
    {
        self.payload
            .and_then(|payload| payload.downcast::<T>().ok())
            .map(|typed| *typed)
    }

    pub fn map_payload<T>(self, update: impl FnOnce(T) -> T) -> Self
    where
        T: Send + 'static,
    {
        let Some(payload) = self.payload else {
            return self;
        };
        match payload.downcast::<T>() {
            Ok(typed) => Self::with_payload(update(*typed)),
            Err(payload) => Self {
                payload: Some(payload),
            },
        }
    }
}

impl Default for MergePreparation {
    fn default() -> Self {
        Self::none()
    }
}

/// Result of tearing a buffer/pane out of an app.
pub struct TearResult {
    /// Command to spawn the torn-out content as a new window.
    /// None if the app already created the window itself.
    pub spawn_command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppCapabilities {
    pub probe: bool,
    pub focus: bool,
    pub move_internal: bool,
    pub resize_internal: bool,
    pub rearrange: bool,
    pub tear_out: bool,
    pub merge: bool,
}

impl AppCapabilities {
    pub const fn none() -> Self {
        Self {
            probe: false,
            focus: false,
            move_internal: false,
            resize_internal: false,
            rearrange: false,
            tear_out: false,
            merge: false,
        }
    }
}

pub type AdapterCapabilities = AppCapabilities;

/// Trait for apps that support deep focus/move integration with the current WM domain.
pub trait DeepApp: Send {
    /// Human-readable adapter name used in diagnostics.
    fn adapter_name(&self) -> &'static str;

    /// High-level app category used by domain resolution policy.
    fn kind(&self) -> AppKind;

    /// Explicit capability declaration used by orchestrator routing.
    fn capabilities(&self) -> AdapterCapabilities;

    /// Whether the app can navigate internally in this direction.
    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool>;

    /// Navigate internally in the given direction.
    fn focus(&self, dir: Direction, pid: u32) -> Result<()>;

    /// Decide what to do for a move operation in this direction.
    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision>;

    /// Swap/move the current buffer internally.
    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()>;

    /// Whether the app can resize internally in this direction.
    fn can_resize(&self, _dir: Direction, _grow: bool, _pid: u32) -> Result<bool> {
        Ok(false)
    }

    /// Resize internally in the given direction.
    fn resize_internal(&self, _dir: Direction, _grow: bool, _step: i32, _pid: u32) -> Result<()> {
        Err(unsupported_operation(
            self.adapter_name(),
            "resize_internal",
        ))
    }

    /// Rearrange panes: move the current pane to `dir` by reorganizing the layout.
    /// e.g. [A|B*] move north → [B*-A] (horizontal to vertical).
    fn rearrange(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_operation(self.adapter_name(), "rearrange"))
    }

    /// Tear the current buffer/pane out, returning spawn info for a new window.
    fn move_out(&self, dir: Direction, pid: u32) -> Result<TearResult>;

    /// Merge the current window's content into the adjacent same-app window,
    /// and close the source. Called while the source window is still focused.
    /// `dir` is the direction toward the merge target.
    fn merge_into(&self, _dir: Direction, _source_pid: u32) -> Result<()> {
        Err(unsupported_operation(self.adapter_name(), "merge_into"))
    }

    /// Whether merge should execute while source or target window is focused.
    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::SourceFocused
    }

    /// Capture source-side merge state before focus moves to target window.
    fn prepare_merge(&self, _source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        Ok(MergePreparation::none())
    }

    /// Allow adapters to enrich source merge preparation once a concrete target is known.
    fn augment_merge_preparation_for_target(
        &self,
        preparation: MergePreparation,
        _target_window_id: Option<u64>,
    ) -> MergePreparation {
        preparation
    }

    /// Merge source content into target window context.
    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<ProcessId>,
        _target_pid: Option<ProcessId>,
        _preparation: MergePreparation,
    ) -> Result<()> {
        self.merge_into(dir, legacy_pid(source_pid))
    }
}

pub fn unsupported_operation(adapter: &str, operation: &str) -> anyhow::Error {
    anyhow!(
        "adapter '{}' does not support operation '{}'",
        adapter,
        operation
    )
}

fn legacy_pid(pid: Option<ProcessId>) -> u32 {
    pid.map(ProcessId::get).unwrap_or(0)
}
