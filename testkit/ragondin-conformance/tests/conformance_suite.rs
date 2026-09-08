//! The conformance suite exercised against in-suite stubs.
//!
//! Two obligations (#17). A correct stub must pass every family's conformance
//! function, and a **deliberately broken one must fail the check it breaks**.
//! The second half is what proves the suite has teeth: a suite that cannot fail
//! makes INV-7 a slogan again, which is the one thing this crate exists to
//! prevent. So there is a broken stub per check name, not per family, and each
//! test asserts on `failure.check()` — proving the suite *discriminates*, not
//! merely that it fails.
//!
//! The stubs live here rather than in the library because Scope — OUT of #17
//! forbids shipping a component implementation; these exist only to test the
//! harness itself.

use std::sync::Mutex;

use async_trait::async_trait;
use ragondin_conformance::{
    assert_embedder_conformance, assert_fusion_conformance, assert_reranker_conformance,
    assert_retriever_conformance, assert_vector_store_conformance, check_embedder_conformance,
    check_fusion_conformance, check_reranker_conformance, check_retriever_conformance,
    check_vector_store_conformance,
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

fn deduped(inputs: Vec<Vec<ScoredChunk>>) -> Vec<ScoredChunk> {
    let mut kept: Vec<ScoredChunk> = Vec::new();
    for hit in inputs.into_iter().flatten() {
        match kept.iter_mut().find(|k| k.chunk.id == hit.chunk.id) {
            Some(existing) => existing.score = existing.score.max(hit.score),
            None => kept.push(hit),
        }
    }
    kept
}

fn rescored_descending(mut hits: Vec<ScoredChunk>) -> Vec<ScoredChunk> {
    for (i, hit) in hits.iter_mut().enumerate() {
        hit.score = 1.0 - i as f32 / 10.0;
    }
    hits
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

/// Fails a call the contract requires to succeed — a component author's most
/// common first failure, and the only stub that exercises the error path.
struct UnavailableRetriever;

#[async_trait]
impl Retriever for UnavailableRetriever {
    async fn retrieve(
        &self,
        _query: &Query,
        _params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        Err(ComponentError::Unavailable("index not loaded".into()))
    }
}

/// Returns one more result than it was asked for.
struct OverlongRetriever;

#[async_trait]
impl Retriever for OverlongRetriever {
    async fn retrieve(
        &self,
        _query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        Ok(ranked("r", params.top_k + 1))
    }
}

#[tokio::test]
async fn a_conformant_retriever_passes() {
    check_retriever_conformance(|| Box::new(GoodRetriever))
        .await
        .expect("the stub honours the retriever contract");
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

#[tokio::test]
async fn a_retriever_failing_a_well_formed_call_fails() {
    let failure = check_retriever_conformance(|| Box::new(UnavailableRetriever))
        .await
        .expect_err("a well-formed query must not error");
    assert_eq!(failure.check(), "well-formed call succeeds");
    assert!(
        failure.detail().contains("index not loaded"),
        "the component's own message must reach the implementer: {}",
        failure.detail()
    );
}

#[tokio::test]
async fn a_retriever_returning_more_than_top_k_fails() {
    let failure = check_retriever_conformance(|| Box::new(OverlongRetriever))
        .await
        .expect_err("top_k bounds the answer");
    assert_eq!(failure.check(), "top_k respected");
}

#[tokio::test]
#[should_panic(expected = "descending scores")]
async fn the_retriever_assert_wrapper_panics_on_a_broken_component() {
    assert_retriever_conformance(|| Box::new(AscendingRetriever)).await;
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
        let mut fused = deduped(inputs);
        fused.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(fused)
    }
}

/// Invents a chunk no upstream leg produced — but only when there was
/// something to fuse, so the empty-input scenario does not mask it.
struct FabricatingFusion;

#[async_trait]
impl Fusion for FabricatingFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let mut fused = deduped(inputs);
        if fused.is_empty() {
            return Ok(fused);
        }
        fused.push(scored("invented", -1.0));
        fused.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(fused)
    }
}

/// Concatenates its legs without merging them: the characteristic fusion bug,
/// invisible except as a double-counted chunk.
struct ConcatenatingFusion;

