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
    /// The workspace's root directory, as the binary resolved it.
    pub path: String,
    /// The deployment settings the workspace holds.
    pub settings: SettingsSummary,
    /// The build's identity. The UI compares it with its own and reloads when
    /// they differ (ADR-C36 § 1); the same value is on every response in the
    /// `x-ragondin-build` header.
    pub build: String,
    /// What this build can run, as the launcher reports it.
    pub capabilities: Capabilities,
    /// What the workspace holds, counted on this request.
    pub counts: WorkspaceCounts,
}

/// What a workspace holds, counted when asked: nothing here is cached.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkspaceCounts {
    /// The pipeline documents under `pipelines/`, valid or not.
    pub pipelines: u64,
    /// The runs the store lists, readable or not.
    pub runs: u64,
    /// The benchmarks on disk whose digest is the one expected of them:
    /// `ready` or `local`.
    pub benchmarks_ready: u64,
    /// The bound services whose last probe, by this server, at their current
    /// address, read an identity.
    pub services_connected: u64,
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
    /// The family, spelled as `--remote` and a service binding spell it: a
    /// node family as a configuration's `component:` value, or `embedder`,
    /// which no node is and a `dense` node names with `embedder:`.
    pub family: String,
    /// The names this build gives a `Local` component in it: `impl:` values,
    /// or for `embedder`, `embedder:` values.
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

/// `GET /runs/{id}/queries`: every query the run executed, its scores read
/// from the trace against the run's own ground truth, and the per-node
/// ranking metrics.
///
/// Derived data: computed on read and cached under the workspace's `cache/`,
/// never stored in the run, and identical whether the cache was there or not.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct RunQueries {
    /// The run's id.
    pub run: String,
    /// Whether the ground truth the scores are read against is the run's
    /// own: the dataset on disk digests to the run's `dataset_version`. The
    /// chunk set is not compared — a score depends on the qrels and the
    /// reference answers, not on passage text. Scores and per-node metrics
    /// are present only when it is `verified`.
    pub ground_truth: DatasetCheck,
    /// The metrics a query can be scored on: those the run recorded that are
    /// read per query, by name.
    pub metrics: Vec<String>,
    /// The node whose ranking the ranking metrics read (ADR-C30 § 3), when the
    /// pipeline has one.
    pub ranking_node: Option<String>,
    /// The node whose answer the answer metrics read, when the pipeline ends
    /// in one.
    pub answer_node: Option<String>,
    /// The queries, in the run's order (by id) — all of them, or those the
    /// `missing_gold_at` filter kept.
    pub queries: Vec<QueryScores>,
    /// Every node of the pipeline, in the canonical order, with its ranking
    /// metrics averaged over the run's judged queries.
    pub nodes: Vec<NodeMetrics>,
    /// Why the figures could not be cached under the workspace's `cache/`,
    /// when they could not; `null` otherwise. The response is complete
    /// either way: the cache is never a truth, so its failure fails nothing.
    pub cache_error: Option<String>,
}

/// One query, as the run executed it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct QueryScores {
    /// The query's id.
    pub id: String,
    /// Its scores at the run's output, by metric name: the ranking metrics
    /// when its qrels are non-empty, the answer metrics when it has a
    /// reference. Empty when it is judged on neither, or when the ground
    /// truth is not verified.
    pub scores: BTreeMap<String, f64>,
    /// The sum of its nodes' durations, in nanoseconds: each component's own
    /// time, as the trace records it.
    pub duration_nanos: u64,
}

/// One node's ranking metrics over the run.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct NodeMetrics {
    /// The node's id.
    pub node: String,
    /// Whether the node produced a ranking for at least one query. A context
    /// builder, a generator and a node that failed on every query did not.
    pub produces_ranking: bool,
    /// How many queries the means are over: the judged queries for which the
    /// node produced a ranking. A query without qrels is never in it, as it
    /// is never in the harness's means; `0` when the ground truth is not
    /// verified.
    pub judged_queries: u64,
    /// The ranking metrics of its ranking, averaged over `judged_queries`;
    /// `null` when that is none, or when the ground truth is not verified.
    pub metrics: Option<BTreeMap<String, f64>>,
}

/// `GET /runs/{id}/trace/{query}`: one query's trace, node by node, in the
/// order the nodes ran, with each named chunk's passage text when the run's
/// own dataset is on disk.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct QueryTrace {
    /// The run's id.
    pub run: String,
    /// The query's id.
    pub query: String,
    /// Whether passage text could be resolved: only against the dataset the
    /// run was evaluated on, digests compared (ADR-C36 § 4).
    pub passages: DatasetCheck,
    /// The query's scores at the run's output, as `GET /runs/{id}/queries`
    /// reports them: present when the dataset on disk is the run's, whatever
    /// the chunk set.
    pub scores: BTreeMap<String, f64>,
    /// The nodes, in execution order.
    pub nodes: Vec<TraceNodeView>,
}

