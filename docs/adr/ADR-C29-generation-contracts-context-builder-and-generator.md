---
id: ADR-C29
title: The generation contracts — ContextBuilder and Generator, their nodes and kinds, their model identity, and what the trace names
status: accepted
invariants: [INV-1, INV-2, INV-7, INV-9, INV-10]
supersedes: []
superseded_by: null
---

# ADR-C29: The generation contracts — ContextBuilder and Generator, their nodes and kinds, their model identity, and what the trace names

## Context

The platform retrieves, fuses and reranks. It does not generate. `ragondin-contracts`
defines five families — `Retriever`, `Fusion`, `Reranker`, `Embedder` and
`VectorStore` — and its crate documentation says that `ContextBuilder`,
`Generator` and `Grader` "are not defined here", because "adding a trait is
additive on this boundary, so each of those arrives with the work that first
consumes it; defining one before anything needs it would be dead API". The same
stance is recorded one crate down: `ragondin-types` says its generation-side
types "arrive with the milestone that uses them, not before". This is that
milestone, and this decision is what those two sentences were waiting for.

Nothing in the repository settles the shape. `docs/system-architecture.md` §5.2
lists a context builder and a generator among the component kinds and gives a
signature for neither. `docs/code-architecture.md` §6.2 sketches a
`Generator(GeneratorNode)` variant and a `Grader(GraderNode)` variant on
`LogicalNode` and **no context-builder node at all** — leaving open whether
building a prompt is a node of the graph, a generator's own private business, or
an `Extension`. ADR-C16 fixes that a node's port kinds are coarse, parameterless
and derived from its variant, and says in its Consequences that `NodeValue` stays
refactorable "as kinds arrive in M3 and M4" — so a new kind is expected, and
nothing says which. ADR-C28 decides what the trace names for a list of chunks and
is silent about text. ADR-9 and ADR-15 require that a generator's model, prompt,
temperature and seed be recorded on the same footing as the embedder's, and
nothing says how a component that is a service rather than a file reports one.

Four choices follow from that gap, and **none of them can be made apart from the
others**, which is why they are one decision:

- **The traits and the values they move.** Whether a context is a typed value
  carrying which chunks it holds, or a `String`. The answer decides what M4's
  context precision and recall can be computed from, and whether per-node replay
  can show a prompt's provenance without re-deriving it from a neighbour.
- **The pipeline side.** Which `LogicalNode` variants exist, what each consumes
  and produces, and which `ValueKind`s name the two new values. Neither
  `LogicalNode` nor `ValueKind` is `#[non_exhaustive]` —
  `core/ragondin-pipeline/ARCHITECTURE.md` records that as deliberate, so that
  adding a primitive is a visible act rather than a silent one — so every variant
  added here is a versioned INV-1 break, and the wire schema gains two accepted
  values for `RawNode.component`, which bumps `SchemaVersion` under INV-9.
- **Model identity.** `bin/ragondin/src/wiring.rs` builds `model_hashes` by
  digesting model **files**: its `model_hashes` walks the pipeline's nodes, finds
  the model path in a node's params, and streams the file through SHA-256 in
  `file_digest`. A generator behind a URL has no file to digest. Without an
  answer, every M3 run's identity omits the one model that decides its answers,
  and ADR-9's self-preference check has no hook to hang on.
- **What the trace names.** ADR-C28 made a node's output entry name the chunks it
  produced. M3's metrics need the answer text somewhere; per-node replay (ADR-14)
  needs it in the trace; the per-query fixture ADR-10 requires needs it in the
  stored run.

Each of the four lands on the escalation list in `AGENTS.md` § Rules of
engagement: the public API of the three core crates (INV-1), the wire format
(INV-9), and what the trace carries. Decided in #251.

## Decision

**The retrieval-to-generation chain is contracted end to end, by two traits over
three typed values, two node variants, two kinds, a model identity the component
reports itself, and a trace that names the context and the answer.**