#[async_trait]
impl Fusion for ConcatenatingFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let mut fused: Vec<ScoredChunk> = inputs.into_iter().flatten().collect();
        fused.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(fused)
    }
}

/// Re-scores its output descending, but inverts the order it was given.
struct ReversingFusion;

#[async_trait]
impl Fusion for ReversingFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let mut fused = deduped(inputs);
        fused.reverse();
        Ok(rescored_descending(fused))
    }
}

/// Fuses everything to nothing.
struct EmptyFusion;

#[async_trait]
impl Fusion for EmptyFusion {
    async fn fuse(
        &self,
        _inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        Ok(Vec::new())
    }
}

/// Merges correctly but leaves its output ascending.
struct AscendingFusion;

#[async_trait]
impl Fusion for AscendingFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let mut fused = deduped(inputs);
        fused.sort_by(|a, b| a.score.total_cmp(&b.score));
        Ok(fused)
    }
}

/// Answers even when given nothing to fuse. Only the empty-input scenario
/// sees it, which is why that scenario needs a stub of its own.
struct EchoingFusion;

#[async_trait]
impl Fusion for EchoingFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let mut fused = deduped(inputs);
        if fused.is_empty() {
            fused.push(scored("out-of-nowhere", 1.0));
        }
        Ok(rescored_descending(fused))
    }
}

/// Emits one chunk of a single leg twice.
struct DoublingFusion;

#[async_trait]
impl Fusion for DoublingFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let mut fused = deduped(inputs);
        if let Some(first) = fused.first().cloned() {
            fused.insert(1, first);
        }
        Ok(rescored_descending(fused))
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

#[tokio::test]
async fn a_fusion_that_does_not_merge_overlapping_legs_fails() {
    let failure = check_fusion_conformance(|| Box::new(ConcatenatingFusion))
        .await
        .expect_err("a chunk two legs both returned must come back once");
    assert_eq!(failure.check(), "no duplicate ids");
}

#[tokio::test]
async fn a_fusion_reordering_a_single_leg_fails() {
    let failure = check_fusion_conformance(|| Box::new(ReversingFusion))
        .await
        .expect_err("re-scoring may change the scores, not the order");
    assert_eq!(failure.check(), "order preserved");
}

#[tokio::test]
async fn a_fusion_returning_nothing_fails() {
    let failure = check_fusion_conformance(|| Box::new(EmptyFusion))
        .await
        .expect_err("a fusion sees its whole input; it cannot have nothing to say");
    assert_eq!(failure.check(), "non-empty input yields output");
}

#[tokio::test]
async fn a_fusion_returning_ascending_scores_fails() {
    let failure = check_fusion_conformance(|| Box::new(AscendingFusion))
        .await
        .expect_err("the ranking contract binds every family that ranks");
    assert_eq!(failure.check(), "descending scores");
}

#[tokio::test]
async fn a_fusion_answering_an_empty_input_fails() {
    let failure = check_fusion_conformance(|| Box::new(EchoingFusion))
        .await
        .expect_err("with nothing offered, any answer is invented");
    assert_eq!(failure.check(), "no fabricated ids");
}

#[tokio::test]
async fn a_fusion_repeating_a_chunk_of_one_leg_fails() {
    let failure = check_fusion_conformance(|| Box::new(DoublingFusion))
        .await
        .expect_err("a ranked list ranks each chunk once");
    assert_eq!(failure.check(), "no duplicate ids");
}

#[tokio::test]
#[should_panic(expected = "no fabricated ids")]
async fn the_fusion_assert_wrapper_panics_on_a_broken_component() {
    assert_fusion_conformance(|| Box::new(FabricatingFusion)).await;
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
        Ok(rescored_descending(chunks))
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
        if chunks.is_empty() {
            return Ok(chunks);
        }
        let mut out = vec![scored("hallucinated", 1.0)];
        out.extend(chunks);
        out.truncate(params.top_k);
        Ok(rescored_descending(out))
    }
}

/// Returns one candidate twice.
struct DuplicatingReranker;

