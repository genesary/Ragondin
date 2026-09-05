//! # ragondin-contracts
//!
//! The **component contract**: the traits an external contributor implements.
//! This crate is **face 1** of the two-faced contract (ADR-C3) — the Rust
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
//! Today it carries the families the M2 retrieval bench exercises. `Chunker`,
//! `Indexer`, `ContextBuilder`, `Generator` and `Grader` arrive with M3/M4 —
//! adding a trait is additive, so defining them before anything implements
//! them would be dead API.
//!
//! # Where parameters come from
//!
//! A pipeline node carries an untyped parameter map (`ragondin-pipeline`), and each
//! trait here takes a **typed** params struct. The two are bridged at physical
//! planning (`docs/code-architecture.md` §6.3), which resolves an `impl:` name
//! into a *constructed* component and applies defaults: implementation-specific
//! configuration — BM25's `k1` and `b`, a model path — is handed to the
//! constructor, while the params structs below carry only what varies **per
//! call**. §5.1's reference pipeline shows the split: `top_k` on a retriever
//! and on a reranker, nothing on the fusion.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

use async_trait::async_trait;
use ragondin_types::{Chunk, Embedding, Query, ScoredChunk};
use thiserror::Error;

/// The error every component boundary returns.
///
/// One type for both faces (ADR-C3): the engine cannot tell a `Local` call
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
    /// and the engine cannot tell which face it called (ADR-C3).
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
/// Empty today — §5.1's `fuse` node carries no params, and a fusion's own
/// constants (RRF's `k`) are constructor configuration. It exists so that a
/// future knob is a field rather than a change to the trait's signature, which
/// would break every implementation in and out of the repository — including
/// every third-party `Remote` service, which is the contribution funnel ADR-C3
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
    pub fn new(top_k: usize) -> Self {
        Self { top_k }
    }
}

/// Per-call parameters of an [`Embedder`].
///
/// Empty today, and deliberately so: **whether an embedder needs to know its
/// role is an open question.** Asymmetric retrieval models — E5, BGE, GTE —
/// prefix a query differently from a passage, and embedding both the same way
/// costs retrieval quality *silently*, with no error anywhere. Either that role
/// belongs here as a field, or it is constructor configuration and one model
/// registers twice under two `impl:` names. Both have real costs, so the choice
/// is not made in passing: see the decision issue linked from
/// `ARCHITECTURE.md`. The struct exists now so that whichever way it goes, the
/// answer is a field rather than a change to [`Embedder::embed`]'s signature.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbedParams {}

impl EmbedParams {
    /// The default embedding parameters.
    pub fn new() -> Self {
        Self {}
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
/// writes will panic. `ragondin-conformance` (#17) is where this is enforced across
/// implementations.
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

// D-11: the `Send + Sync` bounds live next to what they constrain, not only in
// a test whose deletion would remove the guarantee silently.
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
            .embed(&["ab".to_string(), "abcd".to_string()], &EmbedParams::new())
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
        // The engine cannot tell `Local` from `Remote` (ADR-C3), so a failure
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
        // §5.1 sets `params: { top_k: 50 }` on a retriever and
        // `params: { top_k: 8 }` on a reranker, and none on the fusion.
        assert_eq!(RetrieveParams::new(50).top_k, 50);
        assert_eq!(RerankParams::new(8).top_k, 8);
        assert_eq!(SearchParams::new(50).top_k, 50);
        let _ = FusionParams::new();
        let _ = EmbedParams::new();
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
