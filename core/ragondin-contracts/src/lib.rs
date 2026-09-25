//! # ragondin-contracts
//!
//! The **component contract**: the traits an external contributor implements.
//! This crate is **face 1** of the two-faced contract (ADR-3) — the Rust
//! trait, implemented in-process by a `Local` component. Face 2 is the
//! protobuf mirror in `ragondin-proto`, spoken by a `Remote` component over gRPC.
//! The engine calls the trait either way and cannot tell them apart, which is
//! why every trait here must be **`dyn`-compatible** and `Send + Sync`.
//!
//! This crate is a **stable API boundary** (INV-1) and **the crate a
//! contributor compiles against**. It must stay light (INV-4): implementing a
//! component requires only this crate and `ragondin-types`, never the engine.
//!
//! **No privilege for built-ins (INV-7).** These traits are the only API. A
//! first-party component and a third-party one implement exactly the same
//! thing, and there is no faster path for either.
//!
//! This crate defines seven families: `Retriever`, `Fusion`, `Reranker`,
//! `Embedder`, `VectorStore`, `ContextBuilder` and `Generator`. `Chunker`,
//! `Indexer` and `Grader` are not defined here. Adding a trait is additive
//! on this boundary, so each of those arrives with the work that first
//! consumes it; defining one before anything needs it would be dead API.
//!
//! # Where parameters come from
//!
//! A pipeline node carries an untyped parameter map (`ragondin-pipeline`), and each
//! trait here takes a **typed** params struct. The two are bridged at physical
//! planning (`docs/code-architecture.md` §6.3), which resolves an `impl:` name
//! into a *constructed* component and applies defaults: implementation-specific
//! configuration — BM25's `k1` and `b`, a model path — is handed to the
//! constructor, while the params structs below carry what varies **per
//! call**. The reference pipeline in `docs/system-architecture.md` §5.1 shows
//! the split: `top_k` on a retriever and on a reranker, nothing on the fusion.
//!
//! One qualification: a params struct also carries a setting fixed per node
//! when the setting must reach a `Remote` component. Face 2 carries the
//! trait's calls and nothing else, so constructor configuration never crosses
//! the wire, and a setting a researcher varies between two runs must be in the
//! pipeline representation and its hash (ADR-C31 § 2). [`GenerateParams`] is
//! that case: its served model, template, temperature, seed and token cap are
//! fixed per generator node and travel on every call. So is the
//! `served_model` of [`EmbedParams`] and [`RerankParams`], fixed per node and
//! carried on every call so that it reaches a `Remote` embedder or reranker
//! (ADR-C32 § 4).
//!
//! # Empty collections
//!
//! Five methods below take a collection — [`Fusion::fuse`], [`Reranker::rerank`],
//! [`Embedder::embed`], [`VectorStore::upsert`] and [`ContextBuilder::build`].
//! **An empty collection is a valid call on every one of them, and the
//! component does nothing with it** (ADR-C19): empty in, empty out; for
//! `upsert`, whose return carries no data, empty in, nothing done, `Ok(())`;
//! and for `build`, the empty context — no chunks, with its `text` left to the
//! builder, since a template may render a header over no passages. A
//! [`Generator`] handed an empty context is likewise a valid call, and an
//! answer of "I do not know" is conformant. An implementation must not return
//! [`ComponentError::InvalidRequest`] for it, so a caller batching over a corpus
//! never has to guard a batch that came out empty.
//!
//! This is **not** the `top_k` rule and does not weaken it: a `top_k` of zero
//! stays an invalid request wherever it is taken, and so does a zero
//! [`ContextParams::budget`], its twin (ADR-C31 § 2). A zero `top_k` asks for a
//! *result* that cannot exist, and answering it with an empty list makes a
//! caller's arithmetic bug look like an empty corpus; an empty collection asks
//! for a state change, or a transformation, that is trivially satisfiable and
//! returns nothing that could be mistaken for anything else.
//!
//! # A component does not block the thread that called it
//!
//! Every method here is an `async fn`, and the engine cannot tell a `Local`
//! implementation from a `Remote` one. **Work that blocks — a model forward
//! pass, a synchronous disk read, a lock held across either — is moved off the
//! caller's thread by the component itself** (ADR-C25), so that the future
//! this contract hands back yields like any other.
//!
//! The obligation is stated here; the means is the implementation's own.
//! `tokio::task::spawn_blocking`, a dedicated thread with a channel, and a
//! backend that never blocks are all conformant, and no runtime is named by
//! this crate or reachable through it (INV-4). `spawn_blocking` needs an
//! ambient `tokio` runtime and panics without one, so a component that picks
//! it requires that of its caller and says so in its own `ARCHITECTURE.md`;
//! a component that must run under any runtime picks its own thread instead.
//!
//! This governs the calls, not construction: a component is built by a
//! synchronous constructor at physical planning, and what that constructor
//! does — loading a model, building an index — is outside this rule
//! (ADR-C25). Nothing in the conformance suite checks either: detecting a
//! blocked executor means watching the runtime, which is timing-dependent and
//! cannot be told from a component that is simply fast. It is a review item,
//! and a green suite says nothing about it.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

use async_trait::async_trait;
use ragondin_types::{Answer, Chunk, Context, Embedding, ModelIdentity, Query, ScoredChunk};
use thiserror::Error;

