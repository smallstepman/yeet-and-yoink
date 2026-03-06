# Adapter + Topology Unification Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Introduce a shared `Adapter` base trait, split into `AppAdapter` and `MuxAdapter`, and move topology/introspection/mutation behavior into `TopologyHandler` + typed topology models used across app and mux adapters.

**Architecture:** We will make `Adapter` the common contract (`eval` + `eval_mut`), require both `AppAdapter` and `MuxAdapter` to extend it, and migrate mux-specific large impl blocks toward shared topology primitives. `TopologyHandler` becomes the primary place for topology semantics (neighbors, edge decisions, merge target selection) over a generic topology graph that works for terminal muxes and editor apps. `PolicyBoundApp` moves from `apps/` into engine-level decorator infrastructure.

**Tech Stack:** Rust, anyhow, serde_json, clap, existing adapter contracts/tests, cargo test.

---

**Skill refs:** @test-driven-development @systematic-debugging @verification-before-completion

### Task 1: Introduce base `Adapter` trait and `MuxAdapter` specialization

**Files:**
- Modify: `src/engine/contract.rs`
- Modify: `src/adapters/apps/mod.rs`
- Modify: `src/adapters/apps/wezterm.rs`
- Modify: `src/adapters/terminal_multiplexers/mod.rs`
- Modify: `src/adapters/terminal_multiplexers/{wezterm,tmux,zellij,kitty}.rs`

**Step 1: Write the failing test**

Add a contract test in `src/engine/contract.rs` (or `#[cfg(test)]` test module) that enforces base trait inheritance:

```rust
#[test]
fn app_and_mux_implement_adapter_base() {
    fn assert_adapter<T: Adapter>() {}
    fn assert_app<T: AppAdapter>() {}
    fn assert_mux<T: MuxAdapter>() {}

    assert_adapter::<crate::adapters::apps::wezterm::WeztermBackend>();
    assert_app::<crate::adapters::apps::wezterm::WeztermBackend>();
    assert_mux::<crate::adapters::terminal_multiplexers::tmux::TmuxMuxProvider>();
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q app_and_mux_implement_adapter_base`
Expected: FAIL due missing `Adapter`/`MuxAdapter` trait wiring.

**Step 3: Write minimal implementation**

In `src/engine/contract.rs` add:

```rust
pub trait Adapter {
    fn adapter_name(&self) -> &'static str;

    fn eval(&self, _expression: &str, _pid: Option<ProcessId>) -> Result<String> {
        Err(unsupported_operation(self.adapter_name(), "eval"))
    }

    fn eval_mut(&self, _expression: &str, _pid: Option<ProcessId>) -> Result<String> {
        Err(unsupported_operation(self.adapter_name(), "eval_mut"))
    }
}

pub trait AppAdapter: Send + Adapter + TopologyHandler + ChainResolver { ... }
pub trait MuxAdapter: Send + Adapter + TopologyHandler + ChainResolver { ... }
```

Replace `TerminalMuxProvider` references with `MuxAdapter` (or temporary type alias during migration).

**Step 4: Run test to verify it passes**

Run: `cargo test -q app_and_mux_implement_adapter_base`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/engine/contract.rs src/adapters/apps src/adapters/terminal_multiplexers
git commit -m "refactor: introduce Adapter base with AppAdapter and MuxAdapter"
```

---

### Task 2: Add shared typed topology model for all adapters

**Files:**
- Modify: `src/engine/topology.rs`
- Modify: `src/engine/contract.rs`
- Create: `src/engine/topology_solver.rs`
- Test: `src/engine/topology_solver.rs` (module tests)

**Step 1: Write failing tests for generic topology navigation**

Create tests for neighbor and merge-target selection in `src/engine/topology_solver.rs`:

```rust
#[test]
fn neighbor_probe_returns_directional_neighbor_from_shared_topology() { /* ... */ }

#[test]
fn merge_target_selector_rejects_ambiguous_candidates() { /* ... */ }
```

**Step 2: Run tests to verify failure**

Run: `cargo test -q neighbor_probe_returns_directional_neighbor_from_shared_topology`
Expected: FAIL because shared topology model/solver does not exist.

**Step 3: Implement minimal shared topology structures + solver**

In `src/engine/topology.rs` add generic concepts (not mux-prefixed):

```rust
pub struct TopologyWorkspace { ... }
pub struct TopologyWindow { ... }
pub struct TopologyTab { ... }
pub struct TopologyPane { ... }
pub struct TopologyGraph { ... }
```

In `src/engine/contract.rs` extend `TopologyHandler` with typed graph access:

```rust
fn topology_graph(&self, pid: u32) -> Result<TopologyGraph>;
```

In `src/engine/topology_solver.rs`, implement `neighbor_probe(...)` and `select_merge_target(...)` over `TopologyGraph`.

**Step 4: Run tests to verify pass**

Run: `cargo test -q topology_solver`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/engine/topology.rs src/engine/contract.rs src/engine/topology_solver.rs
git commit -m "feat: add shared topology graph and solver utilities"
```

---

### Task 3: Refactor `WeztermMux` to use `Adapter::eval` / `Adapter::eval_mut` boundaries

**Files:**
- Modify: `src/adapters/terminal_multiplexers/wezterm.rs`
- Modify: `src/adapters/terminal_multiplexers/mod.rs`
- Test: `src/adapters/apps/wezterm.rs` existing tests

**Step 1: Write failing test for eval/eval_mut flow**

Add tests in `src/adapters/apps/wezterm.rs` harness module:

```rust
#[test]
fn wezterm_mux_eval_reads_snapshot_data() { /* calls eval("list --format json") path */ }

#[test]
fn wezterm_mux_eval_mut_executes_split_command() { /* calls eval_mut("split-pane ...") path */ }
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q wezterm_mux_eval_mut_executes_split_command`
Expected: FAIL due missing `Adapter::eval_mut` implementation path.

**Step 3: Implement minimal refactor**

In `src/adapters/terminal_multiplexers/wezterm.rs`:
- Implement `Adapter` for `WeztermMux`.
- Route CLI read operations through `eval`.
- Route mutation operations through `eval_mut`.
- Keep topology decisions in `TopologyHandler` methods using shared topology solver where applicable.

**Step 4: Run affected tests**

Run: `cargo test -q adapters::apps::wezterm::tests::`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/terminal_multiplexers/wezterm.rs src/adapters/terminal_multiplexers/mod.rs src/adapters/apps/wezterm.rs
git commit -m "refactor: route wezterm mux introspection and mutation via Adapter eval APIs"
```

---

### Task 4: Implement topology graph support for editor adapters (nvim/emacs/vscode)

**Files:**
- Modify: `src/adapters/apps/nvim.rs`
- Modify: `src/adapters/apps/emacs.rs`
- Modify: `src/adapters/apps/vscode.rs`
- Test: adapter-specific tests in each file/module

**Step 1: Write failing tests for graph availability**

Add one test per adapter:

```rust
#[test]
fn nvim_exposes_topology_graph() { /* topology_graph returns panes/windows */ }
#[test]
fn emacs_exposes_topology_graph() { /* topology_graph returns frame/window data */ }
#[test]
fn vscode_exposes_topology_graph() { /* topology_graph returns groups/tabs mapping */ }
```

**Step 2: Run tests to verify failure**

Run:
- `cargo test -q nvim_exposes_topology_graph`
- `cargo test -q emacs_exposes_topology_graph`
- `cargo test -q vscode_exposes_topology_graph`
Expected: FAIL until implementations are added.

**Step 3: Implement minimal topology graph extraction**

Implement `topology_graph` for each adapter with best available data source:
- Nvim: winlayout/getwininfo-backed extraction.
- Emacs: window-tree/window-edges-backed extraction.
- VSCode: mapped group/tab representation with explicit capability flags for unknown geometry.

Use capabilities to indicate unavailable precision (no fake geometry silently).

**Step 4: Re-run tests**

Run same commands as Step 2.
Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/apps/nvim.rs src/adapters/apps/emacs.rs src/adapters/apps/vscode.rs
git commit -m "feat: add shared topology graph support for editor adapters"
```

---

### Task 5: Move `PolicyBoundApp` from `apps/` into engine decorator layer

**Files:**
- Create: `src/engine/decorators/policy_bound_adapter.rs`
- Modify: `src/engine/mod.rs`
- Modify: `src/adapters/apps/mod.rs`
- Test: existing policy tests currently in `src/adapters/apps/mod.rs`

**Step 1: Write failing test for decorator location behavior**

Add test to ensure policy decorator remains behavior-identical after move:

```rust
#[test]
fn policy_decorator_masks_capabilities_and_directions() { /* same assertions as current behavior */ }
```

**Step 2: Run test to verify failure before move**

Run: `cargo test -q policy_decorator_masks_capabilities_and_directions`
Expected: FAIL if test references new location before code move.

**Step 3: Implement move with no behavior changes**

- Move `PolicyBoundApp` logic to `engine/decorators/policy_bound_adapter.rs`.
- Expose constructor/helper from engine module.
- Replace `apps::bind_policy` internals with engine decorator call.

**Step 4: Run policy and resolver tests**

Run:
- `cargo test -q resolve_chain_tests`
- `cargo test -q policy_decorator_masks_capabilities_and_directions`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/engine/decorators/policy_bound_adapter.rs src/engine/mod.rs src/adapters/apps/mod.rs
git commit -m "refactor: move policy adapter decorator into engine layer"
```

---

### Task 6: Integrate orchestrator/domain with unified contracts and validate end-to-end

**Files:**
- Modify: `src/engine/orchestrator.rs`
- Modify: `src/engine/domain.rs`
- Modify: `src/engine/chain_resolver.rs`
- Optional docs: `README.md` (if trait names change publicly)

**Step 1: Write failing integration assertions**

Add/adjust tests ensuring orchestrator/domain paths operate through unified adapter contracts:

```rust
#[test]
fn move_uses_unified_adapter_topology_path() { /* ... */ }

#[test]
fn domain_classification_uses_chainresolver_and_topology_capabilities() { /* ... */ }
```

**Step 2: Run focused tests to verify failure**

Run:
- `cargo test -q move_uses_unified_adapter_topology_path`
- `cargo test -q domain_classification_uses_chainresolver_and_topology_capabilities`
Expected: FAIL until final integration is complete.

**Step 3: Implement integration updates**

- Remove remaining legacy trait/path assumptions.
- Ensure neighbor/merge decisions consume `TopologyGraph` + solver.
- Ensure fallback behavior is explicit and capability-driven.

**Step 4: Run full verification**

Run:
- `cargo test -q`
- If flaky: `cargo test -q -- --test-threads=1`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/engine/orchestrator.rs src/engine/domain.rs src/engine/chain_resolver.rs src/engine/contract.rs src/engine/topology.rs
[ -f README.md ] && git add README.md || true
git commit -m "refactor: unify adapter and topology contracts across apps and mux backends"
```