### 1. Three values in `ragondin-types`

`Context { chunks: Vec<ContextChunk>, text: String }`, where
`ContextChunk { id: ChunkId, document_id: DocId, score: f32 }`. A context carries
its **provenance by identifier, never the chunk text**. The text of every chunk
is already rendered into `text`, so carrying it a second time in `chunks` would
put the same bytes on the wire twice; and it is exactly the field the engine's
trace already refuses, for the reason `RankedChunk`'s doc comment in
`engine/ragondin-engine/src/trace.rs` gives — the chunk's text "is the one field
that grows with the corpus and the one no reader of a ranking needs". `chunks` is
in **the order the builder placed them**, which is what makes it a ranking rather
than a set.

`Answer { text: String }` — text and nothing else, **deliberately**. Two further
fields were named as candidates and refused for M3: token usage, the cost axis on
which `docs/system-architecture.md` § 7.1 compares one configuration against
another, and a per-call identity of the generation. Neither is needed to score
exact match or F1, and neither has a settled shape. No value type in
`ragondin-types` carries `#[non_exhaustive]`, so **adding a field to `Answer`
later is a deliberate, versioned INV-1 break** rather than an additive one. That
is the cost of this refusal, and it is stated here so that whoever pays it knows
the bill was foreseen.

`ModelIdentity(String)` — an opaque newtype. The empty string is **representable
and not valid**, exactly the shape ADR-C20 gave `Embedding`: a value type in a
crate with no error type cannot reject anything, so the type stays infallible to
construct and the rule lives where a component's obligations live. A `Remote`
adapter that receives an empty identity from a service refuses it as an
`InvalidRequest`-class failure.

All three derive `Clone, Debug, PartialEq, Serialize, Deserialize`. `Context` and
`ContextChunk` do **not** derive `Eq`, because they carry `f32` scores, which is
the same reason `ScoredChunk` does not. `Answer` and `ModelIdentity` do.

**`Context` does not hold the query.** ADR-C18 rejected bundling the query with a
value — there, the proposal to let `ValueKind::Chunks` mean "a retrieval result
and the query that produced it" — and the reason carries over intact: the
generator may legitimately be handed a different query from the one the builder
saw, and a context that carried one would make which query a generator answers
ambiguous.

### 2. Two traits in `ragondin-contracts`

Both are `#[async_trait]` and `Send + Sync`, like every other family (ADR-C8), so
the engine cannot tell a `Local` implementation from a `Remote` one (ADR-3).

```rust
async fn build(&self, query: &Query, chunks: Vec<ScoredChunk>, params: &ContextParams)
    -> Result<Context, ComponentError>;
async fn model_identity(&self) -> Result<ModelIdentity, ComponentError>;
```

```rust
async fn generate(&self, query: &Query, context: &Context, params: &GenerateParams)
    -> Result<Answer, ComponentError>;
async fn model_identity(&self) -> Result<ModelIdentity, ComponentError>;
```

`ContextParams { budget: usize }` is `top_k`'s twin: a per-call cap on the size of
the result. **Zero is refused** as `ComponentError::InvalidRequest`, under the
rule `ragondin-contracts`' crate documentation already states for `top_k` — a
zero cap "asks for a *result* that cannot exist", and answering it with an empty
context makes a caller's arithmetic bug look like an empty corpus.

**The unit of `budget` is implementation-defined, and the implementation
documents it.** Characters, tokens, or whatever a particular builder counts in:
the contract fixes that there is a cap and that zero is refused, and says nothing
about what is being counted. This is the stance `ScoredChunk`'s doc comment
already takes on a score, whose "scale is the scoring component's own and is
comparable only within one ranked list". The alternative — a unit fixed on the
boundary, in tokens — would require every leaf to carry a tokenizer to honour the
contract, and the tokenizer would have to be the generator's, which the builder
does not know.

