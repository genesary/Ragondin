---
id: ADR-C31
title: The generation contracts — ContextBuilder and Generator, their nodes and kinds, their model identity, what the trace names, and the template and served model as experiment variables
status: accepted
invariants: [INV-1, INV-2, INV-7, INV-9, INV-10]
supersedes: [ADR-C29]
superseded_by: null
---

# ADR-C31: The generation contracts — ContextBuilder and Generator, their nodes and kinds, their model identity, what the trace names, and the template and served model as experiment variables

## Context

**This ADR supersedes ADR-C29 in full.** It restates ADR-C29's decision — every
section, every alternative and every consequence — with one change: the prompt
template and the name of the model a generator asks its backend for become
required per-call parameters of the node, and `Generator::model_identity`
becomes a check of that name rather than the only record of it. Everything
ADR-C29 decided that this change does not touch is carried forward in ADR-C29's
own words, so that the issues implementing it (#254–#266) read one text and
never need to open the superseded one. The change and its reasons are set out
under *What changed, and why* below; the background ADR-C29 was written against
comes first, unchanged.

### The background, carried forward from ADR-C29

The platform retrieves, fuses and reranks. It does not generate. `ragondin-contracts`
defines five families — `Retriever`, `Fusion`, `Reranker`, `Embedder` and
`VectorStore` — and its crate documentation says that `ContextBuilder`,
`Generator` and `Grader` "are not defined here", because "adding a trait is
additive on this boundary, so each of those arrives with the work that first
consumes it; defining one before anything needs it would be dead API". The same
stance is recorded one crate down: `ragondin-types` says its generation-side
types "arrive with the milestone that uses them, not before". This is that
milestone, and this decision is what those two sentences were waiting for.

Nothing in the repository settles the shape. `docs/system-architecture.md` § 5.2
lists a context builder and a generator among the component kinds and gives a
signature for neither. `docs/code-architecture.md` § 6.2 sketches a
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
(INV-9), and what the trace carries. ADR-C29 decided them in #251.

### What changed, and why

ADR-C29 § 2 gave `GenerateParams` three per-call fields, all optional —
temperature, seed, maximum tokens — and placed the prompt template with the
implementation: *"The prompt template stays with the implementation —
constructor configuration for a `Local` component, service-side for a `Remote`
one"*, covered by `model_identity`. The name of the model a `Remote` generator
asks its inference server for was nowhere in the pipeline representation
either; it was the service's own configuration. Under that answer, a researcher
who varies the template or the served model between two runs changes nothing in
the pipeline: the two runs are written by one byte-identical YAML, their logical
forms hash alike, and what differed between them is recorded only as an opaque
identity string the service declared after the fact — or, from a service whose
identity omits the template, not at all, in which case the two runs share one
`run_id`. The repository owner's requirement, stated on 2026-09-23 while
deciding #101, is the opposite: a prompt template and a model name are the first
things a researcher varies, so they live in the YAML, like `top_k`.

The accepted ADRs already point that way, and ADR-C29's own reasoning is what
makes the change necessary rather than merely possible. ADR-1 holds that
techniques are configuration; ADR-9 gives the judge its "model hash, prompt,
temperature and seed" "on exactly the same footing as the generator and the
embedder", and makes which model, which prompt and which temperature experiment
variables. Neither says which of them is a node parameter and which constructor
configuration, and ADR-C29 answered for the template in the direction that hides
it. ADR-C29 § 2 also
established that constructor configuration never crosses the `Remote` face, so
per-call params are the only path from the pipeline representation to a
service: that argument, which ADR-C29 made for temperature and seed, holds word
for word for the template and the served model. And ADR-C29 rejected a
hand-typed `model:` node parameter **as an identity mechanism** — a string the
user typed standing in for what answered — not as an experiment variable; the
question of what a researcher varies was not posed from that angle.

The change, item by item against ADR-C29:

- **§ 2, `GenerateParams`** gains two **required** fields, `served_model` and
  `template`, both `String`; ADR-C29's sentence that `GenerateParams` "is per
  call and every field is optional" is replaced. The three optional fields and
  their proto3 presence rule are unchanged; the presence paragraph now says
  "the three optional fields" where ADR-C29 said "the three fields", since there
  are five. Three paragraphs are added: that the argument for per-call params
  holds for the template and the served model, so a `Remote` service holds no
  experiment variable of its own; that the executor reads the two required
  fields as it reads `top_k`; and that they mirror on face 2 as required strings,
  an empty one refused.
- **§ 2, the template.** ADR-C29's paragraph beginning "The prompt template stays
  with the implementation" is **withdrawn**, and replaced by the template's
  grammar, the rule that the component renders it, why a template is not the
  sub-grammar ADR-C22 rejected, and a statement that the context builder's own
  configuration is not changed.
- **§ 2, the conformance suite** may also check a generator's refusal of an
  empty `served_model`, an empty `template` and a malformed template.
- **§ 2, the generator's trait.** `Generator::model_identity` takes the served
  model as an argument. `ContextBuilder::model_identity` does not change.
- **§ 4, model identity.** The generator's identity is *the identity of the model
  the component answers `served_model` with*, read once per generator node with
  that node's `served_model`, and a name the backend does not serve is a
  refusal before the run. The completeness rule is restated so that it no longer
  names the generator's template, which is now in the node's params; the
  opening paragraph of § 4 names both signatures; *How it is read* says which
  name the generator's identity is read with.
- **The lead of the Decision** names the template and the served model.
- **Alternatives rejected** gains five entries — names as variants, the template
  as constructor configuration, `model_identity` unchanged as the source of what
  ran, optional fields with service-side defaults, and an in-place edit of
  ADR-C29 — and the entry rejecting a hand-typed `model:` parameter gains a
  closing sentence saying how it reads beside `served_model`.
- **Consequences** gain what each implementation issue now owes, how the
  citations of ADR-C29 elsewhere read, and what #267's reference service
  becomes; the INV-9 clause says the two keys change no wire shape; the prose
  #255 corrects includes the crate documentation's sentence on what params
  structs carry. Two sentences of ADR-C29 had gone stale before this change and
  are brought up to date: #252, which ADR-C29 left open, has since been decided by
  ADR-C30, so the consequence on `Context.chunks` says so and the list of what is
  left open drops it and names #253 instead.
- **Context.** ADR-C29's closing "Decided in #251." reads "ADR-C29 decided them
  in #251.", since this ADR was decided in #272.

Nothing else moves. The values, the context builder's trait, `ContextParams`,
ADR-C19 and ADR-C25, the nodes, the kinds, the port shapes, the `SchemaVersion`
bump, the trace, the pattern for model-bearing families and its deferral, and
`VectorStore`'s place outside it are ADR-C29's, restated.

**Why a superseding ADR rather than an amendment.** The sentence withdrawn is in
ADR-C29's **Decision**, and process rule 1 in `docs/adr/README.md` says an
accepted decision changes only by supersession; process rule 2's in-place
retraction reaches Context, Alternatives rejected and Consequences, never the
Decision. And the withdrawn sentence is not a factual error in the reasoning —
the reasoning around it stands and is now what argues the other way — but a
changed answer. Nothing implements ADR-C29 § 2 yet: #255, #257, #266 and #267
are not started, and no trait, proto field or service for generation exists in
the tree, so this is the cheapest moment to change it. The repository owner
chose a full rewrite over a narrow ADR that superseded ADR-C29 § 2 alone, so
that an implementer reads one text rather than two that must be read together —
the route ADR-C24 took for ADR-C7. Decided in #272.

## Decision

**The retrieval-to-generation chain is contracted end to end, by two traits over
three typed values, two node variants, two kinds, a model identity the component
reports itself, and a trace that names the context and the answer. The prompt
template and the served model are experiment variables: node parameters, hashed
with the pipeline and carried to the generator on every call.**

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

**`score` is the score the chunk carried in, on the producing node's scale — a
builder never assigns one.** A context builder selects, orders and renders; it
does not judge relevance, so a builder that drops chunks over its budget or
reorders what it keeps carries each surviving chunk's score through untouched.
Stated here because § 5 maps `Context.chunks` onto the trace's `RankedChunk`,
whose doc comment calls its score "the score the node gave it, on the node's own
scale" — true of a retriever, a fusion and a reranker, and read literally of a
builder it would invent a scoring step this contract does not have.

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
and the query that produced it" — and it rejected it on fusion: multi-query
retrieves with several rewritten queries and fuses the results, so "which query
the fused value carries has no correct answer". The same objection recurs here in
a different shape. A context has one producer rather than several legs, so
nothing merges; what makes the bundled query wrong is instead that the generator
may legitimately be handed a different query from the one the builder saw, and a
context carrying the builder's would make which query the generator is answering
ambiguous. Different mechanism, same defect: a value that carries a query cannot
say which query it speaks for.

### 2. Two traits in `ragondin-contracts`

Both are `#[async_trait]` and `Send + Sync`, like every other family (ADR-C8), so
the engine cannot tell a `Local` implementation from a `Remote` one (ADR-3).

```rust
#[async_trait]
pub trait ContextBuilder: Send + Sync {
    async fn build(&self, query: &Query, chunks: Vec<ScoredChunk>, params: &ContextParams)
        -> Result<Context, ComponentError>;
    async fn model_identity(&self) -> Result<ModelIdentity, ComponentError>;
}
```

```rust
#[async_trait]
pub trait Generator: Send + Sync {
    async fn generate(&self, query: &Query, context: &Context, params: &GenerateParams)
        -> Result<Answer, ComponentError>;
    async fn model_identity(&self, served_model: &str) -> Result<ModelIdentity, ComponentError>;
}
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

`GenerateParams { served_model: String, template: String, temperature: Option<f64>, seed: Option<u64>, max_tokens: Option<usize> }`
is per call. **`served_model` and `template` are required; the other three are
optional.** `served_model` is the name the generator asks its backend for: for a
`Remote` generator, the name the inference server serves the model under; for a
`Local` one, a name the component must recognise as the model it loaded, and a
name it does not recognise is refused as `ComponentError::InvalidRequest`.
`template` is the prompt template, whose grammar is stated below. **They are
per-call parameters and not constructor configuration, because of the `Remote`
face.**
`docs/code-architecture.md` § 7.1 lays out the shape a `Remote` adapter takes: a
client over a channel, delegating each trait method to the rpc that mirrors it.
Face 2 therefore carries the trait's calls and nothing else — there is no
configure rpc — so **constructor configuration never crosses the wire**. For the
one family that is `Remote` by design — the LLM inference server is what
`docs/system-architecture.md` § 10 names as deliberately not reimplemented and
called over the network — per-call params are therefore the only path from the
pipeline representation to the service. Knobs left service-side would sit outside
the logical form and therefore outside its hash, and two runs differing only in
temperature would content-address identically. ADR-9 puts a judge's "model hash,
prompt, temperature and seed" on exactly the same footing as the generator's, and
ADR-15 promises that "everything that ran is recorded". Both forbid that outcome.
The same holds of two runs differing only in their prompt template, or only in
the model they ask the inference server for: **a setting a researcher varies
between two runs must be in the pipeline representation**, or the two runs'
configurations are identical and nothing that compares them can say what
differed. A `Remote` generator's service therefore holds **no experiment
variable of its own**: every setting that decides the answer arrives in the
call, and the service relays it. What the service *is* — where it lives, the
HTTP client it uses, the dialect it speaks to its inference server — is not an
experiment variable, and #253 decides it.

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

**`served_model` and `template` are read exactly as `top_k` is.** Both are
ordinary node parameters — flat `String`s, which the grammar ADR-C22 fixed
already holds — so both enter the canonical logical form and the pipeline's
hash like any other parameter: a string parameter is fed to
`LogicalPipeline::content_hash` as its bytes (`feed_str` in
`core/ragondin-pipeline/src/hash.rs`), with no normalisation. The executor reads
each from the generator node's params and refuses the call with
`ExecError::InvalidParam` when the key is absent or holds something other than a
string, as `per_call_top_k` refuses an absent or mistyped `top_k`. **No default
is applied**, for the reason quoted above: a default template or a default model
chosen above the component would be a second, disagreeing copy of a decision that
belongs to the configuration. What the executor does not judge is the value: an
empty string, a name the backend does not serve and a malformed template reach
the component, which refuses each as `InvalidRequest` (below), exactly as a zero
`top_k` reaches a retriever and is refused there.

**Absence must survive face 2, and that needs saying.** The three optional fields mirror
on face 2 with **explicit presence** — proto3 `optional` — so an omitted field
decodes as `None` and never as a value, and the `Remote` adapter passes that
`None` through rather than substituting anything. Without the clause a plain
proto3 `double` that the caller omitted arrives as `0.0`, and a temperature of
zero is greedy decoding: a materially different generation, chosen by nobody,
recorded as if it had been asked for. This is ADR-C17's problem in a second
family. There, a proto3 enum gives an omitted field the number `0` with no notion
of absence, so `EMBED_ROLE_UNSPECIFIED = 0` is reserved and never valid and the
adapter rejects it; here the zero is a legitimate value rather than a reserved
one, so the mirror carries presence instead of reserving a number. Same defect —
the wire cannot say "absent" — and the same obligation: say which, in the
Decision, or a `Remote` implementation silently differs from a `Local` one on the
one axis the two faces promise to agree on.

**The two required fields mirror on face 2 as required strings**, and there
absence has the opposite shape: a proto3 `string` has no presence, so an omitted
field decodes as the empty string. **An empty `served_model` or an empty
`template` is refused as `InvalidRequest`** — by the `Remote` adapter before it
sends the call, by a service on receipt, and by a `Local` generator alike, so the
two faces agree on it. On face 1 the executor has already refused an absent key;
the empty string is what remains, and it names no model and renders no prompt.

**The template is a string with two placeholders, and the component renders
it.** `template` is opaque to everything above the component and hashed byte for
byte, so a YAML block scalar written with `|`, which keeps its final newline, and
one written with `|-`, which strips it, are two templates and two
configurations. Its grammar is the whole of the following, and a `Local`
generator and a `Remote` service implement the same one:

- `{query}` is replaced by the `text` of the `Query` the generator was handed,
  and `{context}` by the `text` of the `Context`.
- `{{` renders a literal `{`, and `}}` a literal `}`.
- Any other `{` or `}` makes the template malformed — a name between braces
  other than `query` or `context`, a `{` that no `}` closes, a lone `}` — and the
  call is refused as `InvalidRequest`. The refusal is the component's, at the
  call; nothing above the component parses a template.
- Each placeholder may appear any number of times, including none.
  Substitution is a single pass over the template: text substituted for a
  placeholder is never scanned again, so a query containing `{context}` renders
  as those nine characters.

**Who renders is the component** — a `Local` generator itself, a `Remote`
service on receipt — because face 2 carries the query and the context
separately, as face 1 does, and rendering them into one message is the
component's business. The rendered text is the whole of what the component asks
its model to answer: it adds no instruction text of its own, since a system
prompt kept service-side would be exactly the hidden template this decision
withdraws. How that one message is framed for the inference server is the
dialect's, and #253 decides it.

**A template is a value, not the sub-grammar ADR-C22 rejected.** ADR-C22 refused
flattened parameter **keys** — `filters.lang` — because they invent "a sub-grammar
inside a string key that nothing validates and nothing canonicalizes". A
template is a parameter's **value**, it is canonicalized as every string value
is, byte for byte, and it is validated by the one party that renders it.

**The context builder is not changed by this.** Its own configuration — the
template it renders passages into `Context.text` with, and the unit its budget is
counted in — stays constructor configuration, covered by its `model_identity`
under § 4, as ADR-C29 decided. #272 changed the generator's template and served
model and nothing else; the generator's template is the one this section names,
and the builder's is not a parameter of its node.

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
`Fusion` and `Reranker`. For a generator, likewise, that an empty `served_model`,
an empty `template` and a malformed template are rejected — each a refusal of
the call's form, which a suite can check without knowing the model. **Nothing
about content.** A suite that does not know
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
never reads the query ignores the port, as the test stub `StubReranker` in
`core/ragondin-contracts/src/lib.rs` ignores its `_query` today; the hashed edge
nobody reads is the verbosity ADR-C18 accepted deliberately, and accepting it
again here costs nothing new.

On the wire, `RawNode.component` accepts `context_builder` and `generator`. No
struct changes shape for it: `component` is a `String` and stays one. What widens
is **the vocabulary of `component:` values a configuration may name**, and that
is a change to the schema all the same, so it **bumps
`SchemaVersion::SUPPORTED`** under INV-9 — as ADR-C18's addition of `inputs` to
`RawGraph` did, which is what `SchemaVersion`'s own doc comment records as the
reason the supported version is what it is. **This ADR sanctions that bump by
name.** The bump is what an older build needs: without it, a document naming
`generator` parses, and the refusal arrives further down as
`ValidationError::UnknownComponent`, which reads as a misspelt family rather than
as a configuration written in a schema this build does not have. With it, the
version gate refuses the document for what it is — **for a document that states
its version**. One that states none still falls through to `UnknownComponent`,
because an absent `version:` reads as the build's own (`RawPipeline::version`
defaults to `SchemaVersion::CURRENT`, and a configuration writes the line only to
pin a version deliberately). The bump is what makes a clean refusal *possible*,
not what makes it universal.

`ParamValue` is untouched: every parameter these two nodes take is a flat scalar,
so the grammar ADR-C22 fixed as `String | Int | Float | Bool | List` needs
nothing added, and no `Map` demander appears here.

These additions are **the deliberate, versioned INV-1 break** on `ragondin-types`,
`ragondin-contracts` and `ragondin-pipeline`, sanctioned here and nowhere else.
`LogicalNode` and `ValueKind` stay non-`#[non_exhaustive]`, keeping the stance
`core/ragondin-pipeline/ARCHITECTURE.md` records: a consumer's exhaustive `match`
breaking is the intended signal that a new node kind needs handling.

