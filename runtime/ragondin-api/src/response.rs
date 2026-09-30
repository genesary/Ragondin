//! Every type the API serializes: the response bodies, and the problem body
//! an error renders as.
//!
//! **This module names no other crate of the workspace**, and a test holds it
//! to that (`tests/response_types.rs`). ADR-C36 § 2 makes every type the API
//! serializes this crate's own, converted to from the experiment plane's and
//! the core's types and never one of them serialized directly — for the reason
//! INV-9 gives for the wire format: the in-memory representation stays free to
//! move behind the surface a reader sees. The conversions are in `convert.rs`.
//!
//! Each type derives `serde::Serialize` and `schemars::JsonSchema`; the API
//! description assembles its schemas from these derives (`description.rs`).
//! Field names are `snake_case`, as the design document § 5 asks.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;

/// `GET /workspace`: where the server works, how it is set up, which build is
/// answering and what that build can run.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Workspace {
    /// The workspace directory, as the binary was given it.
    pub path: String,
    /// The deployment settings the workspace holds.
    pub settings: SettingsSummary,
    /// The build's identity. The UI compares it with its own and reloads when
    /// they differ (ADR-C36 § 1); the same value is on every response in the
    /// `x-ragondin-build` header.
    pub build: String,
    /// What this build can run, as the launcher reports it.
    pub capabilities: Capabilities,
}

/// The workspace's settings: deployment data, never hashed into a run.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct SettingsSummary {
    /// The directory benchmarks are read from.
    pub datasets: String,
    /// The `Remote` bindings the workspace names.
    pub services: Vec<ServiceBinding>,
}

/// A `Remote` component bound by family and name to the address it answers
/// at — in the workspace's settings, and as a run recorded it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ServiceBinding {
    /// The family the name is bound in: `generator`, `embedder`, ….
    pub family: String,
    /// The implementation name a node uses.
    pub name: String,
    /// The service's address, as written.
    pub uri: String,
}

/// What this build can run: the local implementations of each family, and
/// whether it can call a `Remote` one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Capabilities {
    /// One entry per family, as the launcher lists them.
    pub families: Vec<FamilyCapabilities>,
    /// Whether this build carries the `remote` feature.
    pub remote: bool,
}

/// One family's local implementations in this build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FamilyCapabilities {
    /// The family, spelled as a configuration's `component:` value.
    pub family: String,
    /// The `impl:` names this build registers in it.
    pub local: Vec<String>,
}

/// `GET /runs`: every run the store holds, and every one it holds but cannot
/// read.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct RunListing {
    /// The readable runs, in the store's listing order.
    pub runs: Vec<RunSummary>,
    /// The runs the store lists and cannot load — reported, never dropped
    /// and never repaired.
    pub unreadable: Vec<UnreadableRun>,
}

/// One run, as the listing shows it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct RunSummary {
    /// The run's content address: 64 lowercase hex digits.
    pub id: String,
    /// The content hash of the canonical logical pipeline it ran.
    pub pipeline: String,
    /// The benchmark dataset's version.
    pub dataset_version: String,
    /// The index's version.
    pub index_version: String,
    /// The engine's version.
    pub engine_version: String,
    /// What the run scored, by metric name.
    pub metrics: BTreeMap<String, f64>,
}

/// A run the store lists but cannot load, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UnreadableRun {
    /// The id the store lists it under.
    pub id: String,
    /// What loading it reported.
    pub reason: String,
}

/// `GET /runs/{id}`: one run, whole.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct RunDetail {
    /// The run's content address.
    pub id: String,
    /// The components its identity digests.
    pub inputs: RunInputs,
    /// What it scored, by metric name.
    pub metrics: BTreeMap<String, f64>,
    /// The configuration document that produced it, verbatim.
    pub configuration: String,
    /// The `Remote` bindings it used, outside its identity.
    pub bindings: Vec<ServiceBinding>,
    /// The graph lowered from [`configuration`](Self::configuration) by the
    /// pipeline grammar's one implementation — never by the browser.
    pub graph: Graph,
    /// The run this one is a prefix of. Always absent today: no run is
    /// recorded as a prefix yet.
    pub prefix_of: Option<String>,
}

