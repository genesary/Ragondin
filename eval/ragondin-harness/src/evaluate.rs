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
//! 3. **Score each query for every family its ground truth allows** — the
//!    retrieval metrics where it carries qrels, `exact_match` and `token_f1`
//!    where it carries a reference answer — and average each family over its
//!    own judged set, in benchmark order (ADR-C30 § 5). Which families a run
//!    reports is read once, from what the benchmark carries (ADR-8).
//! 4. **Assemble the identity tuple** and name the run by its digest.
//!
//! # What a caller sees while it runs
//!
//! [`evaluate_observed`] is the same loop with two more parameters: an
//! observer called once per executed query, right after the trace is rendered
//! and before anything is scored — so a failing or refused query reaches it
//! too, before the error returns — and a cancellation signal read at the top
//! of every iteration, between two queries and never inside one. The traces
//! the observer has received are the partial record of a run that stopped.
//! [`evaluate`] is that loop with an observer that does nothing and a signal
//! nobody sets.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ragondin_benchmarks::identity::dataset_version;
use ragondin_benchmarks::{Benchmark, CarriedPieces};
use ragondin_engine::{plan_physical, Engine, EngineContext, ExecutionTrace, Output, ValueSummary};
use ragondin_experiments::{
    ranking_node, terminal, ConfigDocument, Metrics, Run, RunInputs, TraceDocument,
};
use ragondin_metrics::{
    documents_by_first_occurrence, exact_match, ndcg_at_k, recall_at_k, reciprocal_rank, token_f1,
};
use ragondin_pipeline::{LogicalPipeline, NodeId};
use ragondin_types::{DocId, QueryId};

use crate::error::{HarnessError, RankingWalkError};
use crate::identity::run_id;
use crate::trace::render;
use crate::CorpusIndex;

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
/// queries that happened to work; [`HarnessError::NoAnswer`] if the benchmark
/// carries reference answers and a query's pipeline returns a ranking or a
/// context rather than an answer; [`HarnessError::NoRanking`] if the benchmark
/// carries qrels and the ranking behind a query's output cannot be found; and
/// [`HarnessError::NothingToScore`] if the benchmark carries neither piece,
/// which is a mean over nothing rather than a score of zero.
pub async fn evaluate(
    evaluation: &Evaluation<'_>,
    ctx: &EngineContext,
) -> Result<Run, HarnessError> {
    evaluate_observed(
        evaluation,
        ctx,
        |_: QueryProgress<'_>| {},
        &AtomicBool::new(false),
    )
    .await
}

/// One executed query, as [`evaluate_observed`] reports it to its observer.
///
/// Borrowed rather than owned: the run keeps its own copy of the trace, and an
/// observer that files it elsewhere clones it once, where it needs to.
#[derive(Debug, Clone, Copy)]
pub struct QueryProgress<'a> {
    /// The query that just ran.
    pub query: &'a QueryId,
    /// Its position in the benchmark, from 1: the `3,982` of `3,982 / 10,570`.
    pub position: usize,
    /// How many queries the benchmark holds.
    pub total: usize,
    /// How long `Engine::execute` took for this query, measured around the
    /// call — the whole query's time, where the node durations inside the
    /// trace are each component's own (ADR-C28).
    pub elapsed: Duration,
    /// The query's trace, rendered: the very document the run files under the
    /// query, or the one a stopping error carries.
    pub trace: &'a TraceDocument,
}

