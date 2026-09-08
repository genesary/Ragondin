//! The conformance suite exercised against in-suite stubs.
//!
//! Two obligations (#17). A correct stub must pass every family's conformance
//! function, and a **deliberately broken one must fail the check it breaks**.
//! The second half is what proves the suite has teeth: a suite that cannot fail
//! makes INV-7 a slogan again, which is the one thing this crate exists to
//! prevent.
//!
//! The stubs live here rather than in the library because Scope — OUT of #17
//! forbids shipping a component implementation; these exist only to test the
//! harness itself.

use std::sync::Mutex;

use async_trait::async_trait;
use ragondin_conformance::{
    assert_retriever_conformance, check_embedder_conformance, check_fusion_conformance,
    check_reranker_conformance, check_retriever_conformance, check_vector_store_conformance,
};
use ragondin_contracts::{
    ComponentError, EmbedParams, EmbeddedChunk, Embedder, Fusion, FusionParams, RerankParams,
    Reranker, RetrieveParams, Retriever, SearchParams, VectorStore,
};
use ragondin_types::{Chunk, ChunkId, DocId, Embedding, Query, ScoredChunk};

const DIM: usize = 3;

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

/// A ranked list of `n` chunks, descending and finite — the shape every
/// conformant component returns.
fn ranked(prefix: &str, n: usize) -> Vec<ScoredChunk> {
    (0..n)
        .map(|i| scored(&format!("{prefix}-{i}"), 1.0 - i as f32 / 10.0))
        .collect()
}

// ---------------------------------------------------------------- Retriever

struct GoodRetriever;

#[async_trait]
impl Retriever for GoodRetriever {
    async fn retrieve(
        &self,
        _query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        Ok(ranked("r", params.top_k.min(3)))
    }
}

/// Returns its results ascending — the failure nDCG@k reports as a bad number
/// rather than as an error.
struct AscendingRetriever;

#[async_trait]
impl Retriever for AscendingRetriever {
    async fn retrieve(
        &self,
        _query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        let mut hits = ranked("r", params.top_k.min(3));
        hits.reverse();
        Ok(hits)
    }
}

/// Scores one hit `NaN` — the value on which every implementer's
/// `partial_cmp(…).unwrap()` panics downstream.
struct NanRetriever;

#[async_trait]
impl Retriever for NanRetriever {
    async fn retrieve(
        &self,
        _query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        Ok(vec![scored("r-0", f32::NAN)])
    }
}

/// Answers a zero `top_k` with an empty list where the contract requires
/// `InvalidRequest`.
struct ZeroTopKRetriever;

#[async_trait]
impl Retriever for ZeroTopKRetriever {
    async fn retrieve(
        &self,
        _query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        Ok(ranked("r", params.top_k.min(3)))
    }
}

#[tokio::test]
async fn a_conformant_retriever_passes() {
    check_retriever_conformance(|| Box::new(GoodRetriever))
        .await
        .expect("the stub honours the retriever contract");
}

#[tokio::test]
async fn the_assert_wrapper_accepts_a_conformant_retriever() {
    assert_retriever_conformance(|| Box::new(GoodRetriever)).await;
}

#[tokio::test]
async fn a_retriever_returning_ascending_scores_fails() {
    let failure = check_retriever_conformance(|| Box::new(AscendingRetriever))
        .await
        .expect_err("an ascending ranked list is not a ranked list");
    assert_eq!(failure.check(), "descending scores");
    assert_eq!(failure.component(), "Retriever");
}

#[tokio::test]
async fn a_retriever_returning_a_nan_score_fails() {
    let failure = check_retriever_conformance(|| Box::new(NanRetriever))
        .await
        .expect_err("NaN is not a score");
    assert_eq!(failure.check(), "finite scores");
}

#[tokio::test]
async fn a_retriever_accepting_a_zero_top_k_fails() {
    let failure = check_retriever_conformance(|| Box::new(ZeroTopKRetriever))
        .await
        .expect_err("a zero top_k is an unmet precondition, not an empty answer");
    assert_eq!(failure.check(), "zero top_k rejected");
}

// ------------------------------------------------------------------- Fusion

struct GoodFusion;