/// The error every component boundary returns.
///
/// One type for both faces (ADR-3): the engine cannot tell a `Local` call
/// from a `Remote` one, so a failure must arrive in the same shape whichever
/// produced it. `#[non_exhaustive]` so that adding a variant is not a breaking
/// change to this stable API boundary.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ComponentError {
    /// The component could not be reached.
    ///
    /// The variant the two-faced contract forces: a `Remote` component fails in
    /// ways a `Local` one cannot — a refused connection, a timeout — and the
    /// engine has to be able to see that without knowing which face it called.
    #[error("component unavailable: {0}")]
    Unavailable(String),

    /// The call cannot be honoured as made.
    ///
    /// A precondition of the call is unmet — an embedding whose dimensionality
    /// does not match the index, a `top_k` of zero. Distinct from
    /// [`ComponentError::Backend`] because the caller, not the component, is
    /// what needs to change.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The underlying implementation failed.
    ///
    /// Wraps the error of whatever the component is built on — a search index,
    /// an inference runtime, a store client — so a `Local` caller can walk
    /// [`std::error::Error::source`] to the cause instead of parsing a string.
    ///
    /// **This fidelity does not cross the wire.** A `Remote` component's error
    /// arrives as a gRPC status, so `ragondin-remote` can only reconstruct a message,
    /// not the original error type. Do not build logic on the concrete type
    /// behind this box: it is present in-process and absent over the network,
    /// and the engine cannot tell which face it called (ADR-3).
    #[error("backend failure: {0}")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Per-call parameters of a [`Retriever`].
///
/// `#[non_exhaustive]` with a constructor rather than public construction: this
/// struct will gain knobs, and on the crate every contributor compiles against,
/// adding one should not break their code. (`ragondin-pipeline` makes the opposite
/// choice for its enums, deliberately — there, an exhaustive `match` that stops
/// compiling is the intended signal that a new node kind needs handling. Here,
/// breaking a caller who wrote a struct literal signals nothing to anyone.)
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RetrieveParams {
    /// How many chunks to return.
    pub top_k: usize,
}

impl RetrieveParams {
    /// Retrieves `top_k` chunks.
    ///
    /// Infallible: a `top_k` of zero is representable here and rejected by the
    /// component, as [`ComponentError::InvalidRequest`] describes. Same stance
    /// as `ragondin-types` takes on an empty `Embedding`.
    pub fn new(top_k: usize) -> Self {
        Self { top_k }
    }
}

/// Per-call parameters of a [`Fusion`].
///
/// Empty today — the `fuse` node of the reference pipeline in
/// `docs/system-architecture.md` §5.1 carries no params, and a fusion's own
/// constants (RRF's `k`) are constructor configuration. It exists so that a
/// future knob is a field rather than a change to the trait's signature, which
/// would break every implementation in and out of the repository — including
/// every third-party `Remote` service, which is the contribution funnel ADR-3
/// exists to protect.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FusionParams {}

impl FusionParams {
    /// The default fusion parameters.
    pub fn new() -> Self {
        Self {}
    }
}

/// Per-call parameters of a [`Reranker`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RerankParams {
    /// How many chunks to keep after reordering.
    pub top_k: usize,
    /// The name of the model the reranker is asked for (ADR-C32 § 4).
    ///
    /// `Some(name)` asks for the model the component serves under `name`;
    /// `None` asks for the model it loaded. A component refuses, as
    /// [`ComponentError::InvalidRequest`], a name it does not serve, an empty
    /// name, and `None` when it has no single loaded model that `None` could
    /// mean. Absence is the only spelling of "no name": there is no empty
    /// string standing in for it.
    ///
    /// The ONNX embedder and reranker (`ragondin-embedder-onnx`,
    /// `ragondin-reranker-onnx`) are configured with no served-model name, so
    /// they refuse every `Some(name)`, on a call and in `model_identity`.
    pub served_model: Option<String>,
}

impl RerankParams {
    /// Keeps `top_k` chunks, from the model the component loaded
    /// (`served_model` is `None`).
    ///
    /// Infallible: a `top_k` of zero is representable here and rejected by the
    /// component, as [`ComponentError::InvalidRequest`] describes.
    pub fn new(top_k: usize) -> Self {
        Self {
            top_k,
            served_model: None,
        }
    }

    /// Asks for the model the component serves under `served_model`.
    pub fn with_served_model(mut self, served_model: impl Into<String>) -> Self {
        self.served_model = Some(served_model.into());
        self
    }
}

/// Which side of a retrieval corpus a text is on.
///
/// **Closed** — deliberately not `#[non_exhaustive]` (ADR-C17). A wildcard arm
/// in an implementation is precisely where a role added later would be
/// mishandled without a word, which is the failure this type exists to
/// prevent; adding a variant is therefore a visible breaking act on this
/// boundary (INV-1), and that is the correct cost for it.
///
/// It describes **the text**, not the model, which is why there is no
/// `Symmetric` variant: a caller that had to choose one would have to know
/// which model is behind the trait object it holds, and hiding exactly that is
/// what this contract is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedRole {
    /// The text is a query being asked of the corpus.
    Query,
    /// The text is a passage of the corpus being indexed.
    Passage,
}

/// Per-call parameters of an [`Embedder`].
///
/// Carries the [`EmbedRole`] the text is on, and carries it **mandatorily**:
/// there is no `Default` and no other constructor, so a call site cannot omit
/// the one fact only the caller knows (ADR-C17). See [`Embedder`]'s role
/// contract for what the role obliges an implementation to do.
///
/// One `EmbedParams` covers a whole batch, so a batch is embedded under
/// exactly one role.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbedParams {
    /// The side the texts of this call are on.
    pub role: EmbedRole,
    /// The name of the model the embedder is asked for (ADR-C32 § 4).
    ///
    /// `Some(name)` asks for the model the component serves under `name`;
    /// `None` asks for the model it loaded. A component refuses, as
    /// [`ComponentError::InvalidRequest`], a name it does not serve, an empty
    /// name, and `None` when it has no single loaded model that `None` could
    /// mean. Absence is the only spelling of "no name". A served-model name is
    /// an identifier the backend resolves, never text prepended to the input:
    /// it is not the prefix ADR-C17 keeps off this struct.
    ///
    /// The ONNX embedder and reranker (`ragondin-embedder-onnx`,
    /// `ragondin-reranker-onnx`) are configured with no served-model name, so
    /// they refuse every `Some(name)`, on a call and in `model_identity`.
    pub served_model: Option<String>,
}

