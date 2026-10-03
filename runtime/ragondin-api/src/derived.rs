//! The data derived from a stored run and its benchmark: per-query scores at
//! the run's output, per-node ranking metrics, and which documents are gold —
//! a passage's grade and a node's gold ranks, read over the same folded
//! ranking the metrics score. Computed here, the figures cached under
//! the workspace's `cache/` (`cache.rs`), and never written into the run —
//! the store writes nothing derived.
//!
//! Every figure is a *reading* of the trace against the benchmark's ground
//! truth, with `ragondin-metrics`' functions and the rules the harness scores
//! by, so that
//! the figure at the last ranking node is the one `metrics.json` holds:
//!
//! - **Which metrics.** The ones the run recorded whose name
//!   `ragondin-metrics`' catalogue knows (`Metric::parse`) — `ndcg@<k>`,
//!   `recall@<k>`, `mrr`, `exact_match`, `token_f1` — each at the cutoff its
//!   own name states, and read by the family the catalogue gives it. A name
//!   the catalogue does not know (a latency percentile, say) has no
//!   per-query figure here; the run's own value of it is still listed, as
//!   family `unknown`, by `GET /runs`.
//! - **Which ranking.** Documents, folded from the chunks a node produced by
//!   first occurrence: several chunks of one document count once, at the rank
//!   of the best one — `ragondin_metrics::documents_by_first_occurrence`, the
//!   fold the harness applies when it writes `metrics.json`.
//! - **Which node is the output.** The ranking the retrieval metrics read is
//!   found by ADR-C30 § 3's walk over the pipeline's shape —
//!   `ragondin_experiments::ranking_node`, the walk the harness scores at. The
//!   answer is the terminal node's (`ragondin_experiments::terminal`), when it
//!   produces one.
//! - **Which queries.** A query is judged for the ranking metrics when its
//!   qrels are non-empty, and for the answer metrics when it has a reference;
//!   an unjudged query has no figure, and a mean is over the judged ones only.
//!   Means are summed in the benchmark's query order and divided once, as the
//!   harness does: floating-point addition is not associative.
//!
//! The fold and the walk each have one definition, in crates both this one
//! and the harness reach (INV-12 keeps the harness out of reach): a change to
//! either reaches the figures written and the figures read back together.

use std::collections::{BTreeMap, HashMap};

use ragondin_benchmarks::Benchmark;
use ragondin_experiments::{lower_median, ranking_node, terminal, Run, Trace, TraceSummary};
use ragondin_metrics::{
    documents_by_first_occurrence, exact_match, ndcg_at_k, recall_at_k, reciprocal_rank, token_f1,
    Family, Metric,
};
use ragondin_pipeline::{produced_kind, LogicalPipeline, NodeId, ValueKind};
use ragondin_types::{DocId, QueryId};

/// The metrics a run recorded that can be read per query, by name, in name
/// order.
#[derive(Clone, Debug)]
pub(crate) struct Metrics(Vec<(String, Metric)>);

impl Metrics {
    pub(crate) fn of<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        Self(
            names
                .into_iter()
                .filter_map(|name| Metric::parse(name).map(|metric| (name.to_owned(), metric)))
                .collect(),
        )
    }

    pub(crate) fn names(&self) -> Vec<String> {
        self.0.iter().map(|(name, _)| name.clone()).collect()
    }

    /// The ranking metrics among them, by name, in name order.
    pub(crate) fn ranking_names(&self) -> Vec<String> {
        self.ranking().map(|(name, _)| name.clone()).collect()
    }

    fn ranking(&self) -> impl Iterator<Item = &(String, Metric)> {
        self.0
            .iter()
            .filter(|(_, metric)| metric.family() == Family::Ranking)
    }

    fn answer(&self) -> impl Iterator<Item = &(String, Metric)> {
        self.0
            .iter()
            .filter(|(_, metric)| metric.family() == Family::Answers)
    }

    /// Every ranking metric of `ranked` against `judgments`.
    fn score_ranking(
        &self,
        ranked: &[DocId],
        judgments: &BTreeMap<DocId, u8>,
    ) -> BTreeMap<String, f64> {
        self.ranking()
            .map(|(name, metric)| {
                let value = match metric {
                    Metric::Ndcg { k } => ndcg_at_k(ranked, judgments, *k),
                    Metric::Recall { k } => recall_at_k(ranked, judgments, *k),
                    Metric::Mrr => reciprocal_rank(ranked, judgments),
                    Metric::ExactMatch | Metric::TokenF1 => unreachable!("not a ranking metric"),
                };
                (name.clone(), value)
            })
            .collect()
    }

    /// Every answer metric of `answer` against `references`, which is not
    /// empty: both metrics are undefined over an empty list.
    fn score_answer(&self, answer: &str, references: &[String]) -> BTreeMap<String, f64> {
        self.answer()
            .map(|(name, metric)| {
                let value = match metric {
                    Metric::ExactMatch => exact_match(answer, references),
                    Metric::TokenF1 => token_f1(answer, references),
                    Metric::Ndcg { .. } | Metric::Recall { .. } | Metric::Mrr => {
                        unreachable!("not an answer metric")
                    }
                };
                (name.clone(), value)
            })
            .collect()
    }
}

