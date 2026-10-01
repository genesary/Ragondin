//! Run comparison: [`compare`] for two runs, [`compare_runs`] for several
//! against a baseline.
//!
//! The diff between runs is the view the platform exists for
//! (`docs/system-architecture.md` §6.5): *this configuration against that one,
//! side by side*. This module is its data — the `ragondin compare` command
//! renders what [`compare`] returns, and `ragondin-api`'s `POST /compare`
//! what [`compare_runs`] returns. The two are one computation: [`compare`] is
//! the two-run case of the same metric table and configuration matrix.
//!
//! It compares **metrics**, and **which configuration parameters differ**:
//! the node-level parameters, `impl:` names and component families one run's
//! stored configuration holds and the other does not, or holds with another
//! value. The graph's wiring — a node's `inputs`, the pipeline's declared
//! inputs — is not listed parameter by parameter; whether the two canonical
//! forms hash equal is carried beside the list, so a difference there is
//! still reported. Both documents are lowered to [`LogicalPipeline`] first,
//! through `ragondin-config`'s [`parse_document`] — into `ragondin-pipeline`'s
//! wire schema, then through its validation pass — so the difference is one
//! between canonical logical forms and never between texts (the spirit of
//! INV-8): a document respelled — keys reordered, flow style for block style —
//! differs in nothing.

use std::collections::{BTreeMap, BTreeSet};

use ragondin_config::{parse_document, DocumentError};
use ragondin_pipeline::{LogicalNode, LogicalPipeline, NodeId, ParamValue};

use crate::run::{ConfigDocument, Run, RunId};

/// Compares two runs, metric by metric and parameter by parameter.
///
/// Every metric either run recorded appears in the result, in name order, so a
/// caller can render the two columns without deciding what to show.
///
/// Infallible even when a stored configuration does not lower — a run stored
/// under an older schema, say: the metrics are still compared, and
/// [`RunComparison::configuration`] says which side could not be read and why.
///
/// The two-run case of [`compare_runs`], computed by the same table and the
/// same matrix, the left-hand run in the baseline's place. Unlike it, two
/// runs of different benchmarks are still compared: `ragondin compare` has
/// always answered for any two stored runs, and its output does not change.
pub fn compare(left: &Run, right: &Run) -> RunComparison {
    let runs = [left, right];
    RunComparison {
        left: left.id,
        right: right.id,
        metrics: table(&runs)
            .into_iter()
            .map(|row| MetricComparison {
                name: row.name,
                left: row.values[0],
                right: row.values[1],
            })
            .collect(),
        configuration: match matrix(&runs) {
            ConfigurationMatrix::Compared {
                parameters,
                same_logical_form,
            } => ConfigurationComparison::Compared {
                differences: parameters
                    .into_iter()
                    .map(|row| {
                        let mut values = row.values.into_iter();
                        ParameterDifference {
                            node: row.node,
                            key: row.key,
                            left: values.next().flatten(),
                            right: values.next().flatten(),
                        }
                    })
                    .collect(),
                same_logical_form,
            },
            ConfigurationMatrix::Unavailable { column, reason, .. } => {
                ConfigurationComparison::Unavailable {
                    side: if column == 0 { Side::Left } else { Side::Right },
                    reason,
                }
            }
        },
    }
}

/// Compares runs against a baseline: every metric any of them recorded, one
/// value per run, with the best of each row and each run's delta to the
/// baseline ([`MetricRow`]); and every configuration parameter not identical
/// across them, with each run's value ([`ConfigurationMatrix`]).
///
/// The columns are the baseline first, then `others` in the order given.
/// Like [`compare`], it is not refused when a stored configuration does not
/// lower: the metrics are still compared, and the matrix names the run.
///
/// # Errors
///
/// [`NotComparable::DatasetsDiffer`] when a run was evaluated on another
/// benchmark than the baseline — another `dataset_version` — naming the
/// first such run and both versions. A metric is what a benchmark's ground
/// truth allows, so two benchmarks' figures side by side would compare the
/// benchmarks rather than the runs.
pub fn compare_runs(baseline: &Run, others: &[&Run]) -> Result<Comparison, NotComparable> {
    if let Some(run) = others
        .iter()
        .find(|run| run.inputs.dataset_version != baseline.inputs.dataset_version)
    {
        return Err(NotComparable::DatasetsDiffer {
            baseline: baseline.id,
            baseline_version: baseline.inputs.dataset_version.clone(),
            run: run.id,
            run_version: run.inputs.dataset_version.clone(),
        });
    }
    let runs: Vec<&Run> = std::iter::once(baseline)
        .chain(others.iter().copied())
        .collect();
    Ok(Comparison {
        runs: runs.iter().map(|run| run.id).collect(),
        metrics: table(&runs),
        configuration: matrix(&runs),
    })
}

