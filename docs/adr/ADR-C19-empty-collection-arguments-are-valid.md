---
id: ADR-C19
title: An empty collection argument is a valid call, not an invalid request
status: accepted
invariants: [INV-1, INV-4, INV-7]
supersedes: []
superseded_by: null
---

# ADR-C19: An empty collection argument is a valid call, not an invalid request

## Context

Four methods on `ragondin-contracts` take a collection: `Fusion::fuse(inputs)`,
`Reranker::rerank(_, chunks, _)`, `Embedder::embed(texts, _)` and
`VectorStore::upsert(entries)`. The contract says nothing about what any of them
does when that collection is empty. `upsert` is documented as *"Inserts or
replaces `entries`, keyed by their chunk ids"* and stops there.

The repository is not silent on the question, though — it is **contradictory**,
and the contradiction sits inside a single crate. `ragondin-contracts`' own
`StubStore` rejects an empty batch with
`ComponentError::InvalidRequest("nothing to upsert")`. That stub is a test
fixture and normative of nothing, but it is the only worked example of the trait
inside the crate that defines it, and it is what a contributor reads to learn the
shape.

Every neighbouring family already answers the other way, and not merely by
convention: `ragondin-conformance` **enforces** it. `check_fusion_conformance`
calls `fuse` with no input lists and with two empty ones; `check_reranker_conformance`
calls `rerank` with an empty chunk list; `check_embedder_conformance` calls
`embed` with an empty batch under both roles, and its documentation states that
an empty batch "must embed to no vectors rather than fail". Each of those calls
propagates an error as a conformance failure, so an implementation that rejected
an empty input would already fail the suite today. Three of the four
collection-taking methods are therefore settled and enforced; `upsert` is the one
that is neither, and the one worked example of it points the opposite way. Every
other `upsert` in the tree already assumes the no-op: `ragondin-store-memory`,
the one shipped `VectorStore` component, treats an empty batch as a
no-op and recorded that as a local choice rather than an answer; and
`ragondin-engine`'s registry test calls `upsert(Vec::new())` and expects it to
succeed. The stub in the contract crate is the only thing in the repository that
says otherwise.

What is at stake is not style. `ComponentError::InvalidRequest` is documented as
*"a precondition of the call is unmet"*, and the conformance suite already
enforces one such precondition — a `top_k` of zero — across every family that
takes a `top_k`. Whether "a non-empty batch" is likewise a precondition decides
whether an indexing loop that reaches a page boundary with nothing left must
guard its call, for every store in and out of this workspace.

Leaving it open has a specific cost, and it is the cost that forces the decision.
A conformance suite that certified both answers would have to accept both, which
means a caller can rely on neither: the same call would be a no-op against one
store and an error against another, and the engine cannot tell which face it
called. That is precisely the `Local`/`Remote` substitutability
[ADR-3](ADR-003-two-faced-component-contract-local-remote.md) promises and
[ADR-C6](ADR-C06-identical-api-plus-conformance-suite.md) exists to verify.

## Decision

**An empty collection argument is a valid call, and the component does nothing
with it.** `VectorStore::upsert(vec![])` returns `Ok(())` and changes no state.
The rule is general and reads identically on every collection-taking method:
`Fusion::fuse(vec![])`, `Reranker::rerank(_, vec![], _)` and `Embedder::embed(&[], _)`
return an empty collection rather than an error. Empty in, empty out — and for
`upsert`, whose return carries no data, empty in, nothing done, success.
`Fusion::fuse` is the one method whose argument nests, and **both of its
degenerate shapes are covered**: no lists at all, `fuse(vec![])`, and lists that
are all themselves empty, `fuse(vec![vec![], vec![]])`. An input carrying no
elements at any level is empty for this rule, and the fusion of nothing is
nothing. An empty
collection is **not** an unmet precondition: `ComponentError::InvalidRequest` is
for a call that cannot be honoured as made, and a request to store no entries is
honoured by doing nothing. This does not touch the `top_k` clause, which stands
unchanged: a `top_k` of zero remains an invalid request for every family that
takes one, because a zero `top_k` asks for a *result* that cannot exist, while an
empty collection asks for a state change that is trivially satisfiable.

The rule binds **both faces** (ADR-3). An empty repeated field is the natural
protobuf encoding of an empty collection and needs no special case, so a `Remote`
component gets the behaviour for free — and, symmetrically, may not reject it.
The contract states the rule where an implementer reads it, in
`ragondin-contracts`, so that it is a documented obligation rather than an
inference from the neighbours.

## Alternatives rejected

- **An empty `upsert` is an invalid request** (option B of the decision issue).
  Its stated merit — that nothing in the repository changes — does not survive
  contact with the tree: it matches the contract crate's stub, and contradicts
  `ragondin-store-memory` and `ragondin-engine`'s registry test, both of which
  would have to change instead. Everything else counts against it too. It makes
  `upsert` the only collection-taking method in the crate with a rejection rule
  — the behavioural counterpart of the per-method exception
  `core/ragondin-contracts/ARCHITECTURE.md` warns against, where it closes the
  params-struct rule: *"The uniformity is the point: an exception is where the
  next knob will land."* That sentence is about signatures rather than
  behaviour, so it is an analogy and not the same rule; the mechanism it names
  is what carries over, because a caller that learns to guard one method guards
  all four. It rests on an analogy to `top_k = 0` that does not hold: the
  reason a zero `top_k` is rejected is that answering it with an empty list makes
  a caller's arithmetic bug look like an empty corpus, and there is no such
  confusion to create here, because an empty `upsert` returns no data to be
  mistaken for anything. It forces every batching caller — the harness, an
  indexing driver, every third-party client — to guard a call that the component
  could satisfy trivially. And it is the harder answer for a `Remote` store,
  which would have to add a check to reject what its wire format encodes
  naturally.

