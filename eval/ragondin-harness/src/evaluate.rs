//! The driver loop: a benchmark in, a [`Run`] out.
//!
//! The whole of P1 is in what this module does *not* contain: no retrieval, no
//! ranking, no scoring of a chunk against a query. It plans a
//! `LogicalPipeline` through `ragondin-engine`'s own planner, calls
//! `Engine::execute` once per query, and scores what comes back. The serving
//! driver will call the same `execute` on the same plan type, which is why an
//! evaluation figure measured here describes production.
//!
//! # The order of operations
//!
//! 1. **Plan once.** Physical planning constructs the components, so planning
//!    per query would rebuild them per query — and a run in which two queries
//!    could meet two different sets of components is not one run. The corpus
//!    ([`crate::CorpusIndex`]) arrives already prepared, on [`Evaluation`]:
//!    the composition root built it and constructed those same components
//!    from it (ADR-C26), so this module does not prepare one of its own.
//! 2. **Execute every query**, keeping the trace the executor returns.
//! 3. **Score the judged ones** and average, in benchmark order.
//! 4. **Assemble the identity tuple** and name the run by its digest.

use std::collections::BTreeMap;

use ragondin_benchmarks::Benchmark;
use ragondin_engine::{plan_physical, Engine, EngineContext, Output};
use ragondin_experiments::{ConfigDocument, Metrics, Run, RunInputs};
use ragondin_metrics::{ndcg_at_k, recall_at_k, reciprocal_rank};
use ragondin_pipeline::LogicalPipeline;
use ragondin_types::DocId;

use crate::corpus::CorpusIndex;
use crate::error::HarnessError;
use crate::identity::{dataset_version, run_id};
use crate::trace::render;

/// What an evaluation run is asked to do.
///
/// The pipeline and its text arrive as a pair because the run store records
/// both, and the text is kept **verbatim**: re-serializing the configuration
/// out of the in-memory pipeline would file a second spelling of it beside the
/// digest of the first.
pub struct Evaluation<'a> {
    /// The pipeline to evaluate, already validated and canonical.
    pub pipeline: &'a LogicalPipeline,
    /// The configuration document that produced it, as it was written.
    pub config: &'a ConfigDocument,
    /// The benchmark to evaluate it over.
    pub benchmark: &'a Benchmark,
    /// The corpus index the caller's components were constructed from.
    ///
    /// Supplied by the caller, never derived here (ADR-C26): the composition
    /// root is the one place that holds both the engine and the concrete
    /// components, so it is the one place that can build a `CorpusIndex` and
    /// know its components agree with it. `evaluate` records this value's
    /// [`CorpusIndex::version`] as `index_version` rather than computing one
    /// of its own from `benchmark`.
    pub index: &'a CorpusIndex,
    /// The rank cutoff `k` of the metrics — the `10` of nDCG@10.
    pub cutoff: usize,
    /// The model hashes of the run, by the role each model played.
    ///
    /// Supplied by the caller rather than discovered here: a model file is a
    /// component's business, and a component reaches this crate only through a
    /// context that has already been assembled. Empty is the honest value for a
    /// pipeline whose components read no model.
    pub model_hashes: BTreeMap<String, String>,
}