#[async_trait]
impl Fusion for GoodFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let mut fused: Vec<ScoredChunk> = Vec::new();
        for hit in inputs.into_iter().flatten() {
            match fused.iter_mut().find(|kept| kept.chunk.id == hit.chunk.id) {
                Some(kept) => kept.score = kept.score.max(hit.score),
                None => fused.push(hit),
            }
        }
        fused.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(fused)
    }
}

/// Invents a chunk no upstream leg produced.
struct FabricatingFusion;

#[async_trait]
impl Fusion for FabricatingFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let mut fused: Vec<ScoredChunk> = inputs.into_iter().flatten().collect();
        fused.push(scored("invented", -1.0));
        fused.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(fused)
    }
}

#[tokio::test]
async fn a_conformant_fusion_passes() {
    check_fusion_conformance(|| Box::new(GoodFusion))
        .await
        .expect("the stub honours the fusion contract");
}

#[tokio::test]
async fn a_fusion_inventing_a_chunk_fails() {
    let failure = check_fusion_conformance(|| Box::new(FabricatingFusion))
        .await
        .expect_err("a fusion merges, it does not create");
    assert_eq!(failure.check(), "no fabricated ids");
    assert_eq!(failure.component(), "Fusion");
}

// ----------------------------------------------------------------- Reranker

struct GoodReranker;

#[async_trait]
impl Reranker for GoodReranker {
    async fn rerank(
        &self,
        _query: &Query,
        mut chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        chunks.sort_by(|a, b| b.chunk.id.as_str().cmp(a.chunk.id.as_str()));
        chunks.truncate(params.top_k);
        for (i, hit) in chunks.iter_mut().enumerate() {
            hit.score = 1.0 - i as f32 / 10.0;
        }
        Ok(chunks)
    }
}

/// The issue's own example of a broken component: a reranker returning a chunk
/// id it was never given.
struct FabricatingReranker;

#[async_trait]
impl Reranker for FabricatingReranker {
    async fn rerank(
        &self,
        _query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        let mut out = vec![scored("hallucinated", 1.0)];
        out.extend(chunks.into_iter().map(|mut hit| {
            hit.score = 0.5;
            hit
        }));
        out.truncate(params.top_k);
        Ok(out)
    }
}

#[tokio::test]
async fn a_conformant_reranker_passes() {
    check_reranker_conformance(|| Box::new(GoodReranker))
        .await
        .expect("the stub honours the reranker contract");
}

#[tokio::test]
async fn a_reranker_fabricating_a_chunk_id_fails() {
    let failure = check_reranker_conformance(|| Box::new(FabricatingReranker))
        .await
        .expect_err("a reranker reorders, it does not invent");
    assert_eq!(failure.check(), "no fabricated ids");
    assert_eq!(failure.component(), "Reranker");
}

// ----------------------------------------------------------------- Embedder

struct GoodEmbedder;

#[async_trait]
impl Embedder for GoodEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        _params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        Ok(texts
            .iter()
            .map(|t| Embedding::new(vec![t.len() as f32, 1.0, 0.0]))
            .collect())
    }
}

/// Drops an input — the shape that silently misaligns a corpus from its index.
struct DroppingEmbedder;

#[async_trait]
impl Embedder for DroppingEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        _params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        Ok(texts
            .iter()
            .skip(1)
            .map(|t| Embedding::new(vec![t.len() as f32, 1.0, 0.0]))
            .collect())
    }
}

/// Varies dimensionality within one batch.
struct RaggedEmbedder;

#[async_trait]
impl Embedder for RaggedEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        _params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        Ok(texts
            .iter()
            .enumerate()
            .map(|(i, t)| Embedding::new(vec![t.len() as f32; i + 1]))
            .collect())
    }
}

#[tokio::test]
async fn a_conformant_embedder_passes() {
    check_embedder_conformance(|| Box::new(GoodEmbedder))
        .await
        .expect("the stub honours the embedder contract");
}

#[tokio::test]
async fn an_embedder_dropping_an_input_fails() {
    let failure = check_embedder_conformance(|| Box::new(DroppingEmbedder))
        .await
        .expect_err("one vector per input, or the corpus and the index disagree");
    assert_eq!(failure.check(), "one vector per input");
    assert_eq!(failure.component(), "Embedder");
}