`GenerateParams { temperature: Option<f64>, seed: Option<u64>, max_tokens: Option<usize> }`
is per call and every field is optional. **They are per-call parameters and not
constructor configuration, because of the `Remote` face.** A `Remote` adapter
holds a client over a channel and delegates each trait method to the rpc that
mirrors it, which is the shape `docs/code-architecture.md` § 7.1 lays out: face 2
carries the trait's calls and nothing else, so there is no configure rpc and
**constructor configuration never crosses the wire**. For the one family that is
`Remote` by design — the LLM inference server is what
`docs/system-architecture.md` § 10 names as deliberately not reimplemented and
called over the network — per-call params are therefore the only path from the
pipeline representation to the service. Knobs left service-side would sit outside
the logical form and therefore outside its hash, and two runs differing only in
temperature would content-address identically. ADR-9 puts a judge's "model hash,
prompt, temperature and seed" on exactly the same footing as the generator's, and
ADR-15 promises that "everything that ran is recorded". Both forbid that outcome.

`Option` is what keeps the executor's rule intact. The executor refuses an absent
required per-call parameter and **invents no default**:
`ExecError::InvalidParam` in `engine/ragondin-engine/src/error.rs` says so in as
many words — "what a component does in the absence of a parameter is the
component's to decide, and a default applied here could only be a second,
disagreeing copy of it" — and `per_call_top_k` in the executor is where the
refusal happens. `docs/code-architecture.md` § 6.3 states the same rule one stage
earlier, of physical planning, which "applies no defaults of its own,
deliberately". An absent optional key reads as `None` and is passed through as
`None`; no default is substituted anywhere above the component.

**The prompt template stays with the implementation** — constructor configuration
for a `Local` component, service-side for a `Remote` one — following the split
`ragondin-contracts`' documentation already draws between what a constructor
receives and what a params struct carries. It is therefore covered by
`model_identity`, not by the params, and §4 below says what that obliges.

**ADR-C19 applies unchanged.** `build` over zero chunks is a valid call, not an
invalid request: it returns the empty context, meaning `chunks.is_empty()`, with
`text` unconstrained — a template may legitimately render a header over no
passages. `generate` over an empty context is likewise a valid call; an answer of
"I do not know" is conformant, and refusing the call with `InvalidRequest` is not.
ADR-C25 applies unchanged too: a `Local` generator that runs a model forward pass
moves that work off the caller's thread itself.

**What the conformance suite may check**, when `ContextBuilder` and `Generator`
join it: that a well-formed call succeeds; that a zero budget is rejected; and
that `Context.chunks` fabricates no chunk id and repeats none — the `no fabricated
ids` and `no duplicate ids` scenarios `ragondin-conformance` already applies to
`Fusion` and `Reranker`. **Nothing about content.** A suite that does not know
which model it is testing cannot say whether an answer is good, and a check that
pretended to would certify less than it appears to.

### 3. Nodes, kinds and the wire schema

`LogicalNode` gains `ContextBuilder(ContextBuilderNode)` and
`Generator(GeneratorNode)`, each shaped exactly like `RerankerNode`: `id`,
`implementation`, `inputs`, `params`.

`ValueKind` gains **two** variants, `Context` and `Answer`, rather than one shared
`Text`. Two variants are what make a generator wired to a generator, or a context
builder wired to a reranker, fail validation **by name** rather than at execution
or not at all. Their `Display` renderings are pinned, as the existing three are,
because a `KindMismatch` message is built from them: `"context"` and `"answer"`.

The port shapes are derived from the variant and never declared (ADR-C16).
`ContextBuilder` consumes `Fixed([Query, Chunks])` and produces `Context`;
`Generator` consumes `Fixed([Query, Context])` and produces `Answer`. **The query
is an explicit edge on both**, which is ADR-C18's decision applied here: it said
in as many words that `Generator` and `Grader` would "inherit the answer" when
they arrived, and the first of the two arrives now. A builder that
never reads the query ignores the port, as `StubReranker` in
`core/ragondin-contracts/src/lib.rs` ignores its `_query` today; the hashed edge
nobody reads is the verbosity ADR-C18 accepted deliberately, and accepting it
again here costs nothing new.

