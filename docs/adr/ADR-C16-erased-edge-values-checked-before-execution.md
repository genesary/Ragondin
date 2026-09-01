# ADR-C16: Erased edge values, with compatibility checked before execution

## Context

The engine executes a `PhysicalPipeline`, which is a graph (ADR-C2). A value travels along each **edge**, from one node's output to the next node's input, and the executor needs a concrete Rust type for the table of intermediate values it carries between nodes.

Nothing settled what that type is, nor **where** input/output compatibility is checked. `docs/code-architecture.md` §6.3 asserts that physical planning "verifies end-to-end type compatibility" without saying whether the verification is static or dynamic; §5.1 of the system architecture calls edges "data flow" and stops there. Until this is settled, neither physical planning nor the executor can be built.

Two facts, established while analysing the question, narrow it considerably.

**The erased value is not a choice.** Pipelines arrive as YAML at run time, so the executor cannot be monomorphised per graph shape: whatever typing discipline is adopted, its value table needs one concrete type. Typed ports would let the executor *skip a check*; they would not remove the union. The representation question and the checking question are therefore independent, and only the second is genuinely open.

**Port kinds are derivable, not declared.** `LogicalNode` is a closed enum of primitives plus an `Extension` variant (ADR-C3). For every primitive, the kind produced and the kinds consumed follow from the variant alone: a `Retriever` yields chunks, a `Fusion` consumes chunk lists and yields chunks, a `Generator` yields a generation. No port declaration need ever appear in a configuration, so nothing enters the canonical form or the content hash. Only an `Extension` node's kinds are unknown to the core — by construction.

Two existing commitments then decide *where* the check belongs. §8.3 already places a well-formedness condition — a `Loop`'s mandatory termination guard — at `LogicalPipeline` validation, and explicitly **not** at execution. And the configuration-delivery service (ADR-6) rejects a configuration with an explicit **NACK**, keeping the previous version in force. Admission control needs a validation that can actually reject: a structurally well-formed configuration whose wiring is incompatible would otherwise be acknowledged, replace a working configuration, and fail on every subsequent query.

## Decision

Edges carry an **erased value**, and compatibility is checked **before execution**, in two layers over one derivation.

`rag-engine` defines a closed `NodeValue` enum over the kinds the executor routes between nodes, and passes it along each edge. Component authors never see it: each node's adapter destructures the value it expects, calls the component with typed arguments, and re-wraps the typed result.

Compatibility is **derived, never declared**. `rag-pipeline` exposes `ValueKind` — a closed enum of primitive kinds plus an opaque variant for extensions — together with the functions deriving a node's produced and consumed kinds from its `LogicalNode` variant. That derivation is called at two points:

- **`LogicalPipeline` validation** rejects an incompatible wiring between primitive nodes. It needs no registry, so `rag validate` catches it and the configuration service can NACK on it.
- **Physical planning** repeats the same check with the registry resolved — the only point at which an `Extension` node's kinds are known. This is §6.3's "verifies end-to-end type compatibility", honoured as written.
- **Execution** keeps a typed error for a kind mismatch as a backstop. Reaching it is a defect in one of the two layers above, not the expected path.

`ValueKind` is deliberately **coarse**: it names what kind of thing travels an edge and carries **no parameters** — no embedding dimensionality, no chunk provenance. A parameterised kind system is a materially larger decision, and it is not taken here.

## Alternatives rejected

- **Checking only at execution.** The cheapest to write, and this ADR's own first draft. Rejected on two counts: it contradicts §8.3, which already places a well-formedness check at validation rather than at execution; and it would leave ADR-6's NACK with almost nothing to reject on, so an incompatible configuration would be acknowledged into a running data plane and fail per request.
- **Declared port types in the configuration.** Would put port kinds into `rag-pipeline`'s public API *and* into the canonical hashed form (INV-8), fixing them before a single real component exists, and would make any later change to the port model rehash stored configurations. Deriving the kinds from the node variant buys the same checking with none of that cost.
- **A single check at physical planning.** Correct and complete, but it leaves `rag validate` unable to reject a bad wiring without a registry, and pushes admission control later than it needs to be. The logical layer is free: the same derivation function, called earlier.
- **`Box<dyn Any>` with `downcast`.** The same erasure, but the set of legal kinds becomes undiscoverable and a mismatch degrades to "downcast failed" with nothing to report. A closed enum gives an exhaustive `match` and a compiler error when a kind is added and a site is missed.
- **One uniform value type for every component.** Homogeneous edges, at the price of unnatural signatures imposed on component authors — and it breaks outright as soon as generation and grading enter the graph.

## Consequences

- **#15 (physical planning) and #16 (executor) are unblocked**, which is the point of the decision.
- **One derivation, two call sites.** Validation and planning call the same functions; planning additionally resolves `Extension` kinds from the registry. This is not two mechanisms to keep in sync, so the usual objection to a layered check does not apply here.
- **`LogicalPipeline` validation (#9) gains the kind check.** Its scope, until now structural well-formedness only, extends to compatibility between primitive nodes — and #9 therefore becomes dependent on this decision.
- **`rag validate` (#30) becomes a real tool** rather than a parser that prints a hash: it rejects an incompatible wiring with no `EngineContext`. Its documentation must state what it does *not* cover — an `Extension` node's kinds are invisible to it.
- **`ValueKind` joins `rag-pipeline`'s INV-1 stable surface.** A real cost, taken deliberately: it is the same bet already made with `LogicalNode` — a closed enum with an escape hatch — and it stays out of the canonical form, so INV-8 and every stored `run_id` are untouched.
- **`NodeValue` stays inside `rag-engine`** and must not appear in `rag-types`, `rag-pipeline`, or `rag-contracts`. It is the executor's representation, and INV-2 keeps it freely refactorable as kinds arrive in M3 and M4.
- **INV-7 is preserved.** Built-in and third-party components are wrapped by the same adapter; the erased edge value creates no fast path for either.
- A kind mismatch surfacing at execution is a defect in validation or planning. Its typed error should say so, so that it is fixed upstream rather than absorbed.
- This ADR does **not** resolve open question 3 (`PhysicalPipeline` serializability): `NodeValue` is an execution-time value, not part of the resolved plan.
- No entry in `docs/OPEN_QUESTIONS.md` is opened, closed, or changed.

## Status

Accepted.