### 4. Model identity is reported by the component

`ContextBuilder` carries `async fn model_identity(&self) -> Result<ModelIdentity, ComponentError>`,
and `Generator` carries
`async fn model_identity(&self, served_model: &str) -> Result<ModelIdentity, ComponentError>`;
each is mirrored by a `GetModelIdentity` rpc on face 2, the generator's taking
the served model as its request. It is `async` because the only M3
implementation of either family is `Remote` and must make an rpc to answer; a
synchronous method would force a `block_on` inside the trait, against ADR-C25.

**The generator's identity is a verification, and it takes the served model.**
`Generator::model_identity(served_model)` returns *the identity of the model this
component answers `served_model` with*:

- a **`Local`** generator returns the digest of the model it loaded — together
  with whatever else of its constructor configuration decides the answer, under
  the completeness rule below — when `served_model` is a name it recognises as
  that model, and refuses any other name as `InvalidRequest`;
- a **`Remote`** generator's adapter forwards `served_model` in the
  `GetModelIdentity` rpc, and the service asks its inference server what the
  server serves under `served_model`, and returns what the server reports — the served name, and its
  revision or digest where the server reports one — refusing, as
  `InvalidRequest`, a name the server does not serve.

For a relay in front of an inference server whose API reports only an alias, the
identity **is** that alias echoed back, plus whatever revision the server
reports. That is weaker than a digest and it is stated here rather than
discovered: the served name is already in the node's params and in the hash, so
what the identity adds is the revision where one is reported, and the check that
the backend serves what was asked. `ContextBuilder::model_identity(&self)` keeps
its shape: nothing handed to a builder per call decides its output beyond its
params, so there is no name for it to check.

