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
- **The executor** — `Engine::execute(plan, query)` runs a `PhysicalPipeline`
  and returns `(Result<Output, ExecError>, ExecutionTrace)`. It schedules
  nodes in a **topological order over the data-flow edges** — a node runs once
  every id its `inputs` name holds a value — and never in the order the plan
  stores them, which is canonical (sorted by `NodeId`, for the content hash)
  and not an execution order. Each edge carries an erased `NodeValue`
  (ADR-C16), and each node's adapter destructures the value it expects, calls
  the component with typed per-call params, and re-wraps the typed result. The
  value table is seeded from the pipeline's **declared inputs** (ADR-C18).
  Control flow is not executed, because no `Branch` or `Loop` variant exists
  yet; when they arrive they are executed here, as they are not components.
- **`ExecutionTrace`** — structured, per-node output: for each node that ran,
  a summary of what it received and produced, how long it took, and what it
  failed with.

## Two rules the executor fixes

Both are stated here because no ADR settles them and both are visible in an
`ExecError` variant.

- **The terminal node.** A node is **terminal** when no other node of the plan
  consumes it. A plan must have exactly one: the executor returns one value,
  and nothing in the representation says which of several unconsumed outputs
  that would be. The M2 `Output` is that node's `Vec<ScoredChunk>`.
- **Per-call parameters.** Planning splits a node's `Params` at the seam
  (`docs/code-architecture.md` §6.3): the constructor took the half that
  configures the implementation, and the executor reads the half that varies
  per call. In M2 that half is one key — `top_k`, on a retriever and on a
  reranker — read as a `ParamValue::Int` and refused when absent, of another
  kind, or negative. **The executor invents no default**: what a component
  does without a parameter is the component's to decide, and a default applied
  here could only be a second, disagreeing copy of it.

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