/// One node of a query's trace.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct TraceNodeView {
    /// The node's id.
    pub node: String,
    /// What it received, one summary per input port, in port order.
    pub inputs: Vec<TraceValue>,
    /// What it produced; `null` when it failed.
    pub output: Option<TraceValue>,
    /// How long its component took, in nanoseconds.
    pub duration_nanos: u64,
    /// The failure it reported; `null` when it succeeded.
    pub error: Option<String>,
    /// The ranking metrics of what it produced for this query: present when
    /// it produced a ranking, the query is judged and the dataset on disk is
    /// the run's; `null` otherwise.
    pub metrics: Option<BTreeMap<String, f64>>,
}

/// A value along an edge, as the trace records it: sized on an input port,
/// named where a node produced it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceValue {
    /// A query, by id.
    Query {
        /// The query's id.
        id: String,
    },
    /// A list of chunks, counted.
    ChunkCount {
        /// How many chunks it held.
        count: u64,
    },
    /// The chunks a node produced, in its own order.
    Ranking {
        /// The chunks, in the order the node returned them.
        chunks: Vec<TracePassage>,
    },
    /// A context, sized.
    ContextSize {
        /// How many chunks it held.
        count: u64,
        /// The length of its text, in bytes of UTF-8.
        text_bytes: u64,
    },
    /// The context a node produced.
    Context {
        /// Its chunks, in the order the builder placed them.
        chunks: Vec<TracePassage>,
        /// Its rendered text, whole, as the trace records it.
        text: String,
    },
    /// An answer, sized.
    AnswerSize {
        /// The length of its text, in bytes of UTF-8.
        text_bytes: u64,
    },
    /// The answer a node produced.
    Answer {
        /// Its text, as the generator returned it.
        text: String,
    },
}

/// One named chunk, with its passage text when it could be resolved.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct TracePassage {
    /// The chunk's id.
    pub chunk: String,
    /// The document it was derived from.
    pub document: String,
    /// The score the node gave it, on the node's own scale.
    pub score: f64,
    /// Its text: present only when the passages are `verified` and the chunk
    /// set derived from the dataset holds this id; `null` otherwise.
    pub text: Option<String>,
}

/// Whether the dataset on disk is the one a run was evaluated on: the
/// benchmark pinned to the run's `dataset_version`, digesting to it — and,
/// for passage text, its derived chunk set digesting to the run's
/// `index_version`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct DatasetCheck {
    /// The verdict.
    pub status: DatasetStatus,
    /// The benchmark the run's `dataset_version` is pinned to, by its
    /// selector; `null` when the registry pins no benchmark to it.
    pub benchmark: Option<String>,
    /// The digests the run recorded.
    pub expected: DatasetVersions,
    /// The digests of what is on disk, as far as they were computed; `null`
    /// when nothing on disk loaded.
    pub found: Option<FoundVersions>,
    /// The verdict in words: what was compared, and what differed.
    pub detail: String,
}

/// Where the run's dataset stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DatasetStatus {
    /// On disk, and every digest compared is the run's: the dataset's for
    /// the ground truth, the dataset's and the chunk set's for passage text.
    Verified,
    /// Not on disk, or pinned by no benchmark the registry knows.
    DatasetAbsent,
    /// On disk, and its digest is not the run's `dataset_version`.
    DatasetDiffers,
    /// On disk, and it does not load: `detail` carries the adapter's error.
    DatasetUnreadable,
    /// Passage text only: the dataset is the run's, but the chunk set this
    /// build derives from it does not digest to the run's `index_version` —
    /// the derivation moved. The scores are unaffected.
    IndexDiffers,
}

/// The two dataset digests of a run's identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DatasetVersions {
    /// The dataset's digest.
    pub dataset_version: String,
    /// The derived chunk set's digest.
    pub index_version: String,
}

/// The digests of the dataset on disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct FoundVersions {
    /// What the dataset on disk digests to.
    pub dataset_version: String,
    /// What its derived chunk set digests to; `null` when it was not
    /// compared — the dataset already differs, or the check is the ground
    /// truth's, which depends on the dataset alone.
    pub index_version: Option<String>,
}

/// One benchmark the registry knows: named by the manifest, or imported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct BenchmarkEntry {
    /// Its selector, `<format>/<dir>` — `beir/scifact` — as `ragondin bench
    /// --benchmark` takes it; for an import whose record cannot be read, its
    /// directory's name alone.
    pub name: String,
    /// The format that reads it: `beir`, `beir-qa` or `squad`; `unknown` for
    /// an import whose record cannot be read.
    pub format: String,
    /// Where it stands against the digest expected of it.
    pub state: BenchmarkState,
    /// The ground truth it carries, read off the loaded dataset; `null` when
    /// nothing on disk loaded.
    pub ground_truth: Option<GroundTruth>,
    /// The dataset's licence, for a benchmark the manifest names, whatever
    /// its state: a downloaded dataset keeps the notice it was obtained
    /// under. `null` for an import, whose licence is its owner's.
    pub licence: Option<String>,
    /// Where that licence is stated; `null` with it.
    pub licence_url: Option<String>,
}

