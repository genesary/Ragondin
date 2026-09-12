# ARCHITECTURE — ragondin-reranker-onnx

**Status: a component — a leaf of the dependency graph, and internal.** Nothing
here is an API boundary (INV-1 protects `core/`, not this crate). The stable
surface a caller depends on is the `Reranker` trait in `ragondin-contracts`;
this crate is one implementation of it and may be refactored freely.

## What lives here

An in-process **cross-encoder reranker** over ONNX Runtime: `OnnxReranker`,
constructed from an `OnnxRerankerConfig` naming a model and a tokenizer,
implementing `Reranker`.

A cross-encoder reads a `(query, chunk)` pair jointly — one forward pass per
pair — rather than comparing two vectors computed apart. That is why it
reorders well and why it cannot retrieve: it scores what it is handed. Running
one **without a Python sidecar** is one of the reasons the data plane is written
in Rust (`docs/system-architecture.md` §10), and it is the "with reranking" half
of the M2 exit criterion.

**Not here, deliberately:** retrieval, fusion, embedding or storage; fetching a
model; and registration on an `EngineContext` — the registry lives on the
engine, and the binary is the composition root that puts the two together
(`docs/code-architecture.md` §8.1).

## Local invariants

- **A component is a leaf (INV-5, CI-enforced).** The dependencies are
  `ragondin-contracts`, `ragondin-types`, `async-trait`, `thiserror` and, behind
  the `onnx` feature, `ort`, `tokenizers` and `tokio` — never `ragondin-engine`,
  never a sibling component. The engine knows only traits, and the check walks
  the dependency graph to prove it.
- **The heavy backend is confined and feature-gated (ADR-C14).** `ort` and
  `tokenizers` are optional dependencies behind `onnx`, and **`onnx` is not a
  default feature**: `cargo build --workspace` compiles neither. With the
  feature off the crate exports nothing — a cross-encoder reranker without an
  inference runtime would be a different component, not a degraded one — so the
  tests are behind the same `cfg`, and `just test-features` is what runs them:
  whole-workspace and naming no crate, so this component inherits it rather than
  adding a recipe. `just check-features` proves the gated code still compiles.
- **No privilege for built-ins (INV-7).** This crate is reachable only through
  the `Reranker` trait and its own public constructor. It has no entry point
  into the engine that a third-party crate could not also call.
- **A call does not block the thread that made it (ADR-C25, review-enforced).**
  Tokenization, the forward pass, and the session lock held across both run
  inside `tokio::task::spawn_blocking`. Nothing CPU-bound happens on the
  caller's thread, and the future `rerank` returns yields like any other. This
  is argued below, because the *means* was this crate's to choose.
- **The ranking contract is honoured, and the ties are too.** `Reranker`
  requires descending, finite scores. Scores are sorted on `(score descending,
  chunk id ascending)`, so one candidate set ranks one way whatever order it
  arrived in — without the tie-break, two chunks the model cannot separate would
  rank by whatever order retrieval happened to hand over, and the only symptom
  would be a benchmark number that moves.
- **The output is a subset of the input, never a superset.** A reranker has no
  corpus of its own: the returned chunks *are* the ones handed in, carrying a
  new score, reordered and truncated. `ragondin-conformance` checks this
  (`no fabricated ids`, `no duplicate ids`, `top_k respected`).
- **A `top_k` of zero is rejected before anything else**, including before the
  empty-input shortcut. ADR-C19 makes an empty chunk list a valid call that does
  nothing; it explicitly does not weaken the `top_k` rule, so the order of the
  two checks is the order the contract states them in, not an accident.
- **Typed errors (ADR-C13).** `ModelError` for loading and for a failed forward
  pass, `ComponentError` at the trait boundary — the latter wrapping the former
  in `Backend`, so a `Local` caller can walk `source()` to the cause. A library
  never imposes `anyhow` on its consumers.

## The choices that were this crate's to make

Each of these stays inside this crate, so `AGENTS.md` § Rules of engagement
makes it the implementer's — and makes recording it obligatory.