/// Evaluates `evaluation` as [`evaluate`] does, reporting each query to
/// `observer` as it finishes and stopping between two queries once `cancel` is
/// set.
///
/// - **The observer** is called once per executed query, in benchmark order,
///   immediately after the query's trace is rendered and before it is scored.
///   A query whose execution fails, or whose output is refused, reaches the
///   observer before the error returns, so every trace the run produced has
///   been delivered by the time it stops, however it stops. It is `Send` so
///   that the returned future is, when the caller holds it across the loop's
///   await points on another thread.
/// - **The signal** is read at the top of each iteration, before the query
///   executes, and nowhere else: a query in flight runs to its end and is
///   observed, and one set before the first query runs nothing. A signal set
///   after the last query has no boundary left to be read at, and the run
///   completes; a benchmark with no query never reads it at all.
///
/// The observer runs synchronously on the loop, so its cost is added to every
/// query: a caller with slow work to do — files, a network push — sends the
/// progress over a channel rather than doing that work inside it.
///
/// # Errors
///
/// Those of [`evaluate`], and [`HarnessError::Cancelled`] when the signal is
/// read set.
pub async fn evaluate_observed<O>(
    evaluation: &Evaluation<'_>,
    ctx: &EngineContext,
    mut observer: O,
    cancel: &AtomicBool,
) -> Result<Run, HarnessError>
where
    O: FnMut(QueryProgress<'_>) + Send,
{
    // Once, before the loop: planning resolves every `impl:` name and
    // constructs the components a plan holds.
    let plan = plan_physical(evaluation.pipeline, ctx)?;
    let engine = Engine::new();

    // ADR-8: the regime follows the pieces the benchmark carries, never a
    // flag, and is decided once for the whole run.
    let (retrieval, generation) = match evaluation.benchmark.carries() {
        CarriedPieces::Neither => (false, false),
        CarriedPieces::QrelsOnly => (true, false),
        CarriedPieces::ReferenceAnswersOnly => (false, true),
        CarriedPieces::QrelsAndReferenceAnswers => (true, true),
    };

    let mut traces = BTreeMap::new();
    let mut retrieval_scores = RetrievalScores::default();
    let mut generation_scores = GenerationScores::default();

    let total = evaluation.benchmark.queries().len();
    for (completed, (query, judgments, references)) in
        evaluation.benchmark.iter_with_references().enumerate()
    {
        // The one place the signal is read: between two queries, so a query
        // is either not started or run to its end and observed.
        if cancel.load(Ordering::SeqCst) {
            return Err(HarnessError::Cancelled { completed });
        }

        let started = Instant::now();
        let (output, trace) = engine.execute(&plan, query.clone()).await;
        let elapsed = started.elapsed();
        let document = render(&trace);

        // Before anything below can return: a query that fails or is refused
        // is delivered too, so the observer holds every trace produced.
        observer(QueryProgress {
            query: &query.id,
            position: completed + 1,
            total,
            elapsed,
            trace: &document,
        });

        let output = output.map_err(|source| HarnessError::Execute {
            query: query.id.clone(),
            trace: document.clone(),
            source: Box::new(source),
        })?;

        // Both checks run for every query once the benchmark carries the
        // piece, judged or not, so a refusal never depends on which queries
        // happen to hold a judgment or a reference.
        if generation {
            let Some(answer) = answer(evaluation.pipeline, &trace) else {
                return Err(HarnessError::NoAnswer {
                    query: query.id.clone(),
                    trace: document,
                    kind: kind(&output),
                });
            };
            // A query with no reference is unjudged for this family (ADR-C30
            // § 1): the metrics are undefined over an empty reference list.
            if !references.is_empty() {
                generation_scores.add(answer, references);
            }
        }
        if retrieval {
            let ranked = ranking_node(evaluation.pipeline)
                .map_err(RankingWalkError::from)
                .and_then(|node| documents_at(&trace, node))
                .map_err(|walk| HarnessError::NoRanking {
                    query: query.id.clone(),
                    trace: document.clone(),
                    walk,
                })?;
            // A query with no qrels line at all is executed and left unscored:
            // `trec_eval` never evaluates one, because it is unjudged rather
            // than judged-and-empty, and counting it as a zero would deflate
            // every mean by the size of the gap between the two files.
            if !judgments.is_empty() {
                retrieval_scores.add(&ranked, judgments, evaluation.cutoff);
            }
        }

        traces.insert(query.id.clone(), document);
    }

    // Each family averages over its own judged set (ADR-C30 § 5), and a
    // family the benchmark does not carry is not reported at all.
    if retrieval_scores.queries == 0 && generation_scores.queries == 0 {
        return Err(HarnessError::NothingToScore);
    }
    let mut metrics = Metrics::default();
    for (name, value) in retrieval_scores
        .means(evaluation.cutoff)
        .into_iter()
        .chain(generation_scores.means())
    {
        metrics.insert(name, value);
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
        metrics,
        inputs,
        config: evaluation.config.clone(),
        traces,
        bindings: Vec::new(),
    })
}