**A mismatch is a refusal, never a warning.** A `served_model` the backend does
not serve is refused by `model_identity`, and the composition root treats that
refusal as fatal before the run begins: a run whose recorded configuration names
a model that did not answer is not recorded at all. `generate` refuses the same
name the same way, as `InvalidRequest`, so a backend that stops serving it after
the check fails the call rather than answering it.

**What the identity must be.** Two properties, and an implementation that fails
either is wrong:

- **Stable** across calls while nothing has changed. No timestamp, no request id,
  no counter — P4 requires that a rerun of the same inputs produce the same
  `run_id`, and an identity that varied per call would make every run unique by
  construction.
- **Complete.** It covers every knob that decides the answer **and is not in the
  node's params**. A generator's template and served model now are, so for a
  generator what remains is the model's revision behind the served name — with,
  for a `Local` generator, whatever of its constructor configuration decides the
  answer — and the check that the backend serves what was asked; for a context
  builder it is still its template and the unit its budget is counted in. A knob that changes the
  answers while neither the node's params nor the identity moves is the failure
  this method exists to prevent.

For a `Local` component the identity is a digest of its own configuration and its
model file — and **the completeness rule is what binds, not the presence of a
file**. A `Local` component with no model digests its configuration alone; the
concatenating context builder of #262 is exactly that case, and its identity is
its template and the unit its budget is counted in, not a file it does not have —
its configuration, never the per-call value it was handed. A constant is
conformant for a component with no configuration that decides its output at all.
What is never conformant is an identity that omits something which does decide
the output. For a `Remote` component the identity is **what the service reports,
and that is the whole extent of the guarantee**: a service that answers wrongly
cannot be checked from here. That is not a new concession. ADR-15 promises
traceability, not verification, and `model_hashes` is already caller-supplied
trust —
`Evaluation::model_hashes` in `eval/ragondin-harness/src/evaluate.rs` is
documented as "supplied by the caller rather than discovered here".

