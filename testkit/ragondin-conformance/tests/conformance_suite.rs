//! The conformance suite exercised against in-suite stubs.
//!
//! Two obligations on the suite itself. A correct stub must pass every
//! family's conformance function, and a **deliberately broken one must fail
//! the check it breaks**.
//! The second half is what proves the suite has teeth: a suite that cannot fail
//! makes INV-7 a slogan again, which is the one thing this crate exists to
//! prevent. So there is a broken stub per check name, not per family, and each
//! test asserts on `failure.check()` — proving the suite *discriminates*, not
//! merely that it fails.
//!
//! The stubs live here rather than in the library because Scope — OUT of #17
//! forbids shipping a component implementation; these exist only to test the
//! harness itself.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use ragondin_conformance::{
    assert_context_builder_conformance, assert_embedder_conformance, assert_fusion_conformance,
    assert_generator_conformance, assert_reranker_conformance, assert_retriever_conformance,
    assert_vector_store_conformance, check_context_builder_conformance, check_embedder_conformance,
    check_fusion_conformance, check_generator_conformance, check_reranker_conformance,
    check_retriever_conformance, check_vector_store_conformance, RolePrefixes,
};
use ragondin_contracts::{
    ComponentError, ContextBuilder, ContextParams, EmbedParams, EmbedRole, EmbeddedChunk, Embedder,
    Fusion, FusionParams, GenerateParams, Generator, RerankParams, Reranker, RetrieveParams,
    Retriever, SearchParams, VectorStore,
};
use ragondin_types::{
    Answer, Chunk, ChunkId, Context, ContextChunk, DocId, Embedding, ModelIdentity, Query,
    ScoredChunk,
};

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

/// Prefixes each role distinctly, the way E5 or BGE is configured to: the
/// shape a fixture declaring `RolePrefixes::Distinct` must have.
struct RoleAwareEmbedder;

#[async_trait]
impl Embedder for RoleAwareEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        let prefix = match params.role {
            EmbedRole::Query => 1.0,
            EmbedRole::Passage => 2.0,
        };
        Ok(texts
            .iter()
            .map(|t| Embedding::new(vec![t.len() as f32, prefix, 0.0]))
            .collect())
    }
}

/// Accepts the role and drops it — the likeliest bug in an asymmetric
/// embedder, and the one no other check can see: every vector it returns is
/// well-formed.
struct RoleIgnoringEmbedder;

#[async_trait]
impl Embedder for RoleIgnoringEmbedder {
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

/// Internally consistent under each role and yet unusable: 3 components under
/// `Query`, 2 under `Passage`. Every per-batch check passes — each batch is
/// homogeneous and finite — so only a comparison *across* the roles sees it.
///
/// This is the shape a real embedder takes when the two roles reach different
/// model sessions, and it is the one the role-separation check cannot catch:
/// vectors of unequal dimensionality are trivially unequal, so declaring
/// `Distinct` would *pass* it. Left `Undeclared` in the test below so the
/// failure has to come from the dimensionality check itself.
struct SplitDimensionEmbedder;

#[async_trait]
impl Embedder for SplitDimensionEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        let dim = match params.role {
            EmbedRole::Query => 3,
            EmbedRole::Passage => 2,
        };
        Ok(texts
            .iter()
            .map(|t| Embedding::new(vec![t.len() as f32; dim]))
            .collect())
    }
}

/// Conformant under `Query`, and drops an input under `Passage`. The suite
/// sees it only because every check runs under **both** roles: an embedder
/// that takes a different path per role can be broken on one side alone, and
/// the passage side is the corpus side, where a dropped vector misaligns an
/// index with no error anywhere.
struct PassageDroppingEmbedder;

#[async_trait]
impl Embedder for PassageDroppingEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        let dropped = match params.role {
            EmbedRole::Query => 0,
            EmbedRole::Passage => 1,
        };
        Ok(texts
            .iter()
            .skip(dropped)
            .map(|t| Embedding::new(vec![t.len() as f32, 1.0, 0.0]))
            .collect())
    }
}