/// Every metric any of `runs` recorded, in name order, one value per run.
fn table(runs: &[&Run]) -> Vec<MetricRow> {
    let names: BTreeSet<&str> = runs
        .iter()
        .flat_map(|run| run.metrics.iter().map(|(name, _)| name))
        .collect();
    names
        .into_iter()
        .map(|name| MetricRow {
            name: name.to_owned(),
            direction: Direction::of(name),
            values: runs.iter().map(|run| run.metrics.get(name)).collect(),
        })
        .collect()
}

/// Every parameter not identical across `runs`' lowered configurations,
/// sorted by node id and then by key — or the first run whose configuration
/// does not lower.
fn matrix(runs: &[&Run]) -> ConfigurationMatrix {
    let mut pipelines = Vec::with_capacity(runs.len());
    for (column, run) in runs.iter().enumerate() {
        match lower_configuration(&run.config) {
            Ok(pipeline) => pipelines.push(pipeline),
            Err(reason) => {
                return ConfigurationMatrix::Unavailable {
                    run: run.id,
                    column,
                    reason,
                }
            }
        }
    }
    let same_logical_form = pipelines
        .windows(2)
        .all(|pair| pair[0].content_hash() == pair[1].content_hash());
    let parameters: Vec<_> = pipelines.iter().map(parameters).collect();
    let keys: BTreeSet<&(NodeId, ParameterKey)> =
        parameters.iter().flat_map(|each| each.keys()).collect();
    let rows = keys
        .into_iter()
        .filter_map(|entry| {
            let values: Vec<Option<&ParamValue>> =
                parameters.iter().map(|each| each.get(entry)).collect();
            values
                .windows(2)
                .any(|pair| pair[0] != pair[1])
                .then(|| ParameterRow {
                    node: entry.0.clone(),
                    key: entry.1.clone(),
                    values: values
                        .into_iter()
                        .map(Option::<&ParamValue>::cloned)
                        .collect(),
                })
        })
        .collect();
    ConfigurationMatrix::Compared {
        parameters: rows,
        same_logical_form,
    }
}

/// Lowers a stored configuration document to its [`LogicalPipeline`]:
/// `ragondin-config`'s [`parse_document`], the one definition of the load,
/// run over the kept text. The `Err` is its verdict, in a sentence about a
/// stored run: a run stored under a version this build cannot read says that
/// rather than reporting a syntax error.
///
/// Public because a reader of a stored run needs the same lowering
/// [`compare`] does — `ragondin-api` draws a run's graph from it — and one
/// path is kept rather than a second written beside it.
pub fn lower_configuration(document: &ConfigDocument) -> Result<LogicalPipeline, String> {
    parse_document(document.as_str()).map_err(|error| match error {
        DocumentError::UnsupportedSchemaVersion(source) => {
            format!("stored under a schema version this build cannot read: {source}")
        }
        DocumentError::Malformed(source) => {
            format!("the stored configuration does not parse: {source}")
        }
        DocumentError::Invalid(source) => {
            format!("the stored configuration does not validate: {source}")
        }
    })
}

/// Every node's component family, `impl:` name and parameters, keyed by node
/// and then by key.
///
/// An extension node's `kind` is where its `impl:` value lands on lowering, so
/// it is that node's [`ParameterKey::Impl`].
fn parameters(pipeline: &LogicalPipeline) -> BTreeMap<(NodeId, ParameterKey), ParamValue> {
    let mut parameters = BTreeMap::new();
    for node in pipeline.nodes() {
        // The family is spelled as the configuration's `component:` value.
        let (component, implementation, params) = match node {
            LogicalNode::Retriever(node) => ("retriever", &node.implementation, &node.params),
            LogicalNode::Fusion(node) => ("fusion", &node.implementation, &node.params),
            LogicalNode::Reranker(node) => ("reranker", &node.implementation, &node.params),
            LogicalNode::ContextBuilder(node) => {
                ("context_builder", &node.implementation, &node.params)
            }
            LogicalNode::Generator(node) => ("generator", &node.implementation, &node.params),
            LogicalNode::Extension(node) => ("extension", &node.kind, &node.params),
        };
        let id = node.id();
        parameters.insert(
            (id.clone(), ParameterKey::Component),
            ParamValue::String(component.to_owned()),
        );
        parameters.insert(
            (id.clone(), ParameterKey::Impl),
            ParamValue::String(implementation.clone()),
        );
        for (key, value) in params {
            parameters.insert(
                (id.clone(), ParameterKey::Param(key.clone())),
                value.clone(),
            );
        }
    }
    parameters
}