On the wire, `RawNode.component` accepts `context_builder` and `generator`. That
is a change to the wire schema's shape, so it **bumps `SchemaVersion::SUPPORTED`**
under INV-9, as ADR-C18's change to the same type did. **This ADR sanctions that
bump by name.** `ParamValue` is untouched: every parameter these two nodes take is
a flat scalar, so the grammar ADR-C22 fixed as `String | Int | Float | Bool | List`
needs nothing added, and no `Map` demander appears here.

These additions are **the deliberate, versioned INV-1 break** on `ragondin-types`,
`ragondin-contracts` and `ragondin-pipeline`, sanctioned here and nowhere else.
`LogicalNode` and `ValueKind` stay non-`#[non_exhaustive]`, keeping the stance
`core/ragondin-pipeline/ARCHITECTURE.md` records: a consumer's exhaustive `match`
breaking is the intended signal that a new node kind needs handling.

### 4. Model identity is reported by the component

Both new traits carry `async fn model_identity(&self) -> Result<ModelIdentity, ComponentError>`,
mirrored by a `GetModelIdentity` rpc on face 2. It is `async` because the only M3
implementation of either family is `Remote` and must make an rpc to answer; a
synchronous method would force a `block_on` inside the trait, against ADR-C25.

**What the identity must be.** Two properties, and an implementation that fails
either is wrong:

- **Stable** across calls while nothing has changed. No timestamp, no request id,
  no counter — P4 requires that a rerun of the same inputs produce the same
  `run_id`, and an identity that varied per call would make every run unique by
  construction.
- **Complete.** It covers every knob that decides the answer and is not in the
  per-call params or in the node's params: the model, its revision, and the prompt
  template whenever the template is not itself in the node's params. A template
  that changes the answers while the identity stays put is the failure this
  method exists to prevent.

For a `Local` component the identity is a digest of its own configuration and its
model file. For a `Remote` one it is **what the service reports, and that is the
whole extent of the guarantee**: a service that answers wrongly cannot be checked
from here. That is not a new concession. ADR-15 promises traceability, not
verification, and `model_hashes` is already caller-supplied trust —
`Evaluation::model_hashes` in `eval/ragondin-harness/src/evaluate.rs` is
documented as "supplied by the caller rather than discovered here".

**How it is read.** The composition root constructs an instance, awaits
`model_identity`, records the result in `model_hashes` under the roles
`generator` and `context_builder`, and registers a constructor that builds
another instance. The synchronous `ComponentCtor` is therefore not an obstacle:
`bench` in `bin/ragondin/src/bench.rs` is already `async`, and already computes
`model_hashes` before it registers anything and before it calls `evaluate` — it
does so today so that a missing model file is found before the expensive steps.
Stated plainly, because it is the part a reader will otherwise assume away: **the
identity is read from an instance other than the one that ran.** For a `Remote`
component both instances reach one URL, so the two agree unless the service
changed mid-run, which is the same window every other caller-supplied hash
already has.

**The pattern, stated for every model-bearing family: any family whose answer
depends on a model reports its identity through its trait.** This ADR applies the
pattern to the two new traits **now**, and deliberately does **not** add the
method to `Embedder`, `Reranker` or `VectorStore`. Neither of the first two has a
`Remote` implementation today, so neither has the problem the method solves; and
adding a method to a trait breaks every existing implementation, in and out of
this workspace. The ADR that resolves #101 — how a node names and constructs a
`Remote` service — is where those three follow, as a deliberate act of its own
rather than a side effect of this one.

### 5. What the trace names

This extends ADR-C28 to the two new values, on the same reasoning and with the
same asymmetry between the two sides of a node.