#[tokio::test]
async fn a_conformant_embedder_passes() {
    check_embedder_conformance(|| Box::new(GoodEmbedder), RolePrefixes::Undeclared)
        .await
        .expect("the stub honours the embedder contract");
}

#[tokio::test]
async fn an_embedder_dropping_an_input_fails() {
    let failure =
        check_embedder_conformance(|| Box::new(DroppingEmbedder), RolePrefixes::Undeclared)
            .await
            .expect_err("one vector per input, or the corpus and the index disagree");
    assert_eq!(failure.check(), "one vector per input");
    assert_eq!(failure.component(), "Embedder");
}

#[tokio::test]
async fn an_embedder_with_a_ragged_batch_fails() {
    let failure = check_embedder_conformance(|| Box::new(RaggedEmbedder), RolePrefixes::Undeclared)
        .await
        .expect_err("a batch has one dimensionality");
    assert_eq!(failure.check(), "constant dimensionality");
}

#[tokio::test]
async fn an_embedder_emitting_a_nan_component_fails() {
    let failure = check_embedder_conformance(|| Box::new(NanEmbedder), RolePrefixes::Undeclared)
        .await
        .expect_err("a non-finite component cannot be read back");
    assert_eq!(failure.check(), "finite components");
}

#[tokio::test]
async fn an_embedder_with_declared_prefixes_may_separate_the_roles() {
    check_embedder_conformance(|| Box::new(RoleAwareEmbedder), RolePrefixes::Distinct)
        .await
        .expect("distinct prefixes produce distinct vectors, as declared");
}

#[tokio::test]
async fn an_embedder_ignoring_a_declared_role_fails() {
    let failure =
        check_embedder_conformance(|| Box::new(RoleIgnoringEmbedder), RolePrefixes::Distinct)
            .await
            .expect_err("a fixture declaring distinct prefixes must not answer both roles alike");
    assert_eq!(failure.check(), "role changes the vector");
    assert_eq!(failure.component(), "Embedder");
}

#[tokio::test]
async fn an_embedder_broken_only_under_passage_fails() {
    // The claim `check_embedder_conformance` makes four times over — every
    // check runs under both roles — is otherwise untested: every other broken
    // stub here breaks under `Query` too, so the `Passage` pass proves nothing
    // about them. Reduce the suite's loop to `[EmbedRole::Query]` and this is
    // the test that goes red. `Undeclared` on purpose: the failure must come
    // from the ordinary per-role checks, not from the opt-in role-separation
    // one.
    let failure = check_embedder_conformance(
        || Box::new(PassageDroppingEmbedder),
        RolePrefixes::Undeclared,
    )
    .await
    .expect_err("a contract broken on one side only is still broken");
    assert_eq!(failure.check(), "one vector per input");
    assert_eq!(failure.component(), "Embedder");
}

#[tokio::test]
async fn an_embedder_whose_dimensionality_depends_on_the_role_fails() {
    // A query vector is dot-producted against a passage vector by
    // construction, so one dimensionality per role is one embedding space per
    // role, and there is no retrieval to be had. Nothing sees it inside a
    // batch — each is homogeneous — and the role-separation check would pass
    // it, since unequal dimensionalities are unequal. Only carrying the
    // dimensionality across the role loop catches it.
    let failure = check_embedder_conformance(
        || Box::new(SplitDimensionEmbedder),
        RolePrefixes::Undeclared,
    )
    .await
    .expect_err("an embedder has one dimensionality, not one per role");
    assert_eq!(failure.check(), "constant dimensionality");
    assert_eq!(failure.component(), "Embedder");
}