/// The answer a query's pipeline produced, read from the terminal node's
/// output entry in the trace, or `None` when that entry is not an answer.
///
/// The terminal node is `ragondin_experiments::terminal`'s, the definition
/// `ragondin-api` reads a stored run's answer at.
///
/// Read from the trace rather than from the executor's `Output`: the trace is
/// what a stored run's `traces.json` holds, and the per-query fixture reads its
/// answers there (ADR-C31 § 5), so the text scored here and the text a
/// re-scorer reads are one value, rendered once.
fn answer<'t>(pipeline: &LogicalPipeline, trace: &'t ExecutionTrace) -> Option<&'t str> {
    let terminal = terminal(pipeline)?;
    match output_entry(trace, terminal.id()) {
        Some(ValueSummary::Answer { text }) => Some(text),
        _ => None,
    }
}

/// The kind of what a pipeline returned, as a configuration names it.
fn kind(output: &Output) -> &'static str {
    match output {
        Output::Chunks(_) => "chunks",
        Output::Context(_) => "context",
        Output::Answer(_) => "answer",
    }
}

/// What `node` produced for this query, as the trace records it.
fn output_entry<'t>(trace: &'t ExecutionTrace, node: &NodeId) -> Option<&'t ValueSummary> {
    trace
        .nodes
        .iter()
        .find(|entry| &entry.node == node)
        .and_then(|entry| entry.output.as_ref())
}

/// The ranked **documents** `node` produced, read from its output entry in
/// `trace` and folded by `ragondin_metrics::documents_by_first_occurrence` —
/// the one definition of the fold, which `ragondin-api` applies to the stored
/// trace of the same query.
///
/// Read from the trace (ADR-C28 names every node's chunks there) rather than
/// from the executor's output, because once a pipeline ends in an answer the
/// ranking is no longer its output. A node that is absent from the trace, that
/// failed, or whose output is not a ranking is named, never read as empty.
fn documents_at(trace: &ExecutionTrace, node: &NodeId) -> Result<Vec<DocId>, RankingWalkError> {
    let Some(ValueSummary::RankedChunks { chunks }) = output_entry(trace, node) else {
        return Err(RankingWalkError::NoRankedChunks { node: node.clone() });
    };
    Ok(documents_by_first_occurrence(
        chunks.iter().map(|hit| &hit.document),
    ))
}

/// The running sums of the retrieval metrics, over the queries that carry at
/// least one judgment.
///
/// Summed in benchmark order and divided once at the end: floating-point
/// addition is not associative, so the order queries are added in is part of
/// what makes two runs of one benchmark produce the same number.
#[derive(Default)]
struct RetrievalScores {
    ndcg: f64,
    recall: f64,
    reciprocal_rank: f64,
    queries: usize,
}

impl RetrievalScores {
    fn add(&mut self, ranked: &[DocId], judgments: &BTreeMap<DocId, u8>, cutoff: usize) {
        self.ndcg += ndcg_at_k(ranked, judgments, cutoff);
        self.recall += recall_at_k(ranked, judgments, cutoff);
        // Uncut, matching `trec_eval`'s `recip_rank`, which reports MRR over
        // the whole ranking rather than truncating it.
        self.reciprocal_rank += reciprocal_rank(ranked, judgments);
        self.queries += 1;
    }

    /// The means, or nothing when no query was scored — a family the
    /// benchmark does not carry is absent from the run, not zero.
    fn means(&self, cutoff: usize) -> Vec<(String, f64)> {
        if self.queries == 0 {
            return Vec::new();
        }
        let queries = self.queries as f64;
        vec![
            (format!("ndcg@{cutoff}"), self.ndcg / queries),
            (format!("recall@{cutoff}"), self.recall / queries),
            ("mrr".to_string(), self.reciprocal_rank / queries),
        ]
    }
}

/// The running sums of the generation metrics, over the queries that carry a
/// non-empty reference list — summed in benchmark order, as the retrieval
/// sums are and for the same reason.
#[derive(Default)]
struct GenerationScores {
    exact_match: f64,
    token_f1: f64,
    queries: usize,
}

