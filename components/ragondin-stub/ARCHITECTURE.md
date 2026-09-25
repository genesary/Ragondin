# ARCHITECTURE — ragondin-stub

**Status: a component, and therefore a leaf** of the dependency graph. Not an
API boundary: nothing in the workspace depends on it except a binary, which
constructs it and registers it on an `EngineContext`.

## What lives here

Four deterministic implementations of contracts in
[`ragondin-contracts`](../../core/ragondin-contracts/src/lib.rs):

- **`StubRetriever`** — a `Retriever` with no corpus. It returns exactly
  `top_k` chunks, with ids `<label>-0`, `<label>-1`, … and score
  `1 / (rank + 1)`. The query text reaches the chunk text and changes nothing
  else, so two different questions get the same answer.
- **`StubFusion`** — a `Fusion` that interleaves: rank 0 of every leg, then
  rank 1 of every leg, in the order the pipeline wires them, skipping a chunk
  already taken. It re-scores by output position, for the reason RRF re-scores:
  two legs score on incomparable scales.
- **`StubContextBuilder`** — a `ContextBuilder` that keeps the first `budget`
  chunks it is handed, in the order handed, and renders their texts verbatim,
  joined by `\n`. Each kept chunk carries its incoming score (ADR-C31 § 1).
- **`StubGenerator`** — a `Generator` with no model. It serves one name, given
  to its constructor, and answers with the first line of the context's text,
  trimmed, when its template places `{context}` in the prompt.

## What it is for

It is the **fixture the end-to-end paths are wired with** — the vertical slice
in [`bin/ragondin/tests/`](../../bin/ragondin/tests/vertical_slice.rs), and the
evaluation harness and `ragondin bench` that build on it. A pipeline made of
these components needs no corpus, no index and no model, so it runs anywhere,
and it runs the same way twice. What it makes observable is the **wiring**: a
configuration file on disk, a registration on an `EngineContext`, a plan, and an
`ExecutionTrace` with one entry per node.

The two generation stubs serve one further purpose: a CI test that cannot call
an LLM still needs a generation leg whose answer is right exactly when the
pipeline ranked the right passage first. That is what their answer function is
chosen for, and it is recorded below.

## Choices made in this crate

ADR-C31 leaves most of these to the implementation; refusing a name the
generator does not serve is § 2's and § 4's, not this crate's. They are
recorded here because a test fixture's contract is what a later test builds on.

- **The stub builder's budget counts chunks.** The contract leaves the unit to
  the implementation (ADR-C31 § 2). A count of chunks needs no tokenizer and no
  rule for cutting a passage in half, and it keeps the first line of the
  context equal to the first line of the first chunk whatever the budget.
- **Its rendering is the chunk texts, verbatim, joined by `\n`** — no header, no
  separator of its own, no trailing newline. Zero chunks render the empty
  string, so the empty context (ADR-C19) is `chunks: []` and `text: ""`.
- **The stub generator's answer function**, stated in one sentence: *the answer
  is the first line of the context's `text`, trimmed of surrounding whitespace,
  when the template places `{context}` in the prompt, and the empty answer
  otherwise.* It is defined over what a generator actually receives — a
  `Context` carries chunk ids and one rendered `text`, never chunk texts
  (ADR-C31 § 1) — rather than over "the first chunk", which the generator
  cannot see. Over `StubContextBuilder` it is the first line of the first
  chunk's text, so a fixture whose right passage opens with the reference
  answer on a line of its own is answered correctly exactly when that passage
  is ranked first; a builder that renders a header first answers with the
  header, which is the fixture's to avoid. It is not configurable: a knob would
  be a second fixture contract nobody asked for.
- **Why the template matters to the answer at all.** The rendered prompt is the
  whole of what a generator asks its model (ADR-C31 § 2), so a context the
  template does not place is one the model never saw; answering from it anyway
  would make the stub answer a question it was not asked. The stub parses the
  template under the whole grammar and refuses a malformed or empty one, but
  assembles no prompt text: what it reads of the rendered prompt is only whether
  it carries the context, and building a string it would not read is ceremony.
  The query, the template's other text, `temperature`, `seed` and `max_tokens`
  change nothing — there is no sampling to steer and no tokenizer to count with.
- **The served name is checked, and decides nothing else.** Refusing any other
  name as `InvalidRequest`, from both `generate` and `model_identity`, is
  mandated by ADR-C31 § 2 and § 4. What this crate chose is the rest: the empty
  name is refused even when the constructor was given it, and the name decides
  whether a call is served, never what it is answered.
- **Both identities are constants**, `StubContextBuilder::IDENTITY` and
  `StubGenerator::IDENTITY`, public so a test asserting on `model_hashes` need
  not copy the string. A constant is conformant where no configuration decides
  the output (ADR-C31 § 4): the builder has no configuration, and the
  generator's one setting, its served name, decides whether a call is served,
  never what it is answered. Each names its rendering or answer function and a
  version, so that changing either is a change to the string.

## Local invariants

- **It is a leaf ([INV-5](../../AGENTS.md)).** It depends on
  `ragondin-contracts`, `ragondin-types` and `async-trait` — never on
  `ragondin-engine`, never on another component. The engine knows only traits
  ([ADR-C5](../../docs/adr/ADR-C05-engine-depends-only-on-traits-components-are-leaves.md)),
  so the arrow points from a binary to this crate and never the reverse. Holding
  a stub to the same edges as a real component is what makes it a usable
  rehearsal of one: a fixture that reached further would rehearse a wiring
  nothing in production could have.
- **No privilege for being built-in (INV-7).** It registers the way a
  third-party component registers, and it passes the same suite
  ([ADR-C6](../../docs/adr/ADR-C06-identical-api-plus-conformance-suite.md)) —
  `tests/conformance.rs` is the call a contributor writes, verbatim. Conformance
  matters more here than elsewhere: a fixture that quietly broke the contract
  would make every pipeline built on it prove the wrong thing.
- **Nothing here is a measurement.** A stub retriever answers every query
  identically and its scores encode rank alone, so no number taken over these
  components says anything about retrieval quality; and the stub generator's
  answer is a fixed function of its context, so none says anything about
  generation either. Conformance is a floor, not an endorsement.
- **Determinism is the product.** No clock, no randomness, no I/O, no
  interior state. Two runs of one pipeline return the same chunks, in the same
  order, with the same scores and the same answer — which is what lets a test
  assert on an output and on a `LogicalPipeline`'s content hash at the same
  time.
- **The `stub` feature gates nothing heavy, on purpose.** There is no heavy
  dependency here to confine; the feature exists so that every component crate
  names its implementation the same way, following
  [ADR-C14](../../docs/adr/ADR-C14-heavy-backends-feature-gated-lean-default-build.md)
  as `ragondin-fusion-rrf` and `ragondin-store-memory` do — a feature gating a
  heavy dependency is off by default, one gating nothing is on by default.

## Two open questions this crate does not answer

- **`components/` granularity** (`docs/OPEN_QUESTIONS.md` #4) — one crate per
  family, or one per implementation. This crate holds four components of four
  different families, and that is a fixture kept in one place, **not** a
  position on how production components should be split. Do not cite it as a
  precedent.
- **Registry ergonomics** (`docs/OPEN_QUESTIONS.md` #1) — the composition root
  registers these stubs explicitly and verbosely, which is the valid interim.
  This crate reaches no registry of its own.

## What is deliberately not here

No corpus, no index, no randomness, and no stub for a family the fixture
pipeline does not use. A vector store is not stubbed: `ragondin-store-memory`
is a real component that needs no external service, so a pipeline that needs one
uses it. Fewer fakes on the path is better.