For what a node **produced**, `ValueSummary` gains
`Context { chunks: Vec<RankedChunk>, text: String }` and `Answer { text: String }`.
For what a node **consumed**, inputs stay summaries, as ADR-C28 decided: a context
is summarised by its chunk count and its text length, an answer by its text
length. An input is the output of the node that produced it, already named under
that node.

Size is bounded by the context builder's `budget`, which is the user's
configuration and never the corpus — the argument ADR-C28 made for `top_k`,
holding here for the same reason. And the trace is **not part of run identity**:
`run_id` in `eval/ragondin-harness/src/identity.rs` digests the five fields of
`RunInputs` and nothing else, so an answer's text never enters `run_id` and two
machines whose generators phrase an answer differently still produce the same run
identity for the same inputs.

INV-10 is untouched: the trace remains the executor's return value, and nothing
here moves per-node detail into a `tracing` macro.

## Alternatives rejected

- **Text only — the context is a `String`, and the generator takes
  `(query, &str)`.** The smallest possible contract, and the one that loses
  provenance at the first node. M4's context precision and recall ask *which*
  chunks were in the prompt, and per-node replay asks the same question; with a
  bare string, both have to reach back into the previous node's trace entry and
  re-derive from a neighbour what the value itself could have carried. A contract
  whose consumers must reconstruct what it dropped is not smaller, only later.

- **No `ContextBuilder` trait — the generator formats its own prompt.** One trait
  fewer, and one experiment variable fewer: prompt construction stops being
  configuration, which is precisely what ADR-1 is against — a technique that
  cannot be varied through the representation cannot be compared. It also taxes
  the contribution funnel ADR-3 exists to protect, since every `Remote` generator
  author would reimplement chunk formatting in their own language before writing
  a line of generation.

- **`Context` carrying full `ScoredChunk`s.** The shape the decision issue
  proposed, and it carries every chunk's text twice on the wire: once in the
  rendered `text` a generator actually reads, once in the provenance list. The
  engine's own trace already refuses chunk text for exactly this reason, and a
  value type that grows with the corpus is worse than a trace that does, because
  it crosses the wire on every call rather than once per run.

- **One `Text` kind for both outputs.** Coarser and cheaper — one INV-1 variant
  instead of two — and ADR-C16 does say kinds are coarse. But it does not say two
  different things may share one: under a single kind a generator wired to a
  generator passes validation, and the wiring error surfaces at execution or not
  at all. The whole purpose of checking kinds before execution is to reject that
  configuration with a name attached.

- **Empty per-call params — every generation knob is constructor
  configuration.** Clean on the `Local` face and unimplementable on the `Remote`
  one, for the reason given above: a constructor's configuration never crosses
  the wire, and the generator family is `Remote` by design. Temperature and seed
  would then live service-side, outside the logical form and outside its hash,
  and two runs differing only in temperature would content-address identically —
  which ADR-9 and ADR-15 both forbid.

- **A hand-typed `model:` node parameter instead of a reported identity.**
  Nothing new on the trait, nothing new on the wire. It makes run identity trust a
  string the user typed: a service that swaps the model behind the same URL
  produces the same `run_id` for two different runs, which is exactly what P4
  forbids. A parameter records an intention; the method records what answered.

- **Nothing in M3 — the generator does not enter `model_hashes`.** Self-preference
  detection waits for the judge in M4 anyway, so the cost looks deferred. It is
  not: the trait gains the method later as an INV-1 break on a boundary with
  implementations behind it, and in the meantime every M3 run's identity omits
  the one model that decides its answers.

- **A synchronous `model_identity`.** Cheaper to call from a composition root and
  wrong for the only implementation M3 has: a `Remote` component must make an rpc
  to answer, and a synchronous method leaves it a `block_on` inside the trait,
  which is what ADR-C25 forbids.

