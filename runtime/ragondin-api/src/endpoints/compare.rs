//! `POST /compare`: runs of one benchmark against a baseline.
//!
//! The metric table and the configuration matrix are `ragondin-experiments`'
//! `compare_runs`, converted. The stages (`stages.rs`), their alignment and
//! the bins (`comparison.rs`) are this crate's, read off the lowered graphs
//! and the derived figures `GET /runs/{id}/queries` serves — the same
//! function, the same cache. The options travel in the JSON body, never in
//! the query string.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use ragondin_experiments::{compare_runs, Direction, Run, Trace};
use ragondin_pipeline::LogicalPipeline;
use ragondin_types::QueryId;

use super::json_body;
use crate::backends::RunDataset;
use crate::comparison::{self, Column, BINS};
use crate::derived::{Metrics, NodeFigures, Outputs};
use crate::error::ApiError;
use crate::handlers::{self, AppState};
use crate::request::CompareRequest;
use crate::response::{
    ComparedRun, Comparison, Confidence, DeltaBin, MetricDeltas, NodeLatency, NodePair, Pairing,
    PairingSource, QueryDelta, RunDeltas, RunLatency, StageCell, StageName, StageNode, StageRow,
    StageValue,
};
use crate::stages::{Stage, Stages};
use crate::{cache, convert, lineage, validation};

/// The most runs one comparison holds: a baseline and four others. The
/// design system gives each compared run an ink, and has four; a sixth run
/// is refused rather than given an invented colour (ADR-016).
const CEILING: usize = 5;

/// One run, read for the comparison.
struct Compared {
    run: Run,
    pipeline: LogicalPipeline,
    traces: Arc<BTreeMap<QueryId, Trace>>,
    metrics: Metrics,
    stages: Stages,
    name: Option<String>,
}

