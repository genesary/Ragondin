# ARCHITECTURE — ragondin-embedder-onnx

**Status: a component, and therefore a leaf** of the dependency graph. Not an
API boundary: nothing in the workspace depends on it except a binary, which
constructs it and registers it on an `EngineContext`.

## What lives here

One implementation of [`Embedder`](../../core/ragondin-contracts/src/lib.rs):
an ONNX sentence-embedding model and its tokenizer, loaded from two paths and
run **in this process**. No Python sidecar, which is one of the reasons this
platform is written in Rust at all
([`docs/system-architecture.md`](../../docs/system-architecture.md) §10).

What it does to a text, in order: prepend the role's prefix, tokenize with
truncation, pad the batch to its own longest member, run the model, **mean-pool
the per-token output over the attention mask**, **L2-normalize**. That pipeline
is the sentence-transformers convention rather than an invention here, and it is
the whole of the crate.

## Local invariants

- **It is a leaf ([INV-5](../../AGENTS.md)).** It depends on
  `ragondin-contracts`, `ragondin-types`, `async-trait`, `thiserror` and — behind
  the `onnx` feature — `ort`, `tokenizers` and `tokio`. Never on
  `ragondin-engine`, and never on a sibling component:
  `ragondin-retriever-dense` is built *from* an `Embedder` and reaches this one
  as a trait object chosen by the composition root
  ([ADR-C5](../../docs/adr/ADR-C05-engine-depends-only-on-traits-components-are-leaves.md)).
- **`ort` is confined here and feature-gated
  ([ADR-C14](../../docs/adr/ADR-C14-heavy-backends-feature-gated-lean-default-build.md)).**
  `onnx` is **off by default**, so no default workspace build compiles ONNX
  Runtime, and `ort` — which is on INV-4's deny-list — reaches no other crate.
  `tokenizers` sits behind the same feature and not a second one: an ONNX graph
  over token ids cannot be driven without the thing that produces them, so a
  build with one and not the other has nothing to offer.
- **No privilege for being built-in (INV-7).** It implements the same `Embedder`
  trait a third-party component would and passes the same suite
  ([ADR-C6](../../docs/adr/ADR-C06-identical-api-plus-conformance-suite.md)) —
  `tests/conformance.rs` is the call a contributor writes, verbatim, under both
  `RolePrefixes::Distinct` and `RolePrefixes::Undeclared`.
- **The role selects a prefix; it never selects a model.**
  [ADR-C17](../../docs/adr/ADR-C17-embedding-role-per-call-prefixes-in-the-constructor.md)
  puts the role on the call and the prefix text in the constructor, and this
  crate is where that split lands: one session, one tokenizer, two configured
  strings. A symmetric model is configured with no prefix on either side rather
  than special-cased, which is why `OnnxEmbedderConfig::new` defaults both to
  empty. Both prefixes are set by one call, because setting one and forgetting
  the other is the silent regression that ADR names and nothing would report.
- **One embedding space.** The width is the model's hidden size, so it is the
  same under both roles and across every batch by construction — the role
  changes what is prepended, never which model answers.
- **Nothing is fetched at run time, and no model at build time.** The model and
  the tokenizer are paths, read once at construction. `tokenizers`' `http`
  feature is off, which is what makes that structural rather than a habit. The
  one thing a build *does* fetch is ONNX Runtime itself: `ort`'s
  `download-binaries` pulls a prebuilt runtime for the host target and caches
  it — a static archive for this release, linked into the binaries under test,
  so nothing has to be found on a library path afterwards. It is a runtime, not
  a model, and how it arrives is the workspace's decision rather than this
  crate's:
  [ADR-C27](../../docs/adr/ADR-C27-onnx-runtime-obtained-by-download-binaries.md)
  fixes `download-binaries` over `tls-rustls`, declared once in
  `[workspace.dependencies]`, and this crate adds no `ort` feature of its own.
- **The model is behind a `Mutex`, because the contract asks for `&self` and
  ONNX Runtime asks for `&mut`.** `Embedder` is `Send + Sync` and takes `&self`,
  so one embedder answers concurrent calls; an `ort` session is not safe to run
  concurrently and says so in its signature. The lock is the interior
  mutability the contract requires of an implementation holding mutable state,
  and calls queue rather than race. A caller that needs real inference
  parallelism builds a second embedder, which is `ort`'s own advice.
- **A call does not block the thread that made it
  ([ADR-C25](../../docs/adr/ADR-C25-a-component-does-not-block-the-caller.md),
  review-enforced).** Tokenization, tensor building, the forward pass and the
  session lock held across them run inside `tokio::task::spawn_blocking`, so
  nothing CPU-bound happens on the caller's thread and the future `embed`
  returns yields like any other. ADR-C25 states the obligation and leaves the
  *means* to the component; the means is argued below, and it asks something of
  the caller.
