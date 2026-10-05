//! `GET /pipelines/{name}/matrix`: one workspace pipeline's node × benchmark
//! matrix over the stored runs (ADR-C39 § 6, the design document § 3).
//!
//! A run fills a cell of the pipeline *N*, whose document now lowers to the
//! canonical hash *H*, only from the current content (ADR-C39 § 6):
//!
//! - **its pipeline hash is *H***, whatever name its launch record gives or
//!   none — a run of the current canonical form;
//! - **it is a prefix of the current form**: its record's
//!   `prefix_of.parent_pipeline_hash` is *H*, or, failing that, its lowered
//!   graph is a prefix of *N*'s current one by the structural test
//!   (`lineage::is_prefix`), asked of every run whatever its record says
//!   (§ 5).
//!
//! A run whose record names *N* and that is neither feeds the matrix without
//! filling a cell: "launched as *N*; content since changed" (§ 7), with its
//! parameter difference against the current document by `compare_runs`'
//! configuration matrix, and a benchmark whose only runs are such runs reads
//! "not run on this version", linking the most recent.
//!
//! Each benchmark's column is the most recent run of the whole current
//! form on it, or, with none, the most recent prefix
//! (`matrix::most_recent_first`), so `missing` never names a launch that
//! exists; the others are listed among the feeding runs, not used. A cell's figure is the per-node figure
//! `GET /runs/{id}/queries` serves — the same function, the same cache — and
//! its gain is taken over the previous stage by `comparison::gain`, over the
//! stages `POST /compare` aligns runs by.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use ragondin_experiments::{compare_runs, terminal, ConfigDocument, Run};
use ragondin_pipeline::{produced_kind, LogicalPipeline, NodeId, ValueKind};

use ragondin_benchmarks::CarriedPieces;

use crate::backends::RunDataset;
use crate::comparison::{self, Gain};
use crate::derived::{Metrics, NodeFigures, Outputs};
use crate::error::ApiError;
use crate::extract::{ApiPath, ApiQuery};
use crate::handlers::{self, AppState};
use crate::jobs::{Job, JobState, Work};
use crate::matrix::{most_recent_first, topological, Recency};
use crate::request::{IncludeAvailable, PipelineMatrixParameters};
use crate::response::{
    ConfigurationMatrix, ContentSinceChanged, FailedAttempt, FeedingRun, GroundTruth, MatrixCell,
    MatrixColumn, MatrixGain, MatrixRow, MissingCells, PipelineMatrix, PrefixOf,
    SinceChangedLaunch,
};
use crate::stages::Stages;
use crate::{cache, convert, lineage, validation};

/// A stored run that counts for the pipeline, and how.
struct Counted {
    run: Run,
    /// The run's id, as the most-recent-run rule reads it.
    id: String,
    standing: Standing,
}

/// How a run counts for the pipeline.
enum Standing {
    /// It fills cells, with the pipeline it ran: the current one for a run of
    /// its canonical form, its own for a prefix.
    Fills {
        pipeline: LogicalPipeline,
        /// The node a prefix run stops at; `None` for the current form.
        up_to: Option<NodeId>,
    },
    /// Launched as the pipeline, with content that has since changed: how,
    /// and its parameter difference against the current document. It fills
    /// nothing.
    SinceChanged(ContentSinceChanged),
}