### The tokenizer: `tokenizers`, loaded from the model's own `tokenizer.json`

A cross-encoder's score depends on being fed exactly the token ids it was
trained on, and an off-the-shelf one ships a `tokenizer.json` that says how.
The `tokenizers` crate reads that file directly, so the component inherits the
model's vocabulary, its normalizer, its pre-tokenizer and its pair template
rather than reimplementing them — a hand-rolled WordPiece would be a second,
silently divergent definition of the model's own input.

Its default features are off: `progressbar` and `esaxx_fast` are not wanted, and
`onig` is a C library where `fancy-regex` is pure Rust. The crate refuses to
compile with neither, so `fancy-regex` is enabled explicitly. `hf-hub` — the
feature that would fetch a tokenizer over the network — is not a default and
stays off.

**Truncation is imposed by this component, not inherited from the file.** The
tensors of a batch are padded to the longest pair in it, so one long pair sets
the width of every pair beside it; `max_sequence_length` therefore has to be a
knob here, and it overrides whatever the `tokenizer.json` declares. The strategy
is `LongestFirst`, which is what a cross-encoder's own preprocessing does: it
trims the passage before it trims the query, and `Right` takes the trim off the
end.

**The floor under that knob is checked here, because `tokenizers` does not check
it.** A pair's budget pays for `[CLS]` and two `[SEP]`s before any text, and
`tokenizers 0.23.2` subtracts that count from `max_length` with unchecked `usize`
arithmetic — in `with_truncation` for a single sequence, and again in `encode`
for a pair. Under the count a debug build panics and a release build wraps to
`usize::MAX`, which switches truncation **off** and pads every batch to an
unbounded width: the opposite of what lowering the knob asks for, arrived at
silently, and differently in the two profiles. `OnnxReranker::new` therefore
rejects a `max_sequence_length` at or below what the tokenizer's own
post-processor adds to a pair, with
`ModelError::MaxSequenceLengthTooSmall`. The count is read from that
post-processor rather than assumed to be three, so a tokenizer that wraps a pair
differently is measured rather than guessed at.

### How the knobs are exposed: a config struct with public fields

`OnnxRerankerConfig::new(model, tokenizer)` fills in defaults for the three
knobs; a caller overrides one by assignment. The alternative — a builder method
per knob — buys nothing for a plain-data struct, and a struct is the shape a
`ComponentCtor` will bridge an untyped parameter map onto at physical planning
(`docs/code-architecture.md` §6.3) when #31 wires this component up.

Each knob is a `NonZeroUsize`. A batch of zero pairs and a sequence of zero
tokens are not configurations a caller could mean, so they are made
unrepresentable rather than rejected at construction with an error variant
nobody should ever see. That is the opposite of `RerankParams::top_k`, and
deliberately: `top_k` sits on a stable API boundary where `ragondin-contracts`
has already decided that zero is representable and rejected per call.

### Determinism: fixed batch composition, and one intra-op thread

`docs/system-architecture.md` §9.7 is honest about dynamic batching: floating
point reduction order varies with batch composition, so a model served that way
is not reproducible even at a fixed seed. The M2 exit criterion compares
retrieval with and without reranking, which means the reranker's contribution
has to be stable across runs. Two things make it so here:

- **Batch composition is fixed by the input, never by arrival time.** Pairs are
  batched in the order the chunks arrive, in consecutive runs of `batch_size`.
  The same candidate list therefore produces the same batches, whatever else the
  process is doing. `batch_size` changes how the work is divided; it does not
  make composition depend on timing.
- **`intra_threads` defaults to one.** ONNX Runtime partitions an operator's
  work across the intra-op threads fixed at session creation, so a session built
  with a different thread count can reduce in a different order. Pinning it to
  one removes that variable, at a real cost in throughput. Raising it is a
  deliberate trade a caller can make, which is why it is a knob and not a
  constant.
