# ARCHITECTURE — rag-server

**Status: internal.** One of the two thin drivers over the engine.

## What lives here

The serving driver: the ingress and the Tower stack that wraps
`Engine::execute`.

```text
ingress → [Tower: timeout | concurrency-limit | retry | metrics] → Engine::execute → response
```

## Local invariants

- **Tower governs the network envelope only (INV-11).** Timeouts, retries,
  concurrency limits, load shedding, backpressure on `Remote` calls,
  instrumentation — the cross-cutting network concerns a mature service-mesh
  data plane has already hardened. Nothing domain-specific belongs in the Tower
  layer.
- **Components are never `tower::Service`s.** Tower's `Service<Request>` is
  *uniform*: one request type, one response type. RAG components are
  *heterogeneous* — a `Retriever` and a `Generator` share neither input nor
  output. Forcing the latter into the former would destroy the legibility of the
  domain contracts and gain nothing. Each abstraction at its own layer.
- **Thin driver, shared engine (P1).** `rag-server` and `rag-harness` wrap the
  **same** `rag-engine`. There is one execution path; Cargo proves it because
  both depend on the same engine crate. This is what makes serving/evaluation
  skew structurally impossible.