**How it is read.** The composition root constructs an instance, awaits
`model_identity`, records the result in `model_hashes` under the roles
`generator` and `context_builder`, and registers a constructor that builds
another instance. The synchronous `ComponentCtor` is therefore not an obstacle:
`bench` in `bin/ragondin/src/bench.rs` is already `async`, and already computes
`model_hashes` before it registers anything and before it calls `evaluate` — it
does so today so that a missing model file is found before the expensive steps.
A generator's identity is read **once per generator node, with that node's
`served_model`**, at that same point, before the run; a refusal ends the run
there, as stated above.
Stated plainly, because it is the part a reader will otherwise assume away: **the
identity is read from an instance other than the one that ran.** For a `Remote`
component both instances reach one URL, so the two agree unless the service
changed mid-run, which is the same window every other caller-supplied hash
already has.

**The pattern, stated for every model-bearing family: any family whose output
depends on a model reports that model's identity through its trait.** This ADR
applies the pattern to the two new traits **now**, and deliberately does **not**
add the method to `Embedder` or `Reranker`. Neither has a `Remote`
implementation today, so neither has the problem the method solves; and adding a
method to a trait breaks every existing implementation, in and out of this
workspace. The ADR that resolves #101 — how a node names and constructs a
`Remote` service — is where those two follow, as a deliberate act of its own
rather than a side effect of this one.