impl EmbedParams {
    /// Embeds under `role`, with the model the component loaded
    /// (`served_model` is `None`).
    pub fn new(role: EmbedRole) -> Self {
        Self {
            role,
            served_model: None,
        }
    }

    /// Asks for the model the component serves under `served_model`.
    ///
    /// The role stays mandatory: this sets the name on params that already
    /// carry one.
    pub fn with_served_model(mut self, served_model: impl Into<String>) -> Self {
        self.served_model = Some(served_model.into());
        self
    }
}

/// Per-call parameters of a [`VectorStore`] search.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SearchParams {
    /// How many nearest chunks to return.
    pub top_k: usize,
}

impl SearchParams {
    /// Returns the `top_k` nearest chunks.
    ///
    /// Infallible: a `top_k` of zero is representable here and rejected by the
    /// component, as [`ComponentError::InvalidRequest`] describes.
    pub fn new(top_k: usize) -> Self {
        Self { top_k }
    }
}

/// Per-call parameters of a [`ContextBuilder`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ContextParams {
    /// The cap on the size of the context — `top_k`'s twin (ADR-C31 § 2).
    ///
    /// **Its unit is the implementation's own, and the implementation
    /// documents it**: characters, tokens, chunks. The contract fixes that
    /// there is a cap and that zero is refused, never what is counted — a unit
    /// fixed here, in tokens, would oblige every builder to carry the
    /// generator's tokenizer.
    pub budget: usize,
}

impl ContextParams {
    /// Caps the context at `budget`.
    ///
    /// Infallible: a `budget` of zero is representable here and rejected by
    /// the component, as a zero `top_k` is — see [`ContextBuilder`]'s context
    /// contract.
    pub fn new(budget: usize) -> Self {
        Self { budget }
    }
}

/// Per-call parameters of a [`Generator`] (ADR-C31 § 2).
///
/// **`served_model` and `template` are required; the other three are
/// optional.** Required fields are the constructor's arguments, and each
/// optional one is absent until a `with_*` method sets it.
///
/// There is deliberately **no `Default`**: with two required fields, a default
/// would have to invent a model and a template, and a default chosen above the
/// component is a second, disagreeing copy of a decision that belongs to the
/// configuration (ADR-C31 § 2). An absent optional is passed through as `None`,
/// and what the component does without it is the component's own.
///
/// Every field is fixed per pipeline node rather than varying from one call to
/// the next. It is carried per call all the same, because the per-call params
/// are the only path from the pipeline representation to a `Remote`
/// generator's service: constructor configuration never crosses the wire, and a
/// setting a researcher varies between two runs must be in the representation
/// and its hash.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct GenerateParams {
    /// The name the generator asks its backend for.
    ///
    /// For a `Remote` generator, the name its inference server serves the
    /// model under; for a `Local` one, a name it recognises as the model it
    /// loaded — the names it recognises are its constructor configuration.
    /// Empty, or a name the component does not serve, is refused as
    /// [`ComponentError::InvalidRequest`].
    pub served_model: String,
    /// The prompt template, in the grammar stated on [`Generator`]. Empty, or
    /// malformed, is refused as [`ComponentError::InvalidRequest`].
    pub template: String,
    /// The sampling temperature, when the configuration sets one.
    pub temperature: Option<f64>,
    /// The sampling seed, when the configuration sets one.
    pub seed: Option<u64>,
    /// The cap on the answer's length, in its model's tokens, when the
    /// configuration sets one.
    pub max_tokens: Option<usize>,
}

impl GenerateParams {
    /// Asks the model served under `served_model` to answer the prompt
    /// `template` renders, with no optional setting.
    ///
    /// Infallible: an empty `served_model` or `template` is representable here
    /// and rejected by the component — see what [`Generator`] refuses.
    pub fn new(served_model: impl Into<String>, template: impl Into<String>) -> Self {
        Self {
            served_model: served_model.into(),
            template: template.into(),
            temperature: None,
            seed: None,
            max_tokens: None,
        }
    }

    /// Sets the sampling temperature.
    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Sets the sampling seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Sets the cap on the answer's length in tokens.
    pub fn with_max_tokens(mut self, max_tokens: usize) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }
}

/// A chunk together with its vector, as a [`VectorStore`] holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddedChunk {
    /// The chunk itself.
    pub chunk: Chunk,
    /// Its vector.
    pub embedding: Embedding,
}