/// Evaluates `evaluation` against the components registered on `ctx`.
///
/// Returns the [`Run`] — the record, named by its content address. Writing it
/// down is the caller's call, because the store root is the caller's: one
/// `FileSystemRunStore::save` away.
///
/// # Errors
///
/// [`HarnessError::Plan`] if the pipeline cannot be planned against `ctx`;
/// [`HarnessError::Execute`] if a query fails — a run is not reported over the
/// queries that happened to work; and [`HarnessError::NothingToScore`] if no
/// query of the benchmark is judged, which is a mean over nothing rather than a
/// score of zero.
pub async fn evaluate(
    evaluation: &Evaluation<'_>,
    ctx: &EngineContext,
) -> Result<Run, HarnessError> {
    // Once, before the loop: planning resolves every `impl:` name and
    // constructs the components a plan holds.
    let plan = plan_physical(evaluation.pipeline, ctx)?;
    let engine = Engine::new();

    let mut traces = BTreeMap::new();
    let mut scored = Scores::default();

    for (query, judgments) in evaluation.benchmark.iter() {
        let (output, trace) = engine.execute(&plan, query.clone()).await;
        let document = render(&trace);

        let output = output.map_err(|source| HarnessError::Execute {
            query: query.id.clone(),
            trace: document.clone(),
            source: Box::new(source),
        })?;
        traces.insert(query.id.clone(), document);

        // A query with no qrels line at all is executed and left unscored:
        // `trec_eval` never evaluates one, because it is unjudged rather than
        // judged-and-empty, and counting it as a zero would deflate every mean
        // by the size of the gap between the two files.
        if judgments.is_empty() {
            continue;
        }

        let ranked = ranked_documents(&output);
        scored.add(&ranked, judgments, evaluation.cutoff);
    }

    if scored.queries == 0 {
        return Err(HarnessError::NothingToScore);
    }

    let inputs = RunInputs {
        pipeline: evaluation.pipeline.content_hash(),
        dataset_version: dataset_version(evaluation.benchmark),
        index_version: evaluation.index.version().to_string(),
        model_hashes: evaluation.model_hashes.clone(),
        // The workspace shares one version, so this crate's own is the version
        // of the engine it was compiled against.
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
    };

    Ok(Run {
        id: run_id(&inputs),
        metrics: scored.means(evaluation.cutoff),
        inputs,
        config: evaluation.config.clone(),
        traces,
    })
}

/// The ranked **documents** behind a ranked list of chunks.
///
/// A metric scores documents and a pipeline returns chunks, so the list is
/// collapsed by first occurrence: the best-ranked chunk of a document is where
/// that document enters the ranking, which is the max-score-per-document rule
/// BEIR evaluations use, expressed over a list that is already sorted by
/// descending score.
fn ranked_documents(output: &Output) -> Vec<DocId> {
    let mut documents: Vec<DocId> = Vec::with_capacity(output.len());
    for hit in output {
        if !documents.contains(&hit.chunk.document_id) {
            documents.push(hit.chunk.document_id.clone());
        }
    }
    documents
}

/// The running sums of the metrics, over the queries that were scored.
///
/// Summed in benchmark order and divided once at the end: floating-point
/// addition is not associative, so the order queries are added in is part of
/// what makes two runs of one benchmark produce the same number.
#[derive(Default)]
struct Scores {
    ndcg: f64,
    recall: f64,
    reciprocal_rank: f64,
    queries: usize,
}

impl Scores {
    fn add(&mut self, ranked: &[DocId], judgments: &BTreeMap<DocId, u8>, cutoff: usize) {
        self.ndcg += ndcg_at_k(ranked, judgments, cutoff);
        self.recall += recall_at_k(ranked, judgments, cutoff);
        // Uncut, matching `trec_eval`'s `recip_rank`, which reports MRR over
        // the whole ranking rather than truncating it.
        self.reciprocal_rank += reciprocal_rank(ranked, judgments);
        self.queries += 1;
    }

    fn means(&self, cutoff: usize) -> Metrics {
        let queries = self.queries as f64;
        Metrics::from_iter([
            (format!("ndcg@{cutoff}"), self.ndcg / queries),
            (format!("recall@{cutoff}"), self.recall / queries),
            ("mrr".to_string(), self.reciprocal_rank / queries),
        ])
    }
}

#[cfg(test)]
mod tests {
    use ragondin_types::{Chunk, ChunkId, ScoredChunk};

    use super::*;

    fn hit(chunk: &str, document: &str, score: f32) -> ScoredChunk {
        ScoredChunk {
            chunk: Chunk {
                id: ChunkId::new(chunk),
                text: format!("the text of {chunk}"),
                document_id: DocId::new(document),
            },
            score,
        }
    }

    #[test]
    fn a_chunk_ranking_collapses_to_its_documents_by_first_occurrence() {
        let output = vec![
            hit("d-1#2", "d-1", 0.9),
            hit("d-2#0", "d-2", 0.8),
            hit("d-1#0", "d-1", 0.7),
            hit("d-3#1", "d-3", 0.6),
        ];

        assert_eq!(
            ranked_documents(&output),
            [DocId::new("d-1"), DocId::new("d-2"), DocId::new("d-3")],
            "a document enters the ranking at its best chunk and never twice"
        );
    }

    #[test]
    fn an_empty_output_ranks_no_document() {
        assert!(ranked_documents(&Output::new()).is_empty());
    }
}