/// What two runs scored, metric by metric, and which configuration
/// parameters they differ in.
#[derive(Clone, Debug, PartialEq)]
pub struct RunComparison {
    /// The run on the left-hand side.
    pub left: RunId,
    /// The run on the right-hand side.
    pub right: RunId,
    /// One entry per metric either run recorded, in name order.
    pub metrics: Vec<MetricComparison>,
    /// Which configuration parameters the two runs differ in.
    pub configuration: ConfigurationComparison,
}

impl RunComparison {
    /// Whether the two runs agree on every metric.
    ///
    /// Metrics only: whether their configurations agree is
    /// [`configuration`](Self::configuration)'s to say.
    ///
    /// Two runs with no metrics at all agree vacuously — there is nothing they
    /// disagree about, and reporting a difference would be inventing one.
    pub fn is_identical(&self) -> bool {
        self.metrics.iter().all(MetricComparison::is_identical)
    }

    /// The metrics the two runs do not agree on.
    pub fn differences(&self) -> impl Iterator<Item = &MetricComparison> {
        self.metrics.iter().filter(|metric| !metric.is_identical())
    }
}

/// One metric, as each run recorded it.
///
/// A side is `None` when that run did not record the metric — which is not the
/// same as recording zero, and is why this is an `Option` rather than a
/// defaulted number.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricComparison {
    /// The metric's name.
    pub name: String,
    /// What the left-hand run scored, if it recorded this metric.
    pub left: Option<f64>,
    /// What the right-hand run scored, if it recorded this metric.
    pub right: Option<f64>,
}

impl MetricComparison {
    /// Right minus left, when both runs recorded the metric.
    ///
    /// The sign therefore reads as *what moved when going from left to right*,
    /// which is the direction a comparison is read in: the left-hand run is
    /// the baseline and the right-hand one is the candidate.
    pub fn delta(&self) -> Option<f64> {
        match (self.left, self.right) {
            (Some(left), Some(right)) => Some(right - left),
            _ => None,
        }
    }

    /// Whether both runs recorded this metric with the same value.
    ///
    /// Compared exactly, not within a tolerance: these are two recorded
    /// numbers read back from a store, not two computations of one number, and
    /// a store that smoothed a difference away would hide the thing it is
    /// being asked about. A metric recorded as `NaN` is therefore never
    /// identical to itself, which is IEEE 754 and not a decision taken here;
    /// two runs *read back from a store* cannot hit it, because the store
    /// refuses a non-finite metric on the way in
    /// ([`NotFinite`](crate::RunStoreError::NotFinite)).
    pub fn is_identical(&self) -> bool {
        matches!((self.left, self.right), (Some(left), Some(right)) if left == right)
    }
}

/// Which configuration parameters two runs differ in, or why that could not be
/// said.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigurationComparison {
    /// Both stored documents lowered.
    Compared {
        /// One entry per parameter — `impl:` name included — that one run's
        /// configuration holds and the other does not, or holds with another
        /// value, sorted by node id and then by key. Empty when the two
        /// configurations agree on every one.
        differences: Vec<ParameterDifference>,
        /// Whether the two canonical logical forms hash equal
        /// ([`LogicalPipeline::content_hash`]). No differing parameter does
        /// not make two configurations one: a node's `inputs` or the declared
        /// inputs can still differ, and this is what says so.
        same_logical_form: bool,
    },
    /// One side's stored document does not lower under this build — the left
    /// one, when neither does.
    Unavailable {
        /// The run whose configuration could not be read.
        side: Side,
        /// What the parser or the validation pass said.
        reason: String,
    },
}

/// One side of a comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The left-hand run: the baseline.
    Left,
    /// The right-hand run: the candidate.
    Right,
}

/// One configuration parameter two runs disagree on.
///
/// A side is `None` when that run's configuration does not set it — its node
/// is absent, or the node sets no such parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterDifference {
    /// The node the parameter belongs to, by its id.
    pub node: NodeId,
    /// Which of the node's parameters.
    pub key: ParameterKey,
    /// The left-hand run's value, if it sets one.
    pub left: Option<ParamValue>,
    /// The right-hand run's value, if it sets one.
    pub right: Option<ParamValue>,
}

/// A node's parameter, as a configuration spells it.
///
/// `component:`, `impl:` and `params:` are separate keys in a node, so a
/// parameter named `impl` under `params:` stays distinct from the node's
/// `impl:` name. The order puts `component:` first, then `impl:`, then the
/// parameters by name.
///
/// The family is a key because the canonical form hashes a node's variant: a
/// node lowered as an extension rather than a retriever, same `impl:` and
/// same `params:`, is another configuration, and without this key it would
/// differ in no listed parameter.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParameterKey {
    /// The node's `component:` family — `retriever`, `fusion`, `reranker`,
    /// `context_builder`, `generator` or `extension` — held as a
    /// [`ParamValue::String`].
    Component,
    /// The node's `impl:` name, held as a [`ParamValue::String`].
    Impl,
    /// A key under the node's `params:`.
    Param(String),
}