/// Retrieves candidate chunks for a query.
///
/// # The ranking contract
///
/// Every trait here that returns `Vec<ScoredChunk>` returns it **sorted by
/// descending score**, and **every score is finite**. Both halves are
/// load-bearing and neither is checkable by the type system: nDCG@k and MRR
/// read position, so an unsorted list silently reports a wrong number; and
/// `f32` admits `NaN`, on which the `partial_cmp(…).unwrap()` every implementer
/// writes will panic. `ragondin-conformance` is where both halves are checked:
/// it is the behavioural suite every implementation must pass, so the contract
/// is enforced rather than trusted, identically for a built-in and a
/// third-party component (INV-7).
#[async_trait]
pub trait Retriever: Send + Sync {
    /// Returns the chunks this retriever considers most relevant to `query`,
    /// sorted by descending score.
    async fn retrieve(
        &self,
        query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError>;
}

/// Merges several retrieval results into one.
#[async_trait]
pub trait Fusion: Send + Sync {
    /// Fuses `inputs` — one ranked list per upstream retrieval leg, **in the
    /// order the pipeline wires them** — into a single list, sorted by
    /// descending score. See [`Retriever`]'s ranking contract.
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError>;
}

/// Reorders retrieved chunks against the query.
#[async_trait]
pub trait Reranker: Send + Sync {
    /// Returns `chunks` reordered by relevance to `query`, sorted by
    /// descending score. See [`Retriever`]'s ranking contract.
    async fn rerank(
        &self,
        query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError>;

    /// Reports the identity of the model this component reranks with when
    /// asked for `served_model` (ADR-C32 § 4) — the same value
    /// [`RerankParams::served_model`] carries on each call.
    ///
    /// `Some(name)` asks for the model served under `name`, and `None` for the
    /// model the component loaded; each is refused, as
    /// [`ComponentError::InvalidRequest`], exactly where a call carrying it
    /// would be. The identity is **stable** across calls while nothing has
    /// changed — no timestamp, no counter — and **complete** over every knob
    /// that decides the scores and is not in the node's params (ADR-C31 § 4):
    /// for a `Local` component, a digest of its model and of whatever else
    /// of its constructor configuration decides the scores, such as its
    /// tokenizer. An empty identity is not valid.
    async fn model_identity(
        &self,
        served_model: Option<&str>,
    ) -> Result<ModelIdentity, ComponentError>;
}

/// Turns text into vectors.
///
/// # The role contract
///
/// Retrieval embedders are frequently **asymmetric**: E5, BGE and GTE prefix a
/// query differently from a passage, and a text embedded on the wrong side
/// simply scores worse — no error is raised anywhere, and a conformance suite
/// cannot see it either, because it does not know which model it is testing.
/// ADR-C17 therefore makes the role a **per-call** parameter, carried by
/// [`EmbedParams::role`]: a caller **must** pass the [`EmbedRole`] that is true
/// of the text it is embedding, and an implementation **must** treat that role
/// as significant unless the model it wraps is symmetric.
///
/// What an asymmetric model prepends for each role is the implementation's own
/// **constructor configuration** and never appears on this boundary, which
/// keeps the contract agnostic of the model: a symmetric model is configured
/// with no prefix on either side rather than special-cased, and honouring the
/// role there means prepending the empty string.
///
/// # One embedding space
///
/// Every vector an implementation returns has **the same dimensionality**,
/// whatever the [`EmbedRole`] and whatever the batch. A query vector is scored
/// against a passage vector by construction, so a width that varies with the
/// role is a second embedding space rather than a second prefix, and there is
/// no retrieval to be had between the two: the role selects what is prepended,
/// never which model answers. Stated here because the role is what makes two
/// widths expressible at all, and because nothing downstream would name the
/// embedder — a mismatch surfaces as a `VectorStore` rejecting a search vector.
///
/// # At least one component
///
/// Every vector an implementation returns has **at least one component**
/// (ADR-C20). `ragondin-types` builds an empty [`Embedding`] without complaint,
/// because a value type with no error type cannot reject anything, but
/// representable is not valid: a vector of width zero has no direction, so no
/// similarity is defined against it and no store can answer a search over it.
/// An implementation that cannot embed a text returns a [`ComponentError`]
/// rather than a vector of no width, which reports a failure as data no caller
/// can tell apart from a result. This constrains a *returned vector*, not a
/// batch: an empty batch still embeds to no vectors.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Embeds `texts`, returning one vector per input **in the same order**.
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError>;

    /// Reports the identity of the model this component embeds with when
    /// asked for `served_model` (ADR-C32 § 4) — the same value
    /// [`EmbedParams::served_model`] carries on each call.
    ///
    /// `Some(name)` asks for the model served under `name`, and `None` for the
    /// model the component loaded; each is refused, as
    /// [`ComponentError::InvalidRequest`], exactly where a call carrying it
    /// would be. The identity is **stable** across calls while nothing has
    /// changed — no timestamp, no counter — and **complete** over every knob
    /// that decides the vectors and is not in the node's params (ADR-C31 § 4):
    /// for a `Local` component, a digest of its model and of whatever else
    /// of its constructor configuration decides the vectors, such as its
    /// tokenizer. An empty identity is not valid.
    async fn model_identity(
        &self,
        served_model: Option<&str>,
    ) -> Result<ModelIdentity, ComponentError>;
}

/// The vector index a dense retriever queries.
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Inserts or replaces `entries`, keyed by their chunk ids.
    ///
    /// An **empty `entries` succeeds and changes nothing** — see the crate's
    /// *Empty collections* rule (ADR-C19). It is not an invalid request.
    ///
    /// Takes `&self`, not `&mut self`: a `Box<dyn VectorStore>` is shared
    /// across concurrent queries, so **an implementation that holds mutable
    /// state must provide its own interior mutability** — a lock, a channel, or
    /// a client that is already `Sync`. This is a requirement on implementers,
    /// not an oversight.
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError>;

    /// Returns the nearest chunks to `embedding`, sorted by descending score.
    /// See [`Retriever`]'s ranking contract.
    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError>;
}