#[tokio::test]
async fn an_embedder_ignoring_an_undeclared_role_conforms() {
    // A symmetric model is correct, and the suite does not know which it is
    // holding: without a declaration there is nothing to compare against.
    check_embedder_conformance(|| Box::new(RoleIgnoringEmbedder), RolePrefixes::Undeclared)
        .await
        .expect("an undeclared fixture is never asked to separate the roles");
}

#[tokio::test]
#[should_panic(expected = "one vector per input")]
async fn the_embedder_assert_wrapper_panics_on_a_broken_component() {
    assert_embedder_conformance(|| Box::new(DroppingEmbedder), RolePrefixes::Undeclared).await;
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

// ----------------------------------------------------------- ContextBuilder

/// What a stub builder gets wrong, if anything. One stub with a named flaw
/// rather than a struct per flaw: every flaw is a one-line departure from the
/// same conformant builder, and a reader should see only the departure.
enum BuilderFlaw {
    None,
    /// Places a chunk it was never handed ahead of the real ones.
    Fabricates,
    /// Answers zero chunks with a context holding one.
    FillsAnEmptyInput,
    /// Places its first chunk twice.
    Duplicates,
    /// Answers a zero budget with the empty context.
    AcceptsAZeroBudget,
    /// Refuses a zero budget, but as a failure of its own rather than the
    /// caller's.
    RefusesAZeroBudgetAsUnavailable,
    /// Treats zero chunks as an invalid request, against ADR-C19.
    RefusesZeroChunks,
    /// Fails every build.
    RefusesEverything,
    /// Reports the empty identity.
    EmptyIdentity,
    /// Reports an identity that changes on every call.
    CountingIdentity(AtomicUsize),
    /// Reports an identity that names the instance, so two instances built by
    /// one constructor disagree.
    InstanceIdentity(usize),
    /// Cannot report an identity at all.
    FailingIdentity,
}

/// A builder whose budget counts chunks — its own unit, as the contract
/// leaves it to be — and which renders each chunk's text on a line.
struct StubBuilder(BuilderFlaw);

#[async_trait]
impl ContextBuilder for StubBuilder {
    async fn build(
        &self,
        _query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &ContextParams,
    ) -> Result<Context, ComponentError> {
        let flaw = &self.0;
        if matches!(flaw, BuilderFlaw::RefusesEverything) {
            return Err(ComponentError::Unavailable("builder down".into()));
        }
        if params.budget == 0 {
            return match flaw {
                BuilderFlaw::AcceptsAZeroBudget => Ok(Context {
                    chunks: Vec::new(),
                    text: String::new(),
                }),
                BuilderFlaw::RefusesAZeroBudgetAsUnavailable => {
                    Err(ComponentError::Unavailable("no room".into()))
                }
                _ => Err(ComponentError::InvalidRequest("a zero budget".into())),
            };
        }
        if chunks.is_empty() {
            match flaw {
                BuilderFlaw::RefusesZeroChunks => {
                    return Err(ComponentError::InvalidRequest("nothing to build".into()))
                }
                BuilderFlaw::FillsAnEmptyInput => {
                    return Ok(Context {
                        chunks: vec![context_chunk(&scored("out-of-nowhere", 1.0))],
                        text: "a passage from nowhere".to_string(),
                    })
                }
                _ => {}
            }
        }
        let kept: Vec<ScoredChunk> = chunks.into_iter().take(params.budget).collect();
        let mut placed: Vec<ContextChunk> = kept.iter().map(context_chunk).collect();
        match flaw {
            BuilderFlaw::Fabricates if !placed.is_empty() => {
                placed.insert(0, context_chunk(&scored("hallucinated", 1.0)));
            }
            BuilderFlaw::Duplicates if !placed.is_empty() => placed.push(placed[0].clone()),
            _ => {}
        }
        Ok(Context {
            text: kept
                .iter()
                .map(|hit| hit.chunk.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            chunks: placed,
        })
    }

    async fn model_identity(&self) -> Result<ModelIdentity, ComponentError> {
        match &self.0 {
            BuilderFlaw::EmptyIdentity => Ok(ModelIdentity::new("")),
            BuilderFlaw::CountingIdentity(calls) => Ok(ModelIdentity::new(format!(
                "stub-builder:chunks#{}",
                calls.fetch_add(1, Ordering::SeqCst)
            ))),
            BuilderFlaw::InstanceIdentity(instance) => Ok(ModelIdentity::new(format!(
                "stub-builder:chunks/instance-{instance}"
            ))),
            BuilderFlaw::FailingIdentity => {
                Err(ComponentError::Unavailable("identity unknown".into()))
            }
            _ => Ok(ModelIdentity::new("stub-builder:chunks")),
        }
    }
}

fn context_chunk(hit: &ScoredChunk) -> ContextChunk {
    ContextChunk {
        id: hit.chunk.id.clone(),
        document_id: hit.chunk.document_id.clone(),
        score: hit.score,
    }
}

/// Runs the suite over builders that each carry the flaw `flaw` makes.
async fn builder_failure(
    flaw: impl Fn() -> BuilderFlaw,
) -> ragondin_conformance::ConformanceFailure {
    check_context_builder_conformance(|| Box::new(StubBuilder(flaw())))
        .await
        .expect_err("the stub is not conformant")
}

#[tokio::test]
async fn a_conformant_context_builder_passes() {
    check_context_builder_conformance(|| Box::new(StubBuilder(BuilderFlaw::None)))
        .await
        .expect("the stub honours the context builder contract");
}

#[tokio::test]
async fn a_context_builder_fabricating_a_chunk_id_fails() {
    let failure =
        check_context_builder_conformance(|| Box::new(StubBuilder(BuilderFlaw::Fabricates)))
            .await
            .expect_err("a builder selects, it does not invent");
    assert_eq!(failure.check(), "no fabricated ids");
    assert_eq!(failure.component(), "ContextBuilder");
}

#[tokio::test]
async fn a_context_builder_filling_an_empty_input_fails() {
    let failure =
        check_context_builder_conformance(|| Box::new(StubBuilder(BuilderFlaw::FillsAnEmptyInput)))
            .await
            .expect_err("zero chunks in, the empty context out (ADR-C19)");
    assert_eq!(failure.check(), "no fabricated ids");
}

#[tokio::test]
async fn a_context_builder_placing_a_chunk_twice_fails() {
    let failure =
        check_context_builder_conformance(|| Box::new(StubBuilder(BuilderFlaw::Duplicates)))
            .await
            .expect_err("a context places each chunk once");
    assert_eq!(failure.check(), "no duplicate ids");
}

#[tokio::test]
async fn a_context_builder_accepting_a_zero_budget_fails() {
    let failure = check_context_builder_conformance(|| {
        Box::new(StubBuilder(BuilderFlaw::AcceptsAZeroBudget))
    })
    .await
    .expect_err("a zero budget is top_k's twin");
    assert_eq!(failure.check(), "zero budget rejected");
}

#[tokio::test]
async fn a_context_builder_refusing_a_zero_budget_as_its_own_failure_fails() {
    let failure = check_context_builder_conformance(|| {
        Box::new(StubBuilder(BuilderFlaw::RefusesAZeroBudgetAsUnavailable))
    })
    .await
    .expect_err("a zero budget is the caller's error, not the component's");
    assert_eq!(failure.check(), "zero budget rejected");
}

#[tokio::test]
async fn a_context_builder_refusing_zero_chunks_fails() {
    let failure =
        check_context_builder_conformance(|| Box::new(StubBuilder(BuilderFlaw::RefusesZeroChunks)))
            .await
            .expect_err("zero chunks is a valid call (ADR-C19)");
    assert_eq!(failure.check(), "well-formed call succeeds");
    assert!(
        failure.detail().contains("zero chunks"),
        "the diagnosis must name the zero-chunk call: {}",
        failure.detail()
    );
}

#[tokio::test]
async fn a_context_builder_failing_a_well_formed_call_fails() {
    let failure =
        check_context_builder_conformance(|| Box::new(StubBuilder(BuilderFlaw::RefusesEverything)))
            .await
            .expect_err("a well-formed call must succeed");
    assert_eq!(failure.check(), "well-formed call succeeds");
}

#[tokio::test]
async fn a_context_builder_reporting_the_empty_identity_fails() {
    let failure = builder_failure(|| BuilderFlaw::EmptyIdentity).await;
    assert_eq!(failure.check(), "identity non-empty");
}

#[tokio::test]
async fn a_context_builder_whose_identity_changes_per_call_fails() {
    let failure = builder_failure(|| BuilderFlaw::CountingIdentity(AtomicUsize::new(0))).await;
    assert_eq!(failure.check(), "identity stable across two calls");
}

#[tokio::test]
async fn a_context_builder_whose_identity_names_the_instance_fails() {
    // The composition root reads the identity from an instance other than the
    // one that runs (ADR-C31 § 4), so two instances of one configuration must
    // agree — an identity naming the instance breaks that on the second call.
    let instances = AtomicUsize::new(0);
    let failure = check_context_builder_conformance(|| {
        Box::new(StubBuilder(BuilderFlaw::InstanceIdentity(
            instances.fetch_add(1, Ordering::SeqCst),
        )))
    })
    .await
    .expect_err("one configuration, one identity");
    assert_eq!(failure.check(), "identity stable across two calls");
}

#[tokio::test]
async fn a_context_builder_unable_to_report_its_identity_fails() {
    let failure = builder_failure(|| BuilderFlaw::FailingIdentity).await;
    assert_eq!(failure.check(), "well-formed call succeeds");
}

#[tokio::test]
#[should_panic(expected = "no fabricated ids")]
async fn the_context_builder_assert_wrapper_panics_on_a_broken_component() {
    assert_context_builder_conformance(|| Box::new(StubBuilder(BuilderFlaw::Fabricates))).await;
}

// ---------------------------------------------------------------- Generator

/// The model the stub generators serve: what a caller of the generator suite
/// hands it.
const SERVED: &str = "stub-model";

/// Which malformations of the template grammar stated on `Generator` a stub
/// refuses. The conformant stub refuses all three; each broken one forgives
/// exactly one, which proves the suite probes every one of them.
#[derive(Clone, Copy)]
struct Strictness {
    unknown_name: bool,
    unclosed_brace: bool,
    lone_close: bool,
}

const STRICT: Strictness = Strictness {
    unknown_name: true,
    unclosed_brace: true,
    lone_close: true,
};

/// Renders `template` in the contract's grammar: one pass, left to right,
/// `{{` and `}}` taken before a placeholder at each position.
fn render(
    template: &str,
    query: &str,
    context: &str,
    strict: Strictness,
) -> Result<String, ComponentError> {
    let malformed = |why: &str| {
        Err(ComponentError::InvalidRequest(format!(
            "malformed template: {why}"
        )))
    };
    let mut out = String::new();
    let mut rest = template;
    while let Some(next) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("{{") {
            out.push('{');
            rest = after;
        } else if let Some(after) = rest.strip_prefix("}}") {
            out.push('}');
            rest = after;
        } else if let Some(after) = rest.strip_prefix('{') {
            match after.find('}') {
                Some(end) => {
                    match &after[..end] {
                        "query" => out.push_str(query),
                        "context" => out.push_str(context),
                        _ if strict.unknown_name => return malformed("an unknown placeholder"),
                        other => out.push_str(other),
                    }
                    rest = &after[end + 1..];
                }
                None if strict.unclosed_brace => return malformed("a `{` no `}` closes"),
                None => {
                    out.push_str(after);
                    rest = "";
                }
            }
        } else if let Some(after) = rest.strip_prefix('}') {
            if strict.lone_close {
                return malformed("a lone `}`");
            }
            rest = after;
        } else {
            out.push(next);
            rest = &rest[next.len_utf8()..];
        }
    }
    Ok(out)
}

/// What a stub generator gets wrong, if anything.
enum GeneratorFlaw {
    None,
    /// Reads the template with one malformation forgiven.
    Forgives(Strictness),
    /// Answers an empty served model with the model it serves.
    AcceptsAnEmptyModel,
    /// Refuses an empty served model, but as a failure of its own.
    RefusesAnEmptyModelAsUnavailable,
    /// Answers an empty template with an empty prompt.
    AcceptsAnEmptyTemplate,
    /// Refuses the `{{` and `}}` escapes, which the grammar allows.
    RefusesEscapes,
    /// Refuses a placeholder that appears more than once, which the grammar
    /// allows.
    RefusesARepeatedPlaceholder,
    /// Treats an empty context as an invalid request, against ADR-C19.
    RefusesAnEmptyContext,
    /// Fails every generate.
    RefusesEverything,
    /// Reports the empty identity.
    EmptyIdentity,
    /// Reports an identity that changes on every call.
    CountingIdentity(AtomicUsize),
    /// Reports an identity that names the instance.
    InstanceIdentity(usize),
    /// Cannot report an identity at all.
    FailingIdentity,
}

struct StubGenerator(GeneratorFlaw);

#[async_trait]
impl Generator for StubGenerator {
    async fn generate(
        &self,
        query: &Query,
        context: &Context,
        params: &GenerateParams,
    ) -> Result<Answer, ComponentError> {
        let flaw = &self.0;
        if matches!(flaw, GeneratorFlaw::RefusesEverything) {
            return Err(ComponentError::Unavailable("generator down".into()));
        }
        let serves = params.served_model == SERVED
            || (params.served_model.is_empty()
                && matches!(flaw, GeneratorFlaw::AcceptsAnEmptyModel));
        if params.served_model.is_empty()
            && matches!(flaw, GeneratorFlaw::RefusesAnEmptyModelAsUnavailable)
        {
            return Err(ComponentError::Unavailable("no default model".into()));
        }
        if !serves {
            return Err(ComponentError::InvalidRequest(format!(
                "model {:?} is not served here",
                params.served_model
            )));
        }
        if params.template.is_empty() && !matches!(flaw, GeneratorFlaw::AcceptsAnEmptyTemplate) {
            return Err(ComponentError::InvalidRequest("an empty template".into()));
        }
        let template = params.template.as_str();
        if matches!(flaw, GeneratorFlaw::RefusesEscapes)
            && (template.contains("{{") || template.contains("}}"))
        {
            return Err(ComponentError::InvalidRequest("no escapes here".into()));
        }
        if matches!(flaw, GeneratorFlaw::RefusesARepeatedPlaceholder)
            && (template.matches("{query}").count() > 1
                || template.matches("{context}").count() > 1)
        {
            return Err(ComponentError::InvalidRequest(
                "each placeholder once".into(),
            ));
        }
        if context.chunks.is_empty() && matches!(flaw, GeneratorFlaw::RefusesAnEmptyContext) {
            return Err(ComponentError::InvalidRequest(
                "nothing to answer from".into(),
            ));
        }
        let strict = match flaw {
            GeneratorFlaw::Forgives(strict) => *strict,
            _ => STRICT,
        };
        // The "model" answers with the prompt it was asked: content is not
        // the suite's business, so the stub need not be a good generator.
        Ok(Answer {
            text: render(&params.template, &query.text, &context.text, strict)?,
        })
    }

    async fn model_identity(&self, served_model: &str) -> Result<ModelIdentity, ComponentError> {
        if served_model != SERVED {
            return Err(ComponentError::InvalidRequest(format!(
                "model {served_model:?} is not served here"
            )));
        }
        match &self.0 {
            GeneratorFlaw::EmptyIdentity => Ok(ModelIdentity::new("")),
            GeneratorFlaw::CountingIdentity(calls) => Ok(ModelIdentity::new(format!(
                "stub-model@rev1#{}",
                calls.fetch_add(1, Ordering::SeqCst)
            ))),
            GeneratorFlaw::InstanceIdentity(instance) => Ok(ModelIdentity::new(format!(
                "stub-model@rev1/instance-{instance}"
            ))),
            GeneratorFlaw::FailingIdentity => {
                Err(ComponentError::Unavailable("identity unknown".into()))
            }
            _ => Ok(ModelIdentity::new("stub-model@rev1")),
        }
    }
}

/// Runs the suite over generators that each carry the flaw `flaw` makes.
async fn generator_failure(
    flaw: impl Fn() -> GeneratorFlaw,
) -> ragondin_conformance::ConformanceFailure {
    check_generator_conformance(|| Box::new(StubGenerator(flaw())), SERVED)
        .await
        .expect_err("the stub is not conformant")
}

#[test]
fn the_stub_renderer_follows_the_template_grammar() {
    // The conformant stub is only as good as its renderer; pin the grammar's
    // own examples so a broken renderer cannot make a broken suite look sound.
    let render = |t: &str| render(t, "Q", "C", STRICT);
    assert_eq!(render("{query}|{context}|{query}").unwrap(), "Q|C|Q");
    assert_eq!(render("{{query}}").unwrap(), "{query}");
    assert_eq!(
        render("{query}\n{context}\n{query} {{literal}}").unwrap(),
        "Q\nC\nQ {literal}"
    );
    assert_eq!(render("no placeholder").unwrap(), "no placeholder");
    for malformed in ["{unknown}", "{query", "}", "{context} {", "{query} }"] {
        assert!(
            matches!(render(malformed), Err(ComponentError::InvalidRequest(_))),
            "{malformed:?} must be refused"
        );
    }
}

#[tokio::test]
async fn a_conformant_generator_passes() {
    check_generator_conformance(|| Box::new(StubGenerator(GeneratorFlaw::None)), SERVED)
        .await
        .expect("the stub honours the generator contract");
}

#[tokio::test]
async fn a_generator_asked_for_a_model_it_does_not_serve_fails_the_well_formed_call() {
    // The served model is the caller's statement of what the fixture serves;
    // the suite asks for exactly that name, so a wrong statement shows here.
    let failure = check_generator_conformance(
        || Box::new(StubGenerator(GeneratorFlaw::None)),
        "some-other-model",
    )
    .await
    .expect_err("the stub serves only its own model");
    assert_eq!(failure.check(), "well-formed call succeeds");
    assert_eq!(failure.component(), "Generator");
}

#[tokio::test]
async fn a_generator_failing_a_well_formed_call_fails() {
    let failure = generator_failure(|| GeneratorFlaw::RefusesEverything).await;
    assert_eq!(failure.check(), "well-formed call succeeds");
}

#[tokio::test]
async fn a_generator_refusing_an_empty_context_fails() {
    let failure = generator_failure(|| GeneratorFlaw::RefusesAnEmptyContext).await;
    assert_eq!(failure.check(), "well-formed call succeeds");
    assert!(
        failure.detail().contains("empty context"),
        "the diagnosis must name the empty context: {}",
        failure.detail()
    );
}

#[tokio::test]
async fn a_generator_refusing_the_escapes_fails() {
    // The suite owns the well-formed template, so a caller cannot certify a
    // generator by handing it a template with nothing in it to get wrong.
    let failure = generator_failure(|| GeneratorFlaw::RefusesEscapes).await;
    assert_eq!(failure.check(), "well-formed call succeeds");
}

#[tokio::test]
async fn a_generator_refusing_a_repeated_placeholder_fails() {
    let failure = generator_failure(|| GeneratorFlaw::RefusesARepeatedPlaceholder).await;
    assert_eq!(failure.check(), "well-formed call succeeds");
}

#[tokio::test]
async fn a_generator_accepting_an_empty_served_model_fails() {
    let failure = generator_failure(|| GeneratorFlaw::AcceptsAnEmptyModel).await;
    assert_eq!(failure.check(), "empty served_model rejected");
}

#[tokio::test]
async fn a_generator_refusing_an_empty_served_model_as_its_own_failure_fails() {
    let failure = generator_failure(|| GeneratorFlaw::RefusesAnEmptyModelAsUnavailable).await;
    assert_eq!(failure.check(), "empty served_model rejected");
}

#[tokio::test]
async fn a_generator_accepting_an_empty_template_fails() {
    let failure = generator_failure(|| GeneratorFlaw::AcceptsAnEmptyTemplate).await;
    assert_eq!(failure.check(), "empty template rejected");
}

#[tokio::test]
async fn a_generator_accepting_an_unknown_placeholder_fails() {
    let failure = generator_failure(|| {
        GeneratorFlaw::Forgives(Strictness {
            unknown_name: false,
            ..STRICT
        })
    })
    .await;
    assert_eq!(failure.check(), "malformed template rejected");
    assert!(
        failure.detail().contains("{unknown}"),
        "the diagnosis must name the template: {}",
        failure.detail()
    );
}

#[tokio::test]
async fn a_generator_accepting_an_unclosed_brace_fails() {
    let failure = generator_failure(|| {
        GeneratorFlaw::Forgives(Strictness {
            unclosed_brace: false,
            ..STRICT
        })
    })
    .await;
    assert_eq!(failure.check(), "malformed template rejected");
    assert!(
        failure.detail().contains("{context} {"),
        "the diagnosis must name the template: {}",
        failure.detail()
    );
}

#[tokio::test]
async fn a_generator_accepting_a_lone_closing_brace_fails() {
    let failure = generator_failure(|| {
        GeneratorFlaw::Forgives(Strictness {
            lone_close: false,
            ..STRICT
        })
    })
    .await;
    assert_eq!(failure.check(), "malformed template rejected");
    assert!(
        failure.detail().contains("{query} }"),
        "the diagnosis must name the template: {}",
        failure.detail()
    );
}

#[tokio::test]
async fn a_generator_reporting_the_empty_identity_fails() {
    let failure = generator_failure(|| GeneratorFlaw::EmptyIdentity).await;
    assert_eq!(failure.check(), "identity non-empty");
}

#[tokio::test]
async fn a_generator_whose_identity_changes_per_call_fails() {
    let failure = generator_failure(|| GeneratorFlaw::CountingIdentity(AtomicUsize::new(0))).await;
    assert_eq!(failure.check(), "identity stable across two calls");
}

#[tokio::test]
async fn a_generator_whose_identity_names_the_instance_fails() {
    let instances = AtomicUsize::new(0);
    let failure = check_generator_conformance(
        || {
            Box::new(StubGenerator(GeneratorFlaw::InstanceIdentity(
                instances.fetch_add(1, Ordering::SeqCst),
            )))
        },
        SERVED,
    )
    .await
    .expect_err("one configuration, one identity");
    assert_eq!(failure.check(), "identity stable across two calls");
}

#[tokio::test]
async fn a_generator_unable_to_report_its_identity_fails() {
    let failure = generator_failure(|| GeneratorFlaw::FailingIdentity).await;
    assert_eq!(failure.check(), "well-formed call succeeds");
}

#[tokio::test]
#[should_panic(expected = "empty template rejected")]
async fn the_generator_assert_wrapper_panics_on_a_broken_component() {
    assert_generator_conformance(
        || Box::new(StubGenerator(GeneratorFlaw::AcceptsAnEmptyTemplate)),
        SERVED,
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
