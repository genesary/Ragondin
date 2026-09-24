---
id: ADR-C32
title: A node names a Remote component by an ordinary implementation name; its address is bound by the composition root, never written in the pipeline; the embedder and the reranker report their model identity
status: accepted
invariants: [INV-1, INV-6, INV-7, INV-8, INV-9]
supersedes: []
superseded_by: null
---

# ADR-C32: A node names a Remote component by an ordinary implementation name; its address is bound by the composition root, never written in the pipeline; the embedder and the reranker report their model identity

## Context

Decision issue #101 asked how a `Remote` `Embedder` or `VectorStore` is
constructed, since every documented route to a `Remote<T>` passes through a
pipeline node and those two families are not nodes. It was then extended to the
generator and moved to M3 as a blocking decision: ADR-C31 makes the generator
the first family the platform runs `Remote` by design, and #266 — the binary
that composes the generation path — cannot register one until something says
what a user writes to point a node at a service and what the composition root
does with it. ADR-C31 also left one obligation here by name: *"The ADR that
resolves #101 — how a node names and constructs a `Remote` service — is where
those two follow, as a deliberate act of its own rather than a side effect of
this one"* — the two being `Embedder` and `Reranker` identity.

### What the tree does today

- **A constructor sees the node's parameters and nothing else.**
  `ComponentCtor` in `engine/ragondin-engine/src/context.rs` is
  `Fn(&Params) -> Result<Box<T>, ConstructionError>`; it is never handed the
  `EngineContext`. Each family's table is a `Registry` over a `BTreeMap`, and
  registering a name twice **replaces** the earlier constructor —
  `register_retriever`'s documentation says "the last registration wins".
- **Planning resolves three families.** `resolve` in
  `engine/ragondin-engine/src/plan.rs` matches a `Retriever`, a `Fusion` and a
  `Reranker`, and `ResolvedComponent` has no variant for an embedder or a store.
  `build_embedder` and `build_vector_store` have no caller outside the engine's
  own tests, and neither have `register_embedder` and `register_vector_store`.
  `ComponentFamily`'s documentation in `engine/ragondin-engine/src/error.rs`
  records the gap: "That is an open question, not a settled design: how a
  `Remote` embedder or vector store is built is #101."
- **The embedder is configured on the `dense` node.** `register` in
  `bin/ragondin/src/wiring.rs` registers `dense` with a closure that reads the
  node's `model`, `tokenizer`, `max_sequence_length`, `query_prefix` and
  `passage_prefix` (`embedder_of`), builds an ONNX embedder from them, and hands
  it with a pre-seeded in-memory store to `DenseRetriever::new`. `embedder_of`
  reads each prefix as `optional_string(…)?.unwrap_or_default()`, so an absent
  prefix and `query_prefix: ""` build the same embedder while hashing as two
  configurations. `embedder_spec` refuses two `dense` nodes that configure
  different embedders, because the corpus is embedded once.
- **Model identity is a file digest keyed on two names.** `model_hashes` in the
  same file walks the nodes, matches the `impl:` names `dense` and
  `cross_encoder`, digests the `model` file through `file_digest`, and refuses a
  second model on one role. Nothing else records a model: the tokenizer enters
  run identity only through its path, which is a node parameter, and never
  through its contents. A reranker registered under any other `impl:` name
  records no hash at all.
- **The order in `bench` is fixed and argued.** `run` in
  `bin/ragondin/src/bench.rs` loads the configuration, computes `model_hashes` —
  its comment: "a model file that is missing is found now, not after the
  expensive step" — then loads the benchmark, builds the `CorpusIndex`, embeds
  the corpus in `prepare`, registers, evaluates and saves. `prepare` is
  compiled only under `#[cfg(feature = "onnx")]`; the lean build's twin embeds
  nothing.

### Why an address cannot be a parameter

`LogicalPipeline::content_hash` in `core/ragondin-pipeline/src/hash.rs` feeds
every node's variant, id, implementation, inputs and every parameter in sorted
key order into the digest, and it admits no exclusion. ADR-C2's Amendments
record why that is wanted: "**complete identity** is the property to preserve".
So a value written in the pipeline configuration is in `run_id`, and the only
way to keep an address out of run identity is to keep it out of the YAML.