/// Selects, orders and renders retrieved chunks into a [`Context`].
///
/// # The context contract
///
/// A builder **selects, orders and renders; it does not judge relevance**
/// (ADR-C31 § 1). Every [`ContextChunk`](ragondin_types::ContextChunk) it
/// returns names a chunk it was handed — no fabricated id, and no id twice —
/// in the order it placed them, and carries that chunk's incoming score
/// untouched, on the producing node's scale: a builder that drops chunks over
/// its budget or reorders what it keeps never assigns a score of its own.
///
/// A zero [`ContextParams::budget`] is refused as
/// [`ComponentError::InvalidRequest`], under the crate's `top_k` rule. **Zero
/// chunks is a valid call** (ADR-C19): it returns the empty context,
/// `chunks.is_empty()`, with `text` unconstrained, since a template may render
/// a header over no passages. ADR-C25 applies unchanged.
///
/// What the builder renders passages with, and the unit its budget is counted
/// in, are its **constructor configuration**, and both are covered by
/// [`model_identity`](Self::model_identity).
///
/// The conformance suite may check that a well-formed call succeeds, that a
/// zero budget is refused, and that the context fabricates no chunk id and
/// repeats none. **Nothing about content**: a suite that does not know the
/// component cannot judge what it rendered.
#[async_trait]
pub trait ContextBuilder: Send + Sync {
    /// Builds the context `query` is answered from, out of `chunks`, capped
    /// at `params.budget`.
    async fn build(
        &self,
        query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &ContextParams,
    ) -> Result<Context, ComponentError>;

    /// Reports the identity of this builder's configuration (ADR-C31 § 4).
    ///
    /// **Stable** across calls while nothing has changed — no timestamp, no
    /// counter — and **complete** over every knob that decides the output and
    /// is not in the node's params: its template and the unit its budget is
    /// counted in. A builder with no model digests its configuration alone.
    /// An empty identity is not valid.
    async fn model_identity(&self) -> Result<ModelIdentity, ComponentError>;
}

/// Answers a query from a [`Context`].
///
/// # The template contract
///
/// The component renders [`GenerateParams::template`] and asks its model to
/// answer the rendered text, and **nothing above the component parses a
/// template** (ADR-C31 § 2). The grammar is the whole of the following, and
/// a `Local` generator and a `Remote` service implement the same one:
///
/// - `{query}` is replaced by the [`Query`]'s `text`, and `{context}` by the
///   [`Context`]'s `text`;
/// - `{{` renders a literal `{`, and `}}` a literal `}`;
/// - any other `{` or `}` — a name other than `query` or `context` between
///   braces, a `{` no `}` closes, a lone `}` — makes the template malformed,
///   and the call is refused as [`ComponentError::InvalidRequest`];
/// - each placeholder may appear any number of times, including none.
///   Substitution is one pass, left to right, taking `{{` or `}}` before a
///   placeholder at each position, so `{{query}}` renders as the text
///   `{query}`; substituted text is never scanned again.
///
/// The rendered text is the whole of what the component asks its model: it
/// adds no instruction text of its own. Framing that one message for an
/// inference server — message roles and the server's own chat encoding — is
/// allowed, and never adds wording of its own (ADR-C31 § 2).
///
/// # What is refused, and what is not
///
/// An empty [`GenerateParams::served_model`], an empty template, a malformed
/// template and a served model the component does not serve are each refused
/// as [`ComponentError::InvalidRequest`]. **An empty context is a valid call**
/// (ADR-C19): an answer of "I do not know" is conformant, and refusing the
/// call is not. ADR-C25 applies unchanged: a `Local` generator running a
/// forward pass moves that work off the caller's thread itself.
///
/// The conformance suite may check that a well-formed call succeeds, and that
/// an empty served model, an empty template and a malformed template are
/// refused — each a refusal of the call's form. **Nothing about content**: a
/// suite that does not know which model it is testing cannot say whether an
/// answer is good.
#[async_trait]
pub trait Generator: Send + Sync {
    /// Answers `query` from `context`, with the model and template `params`
    /// name.
    async fn generate(
        &self,
        query: &Query,
        context: &Context,
        params: &GenerateParams,
    ) -> Result<Answer, ComponentError>;

    /// Reports the identity of the model this component answers
    /// `served_model` with (ADR-C31 § 4) — a verification of the name, not
    /// only a record of it.
    ///
    /// A name the component does not serve is refused as
    /// [`ComponentError::InvalidRequest`], never answered with a warning. The
    /// identity is **stable** across calls while nothing has changed, and
    /// **complete** over every knob that decides the answer and is not in the
    /// node's params — the model's revision behind the served name and, for a
    /// `Local` generator, whatever of its constructor configuration decides
    /// the answer. An empty identity is not valid.
    async fn model_identity(&self, served_model: &str) -> Result<ModelIdentity, ComponentError>;
}

