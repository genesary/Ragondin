# ARCHITECTURE — ragondin-conformance

**Status: test infrastructure, not one of the three stable API boundaries
(INV-1).** But it is what #18–#24 compile against, and their tests assert on the
check names it returns, so a rename is a break for every component crate. Treat
the family functions and the check-name strings as fixed; the modules behind
them are free.

That has been broken **once**, deliberately, and this is the record of it.
ADR-C17 gave `check_embedder_conformance` / `assert_embedder_conformance` a
second parameter, `RolePrefixes`, because the suite cannot know whether the
fixture in front of it was configured with distinct per-role prefixes — a
symmetric embedder answering both roles alike is correct — so the caller
declares it, the same precedent `check_vector_store_conformance(make, dim)`
already set: what the suite cannot know, the caller states in the argument
list. No embedder crate existed when it landed, so nothing broke. The
precedent is the *reason*, not a licence: a family function's argument shape
changes only when a check needs a fact no fixture can supply from inside.

## What lives here

The behavioural suite **every** component implementation must pass, whatever its
nature (`docs/code-architecture.md` §7.4). One `check_*` / `assert_*` pair per
M2 trait family: `Retriever`, `Fusion`, `Reranker`, `Embedder`, `VectorStore`.

This is the operational enforcement of **INV-7**. A built-in component and a
third-party one call the same function, with the same argument shape, and earn
the same guarantee. The crate's dependencies are exactly the surface a
contributor compiles against — `ragondin-contracts` and `ragondin-types`, plus
`thiserror` — so nothing here is reachable by a first-party crate and not by one
outside this workspace.

## Local invariants

- **A check must hold for every correct implementation.** The suite knows
  nothing about the corpus, index or model behind a trait object. A check that a
  legitimate component could fail is worse than a missing one: it blocks a
  contribution and teaches contributors that conformance is noise. This is why
  no family requires a *non-empty* result, and why the retriever suite asserts
  nothing about *which* chunks come back.

- **Two families are exempt from that constraint, and only two.** The suite owns
  a `Fusion`'s entire input — it supplies the lists — and a `VectorStore`'s
  entire content — it writes the vectors before it reads them. There, and only
  there, the suite may say what a correct answer *is*. A vector store must
  therefore be handed a store the suite exclusively owns and that starts empty;
  that precondition is stated on the function, and it exists because under an
  unnormalised metric any pre-existing vector of larger magnitude outranks an
  inserted unit vector, and no probe the suite could build is guaranteed to win.

- **Conformance is a floor, never a proof of usefulness.** Contract behaviour,
  not quality — a conformant reranker need not be a *good* reranker. Retrieval
  quality is `ragondin-metrics` against a benchmark. A component that returns
  nothing, everywhere, is conformant.

- **Every check needs a stub that proves it fires.** A conformance suite that
  cannot fail makes INV-7 a slogan again, which is the one thing this crate
  exists to prevent. `tests/conformance_suite.rs` carries one deliberately
  broken stub **per check name**, not per family, and each test asserts on
  `failure.check()` — proving the suite *discriminates*, not merely that it
  fails. A check added without one is unenforced by construction.

- **Failures are returned, not panicked.** `ConformanceFailure` is what makes
  the suite testable by its own tests; the `assert_*` wrappers turn one back
  into a panic for the ordinary caller. Both forms are public because both are
  used.

- **No component implementation ships from this crate** (#17, Scope — OUT). The
  stubs live in `tests/`, never in `src/`, so they are not part of the public
  surface and cannot become the privileged implementation INV-7 forbids.

- **Keep it light.** A contributor pulls this crate into their dev-dependencies
  to earn a conformance guarantee; it must not drag a runtime or a backend in
  with it. The functions are `async` and start no runtime of their own, and
  `tokio` and `async-trait` are dev-dependencies — the suite *calls* the
  contract traits, it does not declare them.

## Where each clause comes from

The suite enforces nothing it invented. Each clause is either stated in
`ragondin-contracts`, or stated in issue #17, or generalised from a sentence
written on the shared error type — and the third kind is listed here explicitly
so a reviewer can see the reasoning rather than reconstruct it.

| Clause | Grounding |
|---|---|
| descending, finite scores | `ragondin-contracts`' ranking contract, which names this crate as its enforcer |
| at most `top_k` | #17, Scope — IN |
| no fabricated ids; empty inputs; single-leg order; own nearest neighbour | #17, Scope — IN |
| `upsert` replaces by chunk id | `VectorStore::upsert`'s own documentation |
| finite embedding components | `ragondin-types`, on `Embedding` |
| **one dimensionality, across both roles** | `Embedder`'s *One embedding space* clause. The per-batch half predates the role; ADR-C17 is what made two widths expressible at all, so the clause was written onto the trait in the same change that widened the check — the suite still enforces nothing it invented. Deliberately **not** a caller-declared `dim` argument the way `VectorStore` takes one: the suite can observe the width itself, and a family function's argument shape changes only when a check needs a fact no fixture can supply from inside. |
| **a `top_k` of zero is an invalid request** | stated for `Retriever` on `RetrieveParams::new`; **generalised** to `Reranker` and `VectorStore::search` from `ComponentError::InvalidRequest`'s documentation, which names it as the example and is written on the error every boundary returns. #89 mirrors the sentence onto the two params structs that lack it. |
| **the role changes the vector** | ADR-C17 and #97, which require the suite to exercise both roles and to check that a fixture *declaring* distinct per-role prefixes honours them. Opt-in by construction: the ADR states the suite cannot detect a wrong role and must not pretend to. |
| **no duplicate ids** | **generalised**: a ranked list ranks each chunk once. Checked only for `Fusion` and `Reranker`, whose whole input the suite knows, so that "this id appears twice" is a statement about the component and not about a corpus. |

## What the suite deliberately does not decide

Two behaviours on which the repository currently contradicts itself. The suite
certifies both answers rather than picking one, and says so where the check
would have gone:

- **A zero-dimensional embedding** — #90. `ragondin-types` says an empty
  embedding is representable; `ragondin-contracts`' own stub treats one as an
  invalid request.
- **An empty `upsert`** — #91. That same stub rejects it, while the neighbouring
  families all treat an empty input as success.

Nor does it decide **which prefix an asymmetric embedder should apply**. ADR-C17
put the role on `EmbedParams`, so the suite exercises every embedder check under
both `EmbedRole`s — a contract broken on one side only is still broken. Beyond
that it can say one thing and no more: where a caller *declares*
`RolePrefixes::Distinct`, one text must not embed identically under the two
roles, which catches an implementation that accepts the role and ignores it.
Whether the prefix it applied was the *right* one is unknowable here, since the
suite does not know the model — and a symmetric embedder answering both roles
alike is correct, which is why the check is opt-in rather than universal.