- **Batching is internal and cannot be read off the answer.** `batch_size` is a
  performance knob, and the tests pin the consequence: the same texts embed
  identically at batch sizes 1, 2, 5 and 512, and one text embeds identically
  alone and beside a much longer one. The second is the mask check — a batch is
  padded to its longest member, and pooling padding *in* would make a vector
  depend on what happened to be embedded next to it.
- **A text that tokenizes to no tokens embeds to the zero vector.** The mean of
  nothing is `0/0`; unguarded, that is `NaN`, which the contract forbids and
  which `ragondin-types` says cannot be read back once serialized. Normalization
  is guarded the same way, since a zero vector has no direction to preserve.
- **Every failure a call can produce is a `ComponentError::Backend`.** Nothing
  about a call is a precondition this component can find unmet — a batch of any
  size, including none, is a valid request — so `InvalidRequest` has no occasion
  to arise here. `EmbedderError` is boxed inside that variant, and its
  in-process fidelity is what lets a `Local` caller walk `source` back to the
  model or the tokenizer.
- **A `max_sequence_length` that leaves no room for text is refused at
  construction.** A post-processor wraps a sequence in special tokens, and
  `tokenizers` subtracts their count from the truncation limit unchecked. At
  the count the effective limit is zero and every vector would be the special
  tokens' own; below it the subtraction underflows — a debug build panics, a
  release build wraps to `usize::MAX` and truncation is off, which is the
  opposite of what lowering the knob asked for and pads every batch to an
  unbounded width. The floor is the single-sequence count, because this
  component never encodes a pair.
- **A pooled vector whose components are not finite is refused, not returned.**
  Mean pooling propagates a `NaN` out of a hidden state, and normalization does
  not remove it: the norm of such a vector is itself `NaN`, so the zero-norm
  guard below is taken as though the vector were the zero one and the result
  comes back looking well-formed. `Embedder` requires finite components and
  `ragondin-types` cannot read such a vector back once serialized, so the model
  is accused where it can still be named.
- **Determinism is claimed per machine, and the suite proves less than that.**
  The same text embeds to the same vector bit for bit — that is what
  `run_id`'s `model_hashes` (`docs/system-architecture.md` §7.1) needs of an
  embedder, and it is tested repeatedly and across a fresh model load. Two
  things make it so here, and one limit is worth stating rather than
  discovering. **Batch composition is fixed by the input, never by arrival
  time**: texts are batched in the order they arrive, in consecutive runs of
  `batch_size`, so the same list produces the same batches whatever else the
  process is doing. **`intra_threads` defaults to one**: ONNX Runtime
  partitions an operator's work across the intra-op threads fixed at session
  creation, so a session built with a different count can reduce in a different
  order and move a vector's last bits. Pinning it to one removes that variable
  at a real cost in throughput, which is why it is a knob a corpus-embedding
  caller can raise rather than a constant. The limit: the fixture graph is a
  `Gather`, so it performs **no floating-point reduction** at all — every float
  operation the suite exercises is this crate's own arithmetic, and ONNX
  Runtime's determinism is assumed, not tested. What is *not* left implicit is
  the claim: reproducible on one machine, not asserted across two.
- **The inter-op pool is not a second variable, but only because of a default
  this crate does not set.** ONNX Runtime runs a graph's nodes sequentially
  unless parallel execution is switched on — `ort`'s
  `SessionBuilder::with_parallel_execution` is documented as disabled by
  default — and `with_inter_threads` has no effect while the execution mode is
  sequential. This crate calls neither method, so the property is inherited
  rather than asserted: a change that enabled parallel execution would reopen
  the question `intra_threads` closes, and this paragraph with it.
- **What can be checked at construction is.** That both files load, that the
  model declares no input this component cannot supply, and that it declares
  `input_ids` at all. ONNX Runtime reports a missing input per call, which would
  make a wiring mistake look like an intermittent backend failure; the model's
  input names are readable once, at load, so the comparison happens once.

## The off-thread hop: `spawn_blocking`, and what that asks of the caller

[ADR-C25](../../docs/adr/ADR-C25-a-component-does-not-block-the-caller.md)
states the obligation — a `Local` component does not block the thread that
called it — and leaves the means to the component. This one uses
`tokio::task::spawn_blocking`, which is the smallest thing that works given that
`tokio` is already the runtime both drivers select
([`docs/code-architecture.md`](../../docs/code-architecture.md) §11.2) and the
one the conformance suite runs on. `tokio` is therefore a dependency of this
crate, behind the same `onnx` feature as the rest of the backend; it reaches
`ragondin-contracts` nowhere, which is what INV-4 asks.

**The consequence is a requirement on the caller, and it is stated rather than
assumed: `spawn_blocking` needs an ambient `tokio` runtime and panics without
one.** A component that had to be callable under any runtime would take
ADR-C25's other route — its own thread and a channel, which depends on no
runtime at all. This one does not, and a caller outside `tokio` is out of its
range.