/// Where a benchmark stands. Every verdict is a statement about digests: the
/// dataset on disk is loaded and its `dataset_version` compared with the one
/// expected — the manifest's, or the one recorded at import.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BenchmarkState {
    /// On disk, and its digest is the manifest's.
    Ready {
        /// What it digests to.
        dataset_version: String,
    },
    /// Named by the manifest, not on disk.
    Available {
        /// The snapshot's size.
        size_bytes: u64,
    },
    /// On disk, and its digest is another.
    Differs {
        /// The digest expected.
        expected: String,
        /// The digest on disk.
        found: String,
    },
    /// On disk, and it does not load.
    Unreadable {
        /// The adapter's error.
        error: String,
    },
    /// Imported, and its digest is the one recorded at import.
    Local {
        /// What it digests to.
        dataset_version: String,
    },
}

/// The ground truth a benchmark carries: which metric families a run over it
/// can compute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroundTruth {
    /// Neither qrels nor reference answers.
    None,
    /// Qrels: the retrieval metrics.
    Qrels,
    /// Reference answers: the generation metrics against reference.
    ReferenceAnswers,
    /// Both.
    Both,
}

/// `GET /benchmarks`: every benchmark the registry knows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct BenchmarkListing {
    /// The manifest's entries in manifest order, then the imports by name.
    pub benchmarks: Vec<BenchmarkEntry>,
}

/// `GET /pipelines`: every pipeline document in the workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PipelineListing {
    /// One entry per document, by name.
    pub pipelines: Vec<PipelineSummary>,
}

/// A pipeline, as the listing shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct PipelineSummary {
    /// Its name: the file stem.
    pub name: String,
    /// The digest of its bytes, the value `If-Match` names to write it.
    pub etag: String,
    /// When its file was last modified, in milliseconds since the Unix epoch.
    pub modified_ms: u64,
    /// The content hash of its canonical logical form, when it validates.
    pub hash: Option<String>,
    /// Why it does not validate, when it does not.
    pub error: Option<PipelineError>,
}

/// `GET /pipelines/{name}`: one pipeline document, verbatim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct PipelineDetail {
    /// Its name.
    pub name: String,
    /// The document, byte for byte as the file holds it.
    pub document: String,
    /// The digest of those bytes; also the response's `ETag` header, quoted.
    pub etag: String,
    /// The content hash of its canonical logical form, when it validates.
    pub hash: Option<String>,
    /// Why it does not validate, when it does not.
    pub error: Option<PipelineError>,
}

/// Why a pipeline document does not validate: `pipeline_invalid`'s detail and
/// location, inside a response that still answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PipelineError {
    /// What the validation pass said, in the words `ragondin validate` uses.
    pub detail: String,
    /// The node and the edge it concerns, when they can be named.
    pub location: Location,
}

/// `PUT /pipelines/{name}`: what was written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PipelineWritten {
    /// The pipeline's name.
    pub name: String,
    /// The etag of the bytes now stored; also the `ETag` header, quoted.
    pub etag: String,
    /// The content hash of their canonical logical form.
    pub hash: String,
}

/// `POST /pipelines/validate`: the document validates, and this is its hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PipelineValidated {
    /// The content hash of the canonical logical form, as `ragondin validate`
    /// prints it.
    pub hash: String,
}

/// `GET /pipelines/{name}/layout`: the layout beside the document, if it has
/// one. Without one the UI lays the graph out itself, and says so.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct PipelineLayout {
    /// The layout, or `null` when the pipeline has none.
    pub layout: Option<Layout>,
}

/// Where the editor draws each node: UI metadata beside the document, never
/// in its hash. Also the body of `PUT /pipelines/{name}/layout`.
#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize, JsonSchema)]
pub struct Layout {
    /// The layout format's version: `1`, the only one this build reads.
    pub version: u32,
    /// Each node's position, by node id.
    pub nodes: BTreeMap<String, Position>,
}

/// A node's position on the editor's canvas.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, serde::Deserialize, JsonSchema)]
pub struct Position {
    /// Horizontal, in canvas units.
    pub x: f64,
    /// Vertical, in canvas units.
    pub y: f64,
}

/// `GET /services`, and the answer of every write to a service: the bindings
/// `workspace.toml` holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ServiceListing {
    /// Every binding, in the file's order.
    pub services: Vec<ServiceStatus>,
}

/// One binding, and what this server last learnt by probing it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct ServiceStatus {
    /// The family the name is bound in.
    pub family: String,
    /// The implementation name a node uses.
    pub name: String,
    /// The service's address, as written.
    pub uri: String,
    /// Whether this server's last probe of it, at this address, read an
    /// identity. `false` before any probe.
    pub connected: bool,
    /// The identity last read at this address, if any was.
    pub identity: Option<String>,
}

/// `POST /services/{family}/{name}/probe`: the identity a run would record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ProbeResult {
    /// What the service reported, read as the composition root reads it
    /// before a run.
    pub identity: String,
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
