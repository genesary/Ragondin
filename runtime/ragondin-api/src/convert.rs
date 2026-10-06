//! The conversions from the experiment plane's and the core's types into this
//! crate's response types — the one module where both sides are named, so
//! that `response.rs` names neither (ADR-C36 § 2).

use std::collections::{BTreeMap, HashMap};

use ragondin_benchmarks::datasets::{
    DiskState, DownloadError, ImportError, Imported, LocalEntry, MarkerError,
};
use ragondin_benchmarks::manifest::ManifestEntry;
use ragondin_benchmarks::CarriedPieces;
use ragondin_experiments::{
    Direction, ParameterKey, Run, RunBinding, RunInputs as StoredInputs, Trace, TraceChunk,
    TraceSummary,
};
use ragondin_metrics::{Family, Metric};
use ragondin_pipeline::{
    consumed_kinds, produced_kind, ContextBuilderNode, FusionNode, GeneratorNode, LogicalNode,
    LogicalPipeline, NodeId, ParamValue, Params, PortSpec, RawGraph, RawNode, RawParamValue,
    RawPipeline, RerankerNode, RetrieverNode, SchemaVersion, UnsupportedSchemaVersion, ValueKind,
};
use ragondin_types::DocId;

use crate::backends::RunDataset;
use crate::derived::NodeFigures;
use crate::error::ApiError;
use crate::handlers;
use crate::lineage;
use crate::response::{
    BenchmarkEntry, BenchmarkState, ConfigurationMatrix, ConsumedPorts, DatasetCheck,
    DatasetStatus, DatasetVersions, EdgeKind, FamilyPorts, FoundVersions, Graph, GraphEdge,
    GraphInput, GraphNode, GroundTruth, LaunchedAs, LaunchedPrefix, MetricDirection, MetricFamily,
    MetricRow, NameHeld, NodeMetrics, ParameterName, ParameterRow, ParameterValue, PartialNode,
    PrefixOf, RunDetail, RunInputs, RunSummary, Scorable, ServiceBinding, TraceNodeView,
    TracePassage, TraceValue, TypedDocument, TypedGraph, TypedNode,
};

/// One run, as the listing shows it, with the names the request found for
/// it — the workspace pipelines sharing its hash and the benchmarks pinned
/// to its digest, each sorted here — which of those pipelines `index` says
/// the backend refuses to read, and its median query latency, derived by the
/// handler.
pub(crate) fn summary(
    run: &Run,
    index: &lineage::Index,
    mut pipeline_names: Vec<String>,
    prefix_of_documents: Vec<PrefixOf>,
    mut benchmark_names: Vec<String>,
    median_query_latency_nanos: Option<u64>,
) -> RunSummary {
    pipeline_names.sort();
    benchmark_names.sort();
    let (started_at_ms, finished_at_ms) = times(run);
    // A hash match is stored as given, so `held` is never `Gone` for one:
    // `OtherCase` is the one way the backend refuses it.
    let refused_pipeline_names = pipeline_names
        .iter()
        .filter(|name| index.held(name) == NameHeld::OtherCase)
        .cloned()
        .collect();
    RunSummary {
        id: run.id.to_string(),
        pipeline: run.inputs.pipeline.to_string(),
        pipeline_names,
        refused_pipeline_names,
        launched_as: run
            .provenance
            .as_ref()
            .map(|record| launched_as(record, Some(index))),
        prefix_of_documents,
        dataset_version: run.inputs.dataset_version.clone(),
        benchmark_names,
        index_version: run.inputs.index_version.clone(),
        engine_version: run.inputs.engine_version.clone(),
        started_at_ms,
        finished_at_ms,
        metrics: metrics(run),
        metric_families: run
            .metrics
            .iter()
            .map(|(name, _)| (name.to_owned(), metric_family(name)))
            .collect(),
        median_query_latency_nanos,
    }
}

/// The family `ragondin-metrics`' catalogue gives the metric stored under
/// `name`; `unknown` for a name it does not know.
fn metric_family(name: &str) -> MetricFamily {
    match Metric::parse(name).map(Metric::family) {
        Some(Family::Ranking) => MetricFamily::Ranking,
        Some(Family::Answers) => MetricFamily::Answers,
        None => MetricFamily::Unknown,
    }
}

/// The run's times as the API spells them, read from the run and never
/// computed: both `None` when the run recorded none.
fn times(run: &Run) -> (Option<u64>, Option<u64>) {
    match run.times {
        Some(times) => (Some(times.started().get()), Some(times.finished().get())),
        None => (None, None),
    }
}

