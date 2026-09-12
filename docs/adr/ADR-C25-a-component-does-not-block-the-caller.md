---
id: ADR-C25
title: A Local component does not block the calling thread
status: accepted
invariants: [INV-4]
supersedes: []
superseded_by: null
---

# ADR-C25: A `Local` component does not block the calling thread

## Context

Every component method in `ragondin-contracts` is an `async fn` behind
`async_trait` (ADR-C8), and the engine cannot tell a `Local` implementation
from a `Remote` one (ADR-3). What a `Remote` component does inside that method
is a network round-trip, which yields. What a `Local` one does is arbitrary
in-process work, and the contract has so far said nothing about whether that
work may run on the thread that called it.

The first heavy `Local` component made the silence load-bearing.
`ragondin-embedder-onnx` (#20) runs its ONNX forward pass synchronously inside
`Embedder::embed`, behind a `Mutex` over the session, and its `ARCHITECTURE.md`
argues the choice locally: `spawn_blocking` *"would pick a runtime for a
library"*. That reasoning is sound for a library considered on its own. It is a
different question for a component the serving driver will call from inside a
Tower stack on a `tokio` worker, where a 50 ms forward pass stalls every other
future scheduled on that thread, and a mutex around inference caps the
component at one call at a time whatever concurrency limit the envelope
advertises.

Nothing observes this today. The evaluation driver embeds a corpus once and
runs its queries in sequence, so the M2 bench would produce the same numbers
either way. Three things make it a decision that cannot wait for the driver
that does observe it:

- **A component cannot decide a contract question on the contract's behalf.**
  ADR-C20 made exactly this point about `ragondin-store-memory` answering for
  every `Embedder`. An implementation's `ARCHITECTURE.md` records what *it*
  does; it cannot bind the next implementer.
- **The next implementer is already scheduled.** `ragondin-reranker-onnx`
  (#21) has the identical shape — a model, a forward pass per call — and would
  otherwise copy whichever precedent it found.
- **The documents that touch the question disagree about its scope.**
  `docs/code-architecture.md` §11.2 selects `tokio` at the binary level and
  asks libraries to stay runtime-agnostic *"as practical"*, which does not say
  whether blocking is inside that qualifier; the same section places continuous
  batching *"inside the components concerned, behind the trait — invisible to
  the engine"*, which points the other way.

Decided in #198.

## Decision

**A `Local` component does not block the thread that called it.** An
implementation whose work is CPU-bound or otherwise blocking — a model forward
pass, a synchronous disk read, a lock held across either — moves that work off
the caller's thread itself, and returns a future that yields like any other.

**The contract states the obligation and not the means.**
`tokio::task::spawn_blocking`, a dedicated thread with a channel, or a backend
that is already non-blocking are all conformant, and the choice is the
component's — recorded in its `ARCHITECTURE.md` like any other leaf choice. No
runtime is named in `ragondin-contracts` and no dependency is added to it
(INV-4); a component that reaches for `tokio` declares it itself, as a
component-level dependency behind its own feature where the rest of its
backend already sits.

The obligation is stated where an implementer reads the contract: the crate
documentation of `ragondin-contracts` and its `ARCHITECTURE.md` local
invariants. It is **review-enforced**. No conformance test checks it, and the
suite must not be read as though it did.

**This governs the contract's `async fn`s, and not construction.** A component
is built by a `ComponentCtor`, which is a synchronous
`Fn(&Params) -> Result<Box<T>, ConstructionError>` called at physical planning
— and loading an ONNX model or building a tantivy index there blocks whatever
thread planned. That is deliberately left outside this decision, for two
reasons: a constructor runs once per plan rather than once per call, so it
cannot stall a thread repeatedly; and a synchronous function cannot move its
own work off-thread and hand back a value, so the fix is not the component's to
make and would have to change how a driver plans. It becomes observable when a
serving driver plans on a worker thread, and it belongs to the driver that has
that problem — with the plan-time surface it would touch, that is an escalation
of its own rather than a clause here.

## Alternatives rejected

- **The contract permits blocking; the caller hops.** The executor or the
  serving driver wraps each component call in `spawn_blocking` or its runtime's
  equivalent. Rejected because it moves runtime knowledge into the engine,
  which §11.2 wanted kept at the binary, and because ADR-3 denies the engine
  the one piece of information that would let it wrap selectively: it cannot
  tell a `Local` component from a `Remote` one, so it would pay a thread hop on
  every call — a fusion that sorts a list, a gRPC round-trip that already
  yields — to serve the two components that need it.
- **The contract stays silent; each component documents its own choice.** The
  status quo, and free today. Rejected because its cost is paid by whoever
  assembles a pipeline out of components that chose differently, in a driver
  that was not written to expect either; and because "silent" is not neutral —
  the first implementation becomes the precedent the second copies, which is a
  contract decided by order of arrival.
- **A dedicated inference pool, owned by the composition root and handed to
  components at construction.** The most controlled answer, and the one that
  would let a deployment size its inference threads deliberately. Rejected as
  machinery before measurement: it adds a type at or near the contract
  boundary, and a construction-time argument every heavy component must accept,
  before any bench has measured the contention it would relieve. This decision
  does not foreclose it.
- **Mechanize the rule in the conformance suite.** Rejected because the
  property is not observable from outside the call. Detecting that an `async fn`
  blocked its executor means watching the runtime — a worker that stopped
  polling, a timer that fired late — which is timing-dependent, flaky under
  load, and indistinguishable from a component that is simply fast. The suite
  gates what a component computes; this rule is about how it gets there.

## Consequences

- **A heavy component owns its own hop.** `ragondin-embedder-onnx` (#20) is
  corrected where it is written rather than merged and amended: the hop, and
  the `ARCHITECTURE.md` paragraph that argues against it. `ragondin-reranker-onnx`
  (#21) starts conformant, which is why this was decided before it rather than
  after it.
- **`tokio` may appear in a component crate; it does not reach the core.**
  INV-4 is untouched — the dependency, where a component chooses that means,
  is the component's own, and `tokio` is already declared in
  `[workspace.dependencies]`. A component that prefers its own thread and a
  channel is equally conformant, and depends on neither.
- **`spawn_blocking` is conformant and not free of consequence: it requires an
  ambient `tokio` runtime and panics without one.** A component that chooses it
  therefore requires its *caller* to be running under `tokio`, which this
  contract does not say and the engine cannot check. That is acceptable — §11.2
  selects `tokio` at the binary, both drivers use it, and the conformance suite
  runs on it — but it is a property of the component, not of the contract, so
  it is stated in that component's `ARCHITECTURE.md` alongside the choice. A
  component that must be callable under any runtime picks the other means: its
  own thread and a channel, which depend on no runtime at all.
- **The engine stays runtime-agnostic**, and a serving driver's concurrency
  limit means what it says: the futures it schedules yield, so a limit of *n*
  is *n* calls in flight rather than *n* claims on a worker pool that one
  component is holding.
- **It is review-enforced, and the sign in a diff is specific.** CPU-bound work
  — a forward pass, a synchronous file read, a `Mutex` guard held across either
  — inside an `async fn` of a component trait, with nothing moving it off the
  caller's thread. A green conformance run is not evidence about this rule.
- **Concurrency inside a component is a separate question, and stays open.**
  Moving a serialized section off the caller's thread stops it stalling the
  executor; it does not make it concurrent. Whether a component holds one
  session under a mutex or a pool of them is its own recorded choice, and
  §11.2's continuous batching is that choice too.

## Status

Accepted.
