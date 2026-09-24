# ARCHITECTURE — ragondin-engine

**Status: internal — NOT an API boundary, and never will be (INV-2).** Free to
refactor. Do not treat its internals as stable; no external contract depends on
them.

## What lives here

- **`EngineContext`** — the component registry, a table mapping an
  implementation name to a constructor. Passed **explicitly**, never global
  (INV-6). Several contexts can exist in one process — indispensable for the
  harness comparing two configurations side by side. It keeps one table per
  family a pipeline node names — retriever, fusion, reranker — and none for an
  embedder or a vector store: those are not nodes, and the composition root
  builds them itself, inside the constructor closure it registers for a dense
  retriever
  ([ADR-C32](../../docs/adr/ADR-C32-remote-named-by-impl-bound-by-the-composition-root.md)).
  A context builder and a generator are nodes and have no table yet either:
  planning refuses them — see § The generation nodes are refused, not yet
  planned.
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
  what it produced, a summary of what it received, how long it took, and what it
  failed with. A node's **output** names the chunks it produced — chunk id,
  document id, score — in the order it returned them; a node's **input** stays a
  count ([ADR-C28](../../docs/adr/ADR-C28-trace-names-what-each-node-produced.md)).

## How an output's chunks are represented

ADR-C28 fixes *what* a trace promises — ids, document ids and scores, in the
node's own order, on outputs; a count on inputs — and INV-2 leaves the
representation to this crate. The choice made here, so that a reader can
disagree with it:

- **Two `ValueSummary` variants, not one variant with an optional list.**
  `Chunks { count }` is what an input port records and
  `RankedChunks { chunks: Vec<RankedChunk> }` what a node produced, and an
  output's count is the length of its list. The alternative — one `Chunks`
  variant carrying a count *and* an `Option<Vec<_>>` — stores the same fact
  twice and admits a count that disagrees with the list beside it; and it lets a
  producer be written without deciding which side of a node it is summarizing,
  which is the one thing ADR-C28 asks the code to keep straight. The cost is
  that a reader who only wants "how many chunks" matches two variants instead
  of one, and that the enum is exhaustively matched at every such site — which
  is how a third kind added in M3 becomes a compiler error rather than a
  silently unnamed output. `RankedChunk` carries no chunk *text*: it is the one
  field that grows with the corpus and the one no reader of a ranking needs.

## Two rules the executor fixes

Both are stated here because no ADR settles them and both are visible in an
`ExecError` variant.

- **The terminal node.** A node is **terminal** when no other node of the plan
  consumes it. A plan must have exactly one: the executor returns one value,
  and nothing in the representation says which of several unconsumed outputs
  that would be. The M2 `Output` is that node's `Vec<ScoredChunk>`.
- **Per-call parameters.** A node's `Params` has two readers, one on each
  side of the seam (`docs/code-architecture.md` §6.3): planning passes the
  node's whole map to the constructor, which reads what configures the
  implementation, and the executor reads the per-call keys from the same map.
  Nothing splits the map. In M2 the executor's key is one — `top_k`, on a
  retriever and on a reranker — read as a `ParamValue::Int` and refused when
  absent, of another kind, or negative. **The executor invents no default**: what a component
  does without a parameter is the component's to decide, and a default applied
  here could only be a second, disagreeing copy of it.

## The generation nodes are refused, not yet planned

`ragondin-pipeline` validates a `ContextBuilder` and a `Generator` node
(ADR-C31 § 3), and this crate does not plan either yet: no registry family
resolves them. One function, `plannable` in `src/plan.rs`, sorts every
`LogicalNode` variant into one this build plans — a retriever, a fusion, a
reranker — or a typed refusal: `PlanError::ExtensionUnsupported` for an
extension, `PlanError::GenerationUnsupported` (naming the node and its
`component:` value) for a context builder or a generator. Physical planning
calls it for every node in its first pass, before any constructor runs, and
`resolve` calls it again and matches only over what it narrowed to, so
resolution has no arm for a refused variant. The executor's `call` matches
`LogicalNode` exhaustively on purpose, and gives the three refused variants
one arm that returns `ExecError::UnplannableNode` — a defect in this crate by
construction, since no plan `plan_physical` builds holds one. The choice made
here is a typed error rather than an `unreachable!()` for an arm that cannot be
reached today: it costs nothing to make harmless, and a test builds such a plan
by hand to pin it.

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