/// Every current document `pipeline` is a structural prefix of, with the
/// node it stops at, as `RunSummary::prefix_of_documents` serves them.
pub(crate) fn prefixes(index: &lineage::Index, pipeline: &LogicalPipeline) -> Vec<PrefixOf> {
    index
        .prefixes(pipeline)
        .into_iter()
        .map(|(pipeline, up_to)| PrefixOf { pipeline, up_to })
        .collect()
}

/// One run, whole: its stored fields and the graph lowered from its stored
/// configuration by [`handlers::lower`]. A document that no longer lowers, or
/// lowers to another pipeline than the one the run recorded, is
/// `run_unreadable`, never a guessed graph.
pub(crate) fn detail(run: &Run) -> Result<RunDetail, ApiError> {
    let pipeline = handlers::lower(run)?;
    let (started_at_ms, finished_at_ms) = times(run);
    Ok(RunDetail {
        id: run.id.to_string(),
        inputs: inputs(&run.inputs),
        metrics: metrics(run),
        configuration: run.config.as_str().to_owned(),
        bindings: run.bindings.iter().map(binding).collect(),
        started_at_ms,
        finished_at_ms,
        graph: graph(&pipeline),
        launched_as: run
            .provenance
            .as_ref()
            .map(|record| launched_as(record, None)),
    })
}