**`VectorStore` is not in the pattern and not in that deferral.** A store holds
and searches vectors an embedder produced; no model of its own decides what it
returns, so there is no identity for it to report and nothing here says there
should be. It also holds nothing a run's identity does not already derive from
elsewhere: the vectors are a function of the chunk set, which `index_version`
names, and of the embedder, which `model_hashes` records. That derivation is
weaker than it sounds, and this ADR does not overstate it. `index_version` is a
digest of chunk ids, texts and document ids, and `CorpusIndex::version`'s doc
comment in `eval/ragondin-harness/src/corpus.rs` is careful that it "addresses
*a* chunk set, not provably the one that was retrieved from"; and ADR-C17 records
that an embedder's prefixes — configuration that changes the numbers — reach run
identity nowhere today, a hole it names and leaves to #31. Neither gap is closed
here. The claim is only the narrow one: a store adds no **model** of its own to
the tuple, so the pattern does not reach it. The pattern reaches a family because
it is model-bearing, never because it is a component.

### 5. What the trace names

This extends ADR-C28 to the two new values, on the same reasoning and with the
same asymmetry between the two sides of a node.

For what a node **produced**, `ValueSummary` gains
`Context { chunks: Vec<RankedChunk>, text: String }` and `Answer { text: String }`.
Those two shapes are pinned here, because what a node produced is what a reader
of a stored run reads.

