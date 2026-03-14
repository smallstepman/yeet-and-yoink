use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use niri_ipc::socket::Socket;
use niri_ipc::{Action, Request, Response, SizeChange, Window, Workspace, WorkspaceReferenceArg};
use serde::{Deserialize, Serialize};
use std::any::TypeId;

use crate::adapters::window_managers::{
    ConfiguredWindowManager, NiriAdapter, WindowCycleProvider, WindowCycleRequest,
    WindowManagerDomainFactory, WindowManagerFeatures, WindowManagerSpec,
};
use crate::config::WmBackend;
use crate::engine::domain::PaneState;
use crate::engine::domain::{decode_native_window_ref, encode_native_window_ref};
use crate::engine::domain::{
    DomainLeafSnapshot, DomainSnapshot, ErasedDomain, TilingDomain, TopologyModifierImpl,
    TopologyProvider,
};
use crate::engine::topology::Direction;
use crate::engine::topology::{DomainId, LeafId, Rect};
use crate::logging;

pub struct Niri {
    socket: Socket,
}

pub struct NiriSpec;

pub static NIRI_SPEC: NiriSpec = NiriSpec;

struct NiriDomainFactory;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SummonOrigin {
    workspace_id: u64,
    output: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SummonState {
    windows: HashMap<u64, SummonOrigin>,
}

impl WindowManagerSpec for NiriSpec {
    fn backend(&self) -> WmBackend {
        WmBackend::Niri
    }

    fn name(&self) -> &'static str {
        NiriAdapter::NAME
    }

    fn connect(&self) -> Result<ConfiguredWindowManager> {
        let mut features = WindowManagerFeatures::default();
        features.domain_factory = Some(Box::new(NiriDomainFactory));
        features.window_cycle = Some(Box::new(NiriAdapter::connect()?));
        Ok(ConfiguredWindowManager::new(
            Box::new(NiriAdapter::connect()?),
            features,
        ))
    }
}

impl WindowManagerDomainFactory for NiriDomainFactory {
    fn create_domain(&self, domain_id: DomainId) -> Result<Box<dyn ErasedDomain>> {
        Ok(Box::new(NiriDomainPlugin::connect(domain_id)?))
    }
}

impl Niri {
    pub fn connect() -> Result<Self> {
        let _span = tracing::debug_span!("niri.connect").entered();
        logging::debug("niri: connecting to IPC socket");
        let socket = Socket::connect().context("failed to connect to niri IPC socket")?;
        logging::debug("niri: IPC socket connected");
        Ok(Self { socket })
    }