#[tokio::test]
async fn an_embedder_with_a_ragged_batch_fails() {
    let failure = check_embedder_conformance(|| Box::new(RaggedEmbedder))
        .await
        .expect_err("a batch has one dimensionality");
    assert_eq!(failure.check(), "constant dimensionality");
}

// -------------------------------------------------------------- VectorStore

/// A brute-force in-memory store: the minimum that can hold a vector and give
/// it back. `Mutex` because `upsert` takes `&self` — the interior mutability
/// the contract requires of implementers.
struct GoodStore {
    entries: Mutex<Vec<EmbeddedChunk>>,
}

impl GoodStore {
    fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }
}

fn dot(a: &Embedding, b: &Embedding) -> f32 {
    a.as_slice()
        .iter()
        .zip(b.as_slice())
        .map(|(x, y)| x * y)
        .sum()
}

#[async_trait]
impl VectorStore for GoodStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        let mut held = self.entries.lock().expect("the stub is never poisoned");
        for entry in entries {
            match held.iter_mut().find(|e| e.chunk.id == entry.chunk.id) {
                Some(existing) => *existing = entry,
                None => held.push(entry),
            }
        }
        Ok(())
    }

    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        let held = self.entries.lock().expect("the stub is never poisoned");
        let mut hits: Vec<ScoredChunk> = held
            .iter()
            .map(|entry| ScoredChunk {
                chunk: entry.chunk.clone(),
                score: dot(embedding, &entry.embedding),
            })
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(params.top_k);
        Ok(hits)
    }
}

/// Ignores `top_k`.
struct OverlongStore {
    inner: GoodStore,
}

#[async_trait]
impl VectorStore for OverlongStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        self.inner.upsert(entries).await
    }

    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        self.inner
            .search(embedding, &SearchParams::new(usize::MAX))
            .await
    }
}

/// Keeps every version of a chunk instead of replacing it by id.
struct AppendingStore {
    entries: Mutex<Vec<EmbeddedChunk>>,
}

#[async_trait]
impl VectorStore for AppendingStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        self.entries
            .lock()
            .expect("the stub is never poisoned")
            .extend(entries);
        Ok(())
    }

    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        let held = self.entries.lock().expect("the stub is never poisoned");
        let mut hits: Vec<ScoredChunk> = held
            .iter()
            .map(|entry| ScoredChunk {
                chunk: entry.chunk.clone(),
                score: dot(embedding, &entry.embedding),
            })
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(params.top_k);
        Ok(hits)
    }
}

#[tokio::test]
async fn a_conformant_vector_store_passes() {
    check_vector_store_conformance(|| Box::new(GoodStore::new()), DIM)
        .await
        .expect("the stub honours the vector store contract");
}

#[tokio::test]
async fn a_vector_store_ignoring_top_k_fails() {
    let failure = check_vector_store_conformance(
        || {
            Box::new(OverlongStore {
                inner: GoodStore::new(),
            })
        },
        DIM,
    )
    .await
    .expect_err("top_k bounds the answer");
    assert_eq!(failure.check(), "top_k respected");
    assert_eq!(failure.component(), "VectorStore");
}

#[tokio::test]
async fn a_vector_store_appending_instead_of_replacing_fails() {
    let failure = check_vector_store_conformance(
        || {
            Box::new(AppendingStore {
                entries: Mutex::new(Vec::new()),
            })
        },
        DIM,
    )
    .await
    .expect_err("upsert is keyed by chunk id");
    assert_eq!(failure.check(), "upsert replaces by id");
}

#[tokio::test]
async fn a_zero_dimensionality_is_refused_rather_than_silently_checked() {
    let failure = check_vector_store_conformance(|| Box::new(GoodStore::new()), 0)
        .await
        .expect_err("a zero-dimensional store cannot be exercised");
    assert_eq!(failure.check(), "dimensionality");
}

// ------------------------------------------------------- the failure's shape

#[tokio::test]
async fn a_failure_names_the_component_the_check_and_the_detail() {
    let failure = check_retriever_conformance(|| Box::new(AscendingRetriever))
        .await
        .expect_err("the stub is not conformant");
    let rendered = failure.to_string();
    assert!(
        rendered.contains("Retriever") && rendered.contains("descending scores"),
        "a failure must name the component and the check it broke: {rendered}"
    );
    assert!(
        !failure.detail().is_empty(),
        "a failure must say what was observed, not only which check broke"
    );
}
