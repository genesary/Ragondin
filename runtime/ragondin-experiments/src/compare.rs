//! Run comparison: [`compare`], [`RunComparison`], [`MetricComparison`] and
//! [`ConfigurationComparison`].
//!
//! The diff between two runs is the view the platform exists for
//! (`docs/system-architecture.md` §6.5): *this configuration against that one,
//! side by side*. This module is its data — the `ragondin compare` command and
//! any later interface render what [`compare`] returns.
//!
//! It compares **metrics**, and **which configuration parameters differ**:
//! the node-level parameters, `impl:` names and component families one run's
//! stored configuration holds and the other does not, or holds with another
//! value. The graph's wiring — a node's `inputs`, the pipeline's declared
//! inputs — is not listed parameter by parameter; whether the two canonical
//! forms hash equal is carried beside the list, so a difference there is
//! still reported. Both documents are lowered to [`LogicalPipeline`] first,
//! through `ragondin-pipeline`'s own [`RawPipeline`] and [`validate()`], so
//! the difference is one between canonical logical forms and never between
//! texts (the spirit of INV-8): a document respelled — keys reordered, flow
//! style for block style — differs in nothing.

use std::collections::{BTreeMap, BTreeSet};

use ragondin_pipeline::{
    peek_schema_version, validate, LogicalNode, LogicalPipeline, NodeId, ParamValue, RawPipeline,
    SchemaVersionPeekError,
};

use crate::run::{ConfigDocument, Run, RunId};

/// Compares two runs, metric by metric and parameter by parameter.
///
/// Every metric either run recorded appears in the result, in name order, so a
/// caller can render the two columns without deciding what to show.
///
/// Infallible even when a stored configuration does not lower — a run stored
/// under an older schema, say: the metrics are still compared, and
/// [`RunComparison::configuration`] says which side could not be read and why.
pub fn compare(left: &Run, right: &Run) -> RunComparison {
    let names: BTreeSet<&str> = left
        .metrics
        .iter()
        .chain(right.metrics.iter())
        .map(|(name, _)| name)
        .collect();

    RunComparison {
        left: left.id,
        right: right.id,
        metrics: names
            .into_iter()
            .map(|name| MetricComparison {
                name: name.to_owned(),
                left: left.metrics.get(name),
                right: right.metrics.get(name),
            })
            .collect(),
        configuration: compare_configurations(&left.config, &right.config),
    }
}

fn compare_configurations(
    left: &ConfigDocument,
    right: &ConfigDocument,
) -> ConfigurationComparison {
    let left = match lower(left) {
        Ok(pipeline) => pipeline,
        Err(reason) => {
            return ConfigurationComparison::Unavailable {
                side: Side::Left,
                reason,
            }
        }
    };
    let right = match lower(right) {
        Ok(pipeline) => pipeline,
        Err(reason) => {
            return ConfigurationComparison::Unavailable {
                side: Side::Right,
                reason,
            }
        }
    };

    let same_logical_form = left.content_hash() == right.content_hash();
    let left = parameters(&left);
    let right = parameters(&right);
    let keys: BTreeSet<&(NodeId, ParameterKey)> = left.keys().chain(right.keys()).collect();

    let differences = keys
        .into_iter()
        .filter_map(|entry| {
            let (left, right) = (left.get(entry), right.get(entry));
            (left != right).then(|| ParameterDifference {
                node: entry.0.clone(),
                key: entry.1.clone(),
                left: left.cloned(),
                right: right.cloned(),
            })
        })
        .collect();
    ConfigurationComparison::Compared {
        differences,
        same_logical_form,
    }
}

/// The load path `ragondin-config` runs over a file, run over the kept text:
/// into the hand-maintained wire schema, then through the validation pass —
/// never a deserializer pointed at an internal type (INV-9).
///
/// The schema version is peeked first, as `ragondin-config` does, so a run
/// stored under a version this build cannot read says that rather than
/// reporting a syntax error.
fn lower(document: &ConfigDocument) -> Result<LogicalPipeline, String> {
    if let Err(SchemaVersionPeekError::Unsupported(source)) =
        peek_schema_version(serde_yaml::Deserializer::from_str(document.as_str()))
    {
        return Err(format!(
            "stored under a schema version this build cannot read: {source}"
        ));
    }
    let raw: RawPipeline = serde_yaml::from_str(document.as_str())
        .map_err(|error| format!("the stored configuration does not parse: {error}"))?;
    validate(raw).map_err(|error| format!("the stored configuration does not validate: {error}"))
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