impl Counted {
    fn new(run: Run, standing: Standing) -> Self {
        Self {
            id: run.id.to_string(),
            run,
            standing,
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
    let workspace_index = lineage::index(state.backends.pipelines.as_ref()).await?;
    let names_by_hash = &workspace_index.by_hash;
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
        .filter_map(|run| self::counted(run, &name, &hash, &current, &file.document))
        .collect();
    counted.sort_by(|a, b| most_recent_first(a.recency(), b.recency()));
    // Sorted most recent first: on each benchmark the most recent run of the
    // whole current form fills the column, or, with none, the most recent
    // prefix; on a benchmark with neither, the most recent run of earlier
    // content is the one linked.
    let mut whole: BTreeMap<String, usize> = BTreeMap::new();
    let mut prefixes: BTreeMap<String, usize> = BTreeMap::new();
    let mut since_changed: BTreeMap<String, usize> = BTreeMap::new();
    for (index, each) in counted.iter().enumerate() {
        let version = each.run.inputs.dataset_version.clone();
        let slot = match &each.standing {
            Standing::Fills { up_to: None, .. } => &mut whole,
            Standing::Fills { up_to: Some(_), .. } => &mut prefixes,
            Standing::SinceChanged { .. } => &mut since_changed,
        };
        slot.entry(version).or_insert(index);
    }
    let mut filling = prefixes;
    filling.extend(
        whole
            .iter()
            .map(|(version, index)| (version.clone(), *index)),
    );

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
    // Whether the pipeline can be scored is ADR-C30 § 5's question, asked of
    // a column no run of the current content fills: it is never a question
    // for a pipeline ending in an answer, so only another reads a dataset no
    // run of it used.
    let ends_in_answer =
        terminal(&current).is_some_and(|node| produced_kind(node) == ValueKind::Answer);

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
    for (version, index) in &since_changed {
        if filling.contains_key(version) {
            continue;
        }
        let carried = carried_when_asked(&state, version, ends_in_answer).await?;
        columns.push(unfilled_column(
            version,
            names_of(version),
            carried,
            ends_in_answer,
            order.len(),
            MatrixCell::NotRunOnThisVersion {
                run: counted[*index].id.clone(),
            },
        ));
    }
    if include_available {
        for (version, names) in &pinned {
            if filling.contains_key(version) || since_changed.contains_key(version) {
                continue;
            }
            let benchmark = names[0].clone();
            let carried = carried_when_asked(&state, version, ends_in_answer).await?;
            columns.push(unfilled_column(
                version,
                names.clone(),
                carried,
                ends_in_answer,
                order.len(),
                MatrixCell::NotRunYet { benchmark },
            ));
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
    let attempts = failed_attempts(&state.jobs.jobs().await, &hash);
    for column in &mut columns {
        if whole.contains_key(&column.dataset_version) {
            continue;
        }
        column.failed_attempt = column
            .benchmark_names
            .iter()
            .filter_map(|name| attempts.get(name))
            .max_by_key(|(recency, _)| *recency)
            .and_then(|(_, failed)| failed.clone());
    }

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
                        MatrixCell::NotRunYet { .. }
                            | MatrixCell::PrefixStops { .. }
                            | MatrixCell::NotRunOnThisVersion { .. }
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
                launched_as: each
                    .run
                    .provenance
                    .as_ref()
                    .map(|record| convert::launched_as(record, Some(&workspace_index))),
                pipeline_names,
                prefix_of: match &each.standing {
                    Standing::Fills {
                        up_to: Some(up_to), ..
                    } => Some(PrefixOf {
                        pipeline: name.clone(),
                        up_to: up_to.as_str().to_owned(),
                    }),
                    _ => None,
                },
                fills_column: filling.get(&each.run.inputs.dataset_version) == Some(&index),
                content_since_changed: match &each.standing {
                    Standing::SinceChanged(difference) => Some(difference.clone()),
                    Standing::Fills { .. } => None,
                },
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

/// Whether `run` counts for the pipeline `name`, whose current `document`
/// lowers to `current` and hashes to `hash`, and as what (ADR-C39 § 5 to
/// § 7): it fills cells by the current content alone, and otherwise feeds the
/// matrix only when its launch record names the pipeline.
fn counted(
    run: Run,
    name: &str,
    hash: &str,
    current: &LogicalPipeline,
    document: &str,
) -> Option<Counted> {
    if run.inputs.pipeline.to_string() == hash {
        // One canonical hash is one canonical form: the current pipeline.
        let standing = Standing::Fills {
            pipeline: current.clone(),
            up_to: None,
        };
        return Some(Counted::new(run, standing));
    }
    let record = run.provenance.as_ref();
    let recorded_prefix = record
        .and_then(|record| record.prefix_of())
        .is_some_and(|prefix| prefix.parent_pipeline_hash().to_string() == hash);
    if let Ok(pipeline) = handlers::lower(&run) {
        // The structural test is asked of every run the record does not
        // already place: a run's first record wins, so it may name another
        // parent than one it is also a prefix of.
        if recorded_prefix || lineage::is_prefix(&pipeline, current) {
            // A part of the current form with no single output — two
            // terminal nodes — is no prefix a cell can be cut at, and no
            // earlier content either: it counts nowhere.
            let up_to = terminal(&pipeline)?.id().clone();
            let standing = Standing::Fills {
                pipeline,
                up_to: Some(up_to),
            };
            return Some(Counted::new(run, standing));
        }
    }
    let record = record?;
    if record.name() != Some(name) {
        return None;
    }
    let since = ContentSinceChanged {
        // A record with `prefix_of` names the parent: the run is a prefix
        // of an earlier version, never an earlier version (ADR-C39 § 2).
        launched: if record.prefix_of().is_some() {
            SinceChangedLaunch::AsPrefix
        } else {
            SinceChangedLaunch::AsPipeline
        },
        difference: difference(&run, document),
    };
    Some(Counted::new(run, Standing::SinceChanged(since)))
}

/// The parameter difference between the pipeline's current `document` and
/// `run`'s, the current document's column first: `compare_runs`'
/// configuration matrix, the one `POST /compare` serves, over the current
/// document set in a run of its own beside `run`. Only the configuration is
/// read of that run: it carries `run`'s inputs, so the two are one
/// benchmark's and always comparable.
fn difference(run: &Run, document: &str) -> ConfigurationMatrix {
    let current = Run {
        id: run.id,
        inputs: run.inputs.clone(),
        metrics: ragondin_experiments::Metrics::default(),
        config: ConfigDocument::new(document),
        traces: BTreeMap::new(),
        bindings: Vec::new(),
        times: None,
        provenance: None,
    };
    match compare_runs(&current, &[run]) {
        Ok(comparison) => convert::configuration_matrix(&comparison.configuration),
        Err(refusal) => ConfigurationMatrix::Unavailable {
            run: run.id.to_string(),
            reason: refusal.to_string(),
        },
    }
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
    let Standing::Fills { pipeline, up_to } = &counted.standing else {
        unreachable!("only a run that fills cells fills a column");
    };
    let metrics = Metrics::of(run.metrics.iter().map(|(name, _)| name));
    let outputs = Outputs::of(pipeline);
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
                (pipeline.clone(), metrics.clone(), outputs.clone());
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
    let answers = convert::carried(ground_truth).scores_answers();
    let stages = Stages::of(pipeline);
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
            if let Some(up_to) = up_to {
                if !pipeline.nodes().iter().any(|own| own.id() == id) {
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
                        gain: MatrixGain::Unstaged,
                        judged_queries: None,
                    }
                };
            }
            if produced_kind(node) != ValueKind::Chunks {
                return MatrixCell::NotScored;
            }
            // Without the run's own dataset nothing says whether it carries
            // qrels: what the run recorded is not inferred into `no_qrels`.
            if figures.is_none() {
                return MatrixCell::Unverified;
            }
            if !qrels {
                return MatrixCell::NoQrels;
            }
            match figure_of(id) {
                Some(NodeFigures {
                    metrics: Some(values),
                    judged_queries,
                    ..
                }) => MatrixCell::Measured {
                    metrics: values.clone(),
                    gain: match comparison::gain(&stages, id, |other| {
                        figure_of(other).and_then(|figure| figure.metrics.as_ref())
                    }) {
                        Gain::Over(values) => MatrixGain::OverPreviousStage { values },
                        Gain::FirstStage => MatrixGain::FirstStage,
                        Gain::Ambiguous => MatrixGain::Ambiguous,
                        Gain::Unstaged => MatrixGain::Unstaged,
                    },
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
            up_to: up_to.as_ref().map(|up_to| up_to.as_str().to_owned()),
            dataset_check: Some(check),
            cells,
            failed_attempt: None,
        },
        failure,
    ))
}

/// The pieces the benchmark pinned to `version` carries, read off its
/// dataset — only for a pipeline that does not end in an answer, the one
/// kind `CarriedPieces::scorable` can refuse; `None` for one that does, and
/// for a dataset that does not verify, whose pieces are unknown.
async fn carried_when_asked(
    state: &AppState,
    version: &str,
    ends_in_answer: bool,
) -> Result<Option<CarriedPieces>, ApiError> {
    if ends_in_answer {
        return Ok(None);
    }
    Ok(match state.backends.registry.dataset(version).await? {
        RunDataset::Verified { dataset, .. } => Some(dataset.benchmark().carries()),
        _ => None,
    })
}

/// A column no run of the current content fills: every cell `waiting`, or
/// `not_scorable` where the pieces `carried` say the pipeline cannot be
/// scored there (ADR-C30 § 5) — a run there would be refused, so nothing
/// waits for one.
fn unfilled_column(
    version: &str,
    benchmark_names: Vec<String>,
    carried: Option<CarriedPieces>,
    ends_in_answer: bool,
    rows: usize,
    waiting: MatrixCell,
) -> MatrixColumn {
    let scorable = carried.is_none_or(|carried| carried.scorable(ends_in_answer));
    let cell = if scorable {
        waiting
    } else {
        MatrixCell::NotScorable
    };
    MatrixColumn {
        dataset_version: version.to_owned(),
        benchmark_names,
        ground_truth: carried.map(convert::ground_truth),
        run: None,
        up_to: None,
        dataset_check: None,
        cells: vec![cell; rows],
        failed_attempt: None,
    }
}

/// When a job was accepted, as the most recent attempt is chosen by: its
/// acceptance time, an unknown one first, then its position.
type Accepted = (Option<u64>, u64);

/// Per benchmark name, the most recent run job of the whole current form —
/// its snapshotted document lowering to `hash`, the matrix's own content
/// test, whatever name it was launched under — and the failure it ended in,
/// or `None` when it did not fail: queued, running, done or cancelled.
fn failed_attempts(
    jobs: &[Job],
    hash: &str,
) -> BTreeMap<String, (Accepted, Option<FailedAttempt>)> {
    let mut latest: BTreeMap<String, (Accepted, Option<FailedAttempt>)> = BTreeMap::new();
    for job in jobs {
        let Work::Run {
            pipeline,
            benchmark,
            up_to: None,
            ..
        } = &job.work
        else {
            continue;
        };
        let current = validation::lower(pipeline)
            .is_ok_and(|lowered| lowered.content_hash().to_string() == hash);
        if !current {
            continue;
        }
        let accepted = (job.created_at.map(|at| at.get()), job.position);
        if latest
            .get(benchmark)
            .is_some_and(|(newest, _)| *newest >= accepted)
        {
            continue;
        }
        let failure = match &job.state {
            JobState::Failed { error, at_node, .. } => Some(FailedAttempt {
                job: job.id.clone(),
                error: error.clone(),
                at_node: at_node.clone(),
            }),
            _ => None,
        };
        latest.insert(benchmark.clone(), (accepted, failure));
    }
    latest
}
