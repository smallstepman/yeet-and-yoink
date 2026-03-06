# Mux Topology Pattern Extraction Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extract shared tmux/wezterm directional topology decision patterns into higher-level shared types by extending existing contracts.

**Architecture:** Introduce shared directional-neighbor and move-surface value types in engine topology/contracts, then route tmux and wezterm `move_decision` through those shared structures. Keep backend command execution and merge mechanics backend-specific, and only unify decision/probing logic.

**Tech Stack:** Rust, anyhow, existing adapter contracts (`TopologyHandler`, `TerminalMultiplexerProvider`), cargo test.

---

**Skill refs:** @test-driven-development @verification-before-completion

### Task 1: Add shared topology decision value types

**Files:**
- Modify: `src/engine/topology.rs`
- Modify: `src/engine/contract.rs`
- Test: `src/engine/topology.rs` (module tests)

**Step 1: Write the failing test**

Add tests in `src/engine/topology.rs` for:
- `DirectionalNeighbors::in_direction`
- `DirectionalNeighbors::has_perpendicular`
- `MoveSurface::decision_for`

Example test skeleton:

```rust
#[test]
fn move_surface_decision_internal_when_neighbor_exists() {
    let surface = MoveSurface {
        focused_pane_id: 1,
        pane_count: 2,
        neighbors: DirectionalNeighbors {
            west: None,
            east: Some(2),
            north: None,
            south: None,
        },
    };
    assert!(matches!(surface.decision_for(Direction::East), MoveDecision::Internal));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q move_surface_decision_internal_when_neighbor_exists`
Expected: FAIL (types/methods not yet defined).

**Step 3: Write minimal implementation**

In `src/engine/topology.rs`, add:
- `DirectionalNeighbors` struct with 4 optional directions.
- helper methods:
  - `in_direction(&self, dir: Direction) -> Option<u64>`
  - `has_perpendicular(&self, dir: Direction) -> bool`
- `MoveSurface` struct:
  - `focused_pane_id: u64`
  - `pane_count: u32`
  - `neighbors: DirectionalNeighbors`
- `decision_for(&self, dir: Direction) -> MoveDecision`.

In `src/engine/contract.rs`, import and use new shared types where needed by default helper methods.

**Step 4: Run tests to verify pass**

Run: `cargo test -q topology::tests::`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/engine/topology.rs src/engine/contract.rs
git commit -m "feat: add shared directional topology decision types"
```

---

### Task 2: Extend `TopologyHandler` with shared move-surface helper path

**Files:**
- Modify: `src/engine/contract.rs`
- Test: `src/engine/contract.rs` (test module)

**Step 1: Write the failing test**

Add a contract test that validates helper behavior from a graph/neighbor-backed adapter:

```rust
#[test]
fn topology_handler_move_surface_classifies_decisions() {
    // adapter stub returns focused pane, pane count, and directional neighbors
    // assert move_surface(...).decision_for(...) classification is correct
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q topology_handler_move_surface_classifies_decisions`
Expected: FAIL (helper path missing).

**Step 3: Write minimal implementation**

In `TopologyHandler`, add default helper methods:
- `directional_neighbors(&self, pid: u32, focused_pane_id: u64) -> Result<DirectionalNeighbors>`
- `move_surface(&self, pid: u32) -> Result<MoveSurface>`

Defaults should be non-breaking:
- infer focused pane id from adapter-specific path when available,
- fallback to current behavior with explicit errors when unsupported.

**Step 4: Run tests to verify pass**

Run: `cargo test -q topology_handler_move_surface_classifies_decisions`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/engine/contract.rs
git commit -m "refactor: add shared TopologyHandler move-surface helpers"
```

---

### Task 3: Route tmux move decision through shared structures

**Files:**
- Modify: `src/adapters/terminal_multiplexers/tmux.rs`
- Test: `src/adapters/terminal_multiplexers/tmux.rs` (existing + new tests)

**Step 1: Write the failing test**

Add focused tests for tmux `move_decision` classification via shared path:
- single pane => `Passthrough`
- neighbor in direction => `Internal`
- no directional/perpendicular neighbor => `TearOut`

**Step 2: Run test to verify it fails**

Run: `cargo test -q tmux_move_decision_`
Expected: FAIL until tmux uses shared structures.

**Step 3: Write minimal implementation**

In `tmux.rs`:
- build `DirectionalNeighbors` using existing directional neighbor probes (convert “no neighbor” to `None`),
- build `MoveSurface` from focused pane id + `#{window_panes}`,
- change `move_decision` to `move_surface(...).decision_for(dir)`.

Do not change:
- tmux command transport
- merge/session cleanup logic
- `move_out` spawn behavior

**Step 4: Run tests to verify pass**

Run: `cargo test -q tmux`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/terminal_multiplexers/tmux.rs
git commit -m "refactor: route tmux move decisions through shared topology surface"
```

---

### Task 4: Route wezterm move decision through shared structures

**Files:**
- Modify: `src/adapters/terminal_multiplexers/wezterm.rs`
- Test: `src/adapters/apps/wezterm.rs` (existing harness tests)

**Step 1: Write the failing test**

Add or adapt tests proving wezterm classification comes from shared move surface:
- `Passthrough` (single pane)
- `Internal` (directional neighbor)
- `Rearrange` (perpendicular neighbor)
- `TearOut` (edge with no perpendicular)

**Step 2: Run test to verify it fails**

Run: `cargo test -q move_decision_rearranges_when_perpendicular_neighbor_exists`
Expected: FAIL until shared surface wiring is complete.

**Step 3: Write minimal implementation**

In `wezterm.rs`:
- build `DirectionalNeighbors` via `get-pane-direction` probes,
- build `MoveSurface` from focused pane id + active-tab pane count,
- route `move_decision` through `move_surface(...).decision_for(dir)`.

Do not change:
- bridge command enqueue semantics
- merge target fallback order
- CLI execution model

**Step 4: Run tests to verify pass**

Run: `cargo test -q adapters::apps::wezterm::tests::`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/terminal_multiplexers/wezterm.rs src/adapters/apps/wezterm.rs
git commit -m "refactor: route wezterm move decisions through shared topology surface"
```

---

### Task 5: End-to-end verification and docs sync

**Files:**
- Modify: `/Users/m/.local/state/.copilot/session-state/07544df7-451a-431e-b54b-a3bc3ac52811/plan.md`
- Optional Modify: `AGENTS.md` (only if a new surprise appears)

**Step 1: Run full verification**

Run:
- `cargo fmt --all --check`
- `cargo test -q`

Expected: PASS.

**Step 2: Update session plan**

Update `plan.md` with:
- completed extraction summary,
- remaining follow-up work (if any).

**Step 3: Final commit (if docs changed)**

```bash
git add /Users/m/.local/state/.copilot/session-state/07544df7-451a-431e-b54b-a3bc3ac52811/plan.md AGENTS.md
git commit -m "docs: update mux extraction progress notes"
```