/// The components of a run's identity tuple.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RunInputs {
    /// The content hash of the canonical logical pipeline.
    pub pipeline: String,
    /// The benchmark dataset's version.
    pub dataset_version: String,
    /// The index's version.
    pub index_version: String,
    /// The model hashes, by role.
    pub model_hashes: BTreeMap<String, String>,
    /// The engine's version.
    pub engine_version: String,
}

/// A pipeline as a graph: its declared inputs, its nodes and the edges
/// between them.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Graph {
    /// The values the pipeline receives from its caller, in declared order.
    pub inputs: Vec<GraphInput>,
    /// The nodes, sorted by id — the canonical order.
    pub nodes: Vec<GraphNode>,
    /// One edge per entry of a node's `inputs`, grouped by consuming node in
    /// node order, then in port order.
    pub edges: Vec<GraphEdge>,
}

/// A declared pipeline input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GraphInput {
    /// The id nodes name it by.
    pub id: String,
    /// What it carries: the query, for every pipeline this build reads.
    pub kind: EdgeKind,
}

/// A node of the graph.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct GraphNode {
    /// The node's id.
    pub id: String,
    /// Its component family, spelled as a configuration's `component:`
    /// value: `retriever`, `fusion`, `reranker`, `context_builder`,
    /// `generator` or `extension`.
    pub family: String,
    /// Its `impl:` name; an extension node's kind.
    pub implementation: String,
    /// Its parameters, in key order.
    pub parameters: BTreeMap<String, ParameterValue>,
}

/// A node parameter's value.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ParameterValue {
    /// A boolean.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A floating-point number.
    Float(f64),
    /// A string.
    String(String),
    /// An ordered list.
    List(Vec<ParameterValue>),
}

/// A data edge: `from`'s output feeds `to`'s input at `port`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GraphEdge {
    /// The producing node, or a declared input.
    pub from: String,
    /// The consuming node.
    pub to: String,
    /// The position of this edge among `to`'s inputs, from 0.
    pub port: u64,
    /// The kind of value the producer puts on it.
    pub kind: EdgeKind,
}

/// The kind of value travelling along an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// The question.
    Query,
    /// A list of retrieved chunks.
    Chunks,
    /// An assembled context.
    Context,
    /// A generator's answer.
    Answer,
    /// A value an extension node produces, unknown to the core.
    Opaque,
}

/// An error, as `application/problem+json` (RFC 9457) with this API's own
/// members: a stable `code`, a `hint` naming the action, and a `location` for
/// a validation failure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Problem {
    /// `urn:ragondin:problem:<code>`.
    #[serde(rename = "type")]
    pub problem_type: String,
    /// A short, fixed summary of the code.
    pub title: String,
    /// The HTTP status.
    pub status: u16,
    /// What happened, in this occurrence's words.
    pub detail: String,
    /// The stable code a client matches on: one of `ApiError::CODES`, which
    /// the schema lists as an enum so a generated client can narrow on it.
    #[schemars(schema_with = "problem_code")]
    pub code: String,
    /// The action that would resolve it.
    pub hint: String,
    /// Where in a pipeline a validation failure is. Present only for
    /// `pipeline_invalid`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
}

/// Where in a pipeline a validation failure is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct Location {
    /// The node concerned, when there is one.
    pub node: Option<String>,
    /// The edge concerned, when there is one.
    pub edge: Option<EdgeLocation>,
}

/// An edge, as a location names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct EdgeLocation {
    /// The producing node or declared input.
    pub from: String,
    /// The consuming node.
    pub to: String,
    /// The position of the edge among `to`'s inputs, from 0.
    pub port: u64,
}

/// Marks every property of a struct's schema required.
///
/// `schemars` leaves an `Option` field out of `required`, which a generated
/// client reads as "may be absent". The structs this is applied to serialize
/// every field on every response, `null` included, so each is required and
/// nullable: the client types it `T | null`, which is what the JSON holds.
fn every_property_required(schema: &mut schemars::Schema) {
    let names: Vec<serde_json::Value> = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| {
            properties
                .keys()
                .cloned()
                .map(serde_json::Value::String)
                .collect()
        })
        .unwrap_or_default();
    schema.insert("required".to_owned(), serde_json::Value::Array(names));
}

/// The schema of `Problem::code`: a string that is one of the stable codes.
fn problem_code(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "description": "The stable code a client matches on.",
        "enum": crate::error::ApiError::CODES,
    })
}
