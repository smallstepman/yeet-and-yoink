# Mux Topology Pattern Extraction Design

## Context

`src/adapters/terminal_multiplexers/tmux.rs` and `src/adapters/terminal_multiplexers/wezterm.rs` implement similar directional topology reasoning with different command backends.

Current duplication appears in:
- directional neighbor representation (per direction, focused pane anchored),
- move classification (`Passthrough` / `Internal` / `Rearrange` / `TearOut`),
- per-direction probing orchestration for decision making.

The user-approved direction is to **prioritize extending existing traits** (`TopologyHandler`, `TerminalMuxProvider`, `ChainResolver` where relevant) and **avoid introducing a new runtime abstraction layer**.

## Goals

1. Extract shared topology decision concepts into higher-level types.
2. Reuse shared decision logic from both tmux and wezterm implementations.
3. Preserve backend-specific command execution paths (CLI invocation, merge mechanics, bridge behavior).
4. Keep behavior parity while enabling broader unification later.

## Non-goals

- Replacing backend-specific command transport with a generic runtime.
- Normalizing all backend behaviors into identical command semantics.
- Refactoring unrelated adapter layers.

## Proposed Design

### 1) Add shared topology decision types in engine topology/contracts

Introduce shared topology structures used by `TopologyHandler` implementers:

- `DirectionalNeighbors`:
  - `west/east/north/south: Option<u64>`
  - helper methods:
    - `in_direction(dir) -> Option<u64>`
    - `has_perpendicular(dir) -> bool`

- `MoveSurface`:
  - `focused_pane_id: u64`
  - `pane_count: u32`
  - `neighbors: DirectionalNeighbors`
  - helper method:
    - `decision_for(dir) -> MoveDecision` with shared classification:
      - `pane_count <= 1` => `Passthrough`
      - neighbor in `dir` => `Internal`
      - perpendicular neighbor exists => `Rearrange`
      - otherwise => `TearOut`

These are topology concepts and will live with existing topology/contract abstractions, not backend-local modules.

### 2) Extend `TopologyHandler` with default helper path

Add default helper methods (non-breaking defaults):

- `directional_neighbors(&self, pid, focused_pane_id) -> Result<DirectionalNeighbors>`
- `move_surface(&self, pid) -> Result<MoveSurface>`

`move_decision` in mux adapters will use `move_surface(...).decision_for(dir)` to avoid duplicating classification logic.

### 3) Apply in tmux + wezterm

- **tmux**:
  - Keep `tmux` command execution unchanged.
  - Build `DirectionalNeighbors` using existing directional neighbor probes.
  - Build `MoveSurface` using `#{window_panes}` + focused pane id + neighbors.
  - Route `move_decision` through shared `MoveSurface`.

- **wezterm**:
  - Keep CLI + bridge + merge behavior unchanged.
  - Build `DirectionalNeighbors` from `get-pane-direction` probes.
  - Build `MoveSurface` from active-tab pane count + focused pane id + neighbors.
  - Route `move_decision` through shared `MoveSurface`.

### 4) Keep backend-specific mechanics explicit

The following stay backend-specific:
- `focus`
- `move_internal`
- `rearrange`
- `move_out`
- `merge_into_target` / merge target resolution details

Shared extraction covers topology reasoning only.

## Error Handling

- Neighbor probe failures that represent "no pane in direction" become `None` in `DirectionalNeighbors`.
- Structural failures (missing focused pane, broken pane list parse, invalid pid context) remain explicit `Err`.
- No silent fallback defaults beyond current backend behavior.

## Validation Plan

1. Add/adjust unit tests for shared `MoveSurface::decision_for`.
2. Add focused tests ensuring tmux/wezterm `move_decision` still return expected outputs for:
   - single-pane passthrough,
   - direct-neighbor internal move,
   - perpendicular rearrange,
   - edge tear-out.
3. Run full suite:
   - `cargo fmt --all --check`
   - `cargo test -q`

## Expected Outcome

Directional topology reasoning is centralized in shared type structures while tmux/wezterm remain explicit at execution boundaries, matching the user requirement to extend current traits rather than adding a new abstraction dimension.
