# ChainResolver trait integration design

## Problem
Resolver responsibilities are split across modules (`apps/mod.rs`, `engine/domain.rs`, and terminal-aware code). We need a first-class resolver contract so both app backends and terminal mux backends share resolver semantics, while moving resolver logic out of `apps/mod.rs`.

## Goals
- Introduce `ChainResolver` in `engine/contract.rs`.
- Make related traits depend on it so both `XyzBackend` (app) and `XyzMux` (terminal mux) are resolver participants.
- Move resolver implementation out of `apps/mod.rs`.
- Keep resolver flow explicit for `resolve_chain`, `default_domain_adapters`, and `domain_id_for_window`.

## Architecture
1. Add `trait ChainResolver` in `engine/contract.rs`:
   - `resolve_chain(app_id, pid, title) -> Vec<Box<dyn AppAdapter>>`
   - `default_domain_adapters() -> Vec<Box<dyn AppAdapter>>`
   - `domain_id_for_window(app_id, pid, title) -> DomainId`
2. Make both `AppAdapter` and `TerminalMuxProvider` depend on `ChainResolver` (supertrait).
3. Implement concrete resolver logic in a dedicated engine module (e.g. `engine/chain_resolver.rs`) rather than `apps/mod.rs`.
4. Ensure `XyzBackend` and `XyzMux` satisfy the resolver contract via direct impls or explicit delegation.

## Components and data flow
1. `RuntimeChainResolver` owns resolver assembly logic:
   - direct app matching
   - terminal-chain assembly
   - default domain adapters
   - domain-id derivation from resolved chain
2. `engine/orchestrator.rs` and `engine/domain.rs` stop calling `apps::resolve_chain` / `apps::default_domain_adapters` and use resolver APIs instead.
3. `apps/mod.rs` retains app adapter wiring/policy binding only (not resolver ownership).

## Error handling
- Resolver APIs return explicit failures where resolution probes fail unexpectedly.
- No silent swallowing for resolver execution errors.
- Domain-id derivation defaults only through explicit, documented fallback logic.

## Testing strategy
- Move/port resolver tests from `apps/mod.rs` into the resolver module.
- Add tests for:
  - direct app matching and override behavior
  - terminal-chain resolution behavior
  - default-domain adapter surface
  - `domain_id_for_window` mapping behavior
- Run `cargo test -q` and address regressions caused by this refactor.

## Out of scope
- Changing operational semantics unrelated to resolver ownership.
- New adapter feature behavior beyond resolver refactor requirements.