fn metrics(run: &Run) -> std::collections::BTreeMap<String, f64> {
    run.metrics
        .iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

fn inputs(inputs: &StoredInputs) -> RunInputs {
    RunInputs {
        pipeline: inputs.pipeline.to_string(),
        dataset_version: inputs.dataset_version.clone(),
        index_version: inputs.index_version.clone(),
        model_hashes: inputs.model_hashes.clone(),
        engine_version: inputs.engine_version.clone(),
    }
}

fn binding(binding: &RunBinding) -> ServiceBinding {
    ServiceBinding {
        family: binding.family.clone(),
        name: binding.name.clone(),
        uri: binding.uri.clone(),
    }
}

/// The graph of a lowered pipeline. An edge's kind is what its producer
/// puts on it: the query for a declared input — which is what a pipeline's
/// one declared input is (ADR-C18) — and otherwise the kind the core derives
/// from the producing node's variant (ADR-C16).
pub(crate) fn graph(pipeline: &LogicalPipeline) -> Graph {
    let producer_kind = |id: &str| -> EdgeKind {
        pipeline
            .nodes()
            .iter()
            .find(|node| node.id().as_str() == id)
            .map_or(EdgeKind::Query, |node| kind(produced_kind(node)))
    };
    let edges = pipeline
        .nodes()
        .iter()
        .flat_map(|node| {
            node.inputs()
                .iter()
                .enumerate()
                .map(move |(port, from)| (node, port, from))
        })
        .map(|(node, port, from)| GraphEdge {
            from: from.as_str().to_owned(),
            to: node.id().as_str().to_owned(),
            // Lossless: `usize` is at most 64 bits on every target Rust supports.
            port: port as u64,
            kind: producer_kind(from.as_str()),
        })
        .collect();
    Graph {
        inputs: pipeline
            .inputs()
            .iter()
            .map(|id| GraphInput {
                id: id.as_str().to_owned(),
                kind: EdgeKind::Query,
            })
            .collect(),
        nodes: pipeline.nodes().iter().map(node).collect(),
        edges,
    }
}

fn node(node: &LogicalNode) -> GraphNode {
    let (family, implementation, params) = match node {
        LogicalNode::Retriever(node) => ("retriever", &node.implementation, &node.params),
        LogicalNode::Fusion(node) => ("fusion", &node.implementation, &node.params),
        LogicalNode::Reranker(node) => ("reranker", &node.implementation, &node.params),
        LogicalNode::ContextBuilder(node) => {
            ("context_builder", &node.implementation, &node.params)
        }
        LogicalNode::Generator(node) => ("generator", &node.implementation, &node.params),
        LogicalNode::Extension(node) => ("extension", &node.kind, &node.params),
    };
    GraphNode {
        id: node.id().as_str().to_owned(),
        family: family.to_owned(),
        implementation: implementation.clone(),
        parameters: params
            .iter()
            .map(|(key, value)| (key.clone(), parameter(value)))
            .collect(),
    }
}

/// A node's component family, as a configuration's `component:` spells it —
/// the graph's spelling.
pub(crate) fn family(of: &LogicalNode) -> String {
    node(of).family
}

/// A run's launch record, `record`, as the API serves it, `held` read from
/// `index` — or [`NameHeld::Unchecked`] without one, for an endpoint that
/// lists no `pipelines/`.
pub(crate) fn launched_as(
    record: &ragondin_experiments::RunProvenance,
    index: Option<&lineage::Index>,
) -> LaunchedAs {
    LaunchedAs {
        name: record.name().map(str::to_owned),
        held: record
            .name()
            .map(|name| index.map_or(NameHeld::Unchecked, |index| index.held(name))),
        prefix_of: record.prefix_of().map(|prefix| LaunchedPrefix {
            up_to: prefix.up_to().to_owned(),
            parent_pipeline_hash: prefix.parent_pipeline_hash().to_string(),
        }),
    }
}

/// The kind of value a node produces, as an edge leaving it carries it.
pub(crate) fn produces(node: &LogicalNode) -> EdgeKind {
    kind(produced_kind(node))
}

/// The ports a node of `family` declares — `family` spelled as a
/// configuration's `component:` value — or `None` for a name that is no node
/// family a configuration can name: `embedder`, which no node is.
///
/// Read from `ragondin-pipeline`'s `produced_kind` and `consumed_kinds`, the
/// port grammar's one definition (ADR-C16), and never restated: the kinds
/// derive from a node's variant alone, so one node of each family, empty but
/// for its variant, is asked, and the family is matched by the graph's own
/// spelling of it, the one a graph node's `family` carries. An extension
/// node is not among them: its kind is
/// not a family, and its ports are unknown to the core.
pub fn family_ports(family: &str) -> Option<FamilyPorts> {
    let node = one_node_per_family()
        .into_iter()
        .find(|node| self::family(node) == family)?;
    let consumes = match consumed_kinds(&node) {
        PortSpec::Fixed(kinds) => ConsumedPorts::Fixed {
            kinds: kinds.into_iter().map(kind).collect(),
        },
        PortSpec::Variadic(of) => ConsumedPorts::Variadic { kind: kind(of) },
        PortSpec::Unknown => return None,
    };
    Some(FamilyPorts {
        produces: produces(&node),
        consumes,
    })
}

/// Fails to compile when `LogicalNode` gains a variant, so whoever adds one
/// decides here whether [`one_node_per_family`] lists it.
const _: fn(&LogicalNode) = |node| match node {
    // Listed in `one_node_per_family`.
    LogicalNode::Retriever(_)
    | LogicalNode::Fusion(_)
    | LogicalNode::Reranker(_)
    | LogicalNode::ContextBuilder(_)
    | LogicalNode::Generator(_) => {}
    // Not listed: its ports are `PortSpec::Unknown`, declared by no family.
    LogicalNode::Extension(_) => {}
};

/// One node of each family a configuration can name, every field empty: the
/// variant is all the port derivation reads.
fn one_node_per_family() -> [LogicalNode; 5] {
    let id = || NodeId::new("");
    [
        LogicalNode::Retriever(RetrieverNode {
            id: id(),
            implementation: String::new(),
            inputs: Vec::new(),
            params: Params::new(),
        }),
        LogicalNode::Fusion(FusionNode {
            id: id(),
            implementation: String::new(),
            inputs: Vec::new(),
            params: Params::new(),
        }),
        LogicalNode::Reranker(RerankerNode {
            id: id(),
            implementation: String::new(),
            inputs: Vec::new(),
            params: Params::new(),
        }),
        LogicalNode::ContextBuilder(ContextBuilderNode {
            id: id(),
            implementation: String::new(),
            inputs: Vec::new(),
            params: Params::new(),
        }),
        LogicalNode::Generator(GeneratorNode {
            id: id(),
            implementation: String::new(),
            inputs: Vec::new(),
            params: Params::new(),
        }),
    ]
}

/// The metric table of a comparison, each row's best runs named by id.
pub(crate) fn metric_rows(comparison: &ragondin_experiments::Comparison) -> Vec<MetricRow> {
    comparison
        .metrics
        .iter()
        .map(|row| MetricRow {
            name: row.name.clone(),
            direction: row.direction.map(|direction| match direction {
                Direction::HigherIsBetter => MetricDirection::Higher,
                Direction::LowerIsBetter => MetricDirection::Lower,
            }),
            values: row.values.clone(),
            deltas: row.deltas(),
            best: row
                .best()
                .into_iter()
                .map(|column| comparison.runs[column].to_string())
                .collect(),
        })
        .collect()
}

/// The configuration matrix of a comparison.
pub(crate) fn configuration_matrix(
    matrix: &ragondin_experiments::ConfigurationMatrix,
) -> ConfigurationMatrix {
    match matrix {
        ragondin_experiments::ConfigurationMatrix::Compared {
            parameters,
            same_logical_form,
        } => ConfigurationMatrix::Compared {
            parameters: parameters
                .iter()
                .map(|row| ParameterRow {
                    node: row.node.as_str().to_owned(),
                    key: match &row.key {
                        ParameterKey::Component => ParameterName::Component,
                        ParameterKey::Impl => ParameterName::Impl,
                        ParameterKey::Param(name) => ParameterName::Param { name: name.clone() },
                    },
                    values: row
                        .values
                        .iter()
                        .map(|value| value.as_ref().map(parameter))
                        .collect(),
                })
                .collect(),
            same_logical_form: *same_logical_form,
            partial_nodes: partial_nodes(parameters),
        },
        ragondin_experiments::ConfigurationMatrix::Unavailable { run, reason, .. } => {
            ConfigurationMatrix::Unavailable {
                run: run.to_string(),
                reason: reason.clone(),
            }
        }
    }
}

/// The nodes some run lacks. Every node of a configuration names its
/// `component:` family, so a node's `Component` row is unset exactly in the
/// runs that do not hold it — and it is a row at all only when some run
/// lacks the node or holds it as another family. The rows come sorted by
/// node, so the answer is too.
fn partial_nodes(parameters: &[ragondin_experiments::ParameterRow]) -> Vec<PartialNode> {
    parameters
        .iter()
        .filter(|row| row.key == ParameterKey::Component && row.values.iter().any(Option::is_none))
        .map(|row| PartialNode {
            node: row.node.as_str().to_owned(),
            present: row.values.iter().map(Option::is_some).collect(),
        })
        .collect()
}

/// A lowered parameter, with its kind. Every float in a `LogicalPipeline` is
/// finite: the validation pass refuses any other (`NonFiniteParam`).
fn parameter(value: &ParamValue) -> ParameterValue {
    match value {
        ParamValue::String(text) => ParameterValue::String(text.clone()),
        ParamValue::Int(number) => ParameterValue::Int(*number),
        ParamValue::Float(number) => ParameterValue::Float(*number),
        ParamValue::Bool(flag) => ParameterValue::Bool(*flag),
        ParamValue::List(items) => ParameterValue::List(items.iter().map(parameter).collect()),
    }
}

/// A wire-schema document as the editor holds it, or `None` when it holds a
/// value the typed document cannot carry: a non-finite float (ADR-C40 § 4).
/// Field by field, by hand, never derived from `RawPipeline` (ADR-C40 § 3).
pub(crate) fn typed_document(raw: &RawPipeline) -> Option<TypedDocument> {
    Some(TypedDocument {
        version: Some(raw.version.get()),
        pipeline: TypedGraph {
            inputs: raw.pipeline.inputs.clone(),
            nodes: raw
                .pipeline
                .nodes
                .iter()
                .map(|node| {
                    Some(TypedNode {
                        id: node.id.clone(),
                        component: node.component.clone(),
                        implementation: node.implementation.clone(),
                        inputs: node.inputs.clone(),
                        params: node
                            .params
                            .iter()
                            .map(|(key, value)| Some((key.clone(), typed_value(value)?)))
                            .collect::<Option<_>>()?,
                    })
                })
                .collect::<Option<_>>()?,
        },
    })
}

fn typed_value(value: &RawParamValue) -> Option<ParameterValue> {
    Some(match value {
        RawParamValue::String(text) => ParameterValue::String(text.clone()),
        RawParamValue::Int(number) => ParameterValue::Int(*number),
        RawParamValue::Float(number) if number.is_finite() => ParameterValue::Float(*number),
        RawParamValue::Float(_) => return None,
        RawParamValue::Bool(flag) => ParameterValue::Bool(*flag),
        RawParamValue::List(items) => {
            ParameterValue::List(items.iter().map(typed_value).collect::<Option<_>>()?)
        }
    })
}

/// The editor's typed document in the wire schema, or the schema version it
/// states that this build cannot read. The inverse of [`typed_document`] for
/// every document that function answers.
pub(crate) fn wire_document(
    typed: &TypedDocument,
) -> Result<RawPipeline, UnsupportedSchemaVersion> {
    Ok(RawPipeline {
        version: match typed.version {
            Some(version) => SchemaVersion::new(version)?,
            None => SchemaVersion::CURRENT,
        },
        pipeline: RawGraph {
            inputs: typed.pipeline.inputs.clone(),
            nodes: typed
                .pipeline
                .nodes
                .iter()
                .map(|node| RawNode {
                    id: node.id.clone(),
                    component: node.component.clone(),
                    implementation: node.implementation.clone(),
                    inputs: node.inputs.clone(),
                    params: node
                        .params
                        .iter()
                        .map(|(key, value)| (key.clone(), wire_value(value)))
                        .collect(),
                })
                .collect(),
        },
    })
}

fn wire_value(value: &ParameterValue) -> RawParamValue {
    match value {
        ParameterValue::String(text) => RawParamValue::String(text.clone()),
        ParameterValue::Int(number) => RawParamValue::Int(*number),
        ParameterValue::Float(number) => RawParamValue::Float(*number),
        ParameterValue::Bool(flag) => RawParamValue::Bool(*flag),
        ParameterValue::List(items) => RawParamValue::List(items.iter().map(wire_value).collect()),
    }
}

fn kind(kind: ValueKind) -> EdgeKind {
    match kind {
        ValueKind::Query => EdgeKind::Query,
        ValueKind::Chunks => EdgeKind::Chunks,
        ValueKind::Context => EdgeKind::Context,
        ValueKind::Answer => EdgeKind::Answer,
        ValueKind::Opaque => EdgeKind::Opaque,
    }
}

/// A benchmark the manifest names, as the registry lists it: `ready` when the
/// dataset on disk digests to the manifest's `dataset_version`, `available`
/// when nothing is there. Its licence is shown in every state, and so is its
/// ground truth: before the download, the one its manifest entry declares —
/// the same `CarriedPieces` the loaded dataset reports, so what a client reads
/// off `scorable` for it holds once it is on disk.
pub(crate) fn manifest_benchmark(entry: &ManifestEntry, state: DiskState) -> BenchmarkEntry {
    let (state, ground_truth) = match state {
        DiskState::Absent => (
            BenchmarkState::Available {
                size_bytes: entry.size_bytes(),
            },
            Some(ground_truth(entry.carries)),
        ),
        DiskState::Verified(verified) => (
            BenchmarkState::Ready {
                dataset_version: verified.dataset_version,
            },
            Some(ground_truth(verified.carries)),
        ),
        other => not_as_expected(other),
    };
    BenchmarkEntry {
        name: entry.name.clone(),
        format: entry.format.selector().to_owned(),
        state,
        ground_truth,
        licence: Some(entry.licence.clone()),
        licence_url: Some(entry.licence_url.clone()),
    }
}

/// An imported benchmark, as the registry lists it: `local` when it still
/// digests to what it digested to at import. `None` when its directory is
/// gone.
pub(crate) fn local_benchmark(entry: &LocalEntry, state: DiskState) -> Option<BenchmarkEntry> {
    let (state, ground_truth) = match state {
        DiskState::Absent => return None,
        DiskState::Verified(verified) => (
            BenchmarkState::Local {
                dataset_version: verified.dataset_version,
            },
            Some(ground_truth(verified.carries)),
        ),
        other => not_as_expected(other),
    };
    Some(BenchmarkEntry {
        name: entry.selector(),
        format: entry.format.selector().to_owned(),
        state,
        ground_truth,
        licence: None,
        licence_url: None,
    })
}

/// An import whose record this build cannot read: listed `unreadable` under
/// its directory's name, so one bad record fails neither the listing nor the
/// other entries.
pub(crate) fn unreadable_import(error: &MarkerError) -> BenchmarkEntry {
    BenchmarkEntry {
        name: error.name.clone(),
        format: "unknown".to_owned(),
        state: BenchmarkState::Unreadable {
            error: error.reason.clone(),
        },
        ground_truth: None,
        licence: None,
        licence_url: None,
    }
}

/// A dataset on disk that is not the one expected: another digest, or one
/// that does not load.
fn not_as_expected(state: DiskState) -> (BenchmarkState, Option<GroundTruth>) {
    match state {
        DiskState::Differs {
            expected,
            found,
            carries,
        } => (
            BenchmarkState::Differs { expected, found },
            Some(ground_truth(carries)),
        ),
        DiskState::Unreadable { error } => (
            BenchmarkState::Unreadable {
                error: causes(&error),
            },
            None,
        ),
        DiskState::Absent | DiskState::Verified(_) => {
            unreachable!("absent and verified are the caller's to convert")
        }
    }
}

/// What an import registered, as the listing will show it.
pub(crate) fn imported(imported: &Imported) -> BenchmarkEntry {
    BenchmarkEntry {
        name: imported.entry.selector(),
        format: imported.entry.format.selector().to_owned(),
        state: BenchmarkState::Local {
            dataset_version: imported.entry.dataset_version.clone(),
        },
        ground_truth: Some(ground_truth(imported.carries)),
        licence: None,
        licence_url: None,
    }
}

pub(crate) fn ground_truth(carries: CarriedPieces) -> GroundTruth {
    match carries {
        CarriedPieces::Neither => GroundTruth::None,
        CarriedPieces::QrelsOnly => GroundTruth::Qrels,
        CarriedPieces::ReferenceAnswersOnly => GroundTruth::ReferenceAnswers,
        CarriedPieces::QrelsAndReferenceAnswers => GroundTruth::Both,
    }
}

/// The pieces a listed ground truth stands for: [`ground_truth`] read back,
/// so `prefix::scorable` and the pipeline matrix ask `CarriedPieces` rather
/// than restate ADR-C30 § 5.
pub(crate) fn carried(ground_truth: GroundTruth) -> CarriedPieces {
    match ground_truth {
        GroundTruth::None => CarriedPieces::Neither,
        GroundTruth::Qrels => CarriedPieces::QrelsOnly,
        GroundTruth::ReferenceAnswers => CarriedPieces::ReferenceAnswersOnly,
        GroundTruth::Both => CarriedPieces::QrelsAndReferenceAnswers,
    }
}

/// Which ground truths a pipeline can be scored on, by whether it ends in an
/// answer: `CarriedPieces::scorable` asked of every [`GroundTruth`] that
/// carries a piece, the rule served rather than restated by a client.
/// [`GroundTruth::None`] is in neither list: the harness runs on such a
/// benchmark, but nothing would score the run, so no client offers it.
pub(crate) fn scorable() -> Scorable {
    let all = [
        GroundTruth::Qrels,
        GroundTruth::ReferenceAnswers,
        GroundTruth::Both,
    ];
    let by = |ends_in_answer: bool| {
        all.into_iter()
            .filter(|&truth| carried(truth).scorable(ends_in_answer))
            .collect()
    };
    Scorable {
        ending_in_answer: by(true),
        ending_elsewhere: by(false),
    }
}

/// A refused download, as the API reports it. Every refusal left nothing on
/// disk. What the source or the network did is `download_failed`; a
/// directory already there, a cancellation, and a defect of this build or its
/// disk each have their own code.
pub(crate) fn download_error(name: &str, error: DownloadError) -> ApiError {
    match error {
        DownloadError::Occupied { .. } => ApiError::BenchmarkExists {
            name: name.to_owned(),
        },
        DownloadError::Cancelled { .. } => ApiError::DownloadCancelled {
            name: name.to_owned(),
        },
        // The manifest pinned these bytes: a path outside the directory, a
        // snapshot that does not load, or one carrying other pieces than the
        // manifest declares, is this build's defect, not the source's.
        DownloadError::Io { .. }
        | DownloadError::Load { .. }
        | DownloadError::GroundTruth { .. }
        | DownloadError::InvalidPath { .. } => ApiError::BackendFailed {
            detail: causes(&error),
        },
        _ => ApiError::DownloadFailed {
            name: name.to_owned(),
            reason: causes(&error),
        },
    }
}

/// A refused import, as the API reports it: the name, the path or the corpus
/// is the request's to correct; a disk that cannot be written is not.
pub(crate) fn import_error(name: &str, error: ImportError) -> ApiError {
    match error {
        ImportError::Occupied { .. } => ApiError::BenchmarkExists {
            name: name.to_owned(),
        },
        ImportError::Io { .. } => ApiError::BackendFailed {
            detail: causes(&error),
        },
        _ => ApiError::ImportRefused {
            name: name.to_owned(),
            reason: causes(&error),
        },
    }
}

/// An error and every cause beneath it, on one line: the adapter's error
/// names the file in its own text and the filesystem's reason in its source.
pub(crate) fn causes(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// The ground truth's verdict for a run whose dataset verified: the scores
/// depend on the dataset alone, so the chunk set is not compared.
pub(crate) fn ground_verified(name: &str, inputs: &StoredInputs) -> DatasetCheck {
    DatasetCheck {
        status: DatasetStatus::Verified,
        benchmark: Some(name.to_owned()),
        expected: expected(inputs),
        found: Some(FoundVersions {
            dataset_version: inputs.dataset_version.clone(),
            index_version: None,
        }),
        detail: format!(
            "{name} on disk digests to the run's dataset_version; the scores depend on it alone, so the chunk set is not compared"
        ),
    }
}

/// The passages' verdict for a run whose dataset and chunk set both
/// verified.
pub(crate) fn passages_verified(name: &str, inputs: &StoredInputs) -> DatasetCheck {
    DatasetCheck {
        status: DatasetStatus::Verified,
        benchmark: Some(name.to_owned()),
        expected: expected(inputs),
        found: Some(FoundVersions {
            dataset_version: inputs.dataset_version.clone(),
            index_version: Some(inputs.index_version.clone()),
        }),
        detail: format!(
            "{name} on disk digests to the run's dataset_version, and its derived chunk set to the run's index_version"
        ),
    }
}

/// The passages' verdict for a run whose dataset verified and whose chunk
/// set, derived from it by this build, digests to another value: the
/// derivation moved.
pub(crate) fn index_differs(name: &str, inputs: &StoredInputs, found: &str) -> DatasetCheck {
    DatasetCheck {
        status: DatasetStatus::IndexDiffers,
        benchmark: Some(name.to_owned()),
        expected: expected(inputs),
        found: Some(FoundVersions {
            dataset_version: inputs.dataset_version.clone(),
            index_version: Some(found.to_owned()),
        }),
        detail: format!(
            "{name} on disk is the run's dataset, but the chunk set this build derives from it digests to {found}, and the run retrieved over {}",
            inputs.index_version
        ),
    }
}

/// The verdict for every answer of the registry but a verified dataset —
/// the same for the ground truth and for the passages.
pub(crate) fn unverified(dataset: &RunDataset, inputs: &StoredInputs) -> DatasetCheck {
    let (status, benchmark, found, detail) = match dataset {
        RunDataset::Unknown => (
            DatasetStatus::DatasetAbsent,
            None,
            None,
            format!(
                "no benchmark the registry knows is pinned to the run's dataset_version {}",
                inputs.dataset_version
            ),
        ),
        RunDataset::Absent { name } => (
            DatasetStatus::DatasetAbsent,
            Some(name.clone()),
            None,
            format!("{name} is not on disk"),
        ),
        RunDataset::Differs { name, found } => (
            DatasetStatus::DatasetDiffers,
            Some(name.clone()),
            Some(FoundVersions {
                dataset_version: found.clone(),
                index_version: None,
            }),
            format!(
                "{name} on disk digests to {found}; the run was evaluated on {}",
                inputs.dataset_version
            ),
        ),
        RunDataset::Unreadable { name, error } => (
            DatasetStatus::DatasetUnreadable,
            Some(name.clone()),
            None,
            format!("{name} is on disk and does not load: {error}"),
        ),
        RunDataset::Verified { name, .. } => unreachable!(
            "{name} verified: the caller states its verdict with `ground_verified`, `passages_verified` or `index_differs`"
        ),
    };
    DatasetCheck {
        status,
        benchmark,
        expected: expected(inputs),
        found,
        detail,
    }
}

fn expected(inputs: &StoredInputs) -> DatasetVersions {
    DatasetVersions {
        dataset_version: inputs.dataset_version.clone(),
        index_version: inputs.index_version.clone(),
    }
}

/// What an endpoint that needs the ground truth and cannot degrade answers
/// when the dataset is not the run's: `dataset_absent`, or
/// `dataset_differs` naming the digests compared — for a dataset that does
/// not load, that nothing loaded.
pub(crate) fn dataset_error(check: &DatasetCheck) -> ApiError {
    let dataset = check
        .benchmark
        .clone()
        .unwrap_or_else(|| format!("with dataset_version {}", check.expected.dataset_version));
    let expected = check.expected.dataset_version.clone();
    match check.status {
        DatasetStatus::DatasetDiffers => ApiError::DatasetDiffers {
            dataset,
            expected,
            found: check
                .found
                .as_ref()
                .map_or_else(String::new, |found| found.dataset_version.clone()),
        },
        DatasetStatus::DatasetUnreadable => ApiError::DatasetDiffers {
            dataset,
            expected,
            found: "nothing: it does not load".to_owned(),
        },
        // The ground truth's check is never `index_differs` nor, here,
        // `verified`: the caller asks only when the dataset did not verify.
        DatasetStatus::DatasetAbsent | DatasetStatus::IndexDiffers | DatasetStatus::Verified => {
            ApiError::DatasetAbsent { dataset }
        }
    }
}

/// A query's trace as the API shows it. `texts` holds the passage text of
/// each chunk id the run's verified chunk set resolves; `None` when the
/// dataset is not verified, and then no chunk has text. `metrics` gives a
/// node's ranking metrics for this query and `gold_ranks` the ranks of its
/// gold documents, when it has any; `grade` a document's grade for this
/// query, when it has one.
pub(crate) fn trace_view(
    trace: &Trace,
    texts: Option<&HashMap<String, String>>,
    metrics: impl Fn(&NodeId) -> Option<BTreeMap<String, f64>>,
    gold_ranks: impl Fn(&NodeId) -> Option<Vec<u64>>,
    grade: impl Fn(&DocId) -> Option<u8>,
) -> Vec<TraceNodeView> {
    let passages = |chunks: &[TraceChunk]| -> Vec<TracePassage> {
        chunks
            .iter()
            .map(|chunk| TracePassage {
                chunk: chunk.chunk.as_str().to_owned(),
                document: chunk.document.as_str().to_owned(),
                score: chunk.score,
                text: texts.and_then(|texts| texts.get(chunk.chunk.as_str()).cloned()),
                grade: grade(&chunk.document),
            })
            .collect()
    };
    let value = |summary: &TraceSummary| -> TraceValue {
        match summary {
            TraceSummary::Query { id } => TraceValue::Query {
                id: id.as_str().to_owned(),
            },
            TraceSummary::Chunks { count } => TraceValue::ChunkCount { count: *count },
            TraceSummary::RankedChunks { chunks } => TraceValue::Ranking {
                chunks: passages(chunks),
            },
            TraceSummary::ContextSize { count, text_bytes } => TraceValue::ContextSize {
                count: *count,
                text_bytes: *text_bytes,
            },
            TraceSummary::Context { chunks, text } => TraceValue::Context {
                chunks: passages(chunks),
                text: text.clone(),
            },
            TraceSummary::AnswerSize { text_bytes } => TraceValue::AnswerSize {
                text_bytes: *text_bytes,
            },
            TraceSummary::Answer { text } => TraceValue::Answer { text: text.clone() },
        }
    };
    trace
        .nodes
        .iter()
        .map(|node| TraceNodeView {
            node: node.node.as_str().to_owned(),
            inputs: node.inputs.iter().map(value).collect(),
            output: node.output.as_ref().map(value),
            duration_nanos: node.duration_nanos,
            error: node.error.clone(),
            metrics: metrics(&node.node),
            gold_ranks: gold_ranks(&node.node),
        })
        .collect()
}

/// One node's figures over the run, as the API shows them.
pub(crate) fn node_metrics(figures: &NodeFigures) -> NodeMetrics {
    NodeMetrics {
        node: figures.node.clone(),
        produces_ranking: figures.produces_ranking,
        judged_queries: figures.judged_queries,
        metrics: figures.metrics.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Before its download, a manifest entry is listed with the ground truth
    /// it declares: SQuAD's qrels and reference answers, SciFact's qrels.
    #[test]
    fn an_entry_not_yet_downloaded_is_listed_with_the_ground_truth_it_declares() {
        let listed = |name: &str| {
            let entry = ragondin_benchmarks::manifest::manifest()
                .into_iter()
                .find(|entry| entry.name == name)
                .expect("the manifest names it");
            manifest_benchmark(&entry, DiskState::Absent)
        };
        let squad = listed("squad/dev");
        assert!(matches!(squad.state, BenchmarkState::Available { .. }));
        assert_eq!(squad.ground_truth, Some(GroundTruth::Both));
        assert_eq!(
            listed("beir/scifact").ground_truth,
            Some(GroundTruth::Qrels)
        );
    }

    /// The submission check reads ADR-C30 § 5 off the listing's ground
    /// truth, so the way back must lose nothing the way there kept.
    #[test]
    fn the_ground_truth_maps_back_to_the_pieces_it_was_built_from() {
        for carries in [
            CarriedPieces::Neither,
            CarriedPieces::QrelsOnly,
            CarriedPieces::ReferenceAnswersOnly,
            CarriedPieces::QrelsAndReferenceAnswers,
        ] {
            assert_eq!(carried(ground_truth(carries)), carries);
        }
    }

    fn component_row(node: &str, values: &[Option<&str>]) -> ragondin_experiments::ParameterRow {
        ragondin_experiments::ParameterRow {
            node: NodeId::new(node),
            key: ParameterKey::Component,
            values: values
                .iter()
                .map(|v| v.map(|family| ParamValue::String(family.to_owned())))
                .collect(),
        }
    }

    /// A node's `Component` row is also a row when every run holds the node
    /// under different families: such a node is held by every run, so it is
    /// no partial node.
    #[test]
    fn a_node_every_run_holds_under_different_families_is_not_partial() {
        let rows = [
            component_row("leg", &[Some("retriever"), Some("reranker")]),
            component_row("rerank", &[None, Some("reranker")]),
        ];
        assert_eq!(
            partial_nodes(&rows),
            vec![PartialNode {
                node: "rerank".to_owned(),
                present: vec![false, true],
            }]
        );
    }
}
