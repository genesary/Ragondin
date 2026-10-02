//! `GET /pipelines/{name}/matrix`: one workspace pipeline's node × benchmark
//! matrix over the stored runs (ADR-C39 § 6, the design document § 3).
//!
//! A run counts for the pipeline *N*, whose document now lowers to the
//! canonical hash *H*, by its content alone:
//!
//! - **its pipeline hash is *H***, whatever name it was launched under or
//!   none — a run of the current canonical form;
//! - **its lowered graph is a prefix of *N*'s current one**, by the
//!   structural test (`lineage::is_prefix`), whatever its record says.
//!
//! Each benchmark's column is the most recent counted run on it
//! (`matrix::most_recent_first`); the others are listed among the feeding
//! runs, not used. A cell's figure is the per-node figure
//! `GET /runs/{id}/queries` serves — the same function, the same cache — and
//! its gain is taken over the previous stage by `comparison::gain`, over the
//! stages `POST /compare` aligns runs by.
//!
//! **Waiting on the launch record** (ADR-C39 § 1, the issue that adds it to
//! `ragondin-experiments`). This module reads no record, so the feeding runs
//! name their pipeline by content alone (`lineage`), and [`counted`]'s `None`
//! is the seam: there, a run whose record names *N* goes to the feeding runs
//! as "launched as *N*; content since changed", with its parameter difference
//! against the current document from `compare`'s configuration matrix, and a
//! benchmark whose only run is such a run reads "not run on this version".
//! And there the record's `prefix_of.parent_pipeline_hash`, when it equals
//! *H*, makes a run a prefix before the structural test is asked.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use ragondin_experiments::{terminal, Run};
use ragondin_pipeline::{produced_kind, LogicalPipeline, NodeId, ValueKind};

use crate::backends::RunDataset;
use crate::comparison;
use crate::derived::{Metrics, NodeFigures, Outputs};
use crate::error::ApiError;
use crate::extract::{ApiPath, ApiQuery};
use crate::handlers::{self, AppState};
use crate::matrix::{most_recent_first, topological, Recency};
use crate::request::{IncludeAvailable, PipelineMatrixParameters};
use crate::response::{
    FeedingRun, GroundTruth, MatrixCell, MatrixColumn, MatrixRow, MissingCells, PipelineMatrix,
    PrefixOf,
};
use crate::stages::Stages;
use crate::{cache, convert, lineage, validation};

/// A stored run that counts for the pipeline, and the pipeline it ran: the
/// current one for a run of its canonical form, its own for a prefix.
struct Counted {
    run: Run,
    /// The run's id, as the most-recent-run rule reads it.
    id: String,
    pipeline: LogicalPipeline,
    /// The node a prefix run stops at; `None` for the current form.
    up_to: Option<NodeId>,
}

impl Counted {
    fn new(run: Run, pipeline: LogicalPipeline, up_to: Option<NodeId>) -> Self {
        Self {
            id: run.id.to_string(),
            run,
            pipeline,
            up_to,
        }
    }

    fn recency(&self) -> Recency<'_> {
        Recency {
            started_at_ms: self.run.times.map(|times| times.started().get()),
            id: &self.id,
        }
    }
}