The prefixed texts travel into the blocking task and the vectors come back out
of it. Prefixing happens before the hop, on the caller's thread: it is a string
copy, not the CPU-bound work the rule is about, and doing it there is what lets
the task own its input instead of borrowing across an `await`.

**An empty batch never hops.** It returns no vectors from the caller's thread,
without locking the session — there is nothing to run and nothing to stall.

**Moving the work off-thread is not the same as making it concurrent.** The
session stays behind a `Mutex`, so calls queue; ADR-C25 is explicit that this is
a separate question and leaves it open. A pool of sessions, or the continuous
batching §11.2 places inside the component, is a change to this paragraph and to
nothing above it.

**Nothing in the test suite proves this.** ADR-C25 rejects mechanizing the rule
because the property is not observable from outside the call, and says so in its
own alternatives. `concurrent_calls_answer_as_sequential_ones_do` proves
serialization, not the hop. The sign is in the diff.

## Which inputs the model gets

Three names, fed **by name and only when the model declares them**:
`input_ids`, `attention_mask`, and `token_type_ids` as a single segment of
zeros. That covers the BERT-family encoders sentence-embedding models are
exported from. A model declaring anything else is refused rather than fed a
guess — a position id it does not derive itself, a cached key, a temperature —
because the guess would run and return numbers.

The output taken is the **first** one — by position, since
`last_hidden_state` is a convention rather than a rule — and it must be `[batch,
sequence, hidden]` float32.

**Every axis pooling indexes by is checked, and none is taken on trust**, because
the graph's declaration cannot settle any of them: batch and sequence are dynamic
axes, so they are knowable only from the tensor in hand, and a mismatch is
therefore a call-time failure rather than something the load could have caught.

- **Rank.** A model that pools for itself returns `[batch, hidden]` and is
  refused. Supporting it would mean trusting *its* pooling, which is a different
  component's behaviour under one component's name.
- **The batch axis**, against the number of texts fed.
- **The sequence axis**, against the width the padded batch was built to.
  Pooling reads position `n` of the output against position `n` of the mask, so
  a graph that strips its own `[CLS]` — one token shorter out than in — must be
  refused rather than pooled: indexing the output by the width that was fed
  reads past the end of a short answer, and into the next row of a long one.
- **The element type.** A quantized export shipping float16 hidden states is
  refused *as a dtype*, in its own variant. It loaded and it ran, so reporting
  it as a failure to run would accuse the wrong thing.

## The fixtures

`tests/fixtures/` holds eight tiny ONNX graphs and two tokenizers, together
under 16 kB, and `tests/fixtures/generate.py` regenerates all of them from a
fixed seed. No model is fetched at build time — the rule this crate was created
under, in #20 — so they are committed; the generator is committed with them
because eight opaque binaries are not a fixture, they are a liability.

The graphs are deliberately trivial — an embedding table, a `Gather`, and at
most one node after it. What the tests exercise is the component *around* the
model: prefixes, truncation, padding, batching, pooling, normalization, and the
six ways a model is refused — an input this component cannot fill, no
`input_ids`, a rank it cannot pool, a sequence axis disagreeing with the batch
it was fed, hidden states that are not float32, and hidden states that pool to
something not finite. A real sentence transformer would test ONNX Runtime
instead, slowly, and a failure would accuse the wrong code.

The two tokenizers differ by one thing. `tokenizer.json` has **no
post-processor**, which is what makes the empty-sequence case reachable at all;
`tokenizer-bert.json` has a BERT one, which is what gives the truncation floor
a non-zero count to refuse against. The second is used at construction only —
its two special ids sit past the end of the 38-row embedding table every graph
here is built from, so it is never fed to one.

## What is deliberately not here

**No model fetching, and no model registry.** Which model to load is
configuration. A path is what this crate accepts; resolving a name to a path,
caching a download, or hashing a model into run identity
(`docs/system-architecture.md` §7.1) belongs to the runtime and the composition
root, not to a component. Prefixes are configuration that changes the numbers
and therefore owe an entry in `model_hashes`; ADR-C17 names that hole and #31
closes it.

**No corpus embedding loop, and no continuous batching.** `embed` batches the
texts it is handed and returns. Driving a corpus through it — and deciding
whether indexing belongs in the pipeline formalism at all, which is question 5
of [`docs/OPEN_QUESTIONS.md`](../../docs/OPEN_QUESTIONS.md) — is somebody else's
work, and nothing here answers it.

**No pooling or normalization knob.** Mean-over-mask then L2 is what the
sentence-transformers ecosystem's models are trained and evaluated under. A CLS
or max-pooling option is a second behaviour under one name, and YAGNI until a
model in an actual bench run needs it.

**No execution provider selection.** CPU, because that is what CI has and what
the M2 bench needs. A GPU provider is a constructor knob to add when there is a
run that wants one; adding it now would be configuration nothing sets.