#[async_trait]
impl Reranker for DuplicatingReranker {
    async fn rerank(
        &self,
        _query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        let Some(first) = chunks.first().cloned() else {
            return Ok(chunks);
        };
        let mut out = vec![first.clone(), first];
        out.truncate(params.top_k);
        Ok(rescored_descending(out))
    }
}

/// Reorders correctly but leaves the scores ascending.
struct AscendingReranker;

#[async_trait]
impl Reranker for AscendingReranker {
    async fn rerank(
        &self,
        _query: &Query,
        mut chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        chunks.truncate(params.top_k);
        let mut out = rescored_descending(chunks);
        out.reverse();
        Ok(out)
    }
}

/// Treats a zero `top_k` as a request for nothing.
struct ZeroTopKReranker;

#[async_trait]
impl Reranker for ZeroTopKReranker {
    async fn rerank(
        &self,
        _query: &Query,
        mut chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        chunks.truncate(params.top_k);
        Ok(rescored_descending(chunks))
    }
}

/// Keeps every candidate, whatever `top_k` says.
struct OverlongReranker;

#[async_trait]
impl Reranker for OverlongReranker {
    async fn rerank(
        &self,
        _query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        Ok(rescored_descending(chunks))
    }
}

/// Answers even when given no candidates.
struct EchoingReranker;

#[async_trait]
impl Reranker for EchoingReranker {
    async fn rerank(
        &self,
        _query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        let mut out = chunks;
        if out.is_empty() {
            out.push(scored("out-of-nowhere", 1.0));
        }
        out.truncate(params.top_k);
        Ok(rescored_descending(out))
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

#[tokio::test]
async fn a_reranker_returning_a_candidate_twice_fails() {
    let failure = check_reranker_conformance(|| Box::new(DuplicatingReranker))
        .await
        .expect_err("a ranked list ranks each chunk once");
    assert_eq!(failure.check(), "no duplicate ids");
}

#[tokio::test]
async fn a_reranker_returning_ascending_scores_fails() {
    let failure = check_reranker_conformance(|| Box::new(AscendingReranker))
        .await
        .expect_err("the ranking contract binds every family that ranks");
    assert_eq!(failure.check(), "descending scores");
}

#[tokio::test]
async fn a_reranker_accepting_a_zero_top_k_fails() {
    let failure = check_reranker_conformance(|| Box::new(ZeroTopKReranker))
        .await
        .expect_err("a zero top_k is an unmet precondition");
    assert_eq!(failure.check(), "zero top_k rejected");
}

#[tokio::test]
async fn a_reranker_returning_more_than_top_k_fails() {
    let failure = check_reranker_conformance(|| Box::new(OverlongReranker))
        .await
        .expect_err("top_k bounds the answer");
    assert_eq!(failure.check(), "top_k respected");
}

#[tokio::test]
async fn a_reranker_answering_an_empty_candidate_list_fails() {
    let failure = check_reranker_conformance(|| Box::new(EchoingReranker))
        .await
        .expect_err("with no candidates, any answer is invented");
    assert_eq!(failure.check(), "no fabricated ids");
}

#[tokio::test]
#[should_panic(expected = "no fabricated ids")]
async fn the_reranker_assert_wrapper_panics_on_a_broken_component() {
    assert_reranker_conformance(|| Box::new(FabricatingReranker)).await;
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

/// Emits a non-finite component, which serializes without error and cannot be
/// read back.
struct NanEmbedder;

#[async_trait]
impl Embedder for NanEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        _params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        Ok(texts
            .iter()
            .map(|t| Embedding::new(vec![t.len() as f32, f32::NAN, 0.0]))
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

#[tokio::test]
async fn an_embedder_emitting_a_nan_component_fails() {
    let failure = check_embedder_conformance(|| Box::new(NanEmbedder))
        .await
        .expect_err("a non-finite component cannot be read back");
    assert_eq!(failure.check(), "finite components");
}

#[tokio::test]
#[should_panic(expected = "one vector per input")]
async fn the_embedder_assert_wrapper_panics_on_a_broken_component() {
    assert_embedder_conformance(|| Box::new(DroppingEmbedder)).await;
}

// -------------------------------------------------------------- VectorStore

fn dot(a: &Embedding, b: &Embedding) -> f32 {
    a.as_slice()
        .iter()
        .zip(b.as_slice())
        .map(|(x, y)| x * y)
        .sum()
}

fn euclidean(a: &Embedding, b: &Embedding) -> f32 {
    a.as_slice()
        .iter()
        .zip(b.as_slice())
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// Brute-force similarity search: descending by dot product, truncated.
fn by_similarity(
    entries: &[EmbeddedChunk],
    embedding: &Embedding,
    top_k: usize,
) -> Vec<ScoredChunk> {
    let mut hits: Vec<ScoredChunk> = entries
        .iter()
        .map(|entry| ScoredChunk {
            chunk: entry.chunk.clone(),
            score: dot(embedding, &entry.embedding),
        })
        .collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits.truncate(top_k);
    hits
}

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

    fn held(&self) -> std::sync::MutexGuard<'_, Vec<EmbeddedChunk>> {
        self.entries.lock().expect("the stub is never poisoned")
    }
}

#[async_trait]
impl VectorStore for GoodStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        let mut held = self.held();
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
        Ok(by_similarity(&self.held(), embedding, params.top_k))
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

/// Scores by L2 **distance**, ascending — what Qdrant, FAISS and pgvector
/// return natively, and a ranked list read upside down by every metric.
struct DistanceScoredStore {
    inner: GoodStore,
}

#[async_trait]
impl VectorStore for DistanceScoredStore {
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
        let held = self.inner.held();
        let mut hits: Vec<ScoredChunk> = held
            .iter()
            .map(|entry| ScoredChunk {
                chunk: entry.chunk.clone(),
                score: euclidean(embedding, &entry.embedding),
            })
            .collect();
        hits.sort_by(|a, b| a.score.total_cmp(&b.score));
        hits.truncate(params.top_k);
        Ok(hits)
    }
}

/// Answers every query with whatever it stored first.
struct AlwaysFirstStore {
    inner: GoodStore,
}

#[async_trait]
impl VectorStore for AlwaysFirstStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        self.inner.upsert(entries).await
    }

