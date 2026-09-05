# ARCHITECTURE — ragondin-engine

**Status: internal — NOT an API boundary, and never will be (INV-2).** Free to
refactor. Do not treat its internals as stable; no external contract depends on
them.

## What lives here

- **`EngineContext`** — the component registry, a table mapping an
  implementation name to a constructor. Passed **explicitly**, never global
  (INV-6). Several contexts can exist in one process — indispensable for the
  harness comparing two configurations side by side.
- **Physical planning** — `LogicalPipeline` + `EngineContext` →
  `PhysicalPipeline`, resolving each `impl` name to a constructed component
  (`Local` or `Remote`). The logical→physical seam exists; the optimizer is the
  identity function for now.
- **The executor** — runs a `PhysicalPipeline`, executing `Branch`/`Loop`
  control flow itself (they are not components).
- **`ExecutionTrace`** — structured, per-node output.

## Local invariants

- **Knows only traits (INV-5, CI-enforced).** Depends on `ragondin-contracts`, not on
  any crate under `components/`. The temptation — "just import the BM25 crate
  directly, it's faster to wire" — creates a two-tier system where built-ins are
  privileged over third-party components. That is the slow death of a
  contribution-driven project. The crate graph forbids it and CI proves it.
- **Explicit composition, zero globals (INV-6).** The registry lives on
  `EngineContext`. Never a static global registry (`inventory`, `linkme`, or
  equivalent).
- **No privilege for built-ins (INV-7).** A built-in registers through exactly
  the same mechanism as a third-party component. No shortcut, no fast path.
- **Traces are a return value, not a log (INV-10).** The executor's signature
  **returns** the `ExecutionTrace`; per-node UI replay depends on it. `tracing`
  runs in parallel for operational telemetry but never substitutes for the
  trace.
- **Tower is not here (INV-11).** The network envelope belongs to `ragondin-server`.
  Components are heterogeneous domain traits, never `tower::Service`.