/// The run's median query latency: the lower median, over the queries whose
/// trace reads, of each one's latency (`Trace::latency_nanos`); `None` when
/// none reads. A trace that does not read, or whose durations overflow, has
/// no latency to give and is left out of the median rather than failing the
/// listing that shows it. Read from the traces alone: no dataset is needed.
pub(crate) fn median_query_latency(run: &Run) -> Option<u64> {
    lower_median(
        run.traces
            .values()
            .filter_map(|document| Trace::try_from(document).ok())
            .filter_map(|trace| trace.latency_nanos())
            .collect(),
    )
}

/// The nodes of a pipeline the derived data is read at.
#[derive(Clone, Debug)]
pub(crate) struct Outputs {
    /// The node whose ranking the retrieval metrics read, if the walk finds
    /// one.
    pub(crate) ranking: Option<NodeId>,
    /// The terminal node, when it produces an answer.
    pub(crate) answer: Option<NodeId>,
}

impl Outputs {
    pub(crate) fn of(pipeline: &LogicalPipeline) -> Self {
        Self {
            ranking: ranking_node(pipeline).ok().cloned(),
            answer: terminal(pipeline)
                .filter(|node| produced_kind(node) == ValueKind::Answer)
                .map(|node| node.id().clone()),
        }
    }
}

/// What `node` produced in `trace`, when it produced a ranking: its documents,
/// folded from its chunks by `documents_by_first_occurrence`. `None` for a
/// node absent from the trace, one that failed, and one whose output is not a
/// ranking.
fn documents_at(trace: &Trace, node: &NodeId) -> Option<Vec<DocId>> {
    let entry = trace.nodes.iter().find(|entry| &entry.node == node)?;
    let Some(TraceSummary::RankedChunks { chunks }) = &entry.output else {
        return None;
    };
    Some(documents_by_first_occurrence(
        chunks.iter().map(|chunk| &chunk.document),
    ))
}

/// What `node` produced in `trace`, when it produced an answer.
fn answer_text<'t>(trace: &'t Trace, node: &NodeId) -> Option<&'t str> {
    let entry = trace.nodes.iter().find(|entry| &entry.node == node)?;
    match &entry.output {
        Some(TraceSummary::Answer { text }) => Some(text),
        _ => None,
    }
}

/// The qrels of `query`, when it has any: a query with none is unjudged.
fn judgments<'b>(benchmark: &'b Benchmark, query: &QueryId) -> Option<&'b BTreeMap<DocId, u8>> {
    benchmark
        .qrels()
        .for_query(query)
        .filter(|judgments| !judgments.is_empty())
}

/// The scores of one query at the run's output: the ranking metrics when it
/// is judged and the output ranking is in its trace, the answer metrics when
/// it has a reference and the answer is in its trace.
pub(crate) fn query_scores(
    metrics: &Metrics,
    outputs: &Outputs,
    benchmark: &Benchmark,
    query: &QueryId,
    trace: &Trace,
) -> BTreeMap<String, f64> {
    let mut scores = BTreeMap::new();
    if let (Some(node), Some(judgments)) = (&outputs.ranking, judgments(benchmark, query)) {
        if let Some(ranked) = documents_at(trace, node) {
            scores.extend(metrics.score_ranking(&ranked, judgments));
        }
    }
    let references = benchmark
        .reference_answers()
        .for_query(query)
        .filter(|references| !references.is_empty());
    if let (Some(node), Some(references)) = (&outputs.answer, references) {
        if let Some(answer) = answer_text(trace, node) {
            scores.extend(metrics.score_answer(answer, references));
        }
    }
    scores
}

/// The ranking metrics of what `node` produced for one query, when it
/// produced a ranking and the query is judged.
pub(crate) fn node_scores(
    metrics: &Metrics,
    benchmark: &Benchmark,
    query: &QueryId,
    trace: &Trace,
    node: &NodeId,
) -> Option<BTreeMap<String, f64>> {
    let judgments = judgments(benchmark, query)?;
    let ranked = documents_at(trace, node)?;
    Some(metrics.score_ranking(&ranked, judgments))
}