/// `GET /pipelines/{name}/matrix`.
pub(crate) async fn matrix(
    State(state): State<AppState>,
    ApiPath(name): ApiPath<String>,
    ApiQuery(parameters): ApiQuery<PipelineMatrixParameters>,
) -> Result<Json<PipelineMatrix>, ApiError> {
    let include_available = parameters
        .include_available
        .is_some_and(|IncludeAvailable(on)| on);
    let file = state.backends.pipelines.read(&name).await?;
    let current = validation::lower(&file.document)?;
    let hash = current.content_hash().to_string();
    let names_by_hash = lineage::pipelines_by_hash(state.backends.pipelines.as_ref()).await?;
    let mut pinned: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for benchmark in state.backends.registry.pinned().await? {
        pinned
            .entry(benchmark.dataset_version)
            .or_default()
            .push(benchmark.name);
    }
    for names in pinned.values_mut() {
        names.sort();
    }

    let (runs, unreadable) = handlers::load_all(&state).await?;
    let mut counted: Vec<Counted> = runs
        .into_iter()
        .filter_map(|run| self::counted(run, &hash, &current))
        .collect();
    counted.sort_by(|a, b| most_recent_first(a.recency(), b.recency()));
    // Sorted most recent first, so the first run of each benchmark fills it.
    let mut filling: BTreeMap<String, usize> = BTreeMap::new();
    for (index, each) in counted.iter().enumerate() {
        filling
            .entry(each.run.inputs.dataset_version.clone())
            .or_insert(index);
    }

    let order = topological(&current);
    let rows: Vec<MatrixRow> = order
        .iter()
        .map(|node| MatrixRow {
            node: node.id().as_str().to_owned(),
            family: convert::family(node),
            produces: convert::produces(node),
        })
        .collect();
    let names_of = |version: &str| pinned.get(version).cloned().unwrap_or_default();

    let mut columns = Vec::new();
    let mut cache_errors = Vec::new();
    for index in filling.values() {
        let (column, failure) = filled_column(
            &state,
            &counted[*index],
            &order,
            names_of(&counted[*index].run.inputs.dataset_version),
        )
        .await?;
        columns.push(column);
        cache_errors.extend(failure);
    }
    if include_available {
        for (version, names) in &pinned {
            if filling.contains_key(version) {
                continue;
            }
            let benchmark = names[0].clone();
            columns.push(MatrixColumn {
                dataset_version: version.clone(),
                benchmark_names: names.clone(),
                ground_truth: None,
                run: None,
                up_to: None,
                dataset_check: None,
                cells: order
                    .iter()
                    .map(|_| MatrixCell::NotRunYet {
                        benchmark: benchmark.clone(),
                    })
                    .collect(),
            });
        }
    }
    columns.sort_by(|a, b| {
        let key = |column: &MatrixColumn| {
            (
                column.benchmark_names.is_empty(),
                column.benchmark_names.first().cloned(),
                column.dataset_version.clone(),
            )
        };
        key(a).cmp(&key(b))
    });

    let missing = columns
        .iter()
        .filter_map(|column| {
            let nodes: Vec<String> = column
                .cells
                .iter()
                .zip(&rows)
                .filter(|(cell, _)| {
                    matches!(
                        cell,
                        MatrixCell::NotRunYet { .. } | MatrixCell::PrefixStops { .. }
                    )
                })
                .map(|(_, row)| row.node.clone())
                .collect();
            (!nodes.is_empty()).then(|| MissingCells {
                benchmark: column.benchmark_names.first().cloned(),
                dataset_version: column.dataset_version.clone(),
                nodes,
            })
        })
        .collect();

    let feeding_runs = counted
        .iter()
        .enumerate()
        .map(|(index, each)| {
            let mut pipeline_names = names_by_hash
                .get(&each.run.inputs.pipeline.to_string())
                .cloned()
                .unwrap_or_default();
            pipeline_names.sort();
            FeedingRun {
                run: each.id.clone(),
                dataset_version: each.run.inputs.dataset_version.clone(),
                benchmark_names: names_of(&each.run.inputs.dataset_version),
                started_at_ms: each.recency().started_at_ms,
                pipeline_names,
                prefix_of: each.up_to.as_ref().map(|up_to| PrefixOf {
                    pipeline: name.clone(),
                    up_to: up_to.as_str().to_owned(),
                }),
                fills_column: filling.get(&each.run.inputs.dataset_version) == Some(&index),
            }
        })
        .collect();

    Ok(Json(PipelineMatrix {
        pipeline: name,
        pipeline_hash: hash,
        rows,
        columns,
        feeding_runs,
        missing,
        unreadable,
        cache_errors,
    }))
}

/// Whether `run` counts for the pipeline whose current form is `current`,
/// hashing to `hash`, and as what: by its content alone (ADR-C39 § 5, § 6).
fn counted(run: Run, hash: &str, current: &LogicalPipeline) -> Option<Counted> {
    if run.inputs.pipeline.to_string() == hash {
        // One canonical hash is one canonical form: the current pipeline.
        return Some(Counted::new(run, current.clone(), None));
    }
    let pipeline = handlers::lower(&run).ok()?;
    if !lineage::is_prefix(&pipeline, current) {
        // The seam the module's notes describe: a run the launch record
        // names as this pipeline goes to the feeding runs from here.
        return None;
    }
    let up_to = terminal(&pipeline)?.id().clone();
    Some(Counted::new(run, pipeline, Some(up_to)))
}

