pub(crate) mod context;
pub(crate) mod focus;
pub(crate) mod merge;
pub(crate) mod movement;
pub(crate) mod probe;
pub(crate) mod resize;
pub(crate) mod tearout;
// Re-exported for use throughout the actions module and beyond; walk_chain
// is unused until later tasks consume it.
#[allow(unused_imports)]
pub(crate) use context::{AppContext, walk_chain};
pub(crate) use focus::*;
pub(crate) use merge::*;
pub(crate) use movement::*;
pub(crate) use probe::*;
pub(crate) use resize::*;
pub(crate) use tearout::*;

// ---------------------------------------------------------------------------
// Orchestrator — migrated from engine::orchestrator
// ---------------------------------------------------------------------------

use std::collections::BTreeMap;

use anyhow::Result;

use crate::engine::domain::ErasedDomain;
use crate::engine::domain::{domain_id_for_window, encode_native_window_ref};
use crate::engine::domain::{PayloadRegistry, TransferOutcome, TransferPipeline};
use crate::engine::topology::Direction;
use crate::engine::topology::{DomainId, GlobalLeaf, Rect};
use crate::engine::window_manager::{ConfiguredWindowManager, ResizeIntent, ResizeKind, WindowRecord};
use crate::logging;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Focus,
    Move,
    Resize { grow: bool, step: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionRequest {
    pub kind: ActionKind,
    pub direction: Direction,
}

impl ActionRequest {
    pub const fn new(kind: ActionKind, direction: Direction) -> Self {
        Self { kind, direction }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDecision {
    SameDomain,
    CrossDomain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingError {
    UnsupportedTransfer {
        source_domain: DomainId,
        target_domain: DomainId,
    },
}

pub struct Orchestrator {
    payload_registry: PayloadRegistry,
    domains: BTreeMap<DomainId, Box<dyn ErasedDomain>>,
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self {
            payload_registry: PayloadRegistry::default(),
            domains: BTreeMap::new(),
        }
    }
}

impl Orchestrator {
    pub fn register_domain(&mut self, domain: Box<dyn ErasedDomain>) {
        self.domains.insert(domain.domain_id(), domain);
    }

    pub fn execute(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        request: ActionRequest,
    ) -> Result<()> {
        self.execute_session(wm, request)
    }

    fn execute_session(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        request: ActionRequest,
    ) -> Result<()> {
        match request.kind {
            ActionKind::Focus => self.execute_focus_session(wm, request.direction),
            ActionKind::Move => self.execute_move_session(wm, request.direction),
            ActionKind::Resize { grow, step } => {
                self.execute_resize_session(wm, request.direction, grow, step)
            }
        }
    }

    pub fn execute_focus(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        dir: Direction,
    ) -> Result<()> {
        self.execute_focus_session(wm, dir)
    }

    fn execute_focus_session(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        dir: Direction,
    ) -> Result<()> {
        let _span = tracing::debug_span!("orchestrator.execute_focus", ?dir).entered();
        let fallback_dir = dir.into();
        if attempt_focused_app_focus(wm, fallback_dir)? {
            return Ok(());
        }
        wm.focus_direction(fallback_dir)
    }

    pub fn execute_move(&mut self, wm: &mut ConfiguredWindowManager, dir: Direction) -> Result<()> {
        self.execute_move_session(wm, dir)
    }

    fn execute_move_session(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        dir: Direction,
    ) -> Result<()> {
        let fallback_dir = dir.into();
        if attempt_focused_app_move(wm, fallback_dir)? {
            return Ok(());
        }

        let focused = focused_window_record(wm)?;
        let Some(target_window) = probe_directional_target(
            wm,
            dir,
            focused.id,
            DirectionalProbeFocusMode::RestoreSource,
        )?
        else {
            return wm.move_direction(fallback_dir);
        };
        let focused_leaf = Self::leaf_from_window(&focused, 1);
        let target_leaf = Self::leaf_from_window(&target_window, 2);

        match self.route(&focused_leaf, &target_leaf) {
            RoutingDecision::SameDomain => {
                if self
                    .attempt_same_domain_transfer(&focused_leaf, &target_leaf, dir)
                    .unwrap_or(false)
                {
                    Ok(())
                } else {
                    wm.move_direction(fallback_dir)
                }
            }
            RoutingDecision::CrossDomain => {
                if self
                    .attempt_cross_domain_transfer(&focused_leaf, &target_leaf, dir)
                    .unwrap_or(false)
                {
                    Ok(())
                } else {
                    let err = RoutingError::UnsupportedTransfer {
                        source_domain: focused_leaf.domain,
                        target_domain: target_leaf.domain,
                    };
                    logging::debug(format!("orchestrator: {:?}", err));
                    wm.move_direction(fallback_dir)
                }
            }
        }
    }

    fn leaf_from_window(window: &WindowRecord, leaf_id: u64) -> GlobalLeaf {
        let domain = domain_id_for_window(
            window.app_id.as_deref(),
            window.pid,
            window.title.as_deref(),
        );
        GlobalLeaf {
            id: leaf_id,
            domain,
            native_id: encode_native_window_ref(window.id, window.pid),
            rect: Rect {
                x: leaf_id as i32,
                y: 0,
                w: 1,
                h: 1,
            },
        }
    }

    pub fn execute_resize(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        dir: Direction,
        grow: bool,
        step: i32,
    ) -> Result<()> {
        self.execute_resize_session(wm, dir, grow, step)
    }

    fn execute_resize_session(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        dir: Direction,
        grow: bool,
        step: i32,
    ) -> Result<()> {
        if attempt_focused_app_resize(wm, dir, grow, step.max(1))? {
            return Ok(());
        }
        let intent = ResizeIntent::new(
            dir.into(),
            if grow {
                ResizeKind::Grow
            } else {
                ResizeKind::Shrink
            },
            step.max(1),
        );
        wm.resize_with_intent(intent)
    }

    pub fn route(&self, source: &GlobalLeaf, target: &GlobalLeaf) -> RoutingDecision {
        if source.domain == target.domain {
            RoutingDecision::SameDomain
        } else {
            RoutingDecision::CrossDomain
        }
    }

    fn attempt_cross_domain_transfer(
        &mut self,
        source: &GlobalLeaf,
        target: &GlobalLeaf,
        dir: Direction,
    ) -> Result<bool> {
        let Some(mut source_domain) = self.domains.remove(&source.domain) else {
            return Ok(false);
        };
        let Some(target_domain) = self.domains.get_mut(&target.domain) else {
            self.domains.insert(source.domain, source_domain);
            return Ok(false);
        };

        let pipeline = TransferPipeline::new(&self.payload_registry);
        let outcome = pipeline.transfer_between(
            source_domain.as_mut(),
            &source.native_id,
            target_domain.as_mut(),
            &target.native_id,
            dir,
        );
        self.domains.insert(source.domain, source_domain);

        match outcome {
            Ok(TransferOutcome::Applied { merged_native_id }) => {
                logging::debug(format!(
                    "orchestrator: cross-domain transfer applied source_domain={} target_domain={} merged_native_id_len={}",
                    source.domain,
                    target.domain,
                    merged_native_id.len()
                ));
                Ok(true)
            }
            Ok(TransferOutcome::Fallback { reason }) => {
                logging::debug(format!(
                    "orchestrator: cross-domain transfer fallback source_domain={} target_domain={} reason={}",
                    source.domain, target.domain, reason
                ));
                Ok(false)
            }
            Err(err) => {
                logging::debug(format!(
                    "orchestrator: cross-domain transfer error source_domain={} target_domain={} err={:#}",
                    source.domain, target.domain, err
                ));
                Ok(false)
            }
        }
    }

    fn attempt_same_domain_transfer(
        &mut self,
        source: &GlobalLeaf,
        target: &GlobalLeaf,
        dir: Direction,
    ) -> Result<bool> {
        let Some(domain) = self.domains.get_mut(&source.domain) else {
            return Ok(false);
        };
        if domain.supported_payload_types().is_empty() {
            return Ok(false);
        }

        let pipeline = TransferPipeline::new(&self.payload_registry);
        let outcome =
            pipeline.transfer_within(domain.as_mut(), &source.native_id, &target.native_id, dir);

        match outcome {
            Ok(TransferOutcome::Applied { merged_native_id }) => {
                logging::debug(format!(
                    "orchestrator: same-domain transfer applied domain={} merged_native_id_len={}",
                    source.domain,
                    merged_native_id.len()
                ));
                Ok(true)
            }
            Ok(TransferOutcome::Fallback { reason }) => {
                logging::debug(format!(
                    "orchestrator: same-domain transfer fallback domain={} reason={}",
                    source.domain, reason
                ));
                Ok(false)
            }
            Err(err) => {
                logging::debug(format!(
                    "orchestrator: same-domain transfer error domain={} err={:#}",
                    source.domain, err
                ));
                Ok(false)
            }
        }
    }
}