/// `POST /compare`.
///
/// Every step that can refuse the request runs before anything is written:
/// the request's pairing is checked early and kept last, once the response
/// is built, so a refused request — an unreadable run, runs not comparable,
/// a pairing that names nothing — changes nothing on disk.
pub(crate) async fn compare(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<Comparison>, ApiError> {
    let request: CompareRequest = json_body(&body)?;
    let order = order(&request)?;

    let mut runs = Vec::with_capacity(order.len());
    for id in order {
        runs.push(handlers::load_run(&state, id).await?);
    }
    let (baseline, others) = runs
        .split_first()
        .expect("a comparison holds two runs or more");
    let others: Vec<&Run> = others.iter().collect();
    let table = compare_runs(baseline, &others).map_err(|refusal| ApiError::RunsNotComparable {
        detail: refusal.to_string(),
    })?;

    let index = lineage::pipelines_by_hash(state.backends.pipelines.as_ref()).await?;
    let names: Vec<Option<String>> = runs
        .iter()
        .map(|run| lineage::pipeline_of(&index, run))
        .collect();
    if let Some(pairing) = &request.pairing {
        check_pairing(&state, pairing, &names).await?;
    }

    let mut compared = Vec::with_capacity(runs.len());
    for (run, name) in runs.into_iter().zip(names) {
        let pipeline = handlers::lower(&run)?;
        let traces = Arc::new(handlers::read_traces(&run)?);
        compared.push(Compared {
            metrics: Metrics::of(run.metrics.iter().map(|(name, _)| name)),
            stages: Stages::of(&pipeline),
            pipeline,
            traces,
            run,
            name,
        });
    }

    let pairings = pairings(&state, &compared, request.pairing.as_ref()).await?;
    let figures = figures(&state, &compared).await?;
    let (ground_truth, figures, cache_errors) = figures;

    let pairs: Vec<&[NodePair]> = compared
        .iter()
        .enumerate()
        .map(|(index, run)| {
            pairings
                .iter()
                .find(|pairing| index > 0 && Some(&pairing.other) == run.name.as_ref())
                .map_or(&[][..], |pairing| &pairing.pairs[..])
        })
        .collect();
    let columns: Vec<Column<'_>> = compared
        .iter()
        .zip(&pairs)
        .map(|(run, pairs)| Column {
            stages: &run.stages,
            pairs,
        })
        .collect();
    let stages = comparison::align(&columns)
        .into_iter()
        .map(|row| stage_row(row, figures.as_deref()))
        .collect();

    let response = Comparison {
        baseline: compared[0].run.id.to_string(),
        runs: compared
            .iter()
            .map(|run| ComparedRun {
                id: run.run.id.to_string(),
                pipeline_hash: run.run.inputs.pipeline.to_string(),
                pipeline: run.name.clone(),
            })
            .collect(),
        ground_truth,
        metrics: convert::metric_rows(&table),
        configuration: convert::configuration_matrix(&table.configuration),
        stages,
        pairings,
        query_deltas: figures
            .as_deref()
            .map(|figures| query_deltas(&compared, figures))
            .unwrap_or_default(),
        latency: compared.iter().map(latency).collect(),
        cache_errors,
    };

    // Last: every refusal above has had its chance.
    if let Some(pairing) = &request.pairing {
        keep(&state, pairing).await?;
    }
    Ok(Json(response))
}

/// The run ids in the response's order — the baseline first, then the
/// others as given — once the request's shape is checked: two to
/// [`CEILING`] ids, each once, the baseline among them.
fn order(request: &CompareRequest) -> Result<Vec<String>, ApiError> {
    if request.run_ids.len() > CEILING {
        return Err(ApiError::RunsNotComparable {
            detail: format!(
                "{} runs: a comparison holds a baseline and at most four runs, {CEILING} in all",
                request.run_ids.len()
            ),
        });
    }
    if request.run_ids.len() < 2 {
        return Err(ApiError::RequestInvalid {
            detail: format!(
                "{} run: a comparison needs a baseline and at least one other run",
                request.run_ids.len()
            ),
        });
    }
    let mut seen = BTreeSet::new();
    if let Some(twice) = request.run_ids.iter().find(|id| !seen.insert(*id)) {
        return Err(ApiError::RequestInvalid {
            detail: format!("run {twice} is named twice in `run_ids`"),
        });
    }
    if !seen.contains(&request.baseline) {
        return Err(ApiError::RequestInvalid {
            detail: format!("the baseline {} is not among `run_ids`", request.baseline),
        });
    }
    Ok(std::iter::once(request.baseline.clone())
        .chain(
            request
                .run_ids
                .iter()
                .filter(|id| **id != request.baseline)
                .cloned(),
        )
        .collect())
}

/// Checks the request's pairing, writing nothing: two different pipelines,
/// the baseline's and another compared run's (`names`, in the response's
/// order) — the pairing this comparison applies, so one kept through it is
/// one it shows — both in the workspace, and, unless it is a reset, every
/// pair naming a node of its pipeline at a stage a pair may move
/// (`stages.rs`), no node twice.
async fn check_pairing(
    state: &AppState,
    pairing: &Pairing,
    names: &[Option<String>],
) -> Result<(), ApiError> {
    if pairing.pipeline == pairing.other {
        return Err(ApiError::RequestInvalid {
            detail: format!(
                "a pairing is between two pipelines, and both are {}",
                pairing.pipeline
            ),
        });
    }
    let baseline = names[0].as_deref();
    let other_compared = |name: &str| names[1..].iter().any(|each| each.as_deref() == Some(name));
    let side_by_side = (baseline == Some(&pairing.pipeline) && other_compared(&pairing.other))
        || (baseline == Some(&pairing.other) && other_compared(&pairing.pipeline));
    if !side_by_side {
        return Err(ApiError::RequestInvalid {
            detail: format!(
                "a pairing of {} with {} is kept through a comparison that sets them side by \
                 side: one must be the baseline's pipeline ({}) and the other a compared run's",
                pairing.pipeline,
                pairing.other,
                baseline.unwrap_or("none in this workspace")
            ),
        });
    }
    let mut stages = Vec::with_capacity(2);
    for name in [&pairing.pipeline, &pairing.other] {
        let file = state.backends.pipelines.read(name).await?;
        stages.push(Stages::of(&validation::lower(&file.document)?));
    }
    for (side, (name, stages)) in [&pairing.pipeline, &pairing.other]
        .into_iter()
        .zip(&stages)
        .enumerate()
    {
        let mut seen = BTreeSet::new();
        for pair in &pairing.pairs {
            let node = if side == 0 { &pair.node } else { &pair.other };
            if stages
                .pairable_stage_of(&ragondin_pipeline::NodeId::new(node))
                .is_none()
            {
                return Err(ApiError::RequestInvalid {
                    detail: format!(
                        "pipeline {name} has no retriever, fusion or reranker `{node}` to pair"
                    ),
                });
            }
            if !seen.insert(node) {
                return Err(ApiError::RequestInvalid {
                    detail: format!("node `{node}` of pipeline {name} is paired twice"),
                });
            }
        }
    }
    Ok(())
}

/// Keeps a pairing [`check_pairing`] accepted — or, with no pairs, removes
/// the one kept, in either direction.
async fn keep(state: &AppState, pairing: &Pairing) -> Result<(), ApiError> {
    let pipelines = &state.backends.pipelines;
    if pairing.pairs.is_empty() {
        pipelines
            .delete_pairing(&pairing.pipeline, &pairing.other)
            .await
    } else {
        pipelines.write_pairing(pairing).await
    }
}

/// The pairings between the baseline's pipeline and each other run's,
/// oriented from the baseline's, one per pipeline: the request's own for
/// the pair it names — it is kept once the response is built, and a reset
/// is none — and the one kept on disk for every other.
async fn pairings(
    state: &AppState,
    runs: &[Compared],
    requested: Option<&Pairing>,
) -> Result<Vec<Pairing>, ApiError> {
    let Some(baseline) = &runs[0].name else {
        return Ok(Vec::new());
    };
    let mut pairings: Vec<Pairing> = Vec::new();
    let mut seen = BTreeSet::new();
    for run in &runs[1..] {
        let Some(other) = &run.name else { continue };
        if other == baseline || !seen.insert(other) {
            continue;
        }
        let pairing = match requested {
            Some(requested)
                if (&requested.pipeline, &requested.other) == (baseline, other)
                    || (&requested.pipeline, &requested.other) == (other, baseline) =>
            {
                (!requested.pairs.is_empty()).then(|| oriented(requested, baseline))
            }
            _ => {
                state
                    .backends
                    .pipelines
                    .read_pairing(baseline, other)
                    .await?
            }
        };
        pairings.extend(pairing);
    }
    Ok(pairings)
}

/// `pairing`, oriented from the pipeline `from`: each pair turned around
/// when it was given from the other one.
fn oriented(pairing: &Pairing, from: &str) -> Pairing {
    if pairing.pipeline == from {
        return pairing.clone();
    }
    Pairing {
        pipeline: pairing.other.clone(),
        other: pairing.pipeline.clone(),
        pairs: pairing
            .pairs
            .iter()
            .map(|pair| NodePair {
                node: pair.other.clone(),
                other: pair.node.clone(),
                label: pair.label.clone(),
            })
            .collect(),
    }
}

/// Whether the benchmark on disk is the runs' — they share one
/// `dataset_version`, `compare_runs` saw to that — and, when it is, each
/// run's per-query scores and per-node figures, with the cache's failures.
async fn figures(
    state: &AppState,
    runs: &[Compared],
) -> Result<
    (
        crate::response::DatasetCheck,
        Option<Vec<cache::Figures>>,
        Vec<String>,
    ),
    ApiError,
> {
    let inputs = runs[0].run.inputs.clone();
    match state
        .backends
        .registry
        .dataset(&inputs.dataset_version)
        .await?
    {
        RunDataset::Verified { name, dataset } => {
            let check = convert::ground_verified(&name, &inputs);
            let workspace = state.config.workspace.clone();
            let work: Vec<_> = runs
                .iter()
                .map(|run| {
                    (
                        cache::Key::of(&state.config.build, &run.run),
                        run.pipeline.clone(),
                        Arc::clone(&run.traces),
                        run.metrics.clone(),
                        Outputs::of(&run.pipeline),
                    )
                })
                .collect();
            let (figures, failures) = handlers::work(move || {
                let mut figures = Vec::with_capacity(work.len());
                let mut failures = Vec::new();
                for (key, pipeline, traces, metrics, outputs) in &work {
                    let (computed, failure) = handlers::figures(
                        &workspace,
                        key,
                        pipeline,
                        traces,
                        metrics,
                        outputs,
                        dataset.benchmark(),
                    );
                    figures.push(computed);
                    failures.extend(failure);
                }
                Ok((figures, failures))
            })
            .await?;
            Ok((check, Some(figures), failures))
        }
        other => Ok((convert::unverified(&other, &inputs), None, Vec::new())),
    }
}

/// An aligned row, with each node's metrics from its run's figures.
fn stage_row(row: comparison::Row, figures: Option<&[cache::Figures]>) -> StageRow {
    StageRow {
        stage: match row.stage {
            Stage::RetrievalLegs => StageName::RetrievalLegs,
            Stage::AfterFusion => StageName::AfterFusion,
            Stage::AfterRerank => StageName::AfterRerank,
            Stage::FinalRanking => StageName::FinalRanking,
            Stage::Answer => StageName::Answer,
        },
        label: row.label,
        source: if row.manual {
            PairingSource::Manual
        } else {
            PairingSource::Automatic
        },
        confidence: if row.guessed {
            Confidence::Low
        } else {
            Confidence::High
        },
        cells: row
            .cells
            .into_iter()
            .enumerate()
            .map(|(column, cell)| match cell {
                None => StageCell::Absent,
                Some(nodes) => {
                    let run = figures.and_then(|figures| figures.get(column));
                    stage_cell(nodes, run.map(|figures| &figures.nodes[..]))
                }
            })
            .collect(),
    }
}

/// A present cell: each node with its metrics, and per metric the best of
/// them.
fn stage_cell(
    nodes: Vec<(ragondin_pipeline::NodeId, bool)>,
    figures: Option<&[NodeFigures]>,
) -> StageCell {
    let nodes: Vec<StageNode> = nodes
        .into_iter()
        .map(|(node, paired_by_hand)| StageNode {
            metrics: figures
                .and_then(|figures| figures.iter().find(|each| each.node == node.as_str()))
                .and_then(|figures| figures.metrics.clone()),
            node: node.as_str().to_owned(),
            paired_by_hand,
        })
        .collect();
    let mut best: BTreeMap<String, StageValue> = BTreeMap::new();
    for node in &nodes {
        for (metric, value) in node.metrics.iter().flatten() {
            // One rule for which way a metric improves: the metric table's.
            let better = best
                .get(metric)
                .is_none_or(|held| match Direction::of(metric) {
                    Direction::HigherIsBetter => *value > held.value,
                    Direction::LowerIsBetter => *value < held.value,
                });
            if better {
                best.insert(
                    metric.clone(),
                    StageValue {
                        node: node.node.clone(),
                        value: *value,
                    },
                );
            }
        }
    }
    StageCell::Present { nodes, best }
}

/// Each run other than the baseline: per ranking metric both recorded, each
/// query's score minus the baseline's, by query id, and the seven bins.
fn query_deltas(runs: &[Compared], figures: &[cache::Figures]) -> Vec<RunDeltas> {
    let baseline = &figures[0];
    let recorded: BTreeSet<String> = runs[0].metrics.ranking_names().into_iter().collect();
    runs.iter()
        .zip(figures)
        .skip(1)
        .map(|(run, figures)| RunDeltas {
            run: run.run.id.to_string(),
            metrics: run
                .metrics
                .ranking_names()
                .into_iter()
                .filter(|metric| recorded.contains(metric))
                .map(|metric| {
                    let deltas: Vec<(String, f64)> = figures
                        .queries
                        .iter()
                        .filter_map(|(query, scores)| {
                            let score = scores.get(&metric)?;
                            let base = baseline.queries.get(query)?.get(&metric)?;
                            Some((query.clone(), score - base))
                        })
                        .collect();
                    let bins = comparison::bins(&deltas)
                        .into_iter()
                        .zip(BINS)
                        .map(|((bin, queries), (_, lower, upper))| DeltaBin {
                            bin,
                            lower,
                            upper,
                            count: queries.len() as u64,
                            queries,
                        })
                        .collect();
                    MetricDeltas {
                        metric,
                        judged_queries: deltas.len() as u64,
                        deltas: deltas
                            .into_iter()
                            .map(|(query, delta)| QueryDelta { query, delta })
                            .collect(),
                        bins,
                    }
                })
                .collect(),
        })
        .collect()
}

/// A run's latency: each node of its pipeline that ran, in the canonical
/// order, with the median of its durations over the queries.
fn latency(run: &Compared) -> RunLatency {
    RunLatency {
        run: run.run.id.to_string(),
        nodes: run
            .pipeline
            .nodes()
            .iter()
            .filter_map(|node| {
                let durations: Vec<u64> = run
                    .traces
                    .values()
                    .filter_map(|trace| {
                        trace
                            .nodes
                            .iter()
                            // A node that ran more than once in a query
                            // counts its first run: no node does today.
                            .find(|entry| &entry.node == node.id())
                            .map(|entry| entry.duration_nanos)
                    })
                    .collect();
                let queries = durations.len() as u64;
                comparison::median(durations).map(|median_nanos| NodeLatency {
                    node: node.id().as_str().to_owned(),
                    family: convert::family(node),
                    median_nanos,
                    queries,
                })
            })
            .collect(),
    }
}
