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
  the `onnx` feature — `ort` and `tokenizers`. Never on `ragondin-engine`, and
  never on a sibling component: `ragondin-retriever-dense` is built *from* an
  `Embedder` and reaches this one as a trait object chosen by the composition
  root
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
  `download-binaries` pulls a prebuilt shared library for the host target and
  caches it, which is a runtime, not a model.
- **The model is behind a `Mutex`, because the contract asks for `&self` and
  ONNX Runtime asks for `&mut`.** `Embedder` is `Send + Sync` and takes `&self`,
  so one embedder answers concurrent calls; an `ort` session is not safe to run
  concurrently and says so in its signature. The lock is the interior
  mutability the contract requires of an implementation holding mutable state,
  and calls queue rather than race. A caller that needs real inference
  parallelism builds a second embedder, which is `ort`'s own advice.
- **Inference runs synchronously inside the `async fn`.** No `spawn_blocking`:
  that would pick a runtime for a library, and the runtime is selected at the
  binary (`docs/code-architecture.md` §11.2). A caller embedding a whole corpus
  knows which runtime it is on and can wrap the call; this crate does not.
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
- **What can be checked at construction is.** That both files load, that the
  model declares no input this component cannot supply, and that it declares
  `input_ids` at all. ONNX Runtime reports a missing input per call, which would
  make a wiring mistake look like an intermittent backend failure; the model's
  input names are readable once, at load, so the comparison happens once.

## Which inputs the model gets

Three names, fed **by name and only when the model declares them**:
`input_ids`, `attention_mask`, and `token_type_ids` as a single segment of
zeros. That covers the BERT-family encoders sentence-embedding models are
exported from. A model declaring anything else is refused rather than fed a
guess — a position id it does not derive itself, a cached key, a temperature —
because the guess would run and return numbers.

The output taken is the **first** one, and it must be `[batch, sequence,
hidden]` float32. A model that pools for itself returns `[batch, hidden]` and is
refused: it loads, and the rank is only knowable from a real tensor, so that
refusal is a call-time failure. Supporting it would mean trusting *its* pooling,
which is a different component's behaviour under one component's name.

## The fixtures

`tests/fixtures/` holds five tiny ONNX graphs and one `tokenizer.json`, together
under 10 kB, and `tests/fixtures/generate.py` regenerates all of them from a
fixed seed. #20 forbids fetching a model at build time, so they are committed;
the generator is committed with them because five opaque binaries are not a
fixture, they are a liability.

The graphs are deliberately trivial — an embedding table and a `Gather`. What
the tests exercise is the component *around* the model: prefixes, truncation,
padding, batching, pooling, normalization, and the four ways a model can be one
this component cannot drive. A real sentence transformer would test ONNX Runtime
instead, slowly, and a failure would accuse the wrong code.

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