    fn send_action(&mut self, action: Action) -> Result<()> {
        let _span = tracing::debug_span!("niri.send_action", action = ?action).entered();
        logging::debug(format!("niri: action request = {:?}", action));
        let reply = self
            .socket
            .send(Request::Action(action))
            .context("failed to send niri action")?;
        match reply {
            Ok(Response::Handled) => {
                logging::debug("niri: action handled");
                Ok(())
            }
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn focused_window(&mut self) -> Result<Window> {
        let _span = tracing::debug_span!("niri.focused_window").entered();
        logging::debug("niri: requesting focused window");
        let reply = self
            .socket
            .send(Request::FocusedWindow)
            .context("failed to send FocusedWindow request")?;
        match reply {
            Ok(Response::FocusedWindow(Some(w))) => {
                logging::debug(format!(
                    "niri: focused window id={} app_id={:?} pid={:?}",
                    w.id, w.app_id, w.pid
                ));
                Ok(w)
            }
            Ok(Response::FocusedWindow(None)) => bail!("no focused window"),
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn windows(&mut self) -> Result<Vec<Window>> {
        let reply = self
            .socket
            .send(Request::Windows)
            .context("failed to send Windows request")?;
        match reply {
            Ok(Response::Windows(windows)) => Ok(windows),
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn workspaces(&mut self) -> Result<Vec<Workspace>> {
        let reply = self
            .socket
            .send(Request::Workspaces)
            .context("failed to send Workspaces request")?;
        match reply {
            Ok(Response::Workspaces(workspaces)) => Ok(workspaces),
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn focus_direction(&mut self, dir: Direction) -> Result<()> {
        let _span = tracing::debug_span!("niri.focus_direction", ?dir).entered();
        let action = match dir {
            Direction::West => Action::FocusColumnLeft {},
            Direction::East => Action::FocusColumnRight {},
            Direction::North => Action::FocusWindowOrWorkspaceUp {},
            Direction::South => Action::FocusWindowOrWorkspaceDown {},
        };
        self.send_action(action)
    }

    pub fn move_column(&mut self, dir: Direction) -> Result<()> {
        let action = match dir {
            Direction::West => Action::MoveColumnLeft {},
            Direction::East => Action::MoveColumnRight {},
            _ => return Ok(()),
        };
        self.send_action(action)
    }

    pub fn move_direction(&mut self, dir: Direction) -> Result<()> {
        let action = match dir {
            Direction::West => Action::ConsumeOrExpelWindowLeft { id: None },
            Direction::East => Action::ConsumeOrExpelWindowRight { id: None },
            Direction::North => Action::MoveWindowUpOrToWorkspaceUp {},
            Direction::South => Action::MoveWindowDownOrToWorkspaceDown {},
        };
        self.send_action(action)
    }

    pub fn resize_window(&mut self, dir: Direction, grow: bool, step: i32) -> Result<()> {
        let magnitude = step.abs().max(1);
        let directional_delta = match dir {
            Direction::East | Direction::South => magnitude,
            Direction::West | Direction::North => -magnitude,
        };
        let delta = if grow {
            directional_delta
        } else {
            -directional_delta
        };
        let change = SizeChange::AdjustFixed(delta);
        let action = match dir {
            Direction::West | Direction::East => Action::SetWindowWidth { id: None, change },
            Direction::North | Direction::South => Action::SetWindowHeight { id: None, change },
        };
        self.send_action(action)
    }

    pub fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        self.send_action(Action::Spawn { command })
    }

    pub fn spawn_sh(&mut self, command: String) -> Result<()> {
        self.send_action(Action::SpawnSh { command })
    }

    pub fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.send_action(Action::FocusWindow { id })
    }

    pub fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.send_action(Action::CloseWindow { id: Some(id) })
    }

    pub fn move_window_to_workspace(
        &mut self,
        window_id: u64,
        reference: WorkspaceReferenceArg,
        focus: bool,
    ) -> Result<()> {
        self.send_action(Action::MoveWindowToWorkspace {
            window_id: Some(window_id),
            reference,
            focus,
        })
    }

    pub fn move_window_to_monitor(&mut self, id: u64, output: String) -> Result<()> {
        self.send_action(Action::MoveWindowToMonitor {
            id: Some(id),
            output,
        })
    }

    /// After a new tile is created (and focused) to the right of the original,
    /// consume it into the original's column and position it right next to
    /// the original tile (below for south, above for north).
    ///
    /// `original_tile_idx` is the 1-based tile index of the original window
    /// in its column (from `layout.pos_in_scrolling_layout`).
    pub fn consume_into_column_and_move(
        &mut self,
        dir: Direction,
        original_tile_idx: usize,
    ) -> Result<()> {
        // The new tile is in its own column to the right of the original.
        // Consume it leftward into the original's column (goes to bottom).
        self.send_action(Action::ConsumeOrExpelWindowLeft { id: None })?;

        // B is now at the bottom of the column. Query its position.
        let new_window = self.focused_window()?;
        let new_tile_idx = new_window
            .layout
            .pos_in_scrolling_layout
            .map(|(_, t)| t)
            .unwrap_or(1);

        // Target position: right below the original for south, at the
        // original's position (pushing it down) for north.
        let target_idx = match dir {
            Direction::South => original_tile_idx + 1,
            Direction::North => original_tile_idx,
            _ => new_tile_idx, // no-op
        };

        // Move up from bottom to target.
        for _ in 0..new_tile_idx.saturating_sub(target_idx) {
            self.send_action(Action::MoveWindowUp {})?;
        }
        Ok(())
    }
}

impl WindowCycleProvider for NiriAdapter {
    fn focus_or_cycle(&mut self, request: &WindowCycleRequest) -> Result<()> {
        if request.new {
            let spawn = request
                .spawn
                .as_ref()
                .context("--new requires --spawn '<command>'")?;
            return self.inner.spawn_sh(spawn.clone());
        }

        let windows = self.inner.windows()?;
        let focused_id = windows
            .iter()
            .find(|window| window.is_focused)
            .map(|window| window.id);

        let app_id = request.app_id.as_deref();
        let title = request.title.as_deref();
        let mut matches: Vec<Window> = windows
            .iter()
            .filter(|window| window_matches(window, app_id, title))
            .cloned()
            .collect();

        if matches.is_empty() {
            if let Some(spawn) = request.spawn.as_ref() {
                return self.inner.spawn_sh(spawn.clone());
            }
            bail!("no matching windows found and no --spawn provided");
        }

        matches.sort_by(|a, b| focus_sort_key(b).cmp(&focus_sort_key(a)));
        let target_idx = focused_id
            .and_then(|id| matches.iter().position(|window| window.id == id))
            .map(|idx| (idx + 1) % matches.len())
            .unwrap_or(0);
        let target = matches[target_idx].clone();

        if request.summon {
            summon_or_return(&mut self.inner, &target, &windows)?;
            return Ok(());
        }

        self.inner.focus_window_by_id(target.id)
    }
}

fn window_matches(window: &Window, app_id: Option<&str>, title: Option<&str>) -> bool {
    if let Some(app_id) = app_id {
        if window.app_id.as_deref() != Some(app_id) {
            return false;
        }
    }
    if let Some(title) = title {
        let Some(window_title) = window.title.as_deref() else {
            return false;
        };
        if !window_title.to_lowercase().contains(&title.to_lowercase()) {
            return false;
        }
    }
    true
}

fn focus_sort_key(window: &Window) -> (u64, u32, u64) {
    let (secs, nanos) = window
        .focus_timestamp
        .map(|ts| (ts.secs, ts.nanos))
        .unwrap_or((0, 0));
    (secs, nanos, window.id)
}

fn summon_or_return(niri: &mut Niri, target: &Window, all_windows: &[Window]) -> Result<()> {
    let workspaces = niri.workspaces()?;
    let focused_workspace = workspaces
        .iter()
        .find(|workspace| workspace.is_focused)
        .cloned()
        .context("no focused workspace found")?;

    let workspaces_by_id: HashMap<u64, _> = workspaces
        .iter()
        .map(|workspace| (workspace.id, workspace))
        .collect();
    let mut state = load_summon_state()?;

    let live_window_ids: HashSet<u64> = all_windows.iter().map(|window| window.id).collect();
    state
        .windows
        .retain(|window_id, _| live_window_ids.contains(window_id));

    if target.is_focused {
        if let Some(origin) = state.windows.remove(&target.id) {
            niri.move_window_to_workspace(
                target.id,
                WorkspaceReferenceArg::Id(origin.workspace_id),
                false,
            )?;
            if let Some(output) = origin.output {
                niri.move_window_to_monitor(target.id, output)?;
            }
            save_summon_state(&state)?;
            return Ok(());
        }
    }

    if target.workspace_id != Some(focused_workspace.id) {
        state.windows.entry(target.id).or_insert_with(|| {
            let origin_output = target
                .workspace_id
                .and_then(|workspace_id| workspaces_by_id.get(&workspace_id))
                .and_then(|workspace| workspace.output.clone());
            SummonOrigin {
                workspace_id: target.workspace_id.unwrap_or(focused_workspace.id),
                output: origin_output,
            }
        });

        niri.move_window_to_workspace(
            target.id,
            WorkspaceReferenceArg::Id(focused_workspace.id),
            false,
        )?;
        if let Some(output) = focused_workspace.output.clone() {
            niri.move_window_to_monitor(target.id, output)?;
        }
        save_summon_state(&state)?;
    }

    niri.focus_window_by_id(target.id)
}

fn summon_state_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("yeet-and-yoink").join("summon-state.json")
}

fn load_summon_state() -> Result<SummonState> {
    let path = summon_state_path();
    if !path.exists() {
        return Ok(SummonState::default());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read summon state file: {}", path.display()))?;
    let state: SummonState = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse summon state file: {}", path.display()))?;
    Ok(state)
}

fn save_summon_state(state: &SummonState) -> Result<()> {
    let path = summon_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create state directory: {}", parent.display()))?;
    }

    let serialized = serde_json::to_string(state).context("failed to serialize summon state")?;
    fs::write(&path, serialized)
        .with_context(|| format!("failed to write summon state file: {}", path.display()))?;
    Ok(())
}

pub struct NiriDomainPlugin {
    domain_id: DomainId,
    inner: NiriAdapter,
}

impl NiriDomainPlugin {
    pub fn connect(domain_id: DomainId) -> Result<Self> {
        Ok(Self {
            domain_id,
            inner: NiriAdapter::connect()?,
        })
    }

    fn snapshot_leaves(&mut self) -> Result<Vec<DomainLeafSnapshot>> {
        let windows = self.inner.windows()?;
        Ok(windows
            .iter()
            .enumerate()
            .map(|(index, window)| {
                let x = (index as i32) * 1000;
                DomainLeafSnapshot {
                    id: (index as LeafId) + 1,
                    native_id: encode_native_window_ref(window.id, window.pid),
                    rect: Rect {
                        x,
                        y: 0,
                        w: 900,
                        h: 900,
                    },
                    focused: window.is_focused,
                }
            })
            .collect())
    }
}

impl TopologyProvider for NiriDomainPlugin {
    type NativeId = Vec<u8>;
    type Error = anyhow::Error;

    fn domain_name(&self) -> &'static str {
        "niri"
    }

    fn rect(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 10000,
            h: 10000,
        }
    }

    fn fetch_layout(&mut self) -> Result<(), Self::Error> {
        let _ = self.inner.windows()?;
        Ok(())
    }
}

impl TopologyModifierImpl for NiriDomainPlugin {
    fn focus_impl(&mut self, native_id: &Self::NativeId) -> Result<(), Self::Error> {
        let target = decode_native_window_ref(native_id).context("invalid niri native id")?;
        self.inner.focus_window_by_id(target.window_id)
    }

    fn move_impl(&mut self, native_id: &Self::NativeId, dir: Direction) -> Result<(), Self::Error> {
        let target = decode_native_window_ref(native_id).context("invalid niri native id")?;
        self.inner.focus_window_by_id(target.window_id)?;
        self.inner.move_direction(dir)
    }

    fn tear_off_impl(&mut self, _id: &Self::NativeId) -> Result<Box<dyn PaneState>, Self::Error> {
        Err(anyhow!("niri domain does not support payload tear-off"))
    }

    fn merge_in_impl(
        &mut self,
        _target: &Self::NativeId,
        _dir: Direction,
        _payload: Box<dyn PaneState>,
    ) -> Result<Self::NativeId, Self::Error> {
        Err(anyhow!("niri domain does not support payload merge-in"))
    }
}

impl TilingDomain for NiriDomainPlugin {
    fn supported_payload_types(&self) -> &'static [TypeId] {
        &[]
    }
}

impl ErasedDomain for NiriDomainPlugin {
    fn domain_id(&self) -> DomainId {
        self.domain_id
    }

    fn domain_name(&self) -> &'static str {
        "niri"
    }

    fn rect(&self) -> Rect {
        TopologyProvider::rect(self)
    }

    fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
        Ok(DomainSnapshot {
            domain_id: self.domain_id,
            rect: TopologyProvider::rect(self),
            leaves: self.snapshot_leaves()?,
        })
    }

    fn supported_payload_types(&self) -> Vec<TypeId> {
        vec![]
    }

    fn tear_off(&mut self, native_id: &[u8]) -> Result<Box<dyn PaneState>> {
        self.tear_off_impl(&native_id.to_vec())
    }

    fn merge_in(
        &mut self,
        target_native_id: &[u8],
        dir: Direction,
        payload: Box<dyn PaneState>,
    ) -> Result<Vec<u8>> {
        self.merge_in_impl(&target_native_id.to_vec(), dir, payload)
    }
}
