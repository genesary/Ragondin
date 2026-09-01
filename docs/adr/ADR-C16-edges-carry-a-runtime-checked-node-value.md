# ADR-C16: Pipeline edges carry a runtime-checked NodeValue

## Context

The engine executes a `PhysicalPipeline`, which is a graph (ADR-C2). A value must travel along each **edge**, from one node's output to the next node's input. The components at the ends of those edges are **heterogeneous**: a `Retriever` produces `Vec<ScoredChunk>`, a `Fusion` consumes `Vec<Vec<ScoredChunk>>`, an `Embedder` produces `Vec<Embedding>`, a `VectorStore` consumes an `Embedding`. One executor walks one graph over all of them.

Nothing in the architecture settled **what the type of a value on an edge is**, nor **where input/output compatibility is checked**. `docs/code-architecture.md` §6.3 asserts that physical planning "verifies end-to-end type compatibility" without saying whether that verification is static (port types declared at the logical level) or dynamic (a check at execution). §5.1 of the system architecture calls edges "data flow" and stops there. The component traits type each component's own inputs and outputs, but not how the engine routes a value **between two differently-typed components**.

This is a genuine gap rather than a rule that was overlooked, and it is load-bearing: with heterogeneous traits and a single graph walk, some erased edge value is unavoidable unless the logical level gains typed ports. Until it is decided, neither physical planning nor the executor can be built — the moment a resolved plan carries a value from one node to the next, the type of that value must exist.

## Decision

**An edge carries an erased value, checked at execution.**

`rag-engine` defines a closed enum — `NodeValue` — over the value kinds the engine routes between nodes. The executor passes a `NodeValue` along each edge. Each node's adapter destructures the `NodeValue` it expects, calls the component with typed arguments, and re-wraps the typed result. A kind mismatch is a **typed error** (`thiserror`) naming the edge, the expected kind and the kind found — never a panic and never an `unwrap`.

Two boundaries follow from this and are part of the decision:

- **`NodeValue` lives in `rag-engine` and nowhere else.** It must not appear in `rag-types`, `rag-pipeline`, or `rag-contracts`. Component authors write ordinary typed signatures and never see the erased type; the wrapping and unwrapping is the engine's business.
- **`LogicalPipeline` carries no port types.** Logical validation (`RawPipeline` → `LogicalPipeline`) checks **structural well-formedness only**: unique node ids, every referenced input exists, data edges acyclic. Value-kind compatibility is not a logical-level concern.

## Alternatives rejected

- **Statically-typed ports at the logical level.** `LogicalNode` would declare typed input and output ports, and validation would reject an incompatible wiring at canonicalization time. Rejected because port kinds would become part of `rag-pipeline`'s **stable public API (INV-1)** and of the **canonical hash (INV-8)** — the two most expensive surfaces in the workspace to change — and would be fixed there before a single real component exists. It buys an earlier error message at the price of over-fitting the type model to a set of value kinds that is still unknown.
- **The hybrid: dynamic edges plus a best-effort static port-kind pre-pass.** Rejected for v0 as the most code for a benefit that is purely additive. It remains available later (see Consequences) at no cost incurred now.
- **`Box<dyn Any>` with `downcast`.** The same erasure, but the set of legal kinds becomes undiscoverable and a mismatch degrades to "downcast failed" with nothing to report to the author. A closed enum gives an exhaustive `match`, a readable diagnostic, and a compiler error when a new kind is added and a site is missed.
- **One uniform value type for every component.** Forcing every component to consume and produce, say, `Vec<ScoredChunk>` would give homogeneous edges, at the price of unnatural signatures imposed on component authors — and it breaks outright as soon as `Embedder` and `VectorStore` enter the graph.

## Consequences

- **#15 (physical planning) and #16 (executor) are unblocked**, which is the point of the decision.
- `rag-pipeline`'s public surface and its canonical hash stay minimal while the set of value kinds is still unknown. This is the cheap direction to be wrong in.
- Because `rag-engine` is **internal and not an API boundary (INV-2)**, adding a value kind — `Context` and `Generation` arrive in M3 — is a freely refactorable, non-breaking change. This is the property that makes the dynamic option cheap and the static one expensive.
- **A wiring error surfaces at run time, not at `rag validate`.** The `validate` subcommand (#30) therefore validates *structure*, not end-to-end type compatibility, and its documentation and scope must say so plainly rather than imply a guarantee it does not provide.
- **`docs/code-architecture.md` §6.3 must be amended.** Its claim that physical planning "verifies end-to-end type compatibility" is narrowed to structural resolution: every referenced component resolves against the registry, and node arity matches. Value-kind compatibility is enforced at execution.
- **Adopting static port kinds later is additive, not a migration.** A pre-pass over the logical graph can be added without changing stored configurations; the logical hash changes only if port kinds are made part of the canonical form, which such a pre-pass does not require. Doing so would be a new ADR superseding this one.
- **INV-7 is preserved.** A built-in component and a third-party component are wrapped by the same adapter mechanism; the erased edge type creates no fast path for either.
- This ADR does **not** resolve open question 3 (`PhysicalPipeline` serializability). `NodeValue` is an execution-time value flowing along an edge, not part of the resolved plan, so the plan's serializability is untouched and remains open.
- No entry in `docs/OPEN_QUESTIONS.md` is opened, closed, or changed by this decision.

## Status

Proposed — under discussion in decision issue #5, and not in force until accepted.