For what a node **consumed**, inputs stay summaries, as ADR-C28 decided: a context
is summarised by its chunk count and its text length, an answer by its text
length. An input is the output of the node that produced it, already named under
that node. **The input-side shapes are named by #259, inside the engine, and this
ADR deliberately does not pin them.** `ValueSummary` lives in `ragondin-engine`,
which INV-2 keeps freely refactorable; what binds is the content above — count
and text length for a context, text length for an answer — and whether that
arrives as two new variants, as fields on the produced ones, or as something #259
finds better is the engine's to choose. The asymmetry between the two sides is
therefore deliberate, not an omission.

Size is bounded by **the builder's configuration, and never by the corpus** —
the argument ADR-C28 made for `top_k`, holding here in a slightly weaker form.
`budget` is the cap, but its unit is the implementation's (§ 2), so a builder
counting characters bounds the traced text directly while one counting chunks
bounds it only together with the chunking that feeds it. Both are the user's
configuration, so the corpus never bounds a traced context **directly** — but the
qualification is real rather than formal: no chunker exists yet, so a document
becomes one chunk carrying its whole text (`eval/ragondin-harness/src/corpus.rs`
says so and says why), and a chunk-counting builder's traced text therefore
tracks document length until a chunker arrives in the pipeline. ADR-C28's bound
on `top_k` is the tighter one; this is the same argument held to a lower
standard, and saying so is cheaper than discovering it from a large trace.