- **A counts-only trace, or an answer returned but not traced.** ADR-C28's third
  alternative again, with the same consequence: the harness scores from the return
  value and drops it, so there is no per-query regression fixture and no replay.
  It was rejected for chunks in ADR-C28, and nothing about text makes it a better
  answer here — the SciFact per-query fixture reads its rankings out of a stored
  run's `traces.json` precisely because they were traced rather than dropped.

- **`Context` holding the query.** ADR-C18 weighed and rejected the same shape for
  chunks, and its reason holds: the generator may receive a different query from
  the one the builder saw, and a bundled query makes which one it answers a
  question with no correct answer.

## Consequences

- **The M3 implementation issues are unblocked, and each owes this ADR something
  specific.** #254 the three value types; #255 the two traits; #256 the node
  variants, the kinds and the `SchemaVersion` bump; #257 the proto mirror,
  including `GetModelIdentity`; #258 the conformance scenarios; #259 the engine —
  registry, planning, execution and trace; #260 the stubs, including one that
  reports a fixed identity; #261 the `Remote` adapters, including the refusal of
  an empty identity; #262 the concatenating context builder, whose documented
  budget unit is characters; #265 the harness, which reads the answer and the
  context from the trace; #266 the binary, which reads both identities into
  `model_hashes`. None of them reopens this decision; each of them implements a
  named part of it.

- **The INV-1 break is sanctioned here and covers three crates.**
  `ragondin-types` gains three value types, `ragondin-contracts` two traits and
  two params structs, `ragondin-pipeline` two `LogicalNode` variants and two
  `ValueKind` variants. **INV-9**: the wire schema's shape changes and
  `SchemaVersion::SUPPORTED` bumps with it. **INV-2**: `ragondin-engine` changes
  freely — the registry, the planner, `NodeValue` and `ValueSummary` are its own,
  and the executor's output type becomes an enum over what a terminal node may
  produce, which is engine-internal and stable to nobody. **INV-7**: no privilege
  anywhere — the stub, the concatenating builder and the `Remote` adapters
  implement the same two traits and pass the same suites, with no path a
  third-party component could not take. **INV-10**: untouched; the trace is still
  the executor's return value.

- **Prose this decision falsifies, corrected by the diff that creates the
  obligation.** `ragondin-types`' crate documentation promises `Context` and
  `Generation`; the type is named `Answer`. #254 corrects that sentence in the
  same diff that adds the type, not in a follow-up. `ragondin-contracts`' crate
  documentation lists the families it defines and names `ContextBuilder` and
  `Generator` as absent by design; #255 is where both stop being true.

- **The per-query fixture reads answers out of `traces.json`**, as it reads
  rankings there today. That is what carrying the answer text in the trace buys,
  and it is the same mechanism ADR-C28 established rather than a second one.

- **The harness must add reference answers to `dataset_version`.** A benchmark
  typed by reference answers (ADR-8) whose references change is a different
  benchmark; without them in the digest, two benchmarks differing only in their
  references share one identity and their runs collide. That obligation is #263's,
  and this ADR names it rather than discharging it.

- **`Context.chunks` gives #252 a second candidate ranking** — what entered the
  context, beside what the retrieval leg offered — without deciding which of the
  two the retrieval metrics read once the terminal node is a generator. That
  question stays where it is.

- **A trace grows by roughly one context and one answer per query**, both bounded
  by the plan: the context by the builder's `budget`, the answer by
  `max_tokens` where a caller sets one. Neither grows with the corpus.

- **What is deliberately left open.** Which node's output the retrieval metrics
  read (#252). How a node names and constructs a `Remote` service (#101).
  `Embedder`, `Reranker` and `VectorStore` identity, which follows with #101.
  Token usage on `Answer`, which is a later and deliberate INV-1 break. Each is
  named so that the next reader can see it was weighed rather than missed.

- No entry in `docs/OPEN_QUESTIONS.md` is opened, closed, or changed.

## Status

Accepted.
