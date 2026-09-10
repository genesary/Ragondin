---
id: ADR-C20
title: A zero-dimensional embedding is not a valid Embedder output
status: accepted
invariants: [INV-1, INV-7]
supersedes: []
superseded_by: null
---

# ADR-C20: A zero-dimensional embedding is not a valid Embedder output

## Context

`Embedding` is a newtype over `Vec<f32>` with an infallible constructor, so an
embedding with **no components** is buildable and reports `dim() == 0`. Whether
an `Embedder` may *return* one is a different question from whether the type can
*hold* one, and the repository answered the two as if they were the same — twice,
in opposite directions.

`core/ragondin-types/src/lib.rs` argues on `Embedding` that an empty embedding is
representable, that rejecting it would require a fallible constructor and an error
type in a crate that deliberately has none, and that "a dimensionality
disagreement is caught where it is meaningful — by the vector store being
searched." `core/ragondin-contracts/src/lib.rs` demonstrates the opposite in the
contract crate's own reference behaviour: `StubStore::search` returns
`ComponentError::InvalidRequest("an empty embedding has no direction")`, and a
test in the same file asserts that message. Neither file is subordinate to the
other, and neither addresses the **embedder** — the component that produces the
value.

The gap is executable. `check_embedder_conformance`, in the shared conformance
suite every implementation must pass (ADR-C6), enforces three properties, and an
embedder that returns an empty vector for every text satisfies all three: the
batch comes back with one vector per input, the widths are constant because
`0 == 0`, and "no non-finite component" is vacuously true of a vector with no
components. A completely broken embedder is certified conformant — under
`RolePrefixes::Undeclared`, which is what a fixture declares unless it was
configured with distinct per-role prefixes; under `RolePrefixes::Distinct` the
role-separation check happens to catch it, since two empty vectors are equal.
Resting on that accident is not a contract. The suite's own doc comment says the
check is deliberately absent and points at this question.

Downstream, one component has already had to answer it locally.
`ragondin-store-memory` refuses a width-zero entry on `upsert`, because a
width-zero opening batch would fix the store at a width no later vector and no
query can match. Its `ARCHITECTURE.md` is careful to call that a choice about
what *this store will hold* rather than an answer to what an embedder may
*return* — correctly, since a component cannot decide the contract. So the
platform already behaves as though zero were illegitimate, without anything
saying so.

What goes wrong if the question stays open: #20 is the first real embedder, and
its conformance test either checks this or does not, arbitrarily;
`ragondin-retriever-dense` holds an `Embedder` and a `VectorStore` as trait
objects and consumes whatever the first emits; and #33, the M2 exit criterion,
compares hybrid retrieval against a dense leg that could be degenerate while every
test in the workspace stays green. That is the same failure shape ADR-C17 was
written to prevent — a wrong number, reproducibly produced, with nothing red.

## Decision