// The `Send + Sync` bounds live next to what they constrain, not only in a
// test whose deletion would remove the guarantee silently.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn Retriever>();
    assert_send_sync::<dyn Fusion>();
    assert_send_sync::<dyn Reranker>();
    assert_send_sync::<dyn Embedder>();
    assert_send_sync::<dyn VectorStore>();
    assert_send_sync::<dyn ContextBuilder>();
    assert_send_sync::<dyn Generator>();
    assert_send_sync::<ComponentError>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use ragondin_types::{ChunkId, ContextChunk, DocId, QueryId};

    // Each stub proves the trait is `dyn`-compatible (ADR-C8: `async_trait`,
    // not RPITIT) by being coerced to `Box<dyn _>` and called through the
    // vtable. A signature that compiled but could not be made into a trait
    // object would break the engine, which cannot tell `Local` from `Remote`.

    fn chunk(id: &str) -> Chunk {
        Chunk {
            id: ChunkId::new(id),
            text: format!("text of {id}"),
            document_id: DocId::new("doc"),
        }
    }

    fn scored(id: &str, score: f32) -> ScoredChunk {
        ScoredChunk {
            chunk: chunk(id),
            score,
        }
    }

    struct StubRetriever;
    #[async_trait]
    impl Retriever for StubRetriever {
        async fn retrieve(
            &self,
            query: &Query,
            params: &RetrieveParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            Ok((0..params.top_k)
                .map(|i| scored(&format!("{}-{i}", query.id.as_str()), 1.0))
                .collect())
        }
    }

    struct StubFusion;
    #[async_trait]
    impl Fusion for StubFusion {
        async fn fuse(
            &self,
            inputs: Vec<Vec<ScoredChunk>>,
            _params: &FusionParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            Ok(inputs.into_iter().flatten().collect())
        }
    }

    /// The stubs below serve the model they loaded and no name, as the ONNX
    /// components do: `None` is the only served model they answer, on a call
    /// and in `model_identity` alike.
    fn serves_no_name(served_model: Option<&str>) -> Result<(), ComponentError> {
        match served_model {
            None => Ok(()),
            Some(name) => Err(ComponentError::InvalidRequest(format!(
                "model {name:?} is not served here"
            ))),
        }
    }

    struct StubReranker;
    #[async_trait]
    impl Reranker for StubReranker {
        async fn rerank(
            &self,
            _query: &Query,
            mut chunks: Vec<ScoredChunk>,
            params: &RerankParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            serves_no_name(params.served_model.as_deref())?;
            // Reranks by chunk id, then re-scores so the result honours the
            // ranking contract: descending, finite.
            chunks.sort_by(|a, b| b.chunk.id.as_str().cmp(a.chunk.id.as_str()));
            chunks.truncate(params.top_k);
            for (i, c) in chunks.iter_mut().enumerate() {
                c.score = 1.0 - i as f32 / 10.0;
            }
            Ok(chunks)
        }

        async fn model_identity(
            &self,
            served_model: Option<&str>,
        ) -> Result<ModelIdentity, ComponentError> {
            serves_no_name(served_model)?;
            Ok(ModelIdentity::new("stub-reranker@rev1"))
        }
    }

    struct StubEmbedder;
    #[async_trait]
    impl Embedder for StubEmbedder {
        async fn embed(
            &self,
            texts: &[String],
            params: &EmbedParams,
        ) -> Result<Vec<Embedding>, ComponentError> {
            serves_no_name(params.served_model.as_deref())?;
            Ok(texts
                .iter()
                .map(|t| Embedding::new(vec![t.len() as f32]))
                .collect())
        }

        async fn model_identity(
            &self,
            served_model: Option<&str>,
        ) -> Result<ModelIdentity, ComponentError> {
            serves_no_name(served_model)?;
            Ok(ModelIdentity::new("stub-embedder@rev1"))
        }
    }

    struct StubStore;
    #[async_trait]
    impl VectorStore for StubStore {
        async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
            // Contradicts the *Empty collections* rule at the top of this file
            // (ADR-C19, accepted 2026-09-10), deliberately and temporarily: the
            // decision and the behaviour change are separate PRs, so that
            // neither is a side effect of the other. **This is not the worked
            // example to copy.** A conformant store returns `Ok(())` here.
            // ADR-C19's Consequences list what the change owes.
            if entries.is_empty() {
                return Err(ComponentError::InvalidRequest("nothing to upsert".into()));
            }
            Ok(())
        }
        async fn search(
            &self,
            embedding: &Embedding,
            params: &SearchParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            if embedding.is_empty() {
                return Err(ComponentError::InvalidRequest(
                    "an empty embedding has no direction".into(),
                ));
            }
            Ok((0..params.top_k)
                .map(|i| scored(&format!("hit-{i}"), 1.0 - i as f32 / 10.0))
                .collect())
        }
    }

    struct StubContextBuilder;
    #[async_trait]
    impl ContextBuilder for StubContextBuilder {
        async fn build(
            &self,
            _query: &Query,
            chunks: Vec<ScoredChunk>,
            params: &ContextParams,
        ) -> Result<Context, ComponentError> {
            // The budget counts chunks here; the unit is the implementation's
            // own, and a zero cap is refused as `top_k` is.
            if params.budget == 0 {
                return Err(ComponentError::InvalidRequest(
                    "a zero budget asks for a context that cannot exist".into(),
                ));
            }
            let kept: Vec<ScoredChunk> = chunks.into_iter().take(params.budget).collect();
            Ok(Context {
                text: kept
                    .iter()
                    .map(|c| c.chunk.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                chunks: kept
                    .into_iter()
                    .map(|c| ContextChunk {
                        id: c.chunk.id,
                        document_id: c.chunk.document_id,
                        score: c.score,
                    })
                    .collect(),
            })
        }

        async fn model_identity(&self) -> Result<ModelIdentity, ComponentError> {
            Ok(ModelIdentity::new("stub-builder:chunks"))
        }
    }

    struct StubGenerator;
    #[async_trait]
    impl Generator for StubGenerator {
        async fn generate(
            &self,
            query: &Query,
            context: &Context,
            params: &GenerateParams,
        ) -> Result<Answer, ComponentError> {
            if params.served_model != "stub-model" {
                return Err(ComponentError::InvalidRequest(format!(
                    "model {:?} is not served here",
                    params.served_model
                )));
            }
            // Not the template grammar: enough to show the call's inputs reach
            // the implementation through the vtable.
            Ok(Answer {
                text: params
                    .template
                    .replace("{query}", &query.text)
                    .replace("{context}", &context.text),
            })
        }

        async fn model_identity(
            &self,
            served_model: &str,
        ) -> Result<ModelIdentity, ComponentError> {
            if served_model != "stub-model" {
                return Err(ComponentError::InvalidRequest(format!(
                    "model {served_model:?} is not served here"
                )));
            }
            Ok(ModelIdentity::new("stub-model@rev1"))
        }
    }

    #[tokio::test]
    async fn a_context_builder_is_callable_through_a_trait_object() {
        let component: Box<dyn ContextBuilder> = Box::new(StubContextBuilder);
        let query = Query {
            id: QueryId::new("q1"),
            text: "why".to_string(),
        };
        let context = component
            .build(
                &query,
                vec![scored("a", 0.9), scored("b", 0.5), scored("c", 0.1)],
                &ContextParams::new(2),
            )
            .await
            .unwrap();
        assert_eq!(context.chunks.len(), 2);
        assert_eq!(context.chunks[0].id.as_str(), "a");
        assert_eq!(
            context.chunks[1].score, 0.5,
            "a builder carries scores through"
        );
        assert_eq!(context.text, "text of a\ntext of b");
        assert_eq!(
            component.model_identity().await.unwrap().as_str(),
            "stub-builder:chunks"
        );
    }

    #[tokio::test]
    async fn a_generator_is_callable_through_a_trait_object() {
        let component: Box<dyn Generator> = Box::new(StubGenerator);
        let query = Query {
            id: QueryId::new("q1"),
            text: "why".to_string(),
        };
        let context = Context {
            chunks: vec![],
            text: "because".to_string(),
        };
        let answer = component
            .generate(
                &query,
                &context,
                &GenerateParams::new("stub-model", "Q: {query} C: {context}"),
            )
            .await
            .unwrap();
        assert_eq!(answer.text, "Q: why C: because");
        assert_eq!(
            component
                .model_identity("stub-model")
                .await
                .unwrap()
                .as_str(),
            "stub-model@rev1"
        );
        let refused = component.model_identity("other").await.unwrap_err();
        assert!(matches!(refused, ComponentError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn a_zero_budget_is_representable_and_refused_by_the_component() {
        // ADR-C31 § 2: `budget` is `top_k`'s twin. `ContextParams::new(0)`
        // builds, as `RerankParams::new(0)` does, and the component refuses the
        // call as an invalid request rather than answering an empty context.
        // The refusal half is illustrative: it exercises this file's stub,
        // which shows the shape of the refusal and proves nothing about any
        // real builder.
        assert_eq!(ContextParams::new(0).budget, 0);
        let component: Box<dyn ContextBuilder> = Box::new(StubContextBuilder);
        let query = Query {
            id: QueryId::new("q1"),
            text: "why".to_string(),
        };
        let err = component
            .build(&query, vec![scored("a", 1.0)], &ContextParams::new(0))
            .await
            .unwrap_err();
        assert!(matches!(err, ComponentError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn a_retriever_is_callable_through_a_trait_object() {
        let component: Box<dyn Retriever> = Box::new(StubRetriever);
        let query = Query {
            id: QueryId::new("q1"),
            text: "why".to_string(),
        };
        let hits = component
            .retrieve(&query, &RetrieveParams::new(3))
            .await
            .unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].chunk.id.as_str(), "q1-0");
    }

    #[tokio::test]
    async fn a_fusion_is_callable_through_a_trait_object() {
        let component: Box<dyn Fusion> = Box::new(StubFusion);
        let fused = component
            .fuse(
                vec![vec![scored("a", 1.0)], vec![scored("b", 0.5)]],
                &FusionParams::new(),
            )
            .await
            .unwrap();
        assert_eq!(fused.len(), 2);
    }

    #[tokio::test]
    async fn a_reranker_is_callable_through_a_trait_object() {
        let component: Box<dyn Reranker> = Box::new(StubReranker);
        let query = Query {
            id: QueryId::new("q1"),
            text: "why".to_string(),
        };
        let out = component
            .rerank(
                &query,
                vec![scored("a", 1.0), scored("b", 0.5), scored("c", 0.1)],
                &RerankParams::new(2),
            )
            .await
            .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].chunk.id.as_str(), "c");
        assert!(
            out[0].score >= out[1].score && out.iter().all(|c| c.score.is_finite()),
            "a reranker returns finite scores in descending order"
        );
        assert_eq!(
            component.model_identity(None).await.unwrap().as_str(),
            "stub-reranker@rev1"
        );
        let refused = component.model_identity(Some("other")).await.unwrap_err();
        assert!(matches!(refused, ComponentError::InvalidRequest(_)));
        // Refused exactly where `model_identity` refuses it.
        let refused = component
            .rerank(
                &query,
                vec![scored("a", 1.0)],
                &RerankParams::new(2).with_served_model("other"),
            )
            .await
            .unwrap_err();
        assert!(matches!(refused, ComponentError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn an_embedder_is_callable_through_a_trait_object() {
        let component: Box<dyn Embedder> = Box::new(StubEmbedder);
        let out = component
            .embed(
                &["ab".to_string(), "abcd".to_string()],
                &EmbedParams::new(EmbedRole::Passage),
            )
            .await
            .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].dim(), 1);
        assert_eq!(out[1].as_slice(), &[4.0]);
        assert_eq!(
            component.model_identity(None).await.unwrap().as_str(),
            "stub-embedder@rev1"
        );
        let refused = component.model_identity(Some("other")).await.unwrap_err();
        assert!(matches!(refused, ComponentError::InvalidRequest(_)));
        // Refused exactly where `model_identity` refuses it.
        let refused = component
            .embed(
                &["ab".to_string()],
                &EmbedParams::new(EmbedRole::Query).with_served_model("other"),
            )
            .await
            .unwrap_err();
        assert!(matches!(refused, ComponentError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn a_vector_store_is_callable_through_a_trait_object() {
        let component: Box<dyn VectorStore> = Box::new(StubStore);
        component
            .upsert(vec![EmbeddedChunk {
                chunk: chunk("c1"),
                embedding: Embedding::new(vec![1.0]),
            }])
            .await
            .unwrap();
        let hits = component
            .search(&Embedding::new(vec![1.0]), &SearchParams::new(2))
            .await
            .unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[tokio::test]
    async fn a_component_error_crosses_the_trait_object_boundary() {
        // The engine cannot tell `Local` from `Remote` (ADR-3), so a failure
        // has to arrive as this one shared type whichever face produced it.
        let component: Box<dyn VectorStore> = Box::new(StubStore);
        let err = component
            .search(&Embedding::new(vec![]), &SearchParams::new(1))
            .await
            .unwrap_err();
        assert!(matches!(err, ComponentError::InvalidRequest(_)));
        assert_eq!(
            err.to_string(),
            "invalid request: an empty embedding has no direction"
        );
    }

    #[test]
    fn component_errors_read_as_documented() {
        assert_eq!(
            ComponentError::Unavailable("reranker-svc: connection refused".into()).to_string(),
            "component unavailable: reranker-svc: connection refused"
        );
        let inner = std::io::Error::other("index corrupt");
        let err = ComponentError::Backend(Box::new(inner));
        assert_eq!(
            err.to_string(),
            "backend failure: index corrupt",
            "the cause must appear in Display, not only via source()"
        );
        assert_eq!(
            std::error::Error::source(&err).map(|e| e.to_string()),
            Some("index corrupt".to_string()),
            "a backend failure must not swallow the cause it wraps"
        );
    }

    #[test]
    fn params_carry_the_knobs_the_reference_pipeline_sets() {
        // The reference pipeline in `docs/system-architecture.md` §5.1 sets
        // `params: { top_k: 50 }` on a retriever and `params: { top_k: 8 }` on
        // a reranker, and none on the fusion.
        assert_eq!(RetrieveParams::new(50).top_k, 50);
        assert_eq!(RerankParams::new(8).top_k, 8);
        assert_eq!(SearchParams::new(50).top_k, 50);
        let _ = FusionParams::new();
        let _ = EmbedParams::new(EmbedRole::Query);
    }

    #[test]
    fn context_params_carry_the_budget() {
        assert_eq!(ContextParams::new(512).budget, 512);
    }

    #[test]
    fn generate_params_require_a_served_model_and_a_template_and_nothing_else() {
        // ADR-C31 § 2: the two required fields are the constructor's
        // arguments; the three optional ones are absent until set, and no
        // default is invented for any of them.
        let params = GenerateParams::new("llama-3-8b", "{context}\n\n{query}");
        assert_eq!(params.served_model, "llama-3-8b");
        assert_eq!(params.template, "{context}\n\n{query}");
        assert_eq!(params.temperature, None);
        assert_eq!(params.seed, None);
        assert_eq!(params.max_tokens, None);
    }

    #[test]
    fn generate_params_set_each_optional_independently() {
        let params = GenerateParams::new("m", "{query}")
            .with_temperature(0.0)
            .with_seed(7)
            .with_max_tokens(64);
        assert_eq!(params.temperature, Some(0.0));
        assert_eq!(params.seed, Some(7));
        assert_eq!(params.max_tokens, Some(64));
        assert_eq!(
            GenerateParams::new("m", "{query}").with_seed(7).temperature,
            None,
            "setting one optional leaves the others absent"
        );
    }

    #[test]
    fn served_model_is_absent_unless_the_caller_sets_it() {
        // ADR-C32 § 4: `None` asks for the model the component loaded, and
        // absence is the only spelling of it, so the existing constructors
        // leave the field `None`.
        assert_eq!(EmbedParams::new(EmbedRole::Query).served_model, None);
        assert_eq!(RerankParams::new(8).served_model, None);
        let embed = EmbedParams::new(EmbedRole::Passage).with_served_model("bge-m3");
        assert_eq!(embed.served_model.as_deref(), Some("bge-m3"));
        assert_eq!(embed.role, EmbedRole::Passage);
        let rerank = RerankParams::new(8).with_served_model("bge-reranker");
        assert_eq!(rerank.served_model.as_deref(), Some("bge-reranker"));
        assert_eq!(rerank.top_k, 8);
    }

    #[test]
    fn embed_params_carry_the_role_the_caller_states() {
        // ADR-C17: the role is mandatory and reaches the implementation
        // unchanged. `EmbedParams` has no `Default` and no other constructor,
        // so a call site cannot omit it — which is the whole protection, since
        // a wrong role costs nDCG without erroring anywhere.
        assert_eq!(EmbedParams::new(EmbedRole::Query).role, EmbedRole::Query);
        assert_eq!(
            EmbedParams::new(EmbedRole::Passage).role,
            EmbedRole::Passage
        );
    }

    #[test]
    fn embed_role_is_copied_and_compared_rather_than_borrowed() {
        // An implementation matches on the role and carries it into its own
        // prefix table; making that cost a clone would push implementers
        // toward stashing it on the component, which is where a stale role
        // starts.
        fn assert_bounds<T: Clone + Copy + std::fmt::Debug + PartialEq + Eq>() {}
        assert_bounds::<EmbedRole>();
        assert_ne!(EmbedRole::Query, EmbedRole::Passage);
    }

    #[test]
    fn every_component_trait_is_send_and_sync() {
        // The engine holds components across await points and shares them
        // between concurrent queries; a trait object that were not `Send +
        // Sync` would make the whole data plane single-threaded.
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn Retriever>();
        assert_send_sync::<dyn Fusion>();
        assert_send_sync::<dyn Reranker>();
        assert_send_sync::<dyn Embedder>();
        assert_send_sync::<dyn VectorStore>();
        assert_send_sync::<dyn ContextBuilder>();
        assert_send_sync::<dyn Generator>();
        assert_send_sync::<ComponentError>();
    }
}
