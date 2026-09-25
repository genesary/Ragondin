# ARCHITECTURE — ragondin-conformance

**Status: test infrastructure, not one of the three stable API boundaries
(INV-1).** But it is what #18–#24 compile against, and their tests assert on the
check names it returns, so a rename is a break for every component crate. Treat
the family functions and the check-name strings as fixed; the modules behind
them are free.

That has been broken **twice**, deliberately, and this is the record of it.
ADR-C17 gave `check_embedder_conformance` / `assert_embedder_conformance` a
second parameter, `RolePrefixes`, because the suite cannot know whether the
fixture in front of it was configured with distinct per-role prefixes — a
symmetric embedder answering both roles alike is correct — so the caller
declares it, the same precedent `check_vector_store_conformance(make, dim)`
already set: what the suite cannot know, the caller states in the argument
list. No embedder crate existed when it landed, so nothing broke. The
precedent is the *reason*, not a licence: a family function's argument shape
changes only when a check needs a fact no fixture can supply from inside.

The second break: ADR-C32 § 4 gave `Embedder` and `Reranker` a
`model_identity(served_model: Option<&str>)`, and gave both suites the two
identity scenarios. So `check_reranker_conformance(make, served_model)` and
`check_embedder_conformance(make, prefixes, served_model)` — with their
`assert_*` twins — gained a last parameter, `served_model: Option<&str>`,
mirroring the generator suite's. Which model a fixture serves is a fact no
suite can know: a `Local` embedder or reranker that loaded one model answers
`None` and may refuse every name, as the ONNX ones do, while a `Remote` one
refuses `None` and answers the names its service serves. The suite passes the
stated value on **every** call it makes — in the `EmbedParams` or
`RerankParams` of each call, not only to `model_identity` — because a `Remote`
component refuses a call carrying the wrong one as surely as it refuses the
identity. Every caller in the workspace — the two ONNX components' tests —
was updated in the same change.

`check_generator_conformance(make, served_model)` was born with that shape
rather than changed to it, on the same reasoning. Which model a fixture serves
is a fact no suite can know: a `Local` generator recognises the names its
constructor was given, a `Remote` one whatever its inference server serves,
and a name the suite invented would be refused by both — correctly. It is the
caller's claim about its fixture, and a wrong one fails `well-formed call
succeeds`. **The template is not an argument**, because the suite can supply
it: the grammar on `Generator` is the same for every implementation, so the
suite's well-formed template exercises all of it — both placeholders, one of
them twice, and the `{{`/`}}` escapes. A caller-chosen template could leave
out exactly what its generator gets wrong, and then a built-in and a third
party would no longer earn the same guarantee (INV-7).

## What lives here

The behavioural suite **every** component implementation must pass, whatever its
nature (`docs/code-architecture.md` §7.4). One `check_*` / `assert_*` pair per
trait family: the M2 five — `Retriever`, `Fusion`, `Reranker`, `Embedder`,
`VectorStore` — and the two generation families, `ContextBuilder` and
`Generator`.

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
`ragondin-contracts` or an ADR, or stated in issue #17, or generalised from a
sentence written on the shared error type — and the last kind is listed here explicitly
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
| **no duplicate ids** | **generalised**: a ranked list ranks each chunk once, and a context places each chunk once. Checked only for `Fusion`, `Reranker` and `ContextBuilder`, whose whole input the suite knows, so that "this id appears twice" is a statement about the component and not about a corpus. |
| a context's chunks: no fabricated ids, no duplicates; a zero budget rejected | `ContextBuilder`'s context contract, and ADR-C31 § 2's list of what the suite may check. The zero budget is `top_k`'s twin under the contracts crate's *Empty collections* rule. |
| zero chunks, and an empty context, are valid calls | ADR-C19, restated for both families in ADR-C31 § 2. A refusal is reported as `well-formed call succeeds`, and a builder answering zero chunks with a placed chunk as `no fabricated ids` — the same diagnoses the M2 families give an empty input. |
| a well-formed template accepted: both placeholders, a repeated one, the `{{`/`}}` escapes | The template grammar on `Generator`, which every implementation shares. Its refusal is reported as `well-formed call succeeds`. |
| an empty served model, an empty template, a malformed template rejected | `Generator`'s *What is refused* clause and ADR-C31 § 2. "Malformed" is probed once per case the template grammar on `Generator` names — an unknown name between braces, a `{` no `}` closes, a lone `}` — each in a template otherwise well formed, so that a generator forgiving any one of them fails. |
| **identity non-empty**, **identity stable across two calls** | ADR-C31 § 1 (an empty identity is not valid) and § 4 (stable while nothing has changed); ADR-C32 § 4 adds both scenarios for every model-bearing family. Stability is read twice from one instance and once from a **second instance the same constructor built**: § 4's *How it is read* has the composition root read the identity from an instance other than the one that runs, so an identity naming its instance breaks the run record exactly as a counter does. A `model_identity` that fails is reported as `well-formed call succeeds`. |

## What the suite deliberately does not decide

Two behaviours on which the repository currently contradicts itself. The suite
certifies both answers rather than picking one, and says so where the check
would have gone:

- **A zero-dimensional embedding** — #90. `ragondin-types` says an empty
  embedding is representable; `ragondin-contracts`' own stub treats one as an
  invalid request.
- **An empty `upsert`** — #91. That same stub rejects it, while the neighbouring
  families all treat an empty input as success.

Nor does it check three things the generation contracts state, each because
the suite cannot observe it without knowing the component:

- **A context builder's budget is not measured**, only refused at zero. Its
  unit is the builder's own (ADR-C31 § 2); a "budget respected" check would
  have to pick a unit, and would then certify that unit rather than the
  contract.
- **A generator, an embedder or a reranker refusing a model it does not
  serve** is not probed: no name is one the suite could know every fixture
  refuses — and for an embedder or a reranker, `None` is refused by a
  `Remote` fixture and answered by a `Local` one.
- **A builder carrying each placed chunk's incoming score untouched** is on
  `ContextBuilder`'s contract, but not on ADR-C31 § 2's list of what the suite
  may check, and the suite checks what that list and ADR-C32 § 4 name, no
  more.

Nor does it decide **which prefix an asymmetric embedder should apply**. ADR-C17
put the role on `EmbedParams`, so the suite exercises every embedder check under
both `EmbedRole`s — a contract broken on one side only is still broken. Beyond
that it can say one thing and no more: where a caller *declares*
`RolePrefixes::Distinct`, one text must not embed identically under the two
roles, which catches an implementation that accepts the role and ignores it.
Whether the prefix it applied was the *right* one is unknowable here, since the
suite does not know the model — and a symmetric embedder answering both roles
alike is correct, which is why the check is opt-in rather than universal.
