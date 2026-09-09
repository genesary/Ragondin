---
id: ADR-C18
title: A pipeline declares its inputs; the query is an explicit edge
status: accepted
invariants: [INV-1, INV-8, INV-9]
supersedes: []
superseded_by: null
---

# ADR-C18: A pipeline declares its inputs; the query is an explicit edge

## Context

A pipeline is a graph (ADR-C2). A node names in its `inputs` the ids of the nodes whose output it consumes, and ADR-C16 derives the kind of value on each edge from the consuming node's `LogicalNode` variant, so no port declaration ever appears in a configuration.

That a reranker needs the query is not in question and never was: `Reranker::rerank(&self, query, chunks, params)` takes it as a distinct argument, and so does `Retriever::retrieve`. What was never settled is **how the query enters the graph** — whether it travels an edge like every other value, or reaches a node ambiently from the executor.

Two documents in this repository gave different answers, and neither was subordinate to the other. `core/ragondin-pipeline/src/node.rs`'s module documentation states that *"a reranker consumes a query **and** a chunk list — can only address them by position"*: two positional ports, query first. `docs/system-architecture.md` §5.1 writes its reranker with `inputs: [fuse]`: one port, the query nowhere in that node's edges. ADR-C16 is normative for the derivation and stops just short — every example it gives is about what a node *produces*, and it never says how the query enters.

Underneath the disagreement lies a gap in the representation itself: **no variant in the closed primitive set produces `ValueKind::Query`**, and nothing anywhere describes a pipeline's entry point. §5.1's own mermaid diagram draws `Q[Query] --> T`, a box *outside* the node list that the configuration language above it cannot express. The diagram was right about what the query is — an input, not a step — and the grammar had never caught up.

Three facts made the gap executable rather than theoretical.

- Under the two-port reading a reranker's port 0 can be satisfied only by an `Extension` node, because nothing else produces a `Query`. Physical planning refuses every `Extension` node (`PlanError::ExtensionUnsupported`, pending the resolution of extension lookup), so **no pipeline containing a reranker is plannable end to end.**
- The two-port reading is declared but not enforced: "a node has too few inputs" is deliberately unchecked at validation and deferred to the executor, so a `Retriever` with no inputs validates and plans today even though its derivation declares a `Query` port.
- §5.1's illustrative pipeline does not validate as written under the reading `node.rs` documents.

Left unsettled, the question blocks the executor, which cannot know whether to seed the query into its value table or receive it on an edge; it blocks `ragondin validate`, whose whole purpose is to reject an incompatible wiring without a registry; and every query-consuming primitive added later — `Generator`, `Grader` — inherits it.

## Decision

**A pipeline declares its inputs, and the query is an explicit edge like any other value.**

A configuration names the pipeline's inputs alongside its nodes, and a node consumes an input by naming it in `inputs`:

```yaml
pipeline:
  inputs: [question]
  nodes:
    - id: rerank
      component: reranker
      impl: cross_encoder_v2
      inputs: [question, fuse]
```

Four rules follow, and they are the whole decision.

1. **A declared input is a producer.** Validation treats it as one, so naming it in a node's `inputs` is not a dangling reference, and it is what gives `ValueKind::Query` the producer the primitive set does not contain.
2. **An input's kind is fixed by the kind of graph, never written in the configuration.** A serving pipeline declares **exactly one** input, of kind `Query`. Nothing about ports or kinds enters a configuration or the content hash, so ADR-C16 is unchanged in letter and in spirit.
3. **`consumed_kinds` is unchanged.** `Reranker` stays `Fixed([Query, Chunks])` and `Retriever` stays `Fixed([Query])`. `node.rs`'s module documentation stands as written; it is `docs/system-architecture.md` §5.1 that is amended, and amending it is part of the work this decision authorises.
4. **The wire field is permissive; validation is strict.** `inputs` is optional on the `Raw` level with a default, in keeping with the three-level design in which the raw form parses permissively and validation establishes the invariants — a configuration missing it is rejected by `validate` with a typed error, not by a deserialization failure. The wire `SchemaVersion` is bumped (INV-9).

## Alternatives rejected