/// Runs compared against a baseline: [`compare_runs`]'s answer.
///
/// Every list of values is in the order of [`runs`](Self::runs): the
/// baseline first, then the other runs in the order they were given.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// The runs compared, the baseline first.
    pub runs: Vec<RunId>,
    /// One row per metric any of the runs recorded, in name order.
    pub metrics: Vec<MetricRow>,
    /// The configuration parameters not identical across the runs.
    pub configuration: ConfigurationMatrix,
}

/// One metric across the runs compared.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricRow {
    /// The metric's name.
    pub name: String,
    /// Which way the metric improves, read off its name.
    pub direction: Direction,
    /// Each run's value, `None` where that run did not record the metric —
    /// which is not zero, and is never shown as zero.
    pub values: Vec<Option<f64>>,
}

impl MetricRow {
    /// The columns holding the best value of the row, by its
    /// [`direction`](Self::direction): every one of them on a tie, none when
    /// no run recorded the metric. A run that did not record it is never the
    /// best.
    pub fn best(&self) -> Vec<usize> {
        let best =
            self.values
                .iter()
                .flatten()
                .copied()
                .reduce(|best, value| match self.direction {
                    Direction::HigherIsBetter => best.max(value),
                    Direction::LowerIsBetter => best.min(value),
                });
        self.values
            .iter()
            .enumerate()
            .filter(|(_, value)| value.is_some() && **value == best)
            .map(|(column, _)| column)
            .collect()
    }

    /// Each run's value minus the baseline's, when both recorded the metric:
    /// the baseline's own is `0.0`, and the sign reads as what moved going
    /// from the baseline to that run, whichever way the metric improves.
    pub fn deltas(&self) -> Vec<Option<f64>> {
        let baseline = self.values.first().copied().flatten();
        self.values
            .iter()
            .map(|value| match (baseline, value) {
                (Some(baseline), Some(value)) => Some(value - baseline),
                _ => None,
            })
            .collect()
    }
}

/// Which way a metric improves.
///
/// A choice made in this crate, since a run's metrics are no fixed catalogue
/// (`ARCHITECTURE.md` § Local invariants): a metric whose name holds
/// `latency` is better lower, and every other is better higher — as the
/// ranking and answer metrics the harness records, `ndcg@<k>`, `recall@<k>`,
/// `mrr`, `exact_match` and `token_f1`, all are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// A higher value is better: a quality metric.
    HigherIsBetter,
    /// A lower value is better: a latency.
    LowerIsBetter,
}

impl Direction {
    /// The direction the metric `name` improves in.
    pub fn of(name: &str) -> Self {
        if name.contains("latency") {
            Self::LowerIsBetter
        } else {
            Self::HigherIsBetter
        }
    }
}

/// The configuration parameters not identical across the runs compared, or
/// why that could not be said.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigurationMatrix {
    /// Every stored document lowered.
    Compared {
        /// One row per parameter — component family and `impl:` name
        /// included — whose value is not the same in every run, absence
        /// included, sorted by node id and then by key. Empty when the runs
        /// agree on every one.
        parameters: Vec<ParameterRow>,
        /// Whether every canonical logical form hashes equal
        /// ([`LogicalPipeline::content_hash`]): runs that differ only in
        /// their wiring have no row and are still not one configuration.
        same_logical_form: bool,
    },
    /// A run's stored document does not lower under this build — the first
    /// such run, in column order.
    Unavailable {
        /// The run whose configuration could not be read.
        run: RunId,
        /// Its column: `0` for the baseline.
        column: usize,
        /// What the parser or the validation pass said.
        reason: String,
    },
}

/// One configuration parameter that is not the same in every run compared.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterRow {
    /// The node the parameter belongs to, by its id.
    pub node: NodeId,
    /// Which of the node's parameters.
    pub key: ParameterKey,
    /// Each run's value, `None` where its configuration does not set it.
    pub values: Vec<Option<ParamValue>>,
}

/// Why runs cannot be compared.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NotComparable {
    /// A run was evaluated on another benchmark than the baseline.
    #[error(
        "run {run} was evaluated on {run_version} and the baseline {baseline} on \
         {baseline_version}: runs are compared on one benchmark only"
    )]
    DatasetsDiffer {
        /// The baseline.
        baseline: RunId,
        /// The baseline's `dataset_version`.
        baseline_version: String,
        /// The first run evaluated on another benchmark.
        run: RunId,
        /// That run's `dataset_version`.
        run_version: String,
    },
}