/// The column `counted` fills: its ground truth, its dataset check, and a
/// cell per node of `order`, with the cache's failure if any.
async fn filled_column(
    state: &AppState,
    counted: &Counted,
    order: &[&ragondin_pipeline::LogicalNode],
    benchmark_names: Vec<String>,
) -> Result<(MatrixColumn, Option<String>), ApiError> {
    let run = &counted.run;
    let metrics = Metrics::of(run.metrics.iter().map(|(name, _)| name));
    let outputs = Outputs::of(&counted.pipeline);
    let dataset = state
        .backends
        .registry
        .dataset(&run.inputs.dataset_version)
        .await?;
    let (check, ground_truth, figures, failure) = match dataset {
        RunDataset::Verified { name, dataset } => {
            let check = convert::ground_verified(&name, &run.inputs);
            let ground_truth = convert::ground_truth(dataset.benchmark().carries());
            let workspace = state.config.workspace.clone();
            let key = cache::Key::of(&state.config.build, run);
            let traces = Arc::new(handlers::read_traces(run)?);
            let (pipeline, work_metrics, work_outputs) =
                (counted.pipeline.clone(), metrics.clone(), outputs.clone());
            let (figures, failure) = handlers::work(move || {
                Ok(handlers::figures(
                    &workspace,
                    &key,
                    &pipeline,
                    &traces,
                    &work_metrics,
                    &work_outputs,
                    dataset.benchmark(),
                ))
            })
            .await?;
            (check, ground_truth, Some(figures.nodes), failure)
        }
        other => {
            // Without the dataset, the ground truth is what the run could
            // score: the harness records a family's metrics only when the
            // benchmark carries its piece.
            let ground_truth = match (
                metrics.ranking_names().is_empty(),
                metrics.answer_names().is_empty(),
            ) {
                (false, false) => GroundTruth::Both,
                (false, true) => GroundTruth::Qrels,
                (true, false) => GroundTruth::ReferenceAnswers,
                (true, true) => GroundTruth::None,
            };
            (
                convert::unverified(&other, &run.inputs),
                ground_truth,
                None,
                None,
            )
        }
    };
    let qrels = matches!(ground_truth, GroundTruth::Qrels | GroundTruth::Both);
    let answers = matches!(
        ground_truth,
        GroundTruth::ReferenceAnswers | GroundTruth::Both
    );
    let stages = Stages::of(&counted.pipeline);
    let figure_of = |id: &NodeId| -> Option<&NodeFigures> {
        figures
            .as_ref()?
            .iter()
            .find(|figure| figure.node == id.as_str())
    };
    let cells = order
        .iter()
        .map(|node| {
            let id = node.id();
            if let Some(up_to) = &counted.up_to {
                if !counted.pipeline.nodes().iter().any(|own| own.id() == id) {
                    return MatrixCell::PrefixStops {
                        up_to: up_to.as_str().to_owned(),
                    };
                }
            }
            if outputs.answer.as_ref() == Some(id) {
                if !answers {
                    return MatrixCell::NoReferenceAnswers;
                }
                let recorded: BTreeMap<String, f64> = metrics
                    .answer_names()
                    .into_iter()
                    .filter_map(|metric| run.metrics.get(&metric).map(|value| (metric, value)))
                    .collect();
                return if recorded.is_empty() {
                    MatrixCell::NoFigure
                } else {
                    MatrixCell::Measured {
                        metrics: recorded,
                        gain: None,
                        judged_queries: None,
                    }
                };
            }
            if produced_kind(node) != ValueKind::Chunks {
                return MatrixCell::NotScored;
            }
            if !qrels {
                return MatrixCell::NoQrels;
            }
            if figures.is_none() {
                return MatrixCell::Unverified;
            }
            match figure_of(id) {
                Some(NodeFigures {
                    metrics: Some(values),
                    judged_queries,
                    ..
                }) => MatrixCell::Measured {
                    metrics: values.clone(),
                    gain: comparison::gain(&stages, id, |other| {
                        figure_of(other).and_then(|figure| figure.metrics.as_ref())
                    }),
                    judged_queries: Some(*judged_queries),
                },
                _ => MatrixCell::NoFigure,
            }
        })
        .collect();
    Ok((
        MatrixColumn {
            dataset_version: run.inputs.dataset_version.clone(),
            benchmark_names,
            ground_truth: Some(ground_truth),
            run: Some(counted.id.clone()),
            up_to: counted
                .up_to
                .as_ref()
                .map(|up_to| up_to.as_str().to_owned()),
            dataset_check: Some(check),
            cells,
        },
        failure,
    ))
}