- **The query as an ambient value, supplied by the executor (`consumed_kinds(Reranker) = Fixed([Chunks])`).** The cheapest configuration to write, and it makes §5.1's reranker line validate as written. Rejected because it costs a capability, not merely completeness. Applied uniformly — and the question is the same one for `Retriever`, `Generator` and `Grader`, so it must be — no node declares a query port, and a reranker can therefore **never** receive a rewritten query: it always gets the pipeline's. Query transformation (HyDE, multi-query, step-back, corrective rewriting) is the first family ADR-1 lists, is what §5.1 illustrates, and is why the representation carries control flow at all (ADR-2: *"retrieval quality is poor → rewrite the query → retrieve again"*). It also does not save §5.1: a retriever declaring no ports rejects that document's own `inputs: [transform]` as an arity error. Finally, it makes the graph an incomplete account of data flow, leaving one of the two values a reranker consumes unchecked at the layer ADR-C16 built for checking.

- **The status quo: an explicit edge with no producer for it.** Consistent on paper and requiring no change to shipped code, but only by routing the query through the `Extension` escape hatch — a stand-in for a core capability — and physical planning refuses every `Extension` node. It is not a working state, and it leaves §5.1 wrong with no way to write it correctly.

- **An optional query port defaulting to the pipeline query.** §5.1 would validate and an explicit transform could still be wired. Rejected on INV-8: arity becomes variable per variant, which `PortSpec::Fixed` cannot express, and it introduces a defaulting step into canonicalization, which INV-8 requires to be total and deterministic. A defaulted port either enters the hashed form — so changing the default rehashes every stored `run_id` — or does not — so two configurations that execute differently hash identically. Both are wrong, and choosing between them is a second decision.

- **The query carried alongside the chunks, so that `ValueKind::Chunks` means "a retrieval result and the query that produced it".** One port on the reranker, §5.1 valid as written, and a rewritten query propagating naturally; it is also what much of the surrounding ecosystem does. Rejected on fusion: multi-query retrieves with several rewritten queries and fuses the results, and `Fusion` is `Variadic(Chunks)` precisely so that its legs may differ. Which query the fused value carries has no correct answer.

- **A reserved node id seeded by the executor (`inputs: [__query__, fuse]`).** The same ergonomics as the decision taken, and the only option touching no public structure at all. Rejected because it buys that by making the entry point a magic string: special-cased in the dangling-input check, colliding with any node a user names the same, and silent about what a pipeline's inputs *are* — so a second kind of graph needs a second reserved string, and a third needs a third.

- **A `Source` variant on `LogicalNode`.** Gives `Query` a producer as this decision does, but §5.1 defines a node as a component invocation or a control-flow node, and a source is neither; it would also need one variant per kind of input rather than one mechanism. Declaring inputs on the pipeline keeps the node enum meaning what it says.

## Consequences

- **The executor and `ragondin validate` are unblocked**, which is the point of the decision. The executor seeds the value table from the declared inputs and otherwise treats the query as an ordinary value; `validate` accepts or rejects a wiring with no `EngineContext`, as ADR-C16 requires.
- **`ValueKind::Query` has a producer**, so a reranker no longer depends on the extension escape hatch, and a pipeline containing one becomes plannable without waiting on extension lookup.
- **`Generator` and `Grader` inherit the answer** when they arrive, instead of inheriting the question three more times.
- **§5.1 must be amended** — its `pipeline:` block gains `inputs:`, its retriever and reranker nodes name it, and its mermaid diagram becomes literally accurate. `node.rs`'s module documentation is untouched. One of the two had to lose; the illustrative document loses, not the doc comment on an INV-1 boundary.
- **The change to `LogicalPipeline` is additive.** Its `nodes` field is private and its constructor is `pub(crate)`, so a private field plus an accessor adds to the public surface without breaking it. The change to the raw wire type is a breaking struct change, sanctioned here and nowhere else, and it bumps `SchemaVersion` per INV-9.
- **The canonical form changes** (INV-8): declared inputs are ordinary configuration data, entering it totally and deterministically with no defaulting step. Content-addressed hashing is not yet implemented and no `run_id` has ever been stored, so nothing is rehashed. This cost is at its minimum now and rises monotonically.
- **Configurations become more verbose.** A pipeline names its input once and repeats it on nearly every node. This is accepted deliberately: on this platform the configuration *is* the experiment, and the edge that reads as ceremony is exactly the one edited to move a node from the raw query to a rewritten one — and to measure which retrieves better.
- **Nothing here bears on whether indexing shares the pipeline formalism.** That question stays open. The mechanism says only that a graph declares what it receives, which is true whether the project ends with one kind of graph or two; the alternatives rejected above are the ones that would have implicitly bet on one.
- No entry in `docs/OPEN_QUESTIONS.md` is opened, closed, or changed.

## Status

Accepted.