    async fn search(
        &self,
        _embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        Ok(self
            .inner
            .held()
            .first()
            .map(|entry| {
                vec![ScoredChunk {
                    chunk: entry.chunk.clone(),
                    score: 1.0,
                }]
            })
            .unwrap_or_default())
    }
}

/// Keeps every version of a chunk instead of replacing it by id.
struct AppendingStore {
    inner: GoodStore,
}

#[async_trait]
impl VectorStore for AppendingStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        self.inner.held().extend(entries);
        Ok(())
    }

    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        self.inner.search(embedding, params).await
    }
}

/// Inserts only what it has never seen — `ON CONFLICT DO NOTHING`, which
/// serves the vectors of a corpus that was re-indexed to fix them.
struct InsertIfAbsentStore {
    inner: GoodStore,
}

#[async_trait]
impl VectorStore for InsertIfAbsentStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        let mut held = self.inner.held();
        for entry in entries {
            if !held.iter().any(|e| e.chunk.id == entry.chunk.id) {
                held.push(entry);
            }
        }
        Ok(())
    }

    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        self.inner.search(embedding, params).await
    }
}

/// Hands back a store that is not the suite's to write to.
struct SharedStore {
    inner: GoodStore,
}

impl SharedStore {
    fn new() -> Self {
        let inner = GoodStore::new();
        inner.held().push(EmbeddedChunk {
            chunk: chunk("someone-elses-corpus"),
            embedding: Embedding::new(vec![9.0; DIM]),
        });
        Self { inner }
    }
}

#[async_trait]
impl VectorStore for SharedStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        self.inner.upsert(entries).await
    }

    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        self.inner.search(embedding, params).await
    }
}

/// Treats a zero `top_k` as a request for nothing.
struct ZeroTopKStore {
    inner: GoodStore,
}

#[async_trait]
impl VectorStore for ZeroTopKStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        self.inner.upsert(entries).await
    }

    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Ok(Vec::new());
        }
        self.inner.search(embedding, params).await
    }
}