And the trace is **not part of run identity**:
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
  This ADR does put the served model in the node's params, as what is **asked
  for** — an experiment variable — and keeps the reported identity beside it as
  what answered; the rejection stands against a parameter *replacing* the
  identity, not against the parameter existing.

- **Nothing in M3 — the generator does not enter `model_hashes`.** Self-preference
  detection waits for the judge in M4 anyway, so the cost looks deferred. It is
  not: the trait gains the method later as an INV-1 break on a boundary with
  implementations behind it, and in the meantime every M3 run's identity omits
  the one model that decides its answers.

- **A synchronous `model_identity`.** Cheaper to call from a composition root and
  wrong for the only implementation M3 has: a `Remote` component must make an rpc
  to answer, and a synchronous method leaves it a `block_on` inside the trait,
  which is what ADR-C25 forbids.

- **A counts-only trace, or an answer returned but not traced.** ADR-C28's
  *Aggregates only, no per-query record anywhere* again, with the same
  consequence: the harness scores from the return
  value and drops it, so there is no per-query regression fixture and no replay.
  It was rejected for chunks in ADR-C28, and nothing about text makes it a better
  answer here — the SciFact per-query fixture reads its rankings out of a stored
  run's `traces.json` precisely because they were traced rather than dropped.

- **`Context` holding the query.** ADR-C18 weighed and rejected the same shape for
  chunks, on fusion: several legs may carry several rewritten queries, so "which
  query the fused value carries has no correct answer". That ground does not
  transfer literally — a context has one producer — but the objection recurs in a
  different shape, and § 1 above states it: the generator may receive a different
  query from the one the builder saw, so a bundled query leaves which query the
  answer speaks for undecidable.

- **Names as variants** — `impl: vllm-t1` and `impl: vllm-t2`, one service
  instance per template, bound by the composition root, with the identity
  recording what each served. It needs no change to ADR-C29, and it is a
  workaround rather than a design: one deployment per prompt variant, and a YAML
  that names a variant without saying what it is, so the configuration a run
  records says which service answered and not which prompt it was given.

- **The template as constructor configuration** — ADR-C29's own choice, and
  withdrawn here for the reason § 2 gives: constructor configuration never
  crosses the `Remote` face, so a template kept there lives outside the pipeline
  representation and outside its hash, and a setting a researcher varies between
  two runs must be in the representation or the two runs cannot be told apart by
  their configuration.

- **`model_identity` unchanged, as the source of what ran.** Once `served_model`
  is a hashed node parameter it already says what was asked; an identity that
  only echoed the ask back would add nothing to the run's record. What remains
  for the method is verification — the revision behind the name where the
  backend reports one, and the refusal of a name the backend does not serve —
  and that is the job § 4 gives it.

- **Optional `served_model` and `template`, with service-side defaults.**
  Absent and explicit would then be two spellings of one generation with two
  hashes: a node that omits `template` and one that writes out the service's
  default template run the same prompt and content-address differently, and the
  default itself sits service-side, outside the representation, where this
  decision exists to take it from. It is ADR-C22's argument against `Null` — two
  spellings of one configuration on a boundary that is content-addressed — met
  from the other side, and it is refused for the same reason: the executor
  invents no default, and neither does a service.

- **An in-place edit of ADR-C29.** Process rule 1 changes an accepted Decision
  only by supersession, and process rule 2's in-place retraction reaches
  Context, Alternatives rejected and Consequences, never the Decision; the
  sentence withdrawn is in the Decision. ADR-C24's supersession of ADR-C7 is the
  precedent this follows.

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

- **What each issue owes for the template and the served model**, on top of the
  list above. #255: the five-field `GenerateParams`, and
  `Generator::model_identity(&self, served_model: &str)`. #257: `GenerateRequest`
  carries `served_model` and `template` as required strings, and the generator's
  `GetModelIdentity` takes the served model. #258: the generator suite's
  refusals of an empty `served_model`, an empty `template` and a malformed
  template. #259: the executor reads the two
  required params from the generator node and refuses an absent or mistyped one
  with `ExecError::InvalidParam`, applying no default. #260: the stub generator
  recognises one fixed served name and refuses any other, and renders the
  template under the grammar of § 2. #261: the `Remote` generator adapter refuses
  an empty `served_model` or `template` before sending, and calls the identity
  rpc with the served model. #266: the binary reads the generator's identity once
  per generator node with that node's `served_model`, before the run, and a
  refusal is fatal. #267: the reference service is a stateless relay. #268: the
  exit-criterion configurations write `served_model` and `template` on every
  generator node.

