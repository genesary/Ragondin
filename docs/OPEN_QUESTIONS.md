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
- `rag-controller` **deliberately does not exist yet.**
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
- **Blocks:** the custom-benchmark milestone (M7).

## 6. Data-plane cache policy

Local caches (embedding, retrieval, context prefix) must remain **reconstructible optimizations** and never a source of truth, to preserve the pure-compute data plane.

- The **invalidation model is undefined.**
- **Blocks:** the introduction of any data-plane cache. Until settled, no cache may be treated as authoritative.

## 7. Visual control-flow rendering

Node-based editors conventionally manipulate **acyclic** graphs. This IR contains **branch and bounded-loop** nodes. Rendering control flow visually is a **hard interface-design problem**, not an implementation detail.

- **Blocks:** visual graph authoring — the post-M7 visual-authoring phase of the roadmap (see `docs/AGENT_WORKFLOW.md`). The read-only graph replay view is unaffected and remains part of the core.
