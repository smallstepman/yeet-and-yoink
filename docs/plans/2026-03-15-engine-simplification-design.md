# Engine Simplification Design

## Problem

`src/engine/` is no longer a god-file, but the refactor traded one kind of complexity for another: the orchestration layer now spreads one logical flow across many small files, free functions, and helper pairs with overlapping responsibilities. The result is that the reader has to reconstruct the same app-first pipeline multiple times before they can understand how focus, move, resize, merge, tear-out, and WM fallback actually fit together.

The design goal for this pass is to reduce cognitive load in `src/engine/` without pushing more orchestration burden into `src/adapters/`. Adapter implementations should stay the primary home of adapter-specific behavior, with the engine responsible for generic routing and policy only.

## Constraints

- Preserve current behavior at the public command boundary.
- Keep `TopologyHandler` and `AppAdapter` mostly stable unless a change clearly removes code that currently lives outside adapter implementations.
- Prefer canonical `engine::{actions,contracts,resolution,transfer,wm}` imports inside `src/engine/`; keep compatibility shims only as boundary helpers for the rest of the repository.
- Avoid adding new indirect layers that hide the flow more than they clarify it.

## Alternatives Considered

### 1. Conservative dedupe only inside `actions/`

This would only remove obvious repetition in `focus.rs`, `resize.rs`, and `orchestrator.rs`.

**Pros**
- Lowest risk
- Very small diff

**Cons**
- Leaves move/merge/tear-out/probe flow fragmented
- Keeps the main cognitive burden in place

### 2. Recommended: shared focused-app session + engine-only action contexts

Capture focused window metadata and the resolved adapter chain once, then route focus/resize/move through a small number of engine-side context helpers. Keep adapter contracts mostly stable.

**Pros**
- Removes repeated preamble and repeated “walk the chain” scaffolding
- Makes the app-first / WM-fallback policy explicit
- Simplifies `movement.rs`, `merge.rs`, and `tearout.rs` without pushing logic into adapters

**Cons**
- Moderate refactor touching several engine files
- Requires careful test coverage to keep behavior stable

### 3. Richer action concepts in `TopologyHandler`

Push move / merge / tear-out orchestration concepts down into contracts so the engine becomes thinner.

**Pros**
- Could reduce engine branching further

**Cons**
- Touches 10+ adapter `TopologyHandler` implementations
- Risks making adapters harder for contributors to reason about
- Works against the goal of keeping `src/adapters/` rigid and easy to extend

## Selected Design

Use option 2.

### 1. Replace the current “context plus repeated chain resolution” pattern with a shared focused-app session

The current `AppContext` captures focused window metadata, but each action still resolves the adapter chain independently and rebuilds local variables (`owner_pid`, `source_window_id`, `app_id`, `title`, etc.) in slightly different shapes.

Refactor this into a shared focused-app session object in `src/engine/actions/context.rs` that:

- captures the focused window identity once
- resolves the adapter chain once
- exposes the common fields needed by focus/move/resize/merge/tear-out
- provides a single helper for “run only if a focused app session exists”

This keeps the “what is the current processing subject?” logic in one place.

### 2. Make app-first / WM-fallback an explicit orchestrator pattern

`Orchestrator` currently repeats the same shape for focus and resize and then partly repeats it again before move-specific transfer logic.

Refactor `src/engine/actions/orchestrator.rs` so the top-level action flow reads as:

1. try focused-app handling
2. if handled, stop
3. otherwise perform the WM fallback (or move transfer routing, then WM fallback)

Focus and resize should share a generic executor for this pattern. Move should keep its custom routing after app handling fails, but should still read as the same high-level policy.

### 3. Turn probe / merge / tear-out helpers into coherent engine-side contexts

Today the move path is hard to scan because it passes many loose arguments across several free functions:

- `probe_directional_target*`
- `probe_in_place_target_for_adapter`
- `attempt_passthrough_merge`
- `execute_app_tear_out`
- `restore_in_place_target_focus`

The refactor should group these into a few explicit engine-side helpers with narrow responsibility:

- a directional probe helper that owns source-window focus restoration semantics
- a passthrough merge context that separates source-focused and target-focused strategies
- a tear-out context/request that groups the repeated move-out arguments and keeps the tear-out lifecycle linear

The goal is not to create a framework. The goal is to replace argument soup and cross-file control flow with a small number of obvious units.

### 4. Keep adapter contracts stable unless they remove engine-only glue

The default plan is to keep `TopologyHandler` stable. Small contract changes are acceptable only if they clearly delete non-adapter glue and move logic closer to adapter implementations without increasing adapter surface area or special-casing.

If no such change emerges during implementation, leave adapter contracts alone.

## Intended File-Level Outcome

- `src/engine/actions/context.rs`
  - becomes the canonical home of the focused-app session abstraction
- `src/engine/actions/orchestrator.rs`
  - exposes a smaller number of top-level action patterns
- `src/engine/actions/focus.rs` and `src/engine/actions/resize.rs`
  - collapse into simple session-driven chain walkers
- `src/engine/actions/movement.rs`
  - becomes a linear policy loop that delegates merge / tear-out details to focused helpers
- `src/engine/actions/merge.rs`, `src/engine/actions/tearout.rs`, `src/engine/actions/probe.rs`
  - keep behavior but present it through clearer engine-only helpers/contexts
- internal `src/engine` imports
  - prefer canonical modules instead of compatibility shims where possible

## Error Handling

- Keep the current explicit logging style.
- Do not introduce broader “best effort” success-shaped fallbacks.
- Preserve the current distinction between “feature not handled” (`Ok(false)`) and “operation failed” (`Err(...)`) where it already exists.
- Preserve tear-out and merge cleanup diagnostics; simplify structure, not observability.

## Testing Strategy

Use TDD for the refactor:

1. add focused unit tests for new engine helpers before production code
2. keep or expand the existing `actions/orchestrator.rs`, `actions/probe.rs`, and `actions/tearout.rs` regression coverage
3. run targeted action tests while refactoring
4. finish with `cargo test -q` in the isolated worktree

Key behaviors to preserve:

- app-first focus/resize fall back to WM only when not handled
- directional probes correctly restore or keep focus depending on mode
- move passthrough still tries merge before tear-out / WM fallback
- same-domain and cross-domain transfer routing remain unchanged
- tear-out still waits for and places the new WM window the same way

## Non-Goals

- redesigning adapter traits for stylistic purity
- removing the engine compatibility shims used outside `src/engine/`
- changing external CLI behavior
- reworking `transfer/`, `wm/`, or adapter implementations unless required by the simplification