- **Leave it unspecified and let each store choose** (option C). This is the
  status quo, and it is the one option the conformance suite cannot express: the
  suite would have to accept both outcomes, which certifies neither. A guarantee
  that a call succeeds *or* fails is not a guarantee, and a contributor plugging
  a store into the suite would earn nothing for this behaviour. It defeats the
  purpose of a suite whose job is to make `Local` and `Remote` substitutable.

- **Make an empty collection unrepresentable** — a non-empty vector type on the
  boundary, so the compiler rules the case out. It answers the question by
  deleting it, at a price the boundary cannot pay: changing an argument's type is
  a breaking change to a stable API boundary (INV-1) for every implementation in
  and out of the repository. It also has no mirror on face 2 — proto3 cannot
  express a non-empty repeated field — so a `Remote` component would have to
  re-check at runtime anyway, and the two faces would then disagree about where
  the rule lives, which is the one thing ADR-3 does not allow.

- **Succeed, but signal** — return a count of entries written, or emit a warning.
  Returning a count changes `upsert`'s return type, which is the same breaking
  change as above for a fact the caller already has: it knows how many entries it
  passed. A warning would put an operational log inside the crate that must stay
  light (INV-4) and would fire on a correct call, which is how warnings get
  filtered out. The cost this alternative tries to buy off — silence when a batch
  is emptied by accident — is real and is accepted below.

## Consequences

- **The decision is doc-only on the public API, and breaks nothing.** No
  signature, type, field or trait changes; INV-1's boundary is untouched. What
  changes is a documented obligation, which is why the statement in
  `ragondin-contracts` is part of this decision rather than a courtesy.

- **`ragondin-contracts`' own stub now contradicts the contract it demonstrates,
  and this ADR does not fix it.** `StubStore::upsert` still returns
  `InvalidRequest("nothing to upsert")`, and now says at the call site that it
  does so against this decision. Making the decision and changing behaviour are
  deliberately separate acts, so the code changes are listed here and belong to a
  follow-up implementation issue (#176):
  1. `StubStore::upsert` in `core/ragondin-contracts/src/lib.rs` returns `Ok(())`
     for an empty batch. (No test asserts on that message today — the message
     assertion in `a_component_error_crosses_the_trait_object_boundary` is about
     `search`, not `upsert` — so the change is the stub and a test that pins the
     new behaviour through the trait object.)
  2. `ragondin-conformance` gains the scenario it deliberately omitted: `upsert`
     with an empty vector must succeed. That is a new stable check name, so it
     also needs its deliberately-broken stub in `tests/conformance_suite.rs` —
     the crate's rule is that a check added without one is unenforced by
     construction — plus a row in the check-name table in `src/lib.rs` and one in
     the *"Where each clause comes from"* table in `ARCHITECTURE.md`, whose
     *"What the suite deliberately does not decide"* section also names this
     question and must stop doing so.
  3. `components/ragondin-store-memory` needs no behaviour change — it already
     treats an empty batch as a no-op — but its doc comment and `ARCHITECTURE.md`
     both say that choice is "not an answer to #91", and after this ADR it is the
     contract's answer rather than a local one.

- **A batch emptied by accident is silent.** A caller that filters a page down to
  nothing gets success rather than a signal. This is the one real cost of the
  decision and it is accepted: the alternative is a guard at every call site, in
  every store, forever, to catch a caller bug the component is not positioned to
  diagnose.

- **The Qdrant store (#24) is unblocked**, and is pinned by the conformance
  scenario above rather than by its own judgement. `ragondin-store-memory` needs
  no behaviour change; only the prose recording its choice as local does. The
  `Remote` adapter (#13) must preserve the
  answer across its round-trip: an empty repeated field decodes to an empty
  collection and is passed through, never rejected at the adapter. That costs
  nothing today — `ragondin-proto` and `ragondin-remote` are compiling skeletons
  with no `.proto` file and no adapter yet — which is the cheapest moment to fix
  the answer the mirror has to carry.

- **INV-7 is preserved, and the suite is what enforces it.** The rule is one
  sentence on the one API every component implements; it creates no path a
  built-in has and a third party does not, and both earn the same guarantee from
  the same scenario.

- **The `upsert` params-struct exception is untouched.** `ARCHITECTURE.md`
  records that `upsert` is the one trait method taking no params struct, and that
  its first per-call knob cannot arrive as a field. This decision adds no knob and
  neither closes nor widens that gap.

- **The zero-dimensional-embedding question (#90) is independent and is not
  settled here.** Whether an `Embedder` may return an embedding of width zero is
  a question about an *element*, not about the collection: this ADR fixes what
  happens when there are no elements, and says nothing about what one may
  contain. `embed(&[], _)` returning no vectors and `embed(&["".to_string()], _)`
  returning one empty vector are different questions with different answers, and
  the ADR that resolves #90 is where the second is answered.

- **`docs/OPEN_QUESTIONS.md` is unchanged.** This question was never registered
  there; it lived as a decision issue and as the contradiction described above.

## Status

Accepted.
