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
//! This crate defines five families: `Retriever`, `Fusion`, `Reranker`,
//! `Embedder` and `VectorStore`. `Chunker`, `Indexer`, `ContextBuilder`,
//! `Generator` and `Grader` are not defined here. Adding a trait is additive
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
//! constructor, while the params structs below carry only what varies **per
//! call**. The reference pipeline in `docs/system-architecture.md` §5.1 shows
//! the split: `top_k` on a retriever and on a reranker, nothing on the fusion.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

use async_trait::async_trait;
use ragondin_types::{Chunk, Embedding, Query, ScoredChunk};
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
}

impl RerankParams {
    /// Keeps `top_k` chunks.
    ///
    /// Infallible: a `top_k` of zero is representable here and rejected by the
    /// component, as [`ComponentError::InvalidRequest`] describes.
    pub fn new(top_k: usize) -> Self {
        Self { top_k }
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
}

impl EmbedParams {
    /// Embeds under `role`.
    pub fn new(role: EmbedRole) -> Self {
        Self { role }
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
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Embeds `texts`, returning one vector per input **in the same order**.
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError>;
}

/// The vector index a dense retriever queries.
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Inserts or replaces `entries`, keyed by their chunk ids.
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

// The `Send + Sync` bounds live next to what they constrain, not only in a
// test whose deletion would remove the guarantee silently.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn Retriever>();
    assert_send_sync::<dyn Fusion>();
    assert_send_sync::<dyn Reranker>();
    assert_send_sync::<dyn Embedder>();
    assert_send_sync::<dyn VectorStore>();
    assert_send_sync::<ComponentError>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use ragondin_types::{ChunkId, DocId, QueryId};

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

    struct StubReranker;
    #[async_trait]
    impl Reranker for StubReranker {
        async fn rerank(
            &self,
            _query: &Query,
            mut chunks: Vec<ScoredChunk>,
            params: &RerankParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            // Reranks by chunk id, then re-scores so the result honours the
            // ranking contract: descending, finite.
            chunks.sort_by(|a, b| b.chunk.id.as_str().cmp(a.chunk.id.as_str()));
            chunks.truncate(params.top_k);
            for (i, c) in chunks.iter_mut().enumerate() {
                c.score = 1.0 - i as f32 / 10.0;
            }
            Ok(chunks)
        }
    }

    struct StubEmbedder;
    #[async_trait]
    impl Embedder for StubEmbedder {
        async fn embed(
            &self,
            texts: &[String],
            _params: &EmbedParams,
        ) -> Result<Vec<Embedding>, ComponentError> {
            Ok(texts
                .iter()
                .map(|t| Embedding::new(vec![t.len() as f32]))
                .collect())
        }
    }

    struct StubStore;
    #[async_trait]
    impl VectorStore for StubStore {
        async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
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
        assert_send_sync::<ComponentError>();
    }
}
