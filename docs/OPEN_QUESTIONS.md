# Open questions

A living register of the architectural questions that are **deliberately unresolved**. They do not invalidate the architecture, but each must be settled before the work it gates.

**An agent must never resolve one of these in passing.** Each is answered only through a `decision` issue, which produces exactly one ADR in `docs/adr/`. If an implementation task appears to require an answer to one of these, stop and open a `decision` issue (see the `opening-a-decision-issue` skill).

Nothing on this list may be silently closed by a PR. Adding an entry is likewise a deliberate act — a new open question is registered here only after it is recognized as one, not invented to defer ordinary work.

---

## 1. Registry registration ergonomics

Should components be registered **explicitly in the binary** (verbose, but everything is visible at the composition root) or through a **helper** that reduces the boilerplate?

- **Frozen constraint:** there is **no static global** registry (INV-6). Any answer must keep the registry on an explicit `EngineContext`.
- **Blocks:** the ergonomics of every binary that wires components. It does not block the core engine work — a verbose explicit registration is a valid interim.

## 2. Controller implementation language

Go or Rust for the Kubernetes controller?

- The network boundary is clean either way: the controller only translates a custom resource into wire configuration and pushes it over the purpose-built gRPC service.
- `ragondin-controller` **deliberately does not exist yet.**
- **Blocks:** the cloud-native milestone (M6).

## 3. `PhysicalPipeline` serializability

`PhysicalPipeline` holds trait objects (`Box<dyn ...>`) today and is therefore not serializable. Does it stay that way?

- The answer affects **debugging** (can a resolved plan be dumped and inspected?) and **caching** (can a physical plan be persisted?).
- **Blocks:** any feature that would need to serialize a resolved plan. It does not block execution, which does not require serializing the physical level.

## 4. `components/` granularity

One crate **per component family**, or one **per implementation**?

- To be decided **from use, not up front** — split only when a real seam proves itself.
- **Blocks:** nothing yet; it is a question to answer as the number of components grows, not a prerequisite for the first ones.

## 5. Does indexing share the IR formalism?

Should indexing be expressed in the **same pipeline formalism** as serving?

- **Current leaning: yes** — one formalism, two graphs (a batch indexing graph and an online serving graph). This would make **indexing strategies themselves** (GraphRAG vs RAPTOR vs naive chunking) benchmarkable as experiment variables, on the same footing as retrieval strategies, and would extend the skew-free guarantee to the indexing path.
- This is the **least-settled part of the architecture** and **must be formally validated before the custom-benchmark milestone (M7).**
- **Analysis on record:** #111 sketches what an indexing graph would look like if the answer were yes, and names the four frictions that sketch surfaces and what one formalism would buy. It is a record and resolves nothing: read it before settling this question, rather than starting from zero.
- **Frozen constraint:** the question is open; the rules it would be answered under are not. Any answer is bound by all four of these, and none of them is stated for the first time here.
  - **Adding a `ValueKind` variant is a breaking change on a stable boundary (INV-1).** `ValueKind` is part of `ragondin-pipeline`'s INV-1 stable surface, and [`core/ragondin-pipeline/ARCHITECTURE.md`](../core/ragondin-pipeline/ARCHITECTURE.md) records that it is deliberately **not** `#[non_exhaustive]`, so an added `ValueKind` variant is "a visible, deliberate act on this boundary, not a silent one". Any answer that needs new kinds must take that break as INV-1 requires — a deliberate, versioned act, never a side effect of another change.
  - **Kinds are coarse and parameterless, and that is an accepted ADR (ADR-C16).** [ADR-C16](adr/ADR-C16-erased-edge-values-checked-before-execution.md) decides that `ValueKind` is "deliberately **coarse**" and carries "**no parameters**", and says in as many words that "a parameterised kind system is a materially larger decision, and it is not taken here". Any answer that needs parameterised kinds reopens that decision, and `docs/adr/README.md` allows exactly one way to do that: a superseding ADR, never a side effect of implementation work.
  - **Compatibility is "derived, never declared" (ADR-C16), which is what keeps ports out of the canonical hashed form (INV-8).** ADR-C16 satisfies INV-8 by *deriving* a node's port kinds from its `LogicalNode` variant rather than letting a configuration declare them, and rejected declared port types precisely because they would enter the canonical hashed form and rehash stored configurations. Any answer must keep port kinds out of that form, so INV-8 and every stored `run_id` stay untouched.
  - **The executor half is cheap in API terms, because `ragondin-engine` is internal (INV-2).** `NodeValue` and the execution model live inside `ragondin-engine`, which INV-2 says is not an API boundary and never will be; ADR-C16 records that INV-2 is what keeps `NodeValue` freely refactorable. Whatever an answer asks of the execution model costs no stable API — which is worth knowing beside the three constraints above that do.
- **Blocks:** the custom-benchmark milestone (M7).

## 6. Data-plane cache policy

Local caches (embedding, retrieval, context prefix) must remain **reconstructible optimizations** and never a source of truth, to preserve the pure-compute data plane.

- The **invalidation model is undefined.**
- **Blocks:** the introduction of any data-plane cache. Until settled, no cache may be treated as authoritative.

## 7. Visual control-flow rendering

Node-based editors conventionally manipulate **acyclic** graphs. This IR contains **branch and bounded-loop** nodes. Rendering control flow visually is a **hard interface-design problem**, not an implementation detail.

- **Blocks:** visual graph authoring — the post-M7 visual-authoring phase of the roadmap (see `docs/AGENT_WORKFLOW.md`). The read-only graph replay view is unaffected and remains part of the core.
