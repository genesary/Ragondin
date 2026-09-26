//! In-test `Local` components the in-process services host. Each is a
//! conformant member of its family and nothing more; none is a worked example
//! of a real component.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ragondin_contracts::{
    ComponentError, EmbedParams, EmbeddedChunk, Embedder, Fusion, FusionParams, RerankParams,
    Reranker, RetrieveParams, Retriever, SearchParams, VectorStore,
};
use ragondin_types::{Chunk, ChunkId, DocId, Embedding, ModelIdentity, Query, ScoredChunk};

/// The one name the model-bearing stubs serve. As a `Remote` service does,
/// they refuse `None` and every other name (ADR-C32 § 4).
pub const SERVED_MODEL: &str = "stub-model";

/// What the model-bearing stubs report for [`SERVED_MODEL`].
pub const IDENTITY: &str = "stub-model@rev1";

fn serves(served_model: Option<&str>) -> Result<(), ComponentError> {
    match served_model {
        Some(SERVED_MODEL) => Ok(()),
        other => Err(ComponentError::InvalidRequest(format!(
            "model {other:?} is not served here"
        ))),
    }
}

fn zero_top_k(top_k: usize) -> Result<(), ComponentError> {
    if top_k == 0 {
        return Err(ComponentError::InvalidRequest("a top_k of zero".into()));
    }
    Ok(())
}

pub fn chunk(id: &str) -> Chunk {
    Chunk {
        id: ChunkId::new(id),
        text: format!("text of {id}"),
        document_id: DocId::new("doc"),
    }
}

fn by_descending_score(chunks: &mut [ScoredChunk]) {
    chunks.sort_by(|a, b| b.score.total_cmp(&a.score));
}

/// Answers every query with up to three fixed chunks.
pub struct StubRetriever;

#[async_trait]
impl Retriever for StubRetriever {
    async fn retrieve(
        &self,
        _query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        zero_top_k(params.top_k)?;
        Ok((0..params.top_k.min(3))
            .map(|i| ScoredChunk {
                chunk: chunk(&format!("hit-{i}")),
                score: 1.0 - i as f32 / 10.0,
            })
            .collect())
    }
}

/// Reciprocal rank fusion, k = 60.
pub struct StubFusion;

#[async_trait]
impl Fusion for StubFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let mut fused: Vec<ScoredChunk> = Vec::new();
        for leg in inputs {
            for (rank, hit) in leg.into_iter().enumerate() {
                let score = 1.0 / (60.0 + rank as f32 + 1.0);
                match fused.iter_mut().find(|f| f.chunk.id == hit.chunk.id) {
                    Some(seen) => seen.score += score,
                    None => fused.push(ScoredChunk {
                        chunk: hit.chunk,
                        score,
                    }),
                }
            }
        }
        by_descending_score(&mut fused);
        Ok(fused)
    }
}

/// Keeps the best `top_k` chunks by their incoming score.
pub struct StubReranker;

#[async_trait]
impl Reranker for StubReranker {
    async fn rerank(
        &self,
        _query: &Query,
        mut chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        serves(params.served_model.as_deref())?;
        zero_top_k(params.top_k)?;
        by_descending_score(&mut chunks);
        chunks.truncate(params.top_k);
        Ok(chunks)
    }

    async fn model_identity(
        &self,
        served_model: Option<&str>,
    ) -> Result<ModelIdentity, ComponentError> {
        serves(served_model)?;
        Ok(ModelIdentity::new(IDENTITY))
    }
}

/// Embeds a text as a deterministic hash of its bytes, `dim` components wide,
/// and records every batch of texts it receives, as received.
#[derive(Clone)]
pub struct StubEmbedder {
    dim: usize,
    pub received: Arc<Mutex<Vec<Vec<String>>>>,
}

impl StubEmbedder {
    pub fn new(dim: usize) -> Self {
        Self {
            dim,
            received: Arc::default(),
        }
    }
}

fn hash_vector(text: &str, dim: usize) -> Embedding {
    // FNV-1a, re-seeded per component: distinct texts give distinct vectors
    // for any purpose a test here has.
    Embedding::new(
        (0..dim)
            .map(|i| {
                let mut h: u32 = 0x811C_9DC5 ^ i as u32;
                for byte in text.bytes() {
                    h = (h ^ u32::from(byte)).wrapping_mul(0x0100_0193);
                }
                (h % 1000) as f32 / 1000.0
            })
            .collect(),
    )
}

#[async_trait]
impl Embedder for StubEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        serves(params.served_model.as_deref())?;
        self.received.lock().unwrap().push(texts.to_vec());
        Ok(texts.iter().map(|t| hash_vector(t, self.dim)).collect())
    }

    async fn model_identity(
        &self,
        served_model: Option<&str>,
    ) -> Result<ModelIdentity, ComponentError> {
        serves(served_model)?;
        Ok(ModelIdentity::new(IDENTITY))
    }
}

/// A list searched by dot product, replacing by chunk id.
#[derive(Default)]
pub struct StubStore {
    entries: Mutex<Vec<EmbeddedChunk>>,
    pub upserts: Arc<Mutex<Vec<usize>>>,
}

#[async_trait]
impl VectorStore for StubStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        self.upserts.lock().unwrap().push(entries.len());
        let mut stored = self.entries.lock().unwrap();
        for entry in entries {
            stored.retain(|e| e.chunk.id != entry.chunk.id);
            stored.push(entry);
        }
        Ok(())
    }

    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        zero_top_k(params.top_k)?;
        let stored = self.entries.lock().unwrap();
        let mut hits: Vec<ScoredChunk> = stored
            .iter()
            .map(|e| ScoredChunk {
                chunk: e.chunk.clone(),
                score: e
                    .embedding
                    .as_slice()
                    .iter()
                    .zip(embedding.as_slice())
                    .map(|(a, b)| a * b)
                    .sum(),
            })
            .collect();
        by_descending_score(&mut hits);
        hits.truncate(params.top_k);
        Ok(hits)
    }
}