- **The INV-1 break is sanctioned here and covers three crates.**
  `ragondin-types` gains three value types, `ragondin-contracts` two traits and
  two params structs, `ragondin-pipeline` two `LogicalNode` variants and two
  `ValueKind` variants. **INV-9**: no struct on the wire changes shape — what
  widens is the vocabulary of `component:` values a configuration may name — and
  `SchemaVersion::SUPPORTED` bumps for it all the same. `served_model` and
  `template` add nothing to that: they are keys of a node's existing params map,
  in the flat grammar ADR-C22 fixed, and change no wire shape. **INV-2**: `ragondin-engine` changes
  freely — the registry, the planner, `NodeValue` and `ValueSummary` are its own,
  and the executor's output type becomes an enum over what a terminal node may
  produce, which is engine-internal and stable to nobody: ADR-C21 decided that
  `ragondin-engine` is not an API boundary, and this is that licence being used,
  not re-argued. **INV-7**: no privilege anywhere — the stub, the concatenating
  builder and the `Remote` adapters implement the same two traits and pass the
  same suites, with no path a third-party component could not take. **INV-10**:
  untouched; the trace is still the executor's return value.

- **Prose this decision falsifies, corrected by the diff that creates the
  obligation.** `ragondin-types`' crate documentation promises `Context` and
  `Generation`; the type is named `Answer`. #254 corrects that sentence in the
  same diff that adds the type, not in a follow-up. `ragondin-contracts`' crate
  documentation lists the families it defines and names `ContextBuilder` and
  `Generator` as absent by design; #255 is where both stop being true. The same
  crate documentation says the params structs "carry only what varies **per
  call**", and `GenerateParams` carries settings fixed per node — as ADR-C29's
  temperature and seed already were — because per-call params are the only path
  to a `Remote` service; #255 qualifies that sentence in the same diff.

- **ADR-C29 is superseded in full, and marked so.** Every citation of ADR-C29 in
  the tree now resolves to a superseded ADR and is read against this one; the
  sections it cites exist here under the same numbers and headings. Two
  citations lean on the sentence this ADR withdraws. **ADR-C30 § 1** says that
  producing a short answer "is the prompt template's job — constructor
  configuration of the generator under ADR-C29 — and never the metric's", and its
  Alternatives rejected that the template "is recorded in the generator's
  identity (ADR-C29)". The rule they state — no extraction step in the metric —
  does not depend on where the template lives, and stands; the parenthetical and
  the clause about identity are read against this ADR, under which the template
  is a node parameter, recorded in the pipeline's hash. The comments on #254,
  #255 and #256 point at ADR-C29's sections, and the same sections exist here.
  #267's constructor configuration — which lists the model name and the template,
  and the generation knobs besides — shrinks to what the service *is*: the
  inference server's base URL and its dialect (#253). Everything that decides an
  answer arrives in the call.

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
  two the retrieval metrics read once the terminal node is a generator. ADR-C30
  has since decided that question in #252; this ADR leaves its answer untouched.

- **A trace grows by roughly one context and one answer per query**, both bounded
  by the user's configuration: the context by the builder's `budget` and, where
  that budget is not counted in text, by the chunking that feeds it; the answer
  by `max_tokens` where a caller sets one. Neither grows with the corpus.

- **What is deliberately left open.** How a node names and constructs a `Remote`
  service (#101). What the reference `Remote` generator service is — where it
  lives, the HTTP client it brings, the dialect it speaks to its inference
  server (#253).
  `Embedder` and `Reranker` identity, which follows with #101 — `VectorStore` is
  not on that list, because a store is not model-bearing and this ADR says
  nothing about it. Token usage on `Answer`, which is a later and deliberate
  INV-1 break. Each is named so that the next reader can see it was weighed
  rather than missed.

- No entry in `docs/OPEN_QUESTIONS.md` is opened, closed, or changed.

## Status

Accepted.