- **The inter-op pool is not a second variable, but only because of a default
  this crate does not set.** ONNX Runtime runs a graph's nodes sequentially
  unless parallel execution is switched on — `ort`'s
  `SessionBuilder::with_parallel_execution` is documented as disabled by default
  — and `with_inter_threads` "has no effect when the session execution mode is
  set to `Sequential`". So there is no inter-op scheduling to vary. This crate
  calls neither method, which means the property is inherited rather than
  asserted: a future change that enables parallel execution would reopen the
  question `intra_threads` closes, and this paragraph with it.

**What is claimed, exactly.** The same candidate list, reranked by a session
built from the same configuration, produces the same scores in the same order.
That is what the M2 criterion needs. It is *not* a claim that a chunk's score is
independent of its batch-mates: they share a padded tensor, and only the model
decides what that does. A test pins the property for this crate's fixture
cross-encoder — batching five candidates two at a time agrees with scoring them
all at once — and a model that failed it would still be conformant, merely less
reproducible than this component's configuration can make it.

**Padding is zero in all three tensors, and its value is immaterial.**
`attention_mask` is zero at padded positions, so a model that honours its own
mask never reads what `input_ids` and `token_type_ids` hold there.

### The off-thread hop: `spawn_blocking`, and what that asks of the caller

ADR-C25 states the obligation and leaves the means to the component. This one
uses `tokio::task::spawn_blocking`, which is the smallest thing that works given
that `tokio` is already the runtime both drivers select
(`docs/code-architecture.md` §11.2) and the one the conformance suite runs on.

**The consequence is a requirement on the caller, and it is stated rather than
assumed: `spawn_blocking` needs an ambient `tokio` runtime and panics without
one.** A component that had to be callable under any runtime would take
ADR-C25's other route — its own thread and a channel, which depends on no
runtime at all. This one does not, and a caller outside `tokio` is out of its
range.

The chunks travel into the blocking task and come back out of it, rather than
being cloned so that their text can be scored.

### One session, serialized

The session sits behind a `Mutex`, so calls queue rather than run concurrently:
ONNX Runtime's `Run` takes the session mutably through `ort`, and one session
per component is the simple thing. ADR-C25 is explicit that this is a separate
question from the thread hop — moving serialized work off the caller's thread
stops it stalling the executor without making it concurrent — and it stays open
here. A pool of sessions, or the continuous batching `docs/code-architecture.md`
§11.2 places inside the component, is a change to this paragraph and to nothing
above it.

A panic inside a call poisons that mutex. The component then reports
`ModelError::PoisonedSession` rather than carrying on over a session whose state
after a panic inside ONNX Runtime is not this crate's to vouch for.

### What the component requires of a model

- **Inputs.** It builds `input_ids`, `attention_mask` and `token_type_ids`, and
  supplies exactly the subset the model declares. `input_ids` and
  `attention_mask` are required; `token_type_ids` is genuinely optional, because
  a RoBERTa-based cross-encoder has no segment embedding and declares no such
  input. A model declaring anything else is rejected **at construction** —
  better than being fed a tensor of zeros for an input whose meaning this
  component does not know.
- **Output.** The first output, and it must carry exactly one score per pair.
  A two-way classification head, or a token-level output, is a different model,
  and guessing which column means *relevant* would be guessing at the caller's
  expense. A non-finite score is an error too: the ranking contract requires
  finite scores, and a `NaN` reaching the comparison that sorts them would make
  it meaningless rather than merely wrong.

### `ort` is pinned exactly

`ort = "=2.0.0-rc.13"`. It is a pre-release, and pre-release APIs are not bound
by the compatibility a caret requirement assumes — `Session::run` and the value
types this crate uses have moved between release candidates. An exact pin makes
a runtime upgrade a visible, deliberate edit to the workspace manifest.

### What is fetched, and when

**No model is ever fetched.** `ort`'s `fetch-models` feature is off and
`tokenizers`' `http` feature is off, so nothing in this crate can reach the
network for a model or a tokenizer: both arrive as paths the caller supplies.

