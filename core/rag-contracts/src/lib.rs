//! # rag-contracts
//!
//! The **component contract**: the traits an external contributor implements —
//! `Chunker`, `Embedder`, `Indexer`, `Retriever`, `Fusion`, `Reranker`,
//! `ContextBuilder`, `Generator`, `Grader`, `VectorStore`.
//!
//! This crate is a **stable API boundary** (INV-1) and **the crate a
//! contributor compiles against**. It must stay light (INV-4): implementing a
//! component requires only this crate and `rag-types`, never the engine.
//!
//! The traits themselves land in a later issue. What exists today is the
//! compiling skeleton plus the placeholder error type every boundary will
//! return. See `ARCHITECTURE.md`.

use thiserror::Error;

/// The error type a component returns.
///
/// Placeholder for now: concrete, per-boundary variants are added alongside the
/// traits that produce them, in a later issue. `#[non_exhaustive]` so that
/// adding those variants is not a breaking change to this stable API boundary.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ComponentError {
    /// An unspecified component failure.
    #[error("component error: {0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

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
            chunks.reverse();
            chunks.truncate(params.top_k);
            Ok(chunks)
        }
    }

    struct StubEmbedder;
    #[async_trait]
    impl Embedder for StubEmbedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, ComponentError> {
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
            top_k: usize,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            if embedding.is_empty() {
                return Err(ComponentError::InvalidRequest(
                    "an empty embedding has no direction".into(),
                ));
            }
            Ok((0..top_k).map(|i| scored(&format!("hit-{i}"), 0.5)).collect())
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
    }

    #[tokio::test]
    async fn an_embedder_is_callable_through_a_trait_object() {
        let component: Box<dyn Embedder> = Box::new(StubEmbedder);
        let out = component
            .embed(&["ab".to_string(), "abcd".to_string()])
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
            .search(&Embedding::new(vec![1.0]), 2)
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
            .search(&Embedding::new(vec![]), 1)
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
        let inner = std::io::Error::new(std::io::ErrorKind::Other, "index corrupt");
        let err = ComponentError::Backend(Box::new(inner));
        assert_eq!(err.to_string(), "backend failure");
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
        let _ = FusionParams::new();
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