/// Stores happily and finds nothing — a search that was never wired.
struct SilentStore {
    inner: GoodStore,
}

#[async_trait]
impl VectorStore for SilentStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        self.inner.upsert(entries).await
    }

    async fn search(
        &self,
        _embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        Ok(Vec::new())
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
async fn a_vector_store_scoring_by_distance_fails() {
    // The regression test for the suite's own blind spot: with `top_k = 1`
    // every ranked list is trivially ordered, so this store passed until the
    // suite searched for several hits at once.
    let failure = check_vector_store_conformance(
        || {
            Box::new(DistanceScoredStore {
                inner: GoodStore::new(),
            })
        },
        DIM,
    )
    .await
    .expect_err("lower-is-better is not the ranking contract");
    assert_eq!(failure.check(), "descending scores");
}

#[tokio::test]
async fn a_vector_store_ignoring_the_query_vector_fails() {
    let failure = check_vector_store_conformance(
        || {
            Box::new(AlwaysFirstStore {
                inner: GoodStore::new(),
            })
        },
        DIM,
    )
    .await
    .expect_err("a store that ignores the query is not a vector store");
    assert_eq!(failure.check(), "nearest neighbour is itself");
}

#[tokio::test]
async fn a_vector_store_appending_instead_of_replacing_fails() {
    let failure = check_vector_store_conformance(
        || {
            Box::new(AppendingStore {
                inner: GoodStore::new(),
            })
        },
        DIM,
    )
    .await
    .expect_err("upsert is keyed by chunk id");
    assert_eq!(failure.check(), "upsert replaces by id");
}

#[tokio::test]
async fn a_vector_store_discarding_an_update_fails() {
    let failure = check_vector_store_conformance(
        || {
            Box::new(InsertIfAbsentStore {
                inner: GoodStore::new(),
            })
        },
        DIM,
    )
    .await
    .expect_err("a re-inserted chunk must carry its new content");
    assert_eq!(failure.check(), "upsert replaces by id");
    assert!(
        failure.detail().contains("first inserted"),
        "the diagnosis must distinguish a stale entry from a duplicated one: {}",
        failure.detail()
    );
}

#[tokio::test]
async fn a_vector_store_the_suite_does_not_own_fails() {
    let failure = check_vector_store_conformance(|| Box::new(SharedStore::new()), DIM)
        .await
        .expect_err("the suite must own the store it writes to");
    assert_eq!(failure.check(), "empty store yields no results");
}

#[tokio::test]
async fn a_zero_dimensionality_is_refused_rather_than_silently_checked() {
    let failure = check_vector_store_conformance(|| Box::new(GoodStore::new()), 0)
        .await
        .expect_err("a zero-dimensional store cannot be exercised");
    assert_eq!(failure.check(), "dimensionality");
}

#[tokio::test]
async fn a_vector_store_accepting_a_zero_top_k_fails() {
    let failure = check_vector_store_conformance(
        || {
            Box::new(ZeroTopKStore {
                inner: GoodStore::new(),
            })
        },
        DIM,
    )
    .await
    .expect_err("a zero top_k is an unmet precondition");
    assert_eq!(failure.check(), "zero top_k rejected");
}

#[tokio::test]
async fn a_vector_store_that_finds_nothing_fails() {
    let failure = check_vector_store_conformance(
        || {
            Box::new(SilentStore {
                inner: GoodStore::new(),
            })
        },
        DIM,
    )
    .await
    .expect_err("an inserted vector must be findable");
    assert_eq!(failure.check(), "nearest neighbour is itself");
    assert!(
        failure.detail().contains("returned nothing"),
        "the diagnosis must distinguish nothing from the wrong neighbour: {}",
        failure.detail()
    );
}

#[tokio::test]
#[should_panic(expected = "top_k respected")]
async fn the_vector_store_assert_wrapper_panics_on_a_broken_component() {
    assert_vector_store_conformance(
        || {
            Box::new(OverlongStore {
                inner: GoodStore::new(),
            })
        },
        DIM,
    )
    .await;
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