**An `Embedding` returned by an `Embedder` carries at least one component.** A
zero-dimensional embedding is a violation of the `Embedder` contract, not a
degenerate-but-legal value, and the conformance suite must reject an embedder
that produces one. An implementation that cannot embed a text returns a
`ComponentError` — that is what the error channel is for — rather than a vector of
width zero, which reports failure as data no caller can distinguish from a result.
The rule is about a **returned vector**, not about a batch: an empty batch still
embeds to no vectors, which is a different statement and is untouched here (#91).

The tension between the two files is resolved by narrowing, not by overruling
either: **an empty embedding is *representable* by the type, but is not a valid
component *output*.** `ragondin-types` keeps its infallible constructor and its
reason for it — a value type in a crate with no error type cannot reject anything
— and its comment is narrowed to say that representable is not the same as valid.
`ragondin-contracts` states the rule on the `Embedder` trait, where an implementer
reads it, which is also what makes its own stub's rejection of an empty search
vector a consequence of the contract rather than a stub's private opinion.

## Alternatives rejected

- **It is legal; validation belongs to the vector store** — the position
  `ragondin-types` argued, and the option that changes nothing. It catches the
  failure one component too late and attributes it to the wrong one. Concretely,
  the value is produced by an embedder and refused by a store: today
  `ragondin-store-memory::upsert` returns `InvalidRequest` naming *the chunk id*
  whose embedding has no components, because the chunk is all the store has. The
  failure therefore surfaces during **indexing**, after a run has started, as a
  fault of the store's caller. For a `Remote` embedder it is worse: the store's
  error names nothing about which embedder produced the vector, nothing about the
  service behind it, and the operator is left correlating an indexing error
  against a component that is not mentioned in it. Deferring to the store also
  makes conformance a certificate that means less than it appears to, which is a
  cost paid by every third party that trusts it (ADR-C6).
- **Reject it at the type — a fallible `Embedding::new`.** This is the honest way
  to make the value unconstructible, and it is unavailable here. It puts an error
  type in `ragondin-types`, which deliberately has none; and it breaks every
  construction site on an INV-1 boundary for a check that belongs to a
  component's behaviour rather than to a value's shape.
- **Dimensionality as a property of the registered embedder, checked at
  planning** — rejected *for now*, and it is the better answer to the larger
  problem. It catches not only zero but the failure the platform actually has: an
  embedder whose width does not match the store's, which today surfaces only as a
  `VectorStore` rejecting a search vector, as `Embedder`'s own *One embedding
  space* clause already admits. It is rejected here for three reasons and none of
  them is that it is wrong. First, it presupposes a trait method that reports a
  dimensionality, and no method on `Embedder` does: adding one is a signature
  change on an INV-1 boundary — a second decision, of a size this question does
  not authorize. Second, the physical-planning pass cannot host the check as
  things stand: `resolve` matches a retriever, a fusion and a reranker, and
  `ResolvedComponent` has no variant for an embedder or a vector store, because
  neither is a pipeline node — the two are wired into a dense retriever at the
  composition root. Making planning see both is therefore a third change, not a
  detail of the second. Third, on the assumption this platform is built on — that
  a `Remote` embedder is a service the engine calls rather than interrogates, and
  that a component's registration carries no width — its dimensionality is not
  knowable before a call, so a planning-time check would not see it and the run
  would still reach an index with degenerate vectors in it. The two are
  compatible rather than exclusive — a per-call floor
  of one component holds whatever a later width-agreement check does — so this
  decision does not foreclose it. **Whoever opens that decision should read this
  section first: the option was weighed and deferred, not overlooked.**

## Consequences

- **`ragondin-contracts` states the rule, additively.** The `Embedder` trait's
  documentation gains the clause that a returned vector has at least one
  component, and `ARCHITECTURE.md` records it alongside the role clause. This is
  a doc-only change on an INV-1 boundary: no public item is added, removed,
  renamed or re-typed, so nothing in or out of the repository breaks.
- **`ragondin-types`' comment is narrowed, and its decision is untouched.**
  `Embedding::new` stays infallible for the reason it always had; the comment now
  distinguishes representable from valid, and stops implying that the store is the
  only place a zero width is ever caught.
- **The check itself is a follow-up implementation issue (#177), not this PR.** Following
  the #5 → ADR-C16 and #46 → ADR-C17 precedent of separating a decision from its
  implementation, `testkit/ragondin-conformance` is untouched here. What that
  issue owes: in `check_embedder_conformance`, a rejection when a returned vector
  has `dim() == 0`, under a new stable check name, placed where the width is first
  read so that the failure is reported as a width-zero output rather than as a
  disagreement with a later batch; and a test with a fixture embedder returning
  `Embedding::new(vec![])`, proving the suite has teeth on this.
- **Two passages in the conformance crate stop being true on merge, and the
  reason matters.** Where they describe the *suite* — that it does not reject a
  zero-dimensional embedding — they stay accurate until the check lands, and that
  is the correct way round: prose describes what the code does. What this ADR
  falsifies immediately is their framing of the *contract* and of the
  *repository*. `check_embedder_conformance`'s "What it still cannot check"
  paragraph says the suite "does not settle a question **the contract leaves
  open**"; the contract no longer leaves it open, it is settled here, and the
  suite is merely behind it. `testkit/ragondin-conformance/ARCHITECTURE.md` opens
  its "What the suite deliberately does not decide" section with "Two behaviours
  on which **the repository currently contradicts itself**"; the repository now
  contradicts itself about one behaviour, not two. #177 must
  rewrite the framing as well as add the check — a deliberate hole in the suite
  under a decided contract, not a suite declining to pick a side.
- **`ragondin-store-memory`'s local refusal is now the contract's rule.** Nothing
  about that crate's behaviour changes, and it need not: its `upsert` already
  refuses a width-zero entry. Its prose does change. `store.rs` calls the refusal
  a local choice and "not an answer to #90, **which asks** what an `Embedder` may
  legitimately return", and `ARCHITECTURE.md` adds that it would be this store's
  answer "**whichever way** #90 goes" — a present-tense question and a
  counterfactual, both of which this decision retires. That is not cosmetic: it
  leaves a closed issue cited in a `.rs` file as live work, which is one of the
  three prose defects `AGENTS.md` names, and `scripts/gen-map.py`'s closed-issue
  rule scans Rust, so `just map --conflicts` will report `store.rs` repository-wide
  from the moment this merges. Both passages belong to #177's
  scope, not to whoever next happens to touch the crate.
- **The rule binds both faces of the contract (ADR-3).** It constrains what an
  implementation returns, whatever its nature, so a `Remote` embedder is bound by
  it exactly as a `Local` one is and no new wire surface is required to say so.
  This ADR does not require the `Remote` adapter (#13) to add a check of its own;
  the conformance suite is the enforcement point, and it exercises a component
  through the trait regardless of what sits behind it (INV-7).
- **It is not in tension with the empty-collection rule decided in parallel
  (#91).** An empty *input collection* is a no-op that succeeds; an empty
  *embedding vector* is a malformed output. The first is a caller asking for
  nothing, and nothing is a coherent answer; the second is a component claiming to
  have answered while returning a value with no direction, against which no
  similarity is defined. One is about the size of a batch, the other about the
  width of a vector, and the two rules meet only in the case both already agree
  on: an empty batch embeds to no vectors, and no vector of width zero is among
  them.
- **The larger question is named, not closed.** Whether an embedder's
  dimensionality must agree with a store's, and where that is checked, remains
  open — see the third alternative above. This decision puts a floor under it and
  nothing more.
- **`docs/OPEN_QUESTIONS.md` is untouched.** This question was never registered
  there; it lived as the contradiction between two files and as a deliberate hole
  in the conformance suite, both of which this ADR closes.

## Status

Accepted.