**The ONNX Runtime *binary* is fetched at build time**, by `ort`'s
`download-binaries` feature — a prebuilt runtime for the host, not a model, and
the price of not requiring every contributor to install ONNX Runtime themselves.
That trade is the workspace's rather than this crate's:
[ADR-C27](../../docs/adr/ADR-C27-onnx-runtime-obtained-by-download-binaries.md)
decides `download-binaries` over `tls-rustls`, declared once in
`[workspace.dependencies]` and inherited unchanged by every `ort`-backed
component, and this crate adds no `ort` feature of its own. The alternative,
`load-dynamic`, is rejected there: it moves the cost to every developer and
every CI image and turns a build-time failure into a run-time one — the only
run-time failure there is, since what `download-binaries` fetches is a static
archive linked into the executable.

**It is once per machine per target, not once per build.** `ort-sys` extracts
into the *user cache directory* — `cache_dir()/dfbin/<target>/<hash>`, in its
`build/main.rs` — and skips the download when that directory exists. The cache
is outside `target/`, so it survives `cargo clean`.

**And there is an offline route today.** `ort-sys`' build script reads
`ORT_LIB_PATH`/`ORT_LIB_LOCATION` to link a runtime already on the machine, and
skips the download entirely on `ORT_SKIP_DOWNLOAD`, on `ORT_OFFLINE`, or on
Cargo's own `CARGO_NET_OFFLINE` (`build/vars.rs`). An air-gapped build supplies
the library and sets one of those; it does not need this crate to change.

**What `check-deny` does not cover, stated because the rest of this file might
imply otherwise.** `deny.toml` audits the *crate graph*, and the entry there
traces `webpki-roots` to this build script. The script then downloads a
**binary payload that is not in the crate graph at all**: a prebuilt ONNX
Runtime from `cdn.pyke.io`, the `ort` maintainer's CDN. It is pinned — `ort-sys`
ships a `build/download/dist.tsv` of `target`, `url` and `sha256_hash`, and
verifies the hash of what it extracted against that table — so it is not
unverified, but it carries its own licence and its own advisory surface, and
`cargo deny` sees neither. Trusting `download-binaries` is trusting that
publisher, and that is a separate act from the licence and advisory entries in
`deny.toml`.

## Conformance

`tests/conformance.rs` runs `ragondin_conformance::assert_reranker_conformance`
against the same public constructor a third-party caller uses — which is what
makes built-in/third-party equivalence real rather than asserted (ADR-C6). The
rest of that file is what conformance deliberately does not check: the suite
knows nothing of the model behind a reranker, so *which* chunk comes first is
checked here.

**The model it checks against is built in `tests/fixture/`, not downloaded and
not committed.** It has a BERT-style cross-encoder's signature — `input_ids`,
`attention_mask` and `token_type_ids` in, one logit per pair out — and it scores
a pair by **lexical overlap**: how many query positions and passage positions
hold the same token id, with the two segments told apart by `token_type_ids` and
padding excluded by `attention_mask`. That is not what a trained cross-encoder
computes and is not meant to be; it is a relevance signal a test can predict
exactly, which is what makes "the chunk that answers the query ranks first" an
assertion rather than a hope.

It is emitted as ONNX protobuf directly by `tests/fixture/onnx.rs`, and its
tokenizer is written as the `tokenizer.json` `tokenizers` itself serializes.
Both land in `CARGO_TARGET_TMPDIR`. A fixture generated from source is
reproducible from a checkout, needs no network and no Python, and — unlike a
committed binary — can be read by whoever has to debug a test it breaks.

The same file builds one variant: the identical graph with a **two-column
head**, which is not a cross-encoder. It is there to be refused. That failure is
the only one in this crate that would otherwise be silent — with two scores per
pair, pairing scores to chunks positionally gives every chunk a plausible number
belonging to something else — so it is the one worth a fixture of its own.

**Conformance says nothing about the thread hop.** ADR-C25 is explicit that no
conformance test can check it: detecting a blocked executor means watching the
runtime, which is timing-dependent and indistinguishable from a component that
is simply fast. A green suite is not evidence about that rule; a reviewer is.
