//! The conversions from the experiment plane's and the core's types into this
//! crate's response types — the one module where both sides are named, so
//! that `response.rs` names neither (ADR-C36 § 2).

use ragondin_benchmarks::datasets::{DiskState, DownloadError, ImportError, Imported, LocalEntry};
use ragondin_benchmarks::manifest::ManifestEntry;
use ragondin_benchmarks::CarriedPieces;
use ragondin_experiments::{lower_configuration, Run, RunBinding, RunInputs as StoredInputs};
use ragondin_pipeline::{produced_kind, LogicalNode, LogicalPipeline, ParamValue, ValueKind};

use crate::error::ApiError;
use crate::response::{
    BenchmarkEntry, BenchmarkState, EdgeKind, Graph, GraphEdge, GraphInput, GraphNode, GroundTruth,
    ParameterValue, RunDetail, RunInputs, RunSummary, ServiceBinding,
};

/// One run, as the listing shows it.
pub(crate) fn summary(run: &Run) -> RunSummary {
    RunSummary {
        id: run.id.to_string(),
        pipeline: run.inputs.pipeline.to_string(),
        dataset_version: run.inputs.dataset_version.clone(),
        index_version: run.inputs.index_version.clone(),
        engine_version: run.inputs.engine_version.clone(),
        metrics: metrics(run),
    }
}

/// One run, whole: its stored fields and the graph lowered from its stored
/// configuration by `ragondin-experiments`' one lowering path. A document
/// that no longer lowers is `run_unreadable`, never a guessed graph.
pub(crate) fn detail(run: &Run) -> Result<RunDetail, ApiError> {
    let pipeline = lower_configuration(&run.config).map_err(|reason| ApiError::RunUnreadable {
        run_id: run.id.to_string(),
        reason,
    })?;
    Ok(RunDetail {
        id: run.id.to_string(),
        inputs: inputs(&run.inputs),
        metrics: metrics(run),
        configuration: run.config.as_str().to_owned(),
        bindings: run.bindings.iter().map(binding).collect(),
        graph: graph(&pipeline),
        prefix_of: None,
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
fn graph(pipeline: &LogicalPipeline) -> Graph {
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

fn parameter(value: &ParamValue) -> ParameterValue {
    match value {
        ParamValue::String(text) => ParameterValue::String(text.clone()),
        ParamValue::Int(number) => ParameterValue::Int(*number),
        ParamValue::Float(number) => ParameterValue::Float(*number),
        ParamValue::Bool(flag) => ParameterValue::Bool(*flag),
        ParamValue::List(items) => ParameterValue::List(items.iter().map(parameter).collect()),
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
/// when nothing is there.
pub(crate) fn manifest_benchmark(entry: &ManifestEntry, state: DiskState) -> BenchmarkEntry {
    let (state, ground_truth) = match state {
        DiskState::Absent => (
            BenchmarkState::Available {
                size_bytes: entry.size_bytes(),
                licence: entry.licence.clone(),
                licence_url: entry.licence_url.clone(),
            },
            None,
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
    })
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
    }
}

fn ground_truth(carries: CarriedPieces) -> GroundTruth {
    match carries {
        CarriedPieces::Neither => GroundTruth::None,
        CarriedPieces::QrelsOnly => GroundTruth::Qrels,
        CarriedPieces::ReferenceAnswersOnly => GroundTruth::ReferenceAnswers,
        CarriedPieces::QrelsAndReferenceAnswers => GroundTruth::Both,
    }
}

/// A refused download, as the API reports it. Every refusal left nothing on
/// disk; a directory already there, or a disk that cannot be written, is not
/// the download's own failure and has its own code.
pub(crate) fn download_error(name: &str, error: DownloadError) -> ApiError {
    match error {
        DownloadError::Occupied { .. } => ApiError::BenchmarkExists {
            name: name.to_owned(),
        },
        DownloadError::Io { .. } => ApiError::BackendFailed {
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
fn causes(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