/// One node's ranking metrics over the run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NodeFigures {
    pub(crate) node: String,
    /// Whether the node produced a ranking for at least one query.
    pub(crate) produces_ranking: bool,
    /// How many judged queries the means are over.
    pub(crate) judged_queries: u64,
    /// The means, or `None` when no judged query has a ranking from it.
    pub(crate) metrics: Option<BTreeMap<String, f64>>,
}

/// Every node's figures, in the pipeline's canonical order. With no
/// benchmark, only whether each produces a ranking is known.
pub(crate) fn node_figures(
    metrics: &Metrics,
    pipeline: &LogicalPipeline,
    traces: &BTreeMap<QueryId, Trace>,
    benchmark: Option<&Benchmark>,
) -> Vec<NodeFigures> {
    pipeline
        .nodes()
        .iter()
        .map(|node| {
            let id = node.id();
            let produces_ranking = traces
                .values()
                .any(|trace| documents_at(trace, id).is_some());
            let mut sums: BTreeMap<String, f64> = BTreeMap::new();
            let mut judged = 0u64;
            // The benchmark's order, the harness's: see the module's notes.
            for query in benchmark.map_or(&[][..], |benchmark| benchmark.queries()) {
                let (Some(benchmark), Some(trace)) = (benchmark, traces.get(&query.id)) else {
                    continue;
                };
                if let Some(scores) = node_scores(metrics, benchmark, &query.id, trace, id) {
                    for (name, value) in scores {
                        *sums.entry(name).or_default() += value;
                    }
                    judged += 1;
                }
            }
            NodeFigures {
                node: id.as_str().to_owned(),
                produces_ranking,
                judged_queries: judged,
                metrics: (judged > 0).then(|| {
                    sums.into_iter()
                        .map(|(name, sum)| (name, sum / judged as f64))
                        .collect()
                }),
            }
        })
        .collect()
}

/// The grade the qrels give `document` for a judged query: `0` when they do
/// not judge it. That is `ragondin-metrics`' convention — a grade of `0`
/// means not relevant, and an unjudged document counts as `0`, TREC's closed
/// world — which its metrics apply inside and do not export, so this reads
/// the qrels under the same rule rather than another.
fn grade_in(judgments: &BTreeMap<DocId, u8>, document: &DocId) -> u8 {
    judgments.get(document).copied().unwrap_or(0)
}

/// Whether `document` is gold for a judged query: graded above 0.
fn is_gold(judgments: &BTreeMap<DocId, u8>, document: &DocId) -> bool {
    grade_in(judgments, document) > 0
}

/// The grade of `document` for `query`, `0` when the qrels do not judge it;
/// `None` when the query is unjudged, so that no passage of it reads as
/// judged not relevant.
pub(crate) fn grade(benchmark: &Benchmark, query: &QueryId, document: &DocId) -> Option<u8> {
    judgments(benchmark, query).map(|judgments| grade_in(judgments, document))
}

/// The 1-based ranks of the gold documents in what `node` produced for
/// `query`, over its documents folded from its chunks — the ranking every
/// metric of it scores, so a rank here is the rank a metric sees. `None`
/// when the query is unjudged or the node produced no ranking; empty when
/// it ranked no gold document.
pub(crate) fn gold_ranks(
    benchmark: &Benchmark,
    query: &QueryId,
    trace: &Trace,
    node: &NodeId,
) -> Option<Vec<u64>> {
    let judgments = judgments(benchmark, query)?;
    let ranked = documents_at(trace, node)?;
    Some(
        (1u64..)
            .zip(&ranked)
            .filter(|(_, document)| is_gold(judgments, document))
            .map(|(rank, _)| rank)
            .collect(),
    )
}

/// One query's text, the benchmark's, when it holds the query.
pub(crate) fn query_text<'b>(benchmark: &'b Benchmark, query: &QueryId) -> Option<&'b str> {
    benchmark
        .queries()
        .iter()
        .find(|candidate| &candidate.id == query)
        .map(|candidate| candidate.text.as_str())
}

/// Every query's text, by id: the benchmark's.
pub(crate) fn query_texts(benchmark: &Benchmark) -> HashMap<&QueryId, &str> {
    benchmark
        .queries()
        .iter()
        .map(|query| (&query.id, query.text.as_str()))
        .collect()
}

/// Whether `query`, judged, has no gold document in the top `k` of the
/// output ranking. `None` when the query is unjudged or its trace holds no
/// output ranking.
pub(crate) fn gold_missing(
    outputs: &Outputs,
    benchmark: &Benchmark,
    query: &QueryId,
    trace: &Trace,
    k: usize,
) -> Option<bool> {
    let judgments = judgments(benchmark, query)?;
    let ranked = documents_at(trace, outputs.ranking.as_ref()?)?;
    Some(
        !ranked
            .iter()
            .take(k)
            .any(|document| is_gold(judgments, document)),
    )
}