Three accepted texts require exactly that. ADR-3 promises that a `Remote`
component that wins can be ported to `Local` "with no change to any user's
configuration". ADR-7 promises that a developer "deploys to Kubernetes without
changing anything". The M6 milestone's exit criterion is that "the same
configuration runs standalone from a local file (LocalFile ConfigSource) and
in-cluster via the controller and purpose-built gRPC config service (Stream
ConfigSource), unchanged". A laptop's `localhost:50051` and a cluster's service
name are two addresses for one experiment, and P4 identifies a run by the
complete tuple of its **inputs**: where a service happened to listen is not
one of them. An address in the node's parameters would give one experiment two
`run_id`s — that is the decisive reason — and would make porting a component to
`Local` a configuration change on top of whatever keys the two natures read
differently. ADR-3's promise is not kept whole by this decision either: for the
embedder and the reranker the keys differ by nature (§ 1), and Consequences
says so.

The same objection disposes of the prose that points the other way.
`docs/code-architecture.md` § 10 says a `Remote` contribution is "Named by URL
in the configuration, resolved to a `Remote<T>`", and § 8.1 draws the embedder
and store tables beside a box reading "OPEN DECISION #101". Both were written
before the question was decided, and both are corrected under Consequences.

### Two further holes this decision must close to be true

- **Identity.** ADR-C31 gives the generator and the context builder a
  `model_identity` and deferred `Embedder` and `Reranker` here. Once a reranker
  may be bound under a name other than `cross_encoder`, `model_hashes`' match on
  `impl:` names is not a gap but a silent omission: a run over a bound reranker
  would record no reranker at all.
- **Prefixes on face 2.** ADR-C17 put the prefix text in the embedder's
  constructor and the role on the call, and required face 2 to carry the role.
  It did not say, because no `Remote` embedder was reachable, whether the
  `Remote` adapter or the service applies the prefix. If both do, the text is
  prefixed twice, and no conformance suite can see it: it does not know the
  model.

The repository owner decided #101 on 2026-09-23, after two independent
reviews. Decided in #101.

## Decision

**A `Remote` component is named in the pipeline exactly as a `Local` one is, by
an ordinary implementation name. Its address never enters the pipeline
configuration: the composition root binds each name to an address from the
deployment, records the bindings a run used outside its identity, and reads the
identity of every model-bearing component — the embedder and the reranker
included — before the run.**

### 1. Naming: an implementation name, never an address

**A node names a `Remote` component by its `impl:` value** — `impl: vllm`,
`impl: bge-reranker` — exactly as it names a `Local` one. The pipeline crate
reserves no name for it; `remote` is not a convention, and a node naming a
`Remote` component is indistinguishable in the configuration from one naming a
`Local` one. The composition root registers that name through the same
`register_*` call it uses for `bm25` (INV-7).

**The address never enters the pipeline configuration** — not as a parameter
of the node, not as a key anywhere else in the document. What a node says about
a `Remote` component is what it would say about a `Local` one of the same
family: the implementation name and the parameters the component reads.

**A `dense` node names its embedder by one flat key, `embedder: <name>`, and the
key is required.** There is no default: an absent key and `embedder: onnx`
would be two spellings of one configuration with two hashes. The composition
root refuses a `dense` node without it, or with a value that is not a non-empty
string, as a fatal error naming the node. `onnx` is the name the composition
root gives its in-process ONNX embedder. The composition root resolves the name
itself, inside the `dense` constructor closure it writes (§ 3); the engine never
resolves an embedder, and `build_embedder` stays without a caller.

**The keys a `dense` node may carry are fixed by the nature of the embedder it
names**, and every other key is refused:

- on every `dense` node: `top_k` (read by the executor), `embedder`, and the
  optional `query_prefix` and `passage_prefix`;
- when `embedder:` names a `Local` ONNX embedder: `model` and `tokenizer`, both
  required file paths, and the optional `max_sequence_length`; `served_model`
  is refused, since the ONNX embedder recognises no served-model name (§ 4);
- when `embedder:` names a `Remote` embedder: `served_model`, required — the
  name the inference service serves the model under. The composition root
  refuses a node without it, as ADR-C31 § 4 has it refuse a generator node
  without one.

