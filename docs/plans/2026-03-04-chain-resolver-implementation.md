# ChainResolver Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Introduce a first-class `ChainResolver` trait and migrate chain-resolution ownership out of `src/adapters/apps/mod.rs` into engine-level resolver infrastructure used by both app backends and terminal mux backends.

**Architecture:** Add `ChainResolver` to `src/engine/contract.rs` and make `AppAdapter` + `TerminalMuxProvider` depend on it. Create an engine resolver module that contains the moved `resolve_chain/default_domain_adapters/domain_id_for_window` logic and wire orchestrator/domain call sites to this resolver. Keep adapter behavior unchanged while moving ownership and tests.

**Tech Stack:** Rust, anyhow, serde/toml config flow, cargo test.

---

### Task 1: Add `ChainResolver` trait and trait dependencies

**Files:**
- Modify: `src/engine/contract.rs`
- Modify: `src/adapters/apps/*.rs` (types implementing `AppAdapter`)
- Modify: `src/adapters/terminal_multiplexers/*.rs` (types implementing `TerminalMuxProvider`)

**Step 1: Write the failing compile target (trait wiring expectation)**

Add `ChainResolver` references in `contract.rs` first (without impls) so concrete adapter/mux types fail to satisfy new bounds.

**Step 2: Run check to verify expected failures**

Run: `cargo check -q`
Expected: compile errors for missing `ChainResolver` implementations on app/mux types.

**Step 3: Implement minimal trait defaults + supertrait dependencies**

In `contract.rs`, define:

```rust
pub trait ChainResolver {
    fn resolve_chain(&self, app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn AppAdapter>>;
    fn default_domain_adapters(&self) -> Vec<Box<dyn AppAdapter>>;
    fn domain_id_for_window(&self, app_id: Option<&str>, pid: Option<ProcessId>, title: Option<&str>) -> DomainId;
}
```

Then update trait headers:

```rust
pub trait AppAdapter: Send + TopologyHandler + ChainResolver { ... }
pub trait TerminalMuxProvider: TopologyHandler + ChainResolver { ... }
```

Add default no-op impl behavior where needed to keep non-resolver adapters compiling.

**Step 4: Run check to verify compile passes for trait wiring**

Run: `cargo check -q`
Expected: trait-bound errors resolved.

**Step 5: Commit**

```bash
git add src/engine/contract.rs src/adapters/apps src/adapters/terminal_multiplexers
git commit -m "refactor: add ChainResolver trait dependencies"
```

---

### Task 2: Move resolver logic from `apps/mod.rs` into engine resolver module

**Files:**
- Create: `src/engine/chain_resolver.rs`
- Modify: `src/engine/mod.rs`
- Modify: `src/adapters/apps/mod.rs`

**Step 1: Write failing resolver tests in new module**

Port existing `resolve_chain_tests` to `src/engine/chain_resolver.rs` test module and update imports to call `RuntimeChainResolver` methods.

**Step 2: Run test to verify failure before implementation move**

Run: `cargo test -q engine::chain_resolver::tests::`
Expected: FAIL due missing module/impl.

**Step 3: Implement moved resolver module**

In `src/engine/chain_resolver.rs`:
- Move `DirectAdapterSpec`, `resolve_direct_adapter`, `resolve_terminal_chain`, `default_domain_adapters`, and top-level chain assembly from `apps/mod.rs`.
- Introduce concrete resolver type:

```rust
pub struct RuntimeChainResolver;
impl ChainResolver for RuntimeChainResolver { ... }
```

- Expose accessor:

```rust
pub fn runtime_chain_resolver() -> &'static RuntimeChainResolver
```

**Step 4: Remove moved free functions from `apps/mod.rs`**

Keep only app adapter exports, policy binding, and adapter-specific helpers; remove/redirect old resolver free functions.

**Step 5: Run migrated resolver tests**

Run: `cargo test -q engine::chain_resolver::tests::`
Expected: PASS.

**Step 6: Commit**

```bash
git add src/engine/chain_resolver.rs src/engine/mod.rs src/adapters/apps/mod.rs
git commit -m "refactor: move app chain resolution into engine resolver"
```

---

### Task 3: Rewire orchestrator/domain to resolver trait usage

**Files:**
- Modify: `src/engine/orchestrator.rs`
- Modify: `src/engine/domain.rs`
- Modify: `src/engine/contract.rs` (if helper defaults needed)

**Step 1: Write/adjust failing tests for resolver call-sites**

Adapt tests covering:
- focused app move/focus resolution path in orchestrator
- `domain_id_for_window` classification path
- runtime domain adapter seeding

so they use resolver-backed behavior.

**Step 2: Run focused tests to capture failures**

Run:
- `cargo test -q move_prefers_cross_domain_transfer_when_payloads_are_compatible`
- `cargo test -q terminal_app_ids_classify_to_terminal_domain`

Expected: failures where old free-function wiring remains.

**Step 3: Implement resolver wiring**

Update call sites:

```rust
let resolver = chain_resolver::runtime_chain_resolver();
resolver.resolve_chain(...)
resolver.default_domain_adapters()
resolver.domain_id_for_window(...)
```

Remove direct uses of `apps::resolve_chain`/`apps::default_domain_adapters` in engine modules.

**Step 4: Run focused tests again**

Run same commands from Step 2.
Expected: PASS.

**Step 5: Commit**

```bash
git add src/engine/orchestrator.rs src/engine/domain.rs src/engine/contract.rs
git commit -m "refactor: route engine through ChainResolver"
```

---

### Task 4: Ensure `XyzBackend` and `XyzMux` implement resolver behavior

**Files:**
- Modify: `src/adapters/apps/wezterm.rs`
- Modify: `src/adapters/terminal_multiplexers/wezterm.rs`
- Modify: `src/adapters/terminal_multiplexers/mod.rs` (if delegation helpers needed)

**Step 1: Add failing compile expectation for missing impls**

Add explicit `impl ChainResolver for WeztermBackend` and `impl ChainResolver for WeztermMux` stubs (or remove defaults) to force explicit implementation if required.

**Step 2: Run compile check**

Run: `cargo check -q`
Expected: trait method missing errors until impls are completed.

**Step 3: Implement concrete delegations**

- `WeztermBackend` resolver methods delegate to engine runtime resolver.
- `WeztermMux` resolver methods either delegate to runtime resolver or provide terminal-chain specific behavior consistent with architecture.

**Step 4: Run adapter-focused tests**

Run:
- `cargo test -q adapters::apps::wezterm::tests::`
- `cargo test -q adapters::terminal_multiplexers::tests::`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/apps/wezterm.rs src/adapters/terminal_multiplexers/wezterm.rs src/adapters/terminal_multiplexers/mod.rs
git commit -m "refactor: require resolver impls for wezterm backend and mux"
```

---

### Task 5: Final validation and docs sync

**Files:**
- Modify: `AGENTS.md` (if new surprises discovered)
- Modify: `src/engine/chain_resolver.rs` tests/docs comments as needed

**Step 1: Run full suite**

Run: `cargo test -q`
Expected: all tests pass.

**Step 2: If flaky env-sensitive tests fail, verify single-thread baseline**

Run: `cargo test -q -- --test-threads=1`
Expected: stable pass, then isolate flaky tests and fix root cause.

**Step 3: Update AGENTS notes if needed**

Add any new migration surprises (only if encountered).

**Step 4: Final commit**

```bash
git add AGENTS.md src/engine/chain_resolver.rs src/engine/domain.rs src/engine/orchestrator.rs src/engine/contract.rs src/adapters/apps/mod.rs
 git commit -m "refactor: complete ChainResolver migration"
```

