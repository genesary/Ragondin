# ARCHITECTURE — ragondin-engine

**Status: internal — NOT an API boundary, and never will be (INV-2).** Free to
refactor. Do not treat its internals as stable; no external contract depends on
them.

## What lives here

- **`EngineContext`** — the component registry, a table mapping an
  implementation name to a constructor. Passed **explicitly**, never global
  (INV-6). Several contexts can exist in one process — indispensable for the
  harness comparing two configurations side by side. It keeps one table per
  family a pipeline node names — retriever, fusion, reranker, context builder,
  generator
  ([ADR-C31](../../docs/adr/ADR-C31-generation-contracts-template-and-served-model-per-call.md))
  — and none for an embedder or a vector store: those are not nodes, and the
  composition root builds them itself, inside the constructor closure it
  registers for a dense retriever
  ([ADR-C32](../../docs/adr/ADR-C32-remote-named-by-impl-bound-by-the-composition-root.md)).
- **Physical planning** — `LogicalPipeline` + `EngineContext` →
  `PhysicalPipeline`, resolving each `impl` name to a constructed component
  (`Local` or `Remote`). The logical→physical seam exists; the optimizer is the
  identity function for now.
- **The executor** — `Engine::execute(plan, query)` runs a `PhysicalPipeline`
  and returns `(Result<Output, ExecError>, ExecutionTrace)`, where `Output` is
  the terminal node's value: a ranking, a context or an answer. It schedules
  nodes in a **topological order over the data-flow edges** — a node runs once
  every id its `inputs` name holds a value — and never in the order the plan
  stores them, which is canonical (sorted by `NodeId`, for the content hash)
  and not an execution order. Each edge carries an erased `NodeValue`
  (ADR-C16) — a query, a ranking, a context or an answer — and each node's
  adapter destructures the value it expects, calls the component with typed
  per-call params, and re-wraps the typed result. The
  value table is seeded from the pipeline's **declared inputs** (ADR-C18).
  Control flow is not executed, because no `Branch` or `Loop` variant exists
  yet; when they arrive they are executed here, as they are not components.
- **`ExecutionTrace`** — structured, per-node output: for each node that ran,
  what it produced, a summary of what it received, how long it took, and what it
  failed with. A node's **output** names the chunks it produced — chunk id,
  document id, score — in the order it returned them; a node's **input** stays a
  count ([ADR-C28](../../docs/adr/ADR-C28-trace-names-what-each-node-produced.md)).
  A produced context is named by its chunks, in the builder's order, and its
  text; a produced answer by its text; a consumed one of either by its sizes
  (ADR-C31 § 5).

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

## How a context and an answer are represented

ADR-C31 § 5 pins what a node **produced** — `ValueSummary::Context { chunks:
Vec<RankedChunk>, text: String }` and `ValueSummary::Answer { text: String }` —
and what a consumed one must say — a context's chunk count and text length, an
answer's text length — and leaves the input-side shapes to this crate. The
choice made here:

- **Two more input-side variants, `ContextSize { chunks, text_bytes }` and
  `AnswerSize { text_bytes }`,** on the pattern `Chunks { count }` set, and
  not fields on the produced variants, for the reason given above for chunks:
  one variant holding both a size and an optional value states one fact twice.
  Every kind then has one variant per side of a node, which is a table a
  reader can hold in their head.
- **A text length is counted in bytes of UTF-8** — `String::len`, and the name
  says so. Characters would need a decision about what a character is (a
  scalar value, a grapheme) that nothing reading a size needs; bytes are what
  a trace's storage costs.
- **A context's chunks reuse `RankedChunk`**, mapped field for field from
  `ContextChunk`'s id, document id and score. The score is the one the chunk
  carried into the builder (ADR-C31 § 1); the builder assigns none.

## Two rules the executor fixes

Both are visible in an `ExecError` variant. No ADR settles the first; the
keys of the second are ADR-C31's and ADR-C32's, and how an unreadable one is
refused is this crate's.

- **The terminal node.** A node is **terminal** when no other node of the plan
  consumes it. A plan must have exactly one: the executor returns one value,
  and nothing in the representation says which of several unconsumed outputs
  that would be. `Output` is that node's value, one variant per kind a node
  produces — `Chunks`, `Context`, `Answer` (ADR-C31's Consequences) — since
  `validate` admits a pipeline ending on a ranking, on a context builder or on
  a generator.
- **Per-call parameters.** A node's `Params` has two readers, one on each
  side of the seam (`docs/code-architecture.md` §6.3): planning passes the
  node's whole map to the constructor, which reads what configures the
  implementation, and the executor reads the per-call keys from the same map.
  Nothing splits the map. The executor's keys: `top_k` on a retriever and a
  reranker, and an optional `served_model` on a reranker
  ([ADR-C32](../../docs/adr/ADR-C32-remote-named-by-impl-bound-by-the-composition-root.md)
  § 4); `budget` on a context builder; `served_model` and `template` on a
  generator, with `temperature`, `seed` and `max_tokens` optional (ADR-C31
  § 2). A required key that is absent, and any declared key of another kind
  than it is read as — or a negative count — is refused as
  `ExecError::InvalidParam`, which names the kind the key requires, before the
  component is called; an absent optional key reaches the component as `None`.
  **The executor invents no default**: what a component does without a
  parameter is the component's to decide, and a default applied here could
  only be a second, disagreeing copy of it. **Nor does it judge a value**: a
  zero `budget` or an empty `template` reaches the component, which refuses
  it. And **a float is not widened from an integer**: `temperature: 1` is
  refused rather than read as `1.0`, a choice made in this crate — widening is
  a conversion applied above the component, which is the same kind of second
  copy the no-default rule refuses, and the flat parameter grammar keeps `Int`
  and `Float` apart for a configuration to choose between.

## The extension node is refused, not planned

One function, `plannable` in `src/plan.rs`, sorts every `LogicalNode` variant
into one this build plans — a retriever, a fusion, a reranker, a context
builder, a generator — or a typed refusal: `PlanError::ExtensionUnsupported`
for an extension, whose lookup mechanism is undecided (#93). Physical planning
calls it for every node in its first pass, before any constructor runs, and
`resolve` calls it again and matches only over what it narrowed to, so
resolution has no arm for a refused variant. The executor's `call` matches
`LogicalNode` exhaustively on purpose, and gives the extension an arm that
returns `ExecError::UnplannableNode` — a defect in this crate by construction,
since no plan `plan_physical` builds holds one. The choice made here is a typed
error rather than an `unreachable!()` for an arm that cannot be reached today:
it costs nothing to make harmless, and a test builds such a plan by hand to pin
it.

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