**A reranker node has the same shape.** Every reranker node may carry `top_k`.
One naming the `Local` ONNX reranker (`cross_encoder`) carries `model` and
`tokenizer`, required, and the optional `max_sequence_length`, and `served_model`
is refused on it; one naming a bound `Remote` reranker carries `served_model`,
required, and nothing else besides `top_k`.

A key outside those sets is **refused, never hashed as inert**: `served_model`
on a node whose embedder or reranker is the `Local` ONNX one, or `model`,
`tokenizer` or `max_sequence_length` on a node whose embedder or reranker is
`Remote`, is an error naming the node and the key, raised by the composition root
when it reads the node (§ 4's order puts that before the benchmark is loaded).

**`query_prefix` and `passage_prefix` stay parameters of the `dense` node, for
both natures**, as ADR-C17 and `embedder_of` have them. **An empty-string prefix
is refused**; absence is the only spelling of "no prefix". An absent prefix and
`""` build the same embedder, so INV-8 requires them to hash alike, and refusing
the second spelling is how that is made true without a canonicalization step
that deletes a key.

**One embedder per pipeline, for both natures.** Every `dense` node of a
pipeline names the same embedder and gives the same value — or the same
absence — for every key that embedder's nature reads, prefixes included; `top_k`
may differ. Two `dense` nodes that disagree are refused, as `embedder_spec`
refuses them today: the corpus is embedded once.

### 2. Binding: the composition root maps names to addresses

**`ragondin bench` takes `--remote <family>/<name>=<uri>`, repeatable.** The
argument splits at its first `/` and at the first `=` after it: `<family>` is
the text before the `/`, `<name>` the text between the `/` and the `=`, and
`<uri>` the rest. `<family>` is one of the node families — `retriever`,
`fusion`, `reranker`, and, once ADR-C31's nodes land, `context_builder` and
`generator` — or `embedder`, the one family that is not a node and that this
ADR reaches. A binding is keyed by the pair: `reranker/bge` and `embedder/bge`
are two distinct bindings. In M6 the same arguments are written in the pod
spec; the `Stream` configuration source carries only the pipeline
representation, as ADR-7 requires.

**Each of the following is refused, as a fatal error naming the argument,
before the benchmark is loaded and before anything is planned:**

- an argument with no `/`, no `=` after it, or an empty `<family>`, `<name>`
  or `<uri>`;
- a `<family>` that is not one of those above;
- a `<uri>` that is not an absolute URI of the shape `http://<host>` or
  `http://<host>:<port>` — the scheme `http`, a host, an optional port, and no
  path, query or fragment; an `https` URI is refused with them, since TLS is not
  decided (§ 3);
- the same `<family>/<name>` bound twice, whatever the two URIs;
- a `<name>` under which the composition root registers a `Local` component of
  that family — or, for `embedder`, resolves a `Local` embedder — in **any**
  build of it, whatever features this build carries, so
  that one command line is accepted or refused alike by every build — without
  the check, the registry's replace-on-reregister would let the later
  registration silently override the earlier;
- a binding no node of the loaded pipeline uses: for a node family, no node of
  that family has that `impl:`; for `embedder`, no `dense` node has that
  `embedder:`. A binding nothing uses is a typo, and recording it as provenance
  would be false.

The first five are refused when the arguments are parsed, the last once the
configuration is loaded. A build compiled without the `remote` feature
(ADR-C14) accepts the argument, runs the five parsing checks above on it, and
then refuses any binding that passes them, naming the feature.
**`validate` takes no bindings** and passes a node naming any implementation,
bound or not — by design, since it never plans. It applies none of § 1's wiring
checks either: `run` in `bin/ragondin/src/validate.rs` loads, validates and
hashes the configuration and runs no composition-root check, so a `dense` node
without `embedder:`, or with an empty prefix, still validates and prints a hash
that `bench` then refuses.

**One explicit `register_*` call per binding of a node family**, on the
`EngineContext` the composition root already holds; for an `embedder` binding,
the bound name is resolved inside the `dense` constructor closure (§ 3).
Nothing here reaches a registry without an `EngineContext` (INV-6), and
`docs/OPEN_QUESTIONS.md` #1 — registration ergonomics — is untouched.

**The bindings a run used are recorded on the `Run`, outside its identity.**
Each is recorded as its family, its name and its URI as written. They are not a
field of `RunInputs`, and `run_id` does not digest them: two runs with one
`run_id` and different bindings are one experiment run twice. The store keeps
the record it already has — `FileSystemRunStore::save` in
`runtime/ragondin-experiments/src/store.rs` "does nothing if that run is already
stored" — so a rerun of a stored `run_id` leaves the first run's metrics,
traces and bindings in place, the same rule every rerun of an identical `run_id`
follows today. The field's name and shape are #266's.

### 3. Construction

**A bound constructor is an ordinary `ComponentCtor`.** It reads the node's
parameters like any other constructor and returns the `Remote` adapter for its
family over a `tonic` `Channel`. The `Channel` is built **once per binding**, in
the `async` `bench`, from the parsed URI, as a lazily connecting channel
(`Endpoint::connect_lazy`), and cloned into the constructor; `Channel` is
`Clone`, so every node naming the binding shares one channel. **No connection is
attempted at construction.** ADR-C25 "governs the contract's `async fn`s, and
not construction", so it does not forbid a connect there; but a synchronous
constructor called under a running runtime cannot block on an `async` connect,
and a connect at construction would only move the first failure earlier than
the first call that exists to report it. An unreachable service is reported at
its first call — the identity call of § 4 — as `ComponentError::Unavailable`.

**For the embedder, the composition root resolves the name inside the `dense`
closure.** The closure reads `embedder:`; for `onnx` it builds the in-process
ONNX embedder from `model`, `tokenizer` and `max_sequence_length`; for a name
bound under `embedder/<name>` it builds the `Remote` embedder adapter over that
binding's channel, with the node's prefixes. It builds the dense retriever with
a served model as constructor configuration, which the retriever passes in the
`EmbedParams` of every call (§ 4): `None` for `onnx`, meaning the model it
loaded, and the node's `served_model` for a bound embedder. The closure is
the composition root's own code, which a third party composing its own binary
writes the same way (INV-7).

**TLS and authentication are out of scope** until M6 decides them.

### 4. Identity, and the order in `bench`

**`served_model` is a per-call parameter of the embedder and the reranker, on
both faces.** `EmbedParams` and `RerankParams` each gain
`served_model: Option<String>`. `Some(name)` asks the component for the model it
serves under `name`; `None` asks for the model the component loaded. A `Local`
component recognises the names it is configured with — its constructor
configuration, which binds a third-party `Local` component serving several
names — and refuses any other name as `InvalidRequest`; a `Local` component that
loads several models also refuses `None` as `InvalidRequest`, since only a
single-model component has a model `None` can mean. **The ONNX embedder and
reranker are configured with no served-model name**: they answer `None` and
refuse every `Some(name)` as `InvalidRequest`, and the composition root passes
`None` on every call to them. A `Remote` service refuses, as `InvalidRequest`, a
name it does not serve, and **refuses `None`** the same way: a service has no
loaded model that `None` could name, which is why § 1 requires `served_model` on
a node bound to a `Remote` embedder or reranker. The adapter and the service
each refuse an empty name the same way. On face 2 the
field is a proto3 `optional string` on the Embed and Rerank requests, so an
omitted one decodes as `None` under ADR-C31 § 2's presence rule. This is the
treatment ADR-C31 gives the generator, applied to the two families it deferred
here; the generator's value is required and these two are optional, because a
`Local` component that loaded one model needs no name to find it.

A served-model name is not the prefix ADR-C17 refused to put on `EmbedParams`.
That refusal was of model-specific prompt text, prepended to the input and
transmitted per call; a served-model name is an identifier the backend resolves,
and it changes no text. ADR-C31 § 2's "nothing here moves a prefix onto a
per-call struct" still holds.

**Where the value comes from.** For the embedder, the dense retriever holds a
served model as constructor configuration — `None` over the ONNX embedder, the
`dense` node's `served_model` over a bound one, as the composition root reads
it — and passes it in the `EmbedParams` of every call; the corpus embedding of
§ 4's step 4 passes the same value. For the reranker, the executor
reads `served_model` from the reranker node's params on each call, exactly as
`per_call_top_k` in `engine/ragondin-engine/src/execute.rs` reads `top_k`, except
that it is optional: an absent key is `None` — always the case on a
`cross_encoder` node, where § 1 refuses the key — and a value that is not a
string is refused with `ExecError::InvalidParam`.

**`Embedder` and `Reranker` gain
`async fn model_identity(&self, served_model: Option<&str>) -> Result<ModelIdentity, ComponentError>`**,
each mirrored on face 2 by a `GetModelIdentity` rpc whose request carries the
served model as a proto3 `optional string`. The generator's takes `&str`, since
ADR-C31 makes its value required. **This is the deliberate, versioned INV-1
break on `ragondin-contracts` that ADR-C31 deferred here**, sanctioned by this
section and nowhere else: adding a method to a trait breaks every implementation
of it, in and out of the workspace. The two new fields are additive at the Rust
API — `EmbedParams` and `RerankParams` are `#[non_exhaustive]` and built through
constructors — and how a caller sets one is #255's. It is required rather than optional because
`model_hashes` today keys on the `impl:` names `dense` and `cross_encoder`, so a
reranker bound under any other name would record no hash at all, silently. The
two properties ADR-C31 § 4 requires of an identity — **stable** across calls
while nothing changed, and **complete** over every knob that decides the output
and is not in the node's parameters — apply to both methods unchanged.

**A `Local` ONNX component's identity is `<model>+<tokenizer>`**: the
lowercase hex SHA-256 of the model file's bytes, the character `+`, and the
lowercase hex SHA-256 of the tokenizer file's bytes, returned for `None`; any
`Some(name)` is refused as `InvalidRequest`. It covers the ONNX embedder and the ONNX reranker alike. It does not cover the prefixes or
`max_sequence_length`: those are node parameters and already in
`content_hash`, and ADR-C31's completeness rule is about the knobs that are
not. The tokenizer's contents are what it adds, and they are the hole today —
only the tokenizer's path is hashed. **The ONNX crates compute both digests in
their constructor**, beside the synchronous work already done at construction —
loading the session through `commit_from_file` and the tokenizer through
`Tokenizer::from_file` — reading the files' bytes again or from memory; and `model_identity` returns the stored value:
ADR-C25 governs the call, not construction, and the call then does no blocking
work at all.

**A `Remote` embedder's or reranker's identity is what its service reports for
the `served_model` it was asked**, in the shape ADR-C31 § 4 defines for the
generator — for a service whose backend reports only an alias, that alias
echoed back plus whatever revision the backend reports. The adapter forwards
the `served_model` it is handed, in `GetModelIdentity` and in every Embed and
Rerank request. A `served_model` the service does not serve is refused as
`InvalidRequest`, and the composition root treats that refusal as fatal. An
empty identity is refused by the adapter, as ADR-C31 § 1 requires of every
`ModelIdentity`.

**The `Remote` embedder adapter applies the prefixes; the text on the Embed rpc
is final.** The adapter prepends `query_prefix` to each text of a
`EmbedRole::Query` call and `passage_prefix` to each text of a
`EmbedRole::Passage` call, then sends. The role field stays mandatory on the
wire — `EMBED_ROLE_UNSPECIFIED` is refused, as ADR-C17 decided — and **a
service must not prefix or otherwise transform the text by the role**; it **may
use the role for anything that is not text** — selecting a query tower or a
passage tower, for instance. This is normative, because no conformance test can observe double
prefixing. It makes the hashed YAML the one source of the prefix, makes a
`Local` and a `Remote` embedder of one model behave identically given the same
node, and leaves a service author nothing to configure.

**The order in `bench`:**

1. Load the configuration, refuse what v0 does not run, check the `dense` and
   reranker nodes' keys (§ 1) and the bindings' use (§ 2).
2. For each model-bearing node whose name the composition root knows — its
   `Local` names and its bindings — construct one instance per node naming it,
   and await `model_identity` with that node's `served_model`: for a `dense`
   node the instance constructed is the embedder its `embedder:` names, and for
   a reranker node the reranker it names — each read with the node's
   `served_model`, `None` when the key is absent — and for a generator node the
   identity is read with its required `served_model`, as ADR-C31 § 4 states;
   a context builder node's identity is read with no argument, as ADR-C31 § 4
   states. Record the results in
   `model_hashes` under the roles `embedder`, `reranker`, `generator` and
   `context_builder` — by family, never by `impl:` name. Two nodes on one role
   whose identities differ are refused under the one-model-per-role rule
   `model_hashes` already applies. Any failure here, including
   `ComponentError::Unavailable`, is fatal, before anything expensive has run.
3. Load the benchmark and build the `CorpusIndex`.
4. Embed the corpus (`prepare`) through the embedder the `dense` nodes name,
   `Local` or `Remote`.
5. Register: the `Local` constructors, and one per binding.
6. Evaluate, and save.

A node name the composition root does not know is skipped in step 2 and reaches
the planner, whose `PlanError::UnknownImpl` names the family and the name. An
`embedder:` name it does not know has no planner to reach, since no plan ever
looks an embedder up, so the composition root refuses it in step 1, naming the
node and the name. A name the composition root knows — one it registers or
resolves in any build of it, § 2 — but whose backend this build cannot
construct, such as `embedder: onnx` in a build without the `onnx` feature, is
refused in step 1, naming the feature, rather than reaching the planner. Step 2
keeps today's fail-fast — a missing model file, and
now an unreachable service or a model it does not serve, "is found now".

**The conformance suites** — #258's and the existing `Embedder` and `Reranker`
suites — gain two scenarios for every model-bearing family: `identity
non-empty`, and `identity stable across two calls`.

### 5. What this ADR does not reach

**A `Remote` vector store is deferred, with a named trigger.** `VectorStore`
has `upsert` and `search`, and nothing that clears a store or scopes it to a
namespace. An external store keeps its data across runs, so `index_version` —
`CorpusIndex::version` — would name a chunk set that is not provably the one
searched, and ADR-C26's one-index guarantee would fail. A `Remote` store returns
the day `VectorStore` gains an operation that makes a store's content
addressable per run: a decision of its own, taken with #24. Until then
`embedder` is the only non-node family `--remote` accepts.

**Embedding the corpus through a `Remote` embedder is in scope of #266.**
`prepare` is compiled only under `#[cfg(feature = "onnx")]`, and the binary's
`onnx` feature also carries the dense retriever and the in-memory store. A build
with `remote` and without `onnx` must embed the corpus through a bound embedder
and run a `dense` node over it; otherwise such a build refuses `dense` with an
unknown-`impl:` error that names the retriever when the missing piece is the
feature.

## Alternatives rejected

- **`impl: remote` and an `endpoint:` parameter on the node.** The shape
  #101's comment proposed for a node family, and the smallest change. The
  address enters `content_hash`, so the laptop run and the cluster run of one
  configuration get two `run_id`s — the decisive objection; porting the
  component to `Local` changes the configuration further, which ADR-3 promised
  it would not, though for the embedder and the reranker this decision's own
  per-nature keys already dent that promise; and the YAML that runs
  locally is no longer the one that runs in the cluster, which ADR-7 and M6's
  exit criterion forbid.

- **A top-level `remotes:` table in the configuration**, mapping names to
  addresses beside the graph. It keeps nodes clean and changes nothing else: the
  table is still part of the document, so it is still hashed, and it widens the
  wire schema (a `SchemaVersion` bump under INV-9) to carry the same
  information the command line carries for free.

- **Excluding the address key from `content_hash`.** ADR-C2's Amendments make
  complete identity the property to keep, and the encoder admits no exclusion by
  design. An exclusion would also put a component-specific key into
  `ragondin-pipeline`, an INV-1 crate that knows no component.

- **An environment-variable indirection in the value** — `endpoint:
  ${VLLM_URL}`. Either the literal is hashed and the constructor reads an input
  that is in no record, or the loader expands it and the hash changes per
  deployment. It is also a sub-grammar inside a value that nothing validates,
  the objection ADR-C22 makes to "a sub-grammar inside a string key that nothing
  validates and nothing canonicalizes".

- **Planning resolves the composing families** — #101's second option, a `dense`
  node naming its embedder and planning building it before the retriever. It
  needs a constructor that can reach the `EngineContext`, which #99 showed the
  code does not have and which `ComponentCtor`'s shape rules out; and a component
  resolved through another component stops being a leaf.

- **Flat prefixed keys** — `embedder_endpoint:`, `store_endpoint:` on the
  retriever node. The address is in the hash again, and the prefix encodes a
  structure into key names, which is ADR-C22's "sub-grammar inside a string
  key".

- **`Remote` out of scope** — #101's third option. Defensible for the embedder
  and the store in M2, and impossible now: ADR-C31's generator exists only
  `Remote`, so M3 needs an answer for at least one family, and an answer for a
  node family that the embedder could not use would be the two-route design this
  ADR exists to avoid.

- **Deferring `Embedder` and `Reranker` identity again.** Cheaper today, and
  wrong the day a reranker is bound under a name `model_hashes` does not match:
  that run records no reranker, and two runs over two reranker models share one
  `run_id`. The break costs the same later and has more implementations behind
  it by then.

## Consequences

- **What each implementation issue owes this ADR.** #266: the `--remote`
  grammar and its refusals, one `register_*` per binding, the bindings on the
  `Run`, the identity step and the order of § 4, the required `embedder:` key,
  the refusal of inert keys and of empty prefixes, corpus embedding through a
  `Remote` embedder, a build with `remote` and without `onnx`, the refusal of an
  absent `served_model` on a node naming a `Remote` embedder or reranker, and the
  executor reading a reranker node's optional `served_model` per call. #261 and
  #13: the `Remote` adapters apply the prefixes, forward `served_model` from
  `EmbedParams` and `RerankParams` and into `GetModelIdentity`, refuse `None`
  before sending, and call it.
  #257 and #12: the rpc on the embedder and reranker services, `served_model` as
  a proto3 `optional string` on the Embed and Rerank requests and on
  `GetModelIdentity`, the rule that a service refuses an absent one, and the
  rule that a service must not prefix or otherwise transform the text by the
  role and may use it for anything that is not text, written as a comment in the
  `.proto` beside the role field. #255: the `served_model` field
  on `EmbedParams` and `RerankParams` and the two identity methods, in the same
  versioned break as ADR-C31's traits. #266 also owns `ragondin-retriever-dense`
  and the corpus-embedding path taking `served_model` and passing it on every
  `EmbedParams`: the binary and that one component crate, two crates, within
  the limit `AGENTS.md` sets.
  The ONNX crates: the refusal of any `Some(name)` and the digests of § 4,
  computed in the constructor, in the issue that adds the method to them. #258
  and the existing suites: the two identity scenarios.

- **ADR-3's port promise is dented for the embedder and the reranker.** A
  `dense` node over the in-process embedder writes `embedder: onnx` with `model`
  and `tokenizer`; over a bound one it writes the bound name with
  `served_model`. The same holds for a reranker node. Porting a winning `Remote`
  embedder or reranker to `Local` therefore changes those keys, where ADR-3
  promised "no change to any user's configuration". The address is still kept
  out of the configuration, which is the part of the promise this decision can
  keep; the rest is the cost of refusing inert keys rather than hashing them.

- **The calibration moves once, and the metrics do not.** `RECORDED_EMBEDDER`
  and `RECORDED_RERANKER` in `bin/ragondin/tests/calibration.rs` become
  `<model>+<tokenizer>` identities, and the committed configurations gain
  `embedder: onnx`, which moves their pipeline hash. Both move every pinned run
  id — those in `RUNS` in
  `eval/ragondin-metrics/tests/scifact_calibration_fixture.rs`, and those the
  calibration record in `bin/ragondin/ARCHITECTURE.md` lists. The numbers do not
  move, since no vector changes; re-deriving the identities and the run ids from
  a rerun is the recalibration, and the record says so.

- **The cost of reading identity from a separate instance.** Step 2 constructs
  an instance to read its identity, and registration constructs another, so a
  `Local` ONNX model is loaded, and its files hashed, more than once per bench.
  For a `Remote` component both instances reach one service, and the window in
  which they could disagree is the one ADR-C31 § 4 already accepts.

- **The run record gains a field (ADR-13).** `Run` in
  `runtime/ragondin-experiments/src/run.rs` is not `#[non_exhaustive]`, so
  adding the bindings breaks any caller that builds a `Run` by struct literal.
  That crate is not an INV-1 boundary; what binds is ADR-13's on-disk format,
  and the change to it is additive: a run stored before it reads back as bound
  to nothing. `compare` renders metrics only today, so seeing which bindings or
  which configuration two runs differed in needs its own small issue on
  `ragondin-experiments`.

- **Prose this decision falsifies, and who corrects it.** This pull request
  changes no file outside `docs/adr/`, so each correction is assigned to #266,
  the diff that makes the new behaviour true: `docs/code-architecture.md` § 10's
  "Named by URL in the configuration"; § 8.1's "OPEN DECISION #101" box and the
  tables it draws as awaiting a reader; and `ComponentFamily`'s documentation in
  `engine/ragondin-engine/src/error.rs`, which calls #101 open. #266's own title
  says "a Remote generator by URL" and is read against this ADR: the URL is a
  `--remote` binding, never a parameter.

- **ADR-C17 is amended, not superseded, in its own pull request.** Its
  Decision stands: the role is per call, the prefix text is the component's
  constructor configuration — the `Remote` adapter being the component — and the
  wire reserves the zero. Three passages of its Consequences are overtaken, and
  a pull request of their own addresses them under process rule 2: the reason
  given for carrying the role on face 2, that without it "a `Remote` embedder is
  deaf to the role" — which still holds for what the role is now for, since a
  service may use it for anything that is not text, and is narrowed only in
  that a service must not prefix or otherwise transform the text by it; the
  statement that "nothing carries a prefix into run identity today", which is
  already false — the `dense` node's prefixes are parameters of the node, read
  by `embedder_of`, and so hashed, since the bench subcommand #31 asked for
  landed (PR #232), and #31 is closed; and the `embedder: { … }` sub-map, which
  "would require #43" — moot, since the key is a flat name. With it, ADR-C22's
  candidate demanders for a `Map` from #101 are gone: nothing here needs one.
  A reader's note on ADR-C31, which corrects a stale factual claim in its text
  and changes none of its decisions: ADR-C31 repeats the prefix gap twice — § 2
  says it "stays where ADR-C17 left it (#31)", and § 4 calls it "a hole it names
  and leaves to #31" — and both sentences sit in its Decision, which process
  rule 2 cannot amend, so they stand as written and are read as closed by #31,
  with nothing further owed.

- **Dead engine surface is removed in an issue of its own.** No plan resolves an
  embedder or a store, and after this decision nothing ever will through the
  engine: `build_embedder` and `build_vector_store`, `register_embedder` and
  `register_vector_store`, and the `Embedder` and `VectorStore` variants of
  `ComponentFamily` go, in a new issue, rather than inside #266.

- **INV-1**: the two trait methods, sanctioned in § 4. **INV-6**: every
  registration goes through the `EngineContext` the composition root holds.
  **INV-7**: a `Remote` component registers through the call a `Local` one
  uses, under an ordinary name. **INV-8**: refusing the empty prefix is what
  makes two equivalent spellings impossible rather than differently hashed.
  **INV-9**: the pipeline's wire schema (`RawPipeline`) changes no shape —
  `embedder` and `served_model` are keys of a node's existing parameter map, in
  ADR-C22's flat grammar, and a binding is not configuration at all. Face 2's
  `.proto` does gain fields and an rpc, and those are #257's.

- **The `VectorStore` face 2 stays defined and unreachable until § 5's trigger
  fires.** #12, still open, says so in its scope; #17 has closed, and the
  `VectorStore` suite it delivered has no `Remote` store to run against yet.
  `docs/system-architecture.md` § 6.3 and § 10 keep naming an external store as
  the direction, and nothing here contradicts them.

- **What is deliberately left open.** The `Remote` vector store (with #24).
  TLS and authentication on a binding (M6). What the reference generator service
  is (#253). No entry in
  `docs/OPEN_QUESTIONS.md` is opened, closed, or changed.

## Status

Accepted.