impl GenerationScores {
    /// Scores one answer. `references` is non-empty: both metrics panic on an
    /// empty list, and the caller leaves such a query unjudged.
    fn add(&mut self, answer: &str, references: &[String]) {
        self.exact_match += exact_match(answer, references);
        self.token_f1 += token_f1(answer, references);
        self.queries += 1;
    }

    /// The means under the names ADR-C30 § 1 fixes, or nothing when no query
    /// was scored.
    fn means(&self) -> Vec<(String, f64)> {
        if self.queries == 0 {
            return Vec::new();
        }
        let queries = self.queries as f64;
        vec![
            ("exact_match".to_string(), self.exact_match / queries),
            ("token_f1".to_string(), self.token_f1 / queries),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ragondin_engine::{NodeTrace, RankedChunk, ValueSummary};
    use ragondin_types::ChunkId;

    use super::*;

    fn ranked(chunk: &str, document: &str, score: f32) -> RankedChunk {
        RankedChunk {
            chunk: ChunkId::new(chunk),
            document: DocId::new(document),
            score,
        }
    }

    fn entry(node: &str, output: Option<ValueSummary>) -> NodeTrace {
        NodeTrace {
            node: NodeId::new(node),
            inputs: Vec::new(),
            output,
            duration: Duration::ZERO,
            error: None,
        }
    }

    fn ranking(node: &str, chunks: Vec<RankedChunk>) -> NodeTrace {
        entry(node, Some(ValueSummary::RankedChunks { chunks }))
    }

    /// A `LogicalPipeline` read straight from its in-memory shape, as the
    /// engine's own tests forge one: `validate` refuses the malformed wirings
    /// the walk must still name, and the walk reads nothing but the shape.
    fn forged(json: &str) -> LogicalPipeline {
        serde_json::from_str(json).expect("the forged JSON must match LogicalPipeline's shape")
    }

    const GENERATION: &str = r#"{"inputs":["question"],"nodes":[
        {"Generator":{"id":"answer","implementation":"g","inputs":["question","context"],"params":{}}},
        {"ContextBuilder":{"id":"context","implementation":"c","inputs":["question","leg"],"params":{}}},
        {"Retriever":{"id":"leg","implementation":"r","inputs":["question"],"params":{}}}
    ]}"#;

    #[test]
    fn a_node_with_no_ranking_in_the_trace_is_named() {
        // No entry, an entry of another kind, a failed node: each is a walk
        // that found no ranking, never an empty one.
        let trace = ExecutionTrace {
            nodes: vec![
                entry(
                    "context",
                    Some(ValueSummary::Context {
                        chunks: vec![ranked("d-1#0", "d-1", 0.9)],
                        text: "one".to_string(),
                    }),
                ),
                entry("failed", None),
            ],
        };

        for node in ["absent", "context", "failed"] {
            assert_eq!(
                documents_at(&trace, &NodeId::new(node)),
                Err(RankingWalkError::NoRankedChunks {
                    node: NodeId::new(node)
                }),
                "{node}"
            );
        }
    }

    #[test]
    fn the_ranking_is_the_builders_input_not_the_context_it_kept() {
        // ADR-C30 § 3: the retriever's ranking, never `Context.chunks`, which
        // is cut to the builder's budget.
        let trace = ExecutionTrace {
            nodes: vec![
                ranking(
                    "leg",
                    vec![ranked("a#0", "a", 0.9), ranked("b#0", "b", 0.8)],
                ),
                entry(
                    "context",
                    Some(ValueSummary::Context {
                        chunks: vec![ranked("a#0", "a", 0.9)],
                        text: "a".to_string(),
                    }),
                ),
                entry(
                    "answer",
                    Some(ValueSummary::Answer {
                        text: "a".to_string(),
                    }),
                ),
            ],
        };
        let pipeline = forged(GENERATION);

        let node = ranking_node(&pipeline).expect("the walk reaches the retriever");
        assert_eq!(
            documents_at(&trace, node),
            Ok(vec![DocId::new("a"), DocId::new("b")])
        );
    }
}
