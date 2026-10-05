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
use serde::{Deserialize, Serialize};

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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Capabilities {
    /// One entry per family, as the launcher lists them.
    pub families: Vec<FamilyCapabilities>,
    /// Whether this build carries the `remote` feature.
    pub remote: bool,
}

/// One family: its ports, the local implementations this build carries in
/// it with their parameters, those it does not carry and why, and what a
/// bound name takes.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct FamilyCapabilities {
    /// The family, spelled as `--remote` and a service binding spell it: a
    /// node family as a configuration's `component:` value, or `embedder`,
    /// which no node is and a `dense` node names with `embedder:`.
    pub family: String,
    /// The ports a node of the family declares, as the pipeline grammar
    /// derives them from the family alone; `null` for `embedder`, which no
    /// node is.
    pub ports: Option<FamilyPorts>,
    /// The names the binary gives a `Local` component of the family in some
    /// build and not in this one, each with what a build needs to carry it.
    pub not_carried: Vec<NotCarried>,
    /// The names this build gives a `Local` component in it — `impl:`
    /// values, or for `embedder`, `embedder:` values — each with the
    /// parameters a node takes under it. An embedder takes none: its keys
    /// are a `dense` node's, in that node's choice.
    pub parameters: Vec<ImplementationParameters>,
    /// The parameters a node takes under a name bound in the family with
    /// `--remote` or a service binding; empty for `embedder`.
    pub bound: Vec<Parameter>,
}

/// One implementation this build carries, and the parameters a node takes
/// under its name: the keys the executor reads for its family and the keys
/// its constructor reads, as the composition root declares them.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct ImplementationParameters {
    /// The `impl:` name.
    pub name: String,
    /// Its parameters, whatever the value of `choice`'s key.
    pub parameters: Vec<Parameter>,
    /// A key whose value adds further parameters, if it has one.
    pub choice: Option<ParameterChoice>,
}

/// A key whose value adds parameters: a `dense` node's `embedder:`.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct ParameterChoice {
    /// The key.
    pub key: String,
    /// What each value adds. The first case whose `value` is the key's
    /// applies, else the case whose `value` is `null` when the key holds any
    /// other, non-empty text.
    pub cases: Vec<ChoiceCase>,
}

/// The parameters one value of a [`ParameterChoice`]'s key adds.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct ChoiceCase {
    /// The value; `null` for any other non-empty one, a bound name.
    pub value: Option<String>,
    /// The parameters it adds.
    pub parameters: Vec<Parameter>,
}

/// One parameter a node takes.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct Parameter {
    /// The key, as a configuration writes it.
    pub name: String,
    /// The kind of value it holds.
    pub kind: ParameterKind,
    /// Whether a node must declare it: a node without it is refused before
    /// it is stored or run.
    pub required: bool,
    /// What it is for, in a sentence.
    pub description: String,
    /// The value the editor writes under it when a node is placed, or
    /// `null`. Only a required key with no component default has one, and
    /// nothing applies it to a node that lacks the key: it is written as an
    /// ordinary value, or not at all.
    pub start: Option<ParameterValue>,
}

/// The kind of value a parameter holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParameterKind {
    /// An integer of zero or more.
    NonNegativeInteger,
    /// Text, the empty one included unless the description says otherwise.
    String,
    /// A finite floating-point number, never an integer.
    Float,
}

/// The ports of a node family: the kind it puts on its output edge, and
/// the kinds its input edges carry, by position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FamilyPorts {
    /// The kind of value a node of the family produces.
    pub produces: EdgeKind,
    /// The kinds of value it consumes.
    pub consumes: ConsumedPorts,
}

/// What a node family's input edges carry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum ConsumedPorts {
    /// Exactly these kinds, in this order: port `i` carries `kinds[i]`.
    Fixed {
        /// One kind per port, in port order.
        kinds: Vec<EdgeKind>,
    },
    /// Any number of ports, every one carrying `kind`.
    Variadic {
        /// The kind every port carries.
        kind: EdgeKind,
    },
}

/// A `Local` implementation this build does not carry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NotCarried {
    /// Its name: an `impl:` value, or for `embedder`, an `embedder:` value.
    pub name: String,
    /// Why this build does not carry it, in words: the feature a build needs
    /// to carry it.
    pub reason: String,
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
    /// The shape of every pipeline a readable run ran, once per pipeline,
    /// keyed by its canonical hash ([`RunSummary::pipeline`]): the graph
    /// `GET /runs/{id}` serves for a run of it, by the same conversion. A
    /// pipeline whose every run's stored document no longer lowers has no
    /// entry, as `GET /runs/{id}` has no graph for it.
    pub shapes: BTreeMap<String, Graph>,
    /// Why a run's median query latency could not be read from or written
    /// to the workspace's `cache/`, the first such reason; absent when the
    /// cache served or took every run, so a listing whose cache works reads
    /// as it always has. The listing is complete either way: the cache is
    /// never a truth, so its failure fails nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", default)]
    pub cache_error: Option<String>,
}

/// One run, as the listing shows it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct RunSummary {
    /// The run's content address: 64 lowercase hex digits.
    pub id: String,
    /// The content hash of the canonical logical pipeline it ran.
    pub pipeline: String,
    /// Every workspace pipeline document whose canonical hash is the run's,
    /// sorted; empty when none is. The current hash match, found by content
    /// when the listing is asked for, so a document edited since the run no
    /// longer names it. Of the two facts ADR-C39 § 4 exposes about a run's
    /// pipeline, this is the content one; it is not a resolution of
    /// [`launched_as`](Self::launched_as), nor that of this.
    pub pipeline_names: Vec<String>,
    /// The names among [`pipeline_names`](Self::pipeline_names) the backend
    /// refuses to read, sorted; empty when it refuses none. A name is refused
    /// when another stored name is a case alias of it — the same name in
    /// another ASCII case, `request_invalid` on a read — as two documents can
    /// be on a filesystem that keeps case; each of the two is then refused.
    /// Read from the same listing of `pipelines/` as `pipeline_names`, by the
    /// rule [`NameHeld::OtherCase`] reads a recorded name by.
    pub refused_pipeline_names: Vec<String>,
    /// The run's launch record, as it was written once with the run, read
    /// and never computed or inferred; `null` for a run stored without one.
    /// Recorded at launch, so its name may be a pipeline whose content has
    /// since changed, or that no longer exists. ADR-C39 § 4's other fact,
    /// never resolved with [`pipeline_names`](Self::pipeline_names) into one
    /// name.
    pub launched_as: Option<LaunchedAs>,
    /// Every current workspace document the run's pipeline is a structural
    /// prefix of — its declared inputs the same, each of its nodes one of the
    /// document's, equal in canonical form, and fewer of them — each with the
    /// node the run stops at, sorted by name; empty when it is a prefix of
    /// none, or its stored document no longer lowers. A content fact like
    /// [`pipeline_names`](Self::pipeline_names), asked of every run whatever
    /// [`launched_as`](Self::launched_as) records (ADR-C39 § 5), so a prefix
    /// written by hand and run from the command line is one too; the pipeline
    /// matrix counts a prefix by the same test.
    pub prefix_of_documents: Vec<PrefixOf>,
    /// The benchmark dataset's version.
    pub dataset_version: String,
    /// Every registry entry pinned to [`dataset_version`](Self::dataset_version)
    /// — a manifest entry or an import — sorted; empty when none is. The
    /// pinning `GET /runs/{id}/queries` locates the run's dataset by, never
    /// resolved by closeness (ADR-C36 § 4); naming a benchmark here loads and
    /// verifies nothing.
    pub benchmark_names: Vec<String>,
    /// The index's version.
    pub index_version: String,
    /// The engine's version.
    pub engine_version: String,
    /// When the run started, in milliseconds since the Unix epoch, as the
    /// process that ran it recorded; `null` when unknown.
    pub started_at_ms: Option<u64>,
    /// When the run's evaluation finished, in milliseconds since the Unix
    /// epoch, as the process that ran it recorded; `null` when unknown.
    pub finished_at_ms: Option<u64>,
    /// What the run scored, by metric name.
    pub metrics: BTreeMap<String, f64>,
    /// The family of each metric, keyed exactly like
    /// [`metrics`](Self::metrics): from `ragondin-metrics`' catalogue, and
    /// `unknown` for a name it does not know — kept, never dropped.
    pub metric_families: BTreeMap<String, MetricFamily>,
    /// The lower median, over the run's queries, of each query's latency —
    /// the sum of its trace's node durations — in nanoseconds.
    /// Derived, never stored in the run: read from the traces alone, so a
    /// run whose dataset is not on disk has it too, and cached under the
    /// workspace's `cache/`. Not the run's wall time, which
    /// `finished_at_ms − started_at_ms` gives, preparation included. `null`
    /// when no trace of the run reads.
    pub median_query_latency_nanos: Option<u64>,
}

/// Which ground truth a metric reads, as the listing names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetricFamily {
    /// Scored from qrels.
    Ranking,
    /// Scored from reference answers.
    Answers,
    /// A name the catalogue does not know: shown with its stored name and
    /// value, ranked against nothing.
    Unknown,
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
    /// When it started, in milliseconds since the Unix epoch, outside its
    /// identity; `null` when unknown.
    pub started_at_ms: Option<u64>,
    /// When its evaluation finished, in milliseconds since the Unix epoch,
    /// outside its identity; `null` when unknown.
    pub finished_at_ms: Option<u64>,
    /// The graph lowered from [`configuration`](Self::configuration) by the
    /// pipeline grammar's one implementation — never by the browser.
    pub graph: Graph,
    /// The run's launch record, as [`RunSummary::launched_as`] serves it;
    /// `null` for a run stored without one. A prefix run says what it was
    /// cut from here, in the record's `prefix_of`: the parent's name, the
    /// node it stops at and the parent's canonical hash. A prefix written by
    /// hand, with no such record, shows only through `GET /runs`'
    /// `prefix_of_documents`.
    pub launched_as: Option<LaunchedAs>,
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

/// A node parameter's value, tagged with its kind: the one parameter type of
/// the API, in a run's graph, a comparison's rows and a pipeline's typed
/// document alike (ADR-C40 § 2). `60` and `60.0` are two configurations
/// (ADR-C22), so an integer and a float of one value are never one value
/// here.
///
/// An integer travels as decimal text, which the browser's number type
/// cannot round; a float travels as a JSON number and is finite. Read from a
/// request, each refuses what its kind does not carry: a number for an
/// integer, text for a float, an integer written `+1`, `01`, `-0` or wider
/// than 64 bits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ParameterValue {
    /// Text.
    String(String),
    /// An integer of at most 64 bits, as decimal text: `-?(0|[1-9][0-9]*)`,
    /// never `-0`.
    Int(
        #[serde(with = "decimal")]
        #[schemars(schema_with = "decimal::schema")]
        i64,
    ),
    /// A finite floating-point number.
    Float(#[serde(deserialize_with = "finite")] f64),
    /// A boolean.
    Bool(bool),
    /// An ordered list.
    List(Vec<ParameterValue>),
}

/// An integer as decimal text, in the one spelling each value has.
mod decimal {
    use schemars::{json_schema, Schema, SchemaGenerator};
    use serde::{de, Deserialize, Deserializer, Serializer};

    /// Text in the one spelling: `0`, or a minus or none and no leading zero.
    pub(super) fn schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "pattern": "^(0|-?[1-9][0-9]*)$" })
    }

    pub(super) fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        let text = String::deserialize(deserializer)?;
        // One spelling per value — no sign but a minus, no leading zero, no
        // `-0` — so the text a value is shown as is the text it is read from.
        let digits = text.strip_prefix('-').unwrap_or(&text);
        let canonical = !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit())
            && (digits == "0" || !digits.starts_with('0'))
            && text != "-0";
        let refused = || {
            de::Error::custom(format!(
                "an integer is decimal text of at most 64 bits, such as \"60\", not {text:?}"
            ))
        };
        if !canonical {
            return Err(refused());
        }
        text.parse().map_err(|_| refused())
    }
}

/// A float that is finite. JSON carries no other, but the refusal is this
/// type's to say rather than the reader's.
fn finite<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let value = f64::deserialize(deserializer)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(serde::de::Error::custom("a float is a finite number"))
    }
}

/// A pipeline document as the editor holds it (ADR-C40 § 1): the wire
/// schema's shape — an optional schema version, the declared inputs in
/// order, the nodes in order — with every parameter value tagged with its
/// kind. The API's own type, converted by hand to and from the wire schema
/// in `convert.rs`, never derived from it: the configuration format on disk
/// is unchanged, and the browser never reads or writes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypedDocument {
    /// The schema version the document is written in. Absent, the version
    /// this build reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// The pipeline itself.
    pub pipeline: TypedGraph,
}

/// The graph of a `TypedDocument`, as a configuration nests it under
/// `pipeline:`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypedGraph {
    /// The ids of the values the pipeline receives from its caller, in
    /// declared order.
    pub inputs: Vec<String>,
    /// The nodes, in the order the document lists them.
    pub nodes: Vec<TypedNode>,
}

/// One node of a `TypedDocument`, keyed as a configuration writes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypedNode {
    /// The node's id; not yet known to be unique.
    pub id: String,
    /// Its family, a configuration's `component:` value; not yet known to
    /// name one.
    pub component: String,
    /// Its `impl:` value.
    #[serde(rename = "impl")]
    pub implementation: String,
    /// The ids it consumes, in port order; not yet known to exist.
    pub inputs: Vec<String>,
    /// Its parameters, in key order.
    pub params: BTreeMap<String, ParameterValue>,
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
#[schemars(transform = every_property_required)]
pub struct QueryScores {
    /// The query's id.
    pub id: String,
    /// Its text, the dataset's: present when the ground truth is
    /// `verified` and the dataset holds the query; `null` otherwise.
    pub text: Option<String>,
    /// Its scores at the run's output, by metric name: the ranking metrics
    /// when its qrels are non-empty, the answer metrics when it has a
    /// reference. Empty when it is judged on neither, or when the ground
    /// truth is not verified.
    pub scores: BTreeMap<String, f64>,
    /// Its latency, in nanoseconds: the sum of its nodes' durations — each
    /// component's own time, as the trace records it. `null` when that sum
    /// overflows, which only a malformed trace can make it do.
    pub duration_nanos: Option<u64>,
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
#[schemars(transform = every_property_required)]
pub struct QueryTrace {
    /// The run's id.
    pub run: String,
    /// The query's id.
    pub query: String,
    /// The query's text, the dataset's: present when the dataset on disk is
    /// the run's, whatever the chunk set — the gate `scores` is under — and
    /// it holds the query; `null` otherwise, and `passages` says why.
    pub text: Option<String>,
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

/// `GET /jobs/{id}/queries`: the queries a failed or cancelled run job
/// executed before it stopped — the one a failed run stopped on included,
/// its failing node last — whose traces it kept under
/// `jobs/<id>/partial/` — never in the store, which holds a run complete or
/// not at all.
///
/// Nothing here is scored, and no query or passage text is read: a run
/// records the digests of the dataset it was evaluated on, and the ground
/// truth and the text are read only against a dataset that digests to them
/// (ADR-C36 § 4); a job's partial traces record none.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct PartialQueries {
    /// The job, as `GET /jobs/{id}` answers it: what was submitted, the run
    /// id it announced — under which nothing is stored — and how it ended.
    pub job: JobSummary,
    /// The graph lowered from the pipeline document the job snapshotted at
    /// submission.
    pub graph: Graph,
    /// The queries, by id, each with its latency; its `text` is `null` and
    /// its `scores` empty.
    pub queries: Vec<QueryScores>,
    /// The query whose trace holds a failed node — the one a failed run
    /// stopped on, its failing node last; `null` when no kept trace does.
    pub failed_query: Option<String>,
}

/// `GET /jobs/{id}/trace/{query}`: one query's trace from a failed or
/// cancelled run job's partial traces, node by node, in the shape
/// `GET /runs/{id}/trace/{query}` serves a stored run's — each node's
/// `metrics` and `gold_ranks`, and each passage's `text` and `grade`,
/// `null`, for the reason [`PartialQueries`] gives.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct PartialTrace {
    /// The job's id.
    pub job: String,
    /// The query's id.
    pub query: String,
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
    /// The 1-based ranks of the gold documents — graded above 0 — in its
    /// ranking, counted over documents folded from its chunks by first
    /// occurrence, the ranking its `metrics` score: present when it produced
    /// a ranking, the query is judged and the dataset on disk is the run's;
    /// empty when it ranked no gold document; `null` otherwise.
    pub gold_ranks: Option<Vec<u64>>,
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
    /// Its document's grade in the query's qrels, `0` when they do not judge
    /// it: present when the query is judged and the dataset on disk is the
    /// run's, whatever the chunk set; `null` otherwise.
    pub grade: Option<u8>,
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
    /// When its file was last modified, in milliseconds since the Unix
    /// epoch; `null` for a time before the epoch, which is unknown, never
    /// `0`.
    pub modified_ms: Option<u64>,
    /// The content hash of its canonical logical form, when it validates.
    pub hash: Option<String>,
    /// Why it does not validate, when it does not.
    pub error: Option<PipelineError>,
}

/// `GET /pipelines/{name}`: one pipeline document, verbatim, and as the
/// editor holds it when it can.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
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
    /// The document as the editor holds it, whenever its text reads into the
    /// wire schema, whether or not it validates; `null` when it does not
    /// read, or holds a value the typed document cannot carry, such as a
    /// non-finite float (ADR-C40 § 4).
    pub typed: Option<TypedDocument>,
    /// Whether the text is, byte for byte, the server's own rendering of the
    /// document it reads to — what a write from the editor would store.
    /// `false` for a text a person wrote (a comment, another key order,
    /// another formatting) and for one that does not read: the editor warns
    /// before its first save replaces such a text (ADR-016 § 5).
    pub canonical: bool,
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

/// `POST /pipelines/validate`: the document validates, and this is its hash
/// and its rendering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct PipelineValidated {
    /// The content hash of the canonical logical form, as `ragondin validate`
    /// prints it.
    pub hash: String,
    /// The document as the server renders it: for a typed document, the
    /// bytes a write of it stores; for a text, the rendering of the document
    /// it reads to, which keeps none of its comments or formatting. What the
    /// editor exports. `null` only for a text whose document the renderer
    /// cannot write so that it reads back.
    pub rendering: Option<String>,
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
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// The layout format's version: `1`, the only one this build reads.
    pub version: u32,
    /// Each node's position, by node id.
    pub nodes: BTreeMap<String, Position>,
}

/// A node's position on the editor's canvas.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
    /// The stored document's etag, for `precondition_failed` when one is
    /// stored — the value the `ETag` header carries quoted, for a client
    /// that reads the body alone. Present only then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// The parameter, path parameter or header a `parameter_invalid` is
    /// about, when it is known. Absent otherwise — never guessed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// For `run_exists`, where what already holds the run id is read:
    /// `/api/v1/jobs/<id>` for a job queued or running under it,
    /// `/api/v1/runs/<id>` for a stored run. Present only then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
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

/// `POST /compare`: runs of one benchmark compared against a baseline.
///
/// Every list with one entry per run — a metric's values, a stage's cells —
/// is in the order of [`runs`](Self::runs): the baseline first, then the
/// other runs in the order the request gave them.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Comparison {
    /// The baseline's id.
    pub baseline: String,
    /// The runs compared, the baseline first.
    pub runs: Vec<ComparedRun>,
    /// Whether the benchmark on disk is the one the runs were evaluated on,
    /// which the per-query deltas and the per-stage metrics are read
    /// against. Without `verified`, `query_deltas` is empty and no stage
    /// cell carries a metric; the table, the matrix, the stages and the
    /// latency do not depend on it.
    pub ground_truth: DatasetCheck,
    /// One row per metric any run recorded, in name order.
    pub metrics: Vec<MetricRow>,
    /// The configuration parameters not identical across the runs.
    pub configuration: ConfigurationMatrix,
    /// The stages the runs are aligned by, in pipeline order: those at least
    /// one run has.
    pub stages: Vec<StageRow>,
    /// The manual pairings in use: one per run whose pipeline the workspace
    /// holds a pairing for with the baseline's, oriented from the baseline's
    /// pipeline.
    pub pairings: Vec<Pairing>,
    /// Every pair of [`pairings`](Self::pairings) a run could not place,
    /// with the run and the side lacking its node, in the runs' order; empty
    /// when every pair was placed.
    pub unplaced_pairs: Vec<UnplacedPair>,
    /// Each run other than the baseline: its per-query deltas against the
    /// baseline, for every ranking metric both recorded.
    pub query_deltas: Vec<RunDeltas>,
    /// Each run's latency, node by node.
    pub latency: Vec<RunLatency>,
    /// Why derived figures could not be cached under the workspace's
    /// `cache/`, one entry per failure; empty otherwise. The response is
    /// complete either way.
    pub cache_errors: Vec<String>,
}

/// One run of a comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct ComparedRun {
    /// The run's id.
    pub id: String,
    /// The content hash of the canonical logical pipeline it ran.
    pub pipeline_hash: String,
    /// The workspace pipeline whose manual pairings apply to it (decided in #402): the
    /// name its launch record gives when it has one, whether or not that
    /// pipeline's content has changed since; otherwise the one pipeline
    /// document whose canonical hash is the run's. `null` when it has no
    /// record naming one and no document, or several, holds its hash; such a
    /// run is paired automatically only. The lookup's answer, not a claim
    /// about which version the run is: a run of earlier content gets the
    /// pairs whose nodes it still has, and the rest are in
    /// [`Comparison::unplaced_pairs`].
    pub pipeline: Option<String>,
}

/// One metric across the runs compared.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct MetricRow {
    /// The metric's name.
    pub name: String,
    /// Which way it improves, from `ragondin-metrics`' catalogue; `null`
    /// for a name the catalogue does not know, which then has no best.
    pub direction: Option<MetricDirection>,
    /// Each run's value; `null` where the run did not record it.
    pub values: Vec<Option<f64>>,
    /// Each run's value minus the baseline's; `null` where either did not
    /// record it. The baseline's own is `0`.
    pub deltas: Vec<Option<f64>>,
    /// The runs holding the best value, by `direction`: every one on a tie;
    /// none when `direction` is `null`.
    pub best: Vec<String>,
}

/// Which way a metric improves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetricDirection {
    /// A higher value is better.
    Higher,
    /// A lower value is better.
    Lower,
}

/// The configuration parameters not identical across the runs, or why that
/// could not be said.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigurationMatrix {
    /// Every stored configuration lowered.
    Compared {
        /// Every parameter — family and `impl:` name included — whose value
        /// is not the same in every run, sorted by node and key.
        parameters: Vec<ParameterRow>,
        /// Whether every canonical logical form hashes equal: runs that
        /// differ only in their wiring have no row, and are still not one
        /// configuration.
        same_logical_form: bool,
    },
    /// A run's stored configuration does not lower under this build.
    Unavailable {
        /// That run's id.
        run: String,
        /// What the parser or the validation pass said.
        reason: String,
    },
}

/// One parameter that is not the same in every run.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct ParameterRow {
    /// The node it belongs to.
    pub node: String,
    /// Which of the node's parameters.
    pub key: ParameterName,
    /// Each run's value; `null` where its configuration does not set it.
    pub values: Vec<Option<ParameterValue>>,
}

/// A node's parameter, as a configuration spells it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParameterName {
    /// The node's `component:` family.
    Component,
    /// The node's `impl:` name.
    Impl,
    /// A key under the node's `params:`.
    Param {
        /// The key.
        name: String,
    },
}

/// One stage across the runs compared.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct StageRow {
    /// Which stage.
    pub stage: StageName,
    /// What the row compares, in the words of the pair drawn by hand into
    /// it; `null` when no pair named it.
    pub label: Option<String>,
    /// `manual` when a pair drawn by hand placed a node in it.
    pub source: PairingSource,
    /// `low` when a pipeline's graph was ambiguous at this stage — two
    /// fusions, two rerankers, a reranker upstream of the fusion — and the
    /// automatic pairing is a guess.
    pub confidence: Confidence,
    /// Each run's cell.
    pub cells: Vec<StageCell>,
}

/// A stage of a pipeline, in the order a ranking travels through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StageName {
    /// The retrievers.
    RetrievalLegs,
    /// The fusion.
    AfterFusion,
    /// The reranker.
    AfterRerank,
    /// The ranking the run's retrieval metrics were read at.
    FinalRanking,
    /// The generator's answer.
    Answer,
}

/// Where a stage's pairing comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PairingSource {
    /// Derived from the nodes' kinds and positions.
    Automatic,
    /// Drawn by hand, and remembered for the pair of pipelines.
    Manual,
}

/// How sure an automatic pairing is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Every graph was unambiguous at this stage, or the pairing is by hand.
    High,
    /// A guess: the UI offers the manual pairing.
    Low,
}

/// One run at one stage.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StageCell {
    /// The run's pipeline has no node at this stage: "no stage here", an
    /// absence, never a zero.
    Absent,
    /// The run's nodes at this stage.
    Present {
        /// Each node, in the pipeline's canonical order — several only for
        /// the retrieval legs, or where a pair drawn by hand joined one.
        nodes: Vec<StageNode>,
        /// Per metric, the best value among the nodes and the node it is
        /// from — for the legs, the best leg. Empty without a verified
        /// ground truth.
        best: BTreeMap<String, StageValue>,
    },
}

/// A node at a stage.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct StageNode {
    /// The node's id.
    pub node: String,
    /// Whether a pair drawn by hand placed it here.
    pub paired_by_hand: bool,
    /// Its ranking metrics averaged over the judged queries, as
    /// `GET /runs/{id}/queries` reports them; `null` without a verified
    /// ground truth or when it ranked no judged query.
    pub metrics: Option<BTreeMap<String, f64>>,
}

/// A metric's value at a stage, and the node it is read at.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct StageValue {
    /// The node.
    pub node: String,
    /// Its value.
    pub value: f64,
}

/// A manual pairing between two pipelines: a list of node pairs, kept under
/// `pipelines/<pipeline>.pairing/<other>.json` and read in both directions.
/// Also the body's `pairing` in `POST /compare`, which keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pairing {
    /// The pipeline the pairs' `node` belongs to.
    pub pipeline: String,
    /// The pipeline the pairs' `other` belongs to.
    pub other: String,
    /// The pairs. Empty in a request: "Reset to automatic", which removes
    /// the pairing.
    pub pairs: Vec<NodePair>,
}

/// A pair drawn by hand that a comparison could not apply to one of its
/// runs: a node it names is not a retriever, fusion or reranker of the run
/// it would be read in. A pairing is kept against two pipelines' current
/// documents, and applies to a run launched under a pipeline's name whose
/// content has since changed for the nodes that run still has (decided in #402); every
/// other pair is reported here, never skipped and never guessed onto
/// another node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UnplacedPair {
    /// The compared run, other than the baseline, the pair was not applied
    /// to.
    pub run: String,
    /// The pair, oriented from the baseline's pipeline as
    /// [`Comparison::pairings`] holds it.
    pub pair: NodePair,
    /// Which run lacks its node.
    pub absent_from: AbsentFrom,
}

/// Which run of a comparison lacks a node a pair drawn by hand names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AbsentFrom {
    /// The baseline lacks the pair's `node`.
    Baseline,
    /// The run lacks the pair's `other`.
    Run,
    /// Each lacks its node.
    Both,
}

/// Two nodes compared with each other: the second is shown at the first's
/// stage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodePair {
    /// A node of `pipeline`.
    pub node: String,
    /// A node of `other`.
    pub other: String,
    /// What the pair compares, replacing the stage's name in the row; absent
    /// for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// One run's per-query deltas against the baseline.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct RunDeltas {
    /// The run's id.
    pub run: String,
    /// One entry per ranking metric both it and the baseline recorded, in
    /// name order.
    pub metrics: Vec<MetricDeltas>,
}

/// One metric's per-query deltas, and their histogram.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct MetricDeltas {
    /// The metric's name.
    pub metric: String,
    /// How many queries have a delta: those judged, and scored at both
    /// runs' outputs.
    pub judged_queries: u64,
    /// Each such query's score in the run minus its score in the baseline,
    /// by query id.
    pub deltas: Vec<QueryDelta>,
    /// The seven bins, from the worst to the best: they partition the
    /// queries with a delta.
    pub bins: Vec<DeltaBin>,
}

/// One query's delta.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct QueryDelta {
    /// The query's id.
    pub query: String,
    /// Its score in the run minus its score in the baseline.
    pub delta: f64,
}

/// One bin of the per-query histogram.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct DeltaBin {
    /// Which bin.
    pub bin: DeltaBinName,
    /// Its lower bound; `null` for the lowest.
    pub lower: Option<f64>,
    /// Its upper bound; `null` for the highest.
    pub upper: Option<f64>,
    /// How many queries it holds.
    pub count: u64,
    /// Those queries, by id.
    pub queries: Vec<String>,
}

/// The seven bins of a delta `d`: by its sign, and its magnitude against
/// 0.1 and 0.3. A bound belongs to the bin nearer zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeltaBinName {
    /// `d < -0.3`.
    MuchWorse,
    /// `-0.3 <= d < -0.1`.
    Worse,
    /// `-0.1 <= d < 0`.
    SlightlyWorse,
    /// `d = 0`: the same score.
    Unchanged,
    /// `0 < d <= 0.1`.
    SlightlyBetter,
    /// `0.1 < d <= 0.3`.
    Better,
    /// `d > 0.3`.
    MuchBetter,
}

/// One run's latency, node by node.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct RunLatency {
    /// The run's id.
    pub run: String,
    /// Every node that ran, by id.
    pub nodes: Vec<NodeLatency>,
}

/// One node's latency over a run's queries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NodeLatency {
    /// The node's id.
    pub node: String,
    /// Its component family, as a configuration's `component:` spells it.
    pub family: String,
    /// The median of its durations, in nanoseconds: the lower of the two
    /// middle values over an even count, so it is a duration that occurred.
    pub median_nanos: u64,
    /// How many queries it ran for.
    pub queries: u64,
}

/// `GET /pipelines/{name}/matrix`: one workspace pipeline's node × benchmark
/// matrix, over the runs of its current canonical form and of its prefixes
/// (ADR-C39 § 6). Derived from the stored runs on every request: it is no
/// object of its own.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct PipelineMatrix {
    /// The pipeline's name: its document under `pipelines/`.
    pub pipeline: String,
    /// The canonical hash of the document as it is now: the runs whose
    /// pipeline hash is this one fill cells, under whatever name they ran.
    pub pipeline_hash: String,
    /// The pipeline's nodes in topological order, ties by id, so the matrix
    /// reads like the pipeline. A judge row is reserved, and absent until the
    /// judge exists.
    pub rows: Vec<MatrixRow>,
    /// One column per benchmark a feeding run ran on — and, with
    /// `include_available`, per benchmark the registry knows that none did —
    /// ordered by benchmark name.
    pub columns: Vec<MatrixColumn>,
    /// Every run that counts for this pipeline — of its current canonical
    /// form, a prefix of it, or launched as it with content that has since
    /// changed — the most recent first, each saying whether it fills its
    /// column.
    pub feeding_runs: Vec<FeedingRun>,
    /// Per column, the nodes no run measured that a run of the whole
    /// pipeline on that benchmark would: what a launch would fill. Never on a
    /// benchmark where such a run exists.
    pub missing: Vec<MissingCells>,
    /// The runs the store lists and cannot load, with its reason: neither
    /// counted nor silently dropped, as `GET /runs` lists them.
    pub unreadable: Vec<UnreadableRun>,
    /// Why derived figures could not be cached under the workspace's
    /// `cache/`, one entry per failure; empty otherwise. The response is
    /// complete either way.
    pub cache_errors: Vec<String>,
}

/// One node of the matrix's pipeline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct MatrixRow {
    /// The node's id.
    pub node: String,
    /// Its component family, as a configuration's `component:` spells it.
    pub family: String,
    /// The kind of value it produces: `chunks` for a ranking node, `answer`
    /// for a generator.
    pub produces: EdgeKind,
}

/// One benchmark of the matrix, and the run that fills it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct MatrixColumn {
    /// The digest of the benchmark's dataset, as runs over it record it.
    pub dataset_version: String,
    /// Every benchmark the registry pins to that digest, sorted; empty when
    /// it pins none.
    pub benchmark_names: Vec<String>,
    /// The ground truth it carries: read off the dataset when it verified,
    /// otherwise off the metrics the run recorded; for a benchmark no run of
    /// the current content measured, read off its dataset only when the
    /// pipeline does not end in an answer — to say whether it can be scored
    /// there — and `null` otherwise.
    pub ground_truth: Option<GroundTruth>,
    /// The run that fills the column — the most recent run of the whole
    /// current form on this benchmark, or, with none, the most recent prefix
    /// of it; `null` when neither ran on it.
    pub run: Option<String>,
    /// For a prefix run, the node it stops at; `null` otherwise.
    pub up_to: Option<String>,
    /// Whether the dataset on disk is the run's own, which the ranking
    /// figures are read against; `null` when no run fills the column.
    pub dataset_check: Option<DatasetCheck>,
    /// One cell per row, in the rows' order.
    pub cells: Vec<MatrixCell>,
    /// The most recent run job of the whole current form on this benchmark,
    /// when it failed and no run of the whole current form measured the
    /// benchmark; `null` otherwise — no attempt, or a later one that did not
    /// fail.
    pub failed_attempt: Option<FailedAttempt>,
}

/// A run job that failed, as a matrix column names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct FailedAttempt {
    /// The job's id, as `GET /jobs/{id}` serves it.
    pub job: String,
    /// What failed.
    pub error: String,
    /// The node that failed, when one did.
    pub at_node: Option<String>,
}

/// One node on one benchmark: its figure, or why it has none.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatrixCell {
    /// Measured: a ranking node's metrics at its output, as
    /// `GET /runs/{id}/queries` reports them, or the generator's
    /// answer metrics, as the run recorded them.
    #[schemars(transform = every_property_required)]
    Measured {
        /// The figures, by metric name.
        metrics: BTreeMap<String, f64>,
        /// The gain over the previous ranking stage, which is what says
        /// where a node helps, or why there is none.
        gain: MatrixGain,
        /// How many judged queries a ranking node's means are over; `null`
        /// for the generator, whose figures are the run's.
        judged_queries: Option<u64>,
    },
    /// A ranking node, on a benchmark that carries no qrels: never
    /// measurable there.
    NoQrels,
    /// The generator, on a benchmark that carries no reference answers:
    /// never measurable there.
    NoReferenceAnswers,
    /// No counted run on this benchmark: measurable, not measured.
    NotRunYet {
        /// The benchmark to launch.
        benchmark: String,
    },
    /// The column's run is a prefix that stops before this node: "not run:
    /// the prefix run stops at `up_to`".
    PrefixStops {
        /// The node the prefix run stops at.
        up_to: String,
    },
    /// A node whose output no metric reads — a context builder, an
    /// extension's value, a generator that is not the output.
    NotScored,
    /// A ranking node, read against a dataset that is not the run's own on
    /// disk: the column's `dataset_check` says why.
    Unverified,
    /// Measurable and run, and no figure: the node ranked no judged query —
    /// it failed — or the run recorded no answer metric.
    NoFigure,
    /// The pipeline cannot be scored on this benchmark: it does not end in
    /// an answer, and the benchmark carries reference answers (ADR-C30 § 5,
    /// `CarriedPieces::scorable`), so a run there cannot succeed: the
    /// harness refuses it once the job runs, and the refusal arrives as a
    /// failed job. Never missing.
    NotScorable,
    /// Not run on this version: the only runs on this benchmark were launched
    /// as this pipeline before its content changed (ADR-C39 § 6). They fill
    /// no cell; the most recent of them is linked.
    NotRunOnThisVersion {
        /// That run's id, listed among the feeding runs.
        run: String,
    },
}

/// A run that counts for the matrix's pipeline: one that fills a cell, or
/// one launched as it whose content has since changed.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct FeedingRun {
    /// The run's id.
    pub run: String,
    /// The digest of the dataset it ran on.
    pub dataset_version: String,
    /// Every benchmark the registry pins to that digest, sorted.
    pub benchmark_names: Vec<String>,
    /// When it started, from its own record; `null` when unknown.
    pub started_at_ms: Option<u64>,
    /// The run's launch record, as it was written once with the run; `null`
    /// when it has none. ADR-C39 § 4's first fact: what the run was launched
    /// as, never resolved with `pipeline_names` into one name.
    pub launched_as: Option<LaunchedAs>,
    /// Every workspace document whose canonical hash is the run's, sorted —
    /// ADR-C39 § 4's content fact.
    pub pipeline_names: Vec<String>,
    /// For a prefix of the matrix's pipeline's current form — by its launch
    /// record's parent hash, or by the structural test — the pipeline and the
    /// node it stops at; `null` otherwise.
    pub prefix_of: Option<PrefixOf>,
    /// Whether it is the run its column shows.
    pub fills_column: bool,
    /// For a run launched as this pipeline whose content has since changed
    /// (ADR-C39 § 7): how it was launched, and its parameter difference
    /// against the pipeline's current document; `null` for a run that fills
    /// a cell.
    pub content_since_changed: Option<ContentSinceChanged>,
}

/// A run launched as the matrix's pipeline whose content has since changed:
/// stated as a fact, never guessed to be an earlier version (ADR-C39 § 7).
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct ContentSinceChanged {
    /// How its record says it was launched.
    pub launched: SinceChangedLaunch,
    /// Its parameter difference against the pipeline's current document, by
    /// `POST /compare`'s configuration matrix, the current document's column
    /// first.
    pub difference: ConfigurationMatrix,
}

/// How a run of since-changed content was launched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SinceChangedLaunch {
    /// As the pipeline: "launched as N; content since changed".
    AsPipeline,
    /// As a prefix of the pipeline, cut from a version that is not the
    /// current one: "a prefix of an earlier version of N". Never an earlier
    /// version of N itself (ADR-C39 § 2).
    AsPrefix,
}

/// A matrix cell's gain over the previous ranking stage, or why it has none.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatrixGain {
    /// Per metric, the node's value minus the best value of the nearest
    /// stage before its own.
    OverPreviousStage {
        /// The gains, by metric name.
        values: BTreeMap<String, f64>,
    },
    /// The node is at the first ranking stage — a retrieval leg: nothing
    /// comes before it, and the value stands alone.
    FirstStage,
    /// The pipeline's stages are a guess — two fusions, two rerankers, or a
    /// reranker upstream of the fusion — so which stage came before this one
    /// is too, and no gain is served as fact. Compare's stages say
    /// `confidence: low` over the same derivation.
    Ambiguous,
    /// The node is at no ranking stage: the generator.
    Unstaged,
}

/// A run's launch record (ADR-C39 § 1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct LaunchedAs {
    /// The workspace pipeline name it was launched as — for a prefix run,
    /// its parent's; `null` when the record names none.
    pub name: Option<String>,
    /// For a prefix run, where it was cut from its parent; `null` otherwise.
    pub prefix_of: Option<LaunchedPrefix>,
    /// Whether the workspace holds [`name`](Self::name) now; `null` when the
    /// record names none, and only then. `GET /runs` and the pipeline matrix
    /// read it from the same listing of `pipelines/` as their hash matches;
    /// `GET /runs/{id}` lists no `pipelines/`, so it stays one run's read
    /// that a broken document cannot fail, and sends `unchecked`. A fact
    /// about the name, not the content: a document held under it may hold
    /// other content since.
    pub held: Option<NameHeld>,
}

/// Whether the workspace holds a recorded pipeline name now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NameHeld {
    /// A document is stored under the name exactly as recorded, whether or
    /// not it validates, and none under a case alias of it.
    Exactly,
    /// A document is stored under the same name in another ASCII case, a
    /// case alias the backend refuses to read the name as (`request_invalid`)
    /// — whether or not the name is also stored as recorded, as it can be on
    /// a filesystem that keeps case.
    OtherCase,
    /// No document is stored under the name in any case.
    Gone,
    /// This endpoint does not check: `GET /runs/{id}` lists no `pipelines/`.
    Unchecked,
}

/// Where a launch record says a prefix run was cut from its parent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct LaunchedPrefix {
    /// The node it stops at.
    pub up_to: String,
    /// The canonical hash of the parent's version it was cut from.
    pub parent_pipeline_hash: String,
}

/// A prefix run's place in its parent pipeline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PrefixOf {
    /// The parent pipeline's name.
    pub pipeline: String,
    /// The node the prefix stops at: its output.
    pub up_to: String,
}

/// The nodes of one column no run measured, which a run of the whole
/// pipeline on its benchmark would.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct MissingCells {
    /// The benchmark to launch on: the first name pinned to the digest;
    /// `null` when the registry pins none, and nothing can be launched.
    pub benchmark: Option<String>,
    /// The digest of its dataset.
    pub dataset_version: String,
    /// The nodes, in the rows' order.
    pub nodes: Vec<String>,
}

/// `POST /runs`: the job accepted, and the run id it announced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RunAccepted {
    /// The job's id, under `/jobs/{id}`.
    pub job_id: String,
    /// The run id the launcher announced: the one the run is filed under,
    /// unless what ran differs from what was announced, which the job's
    /// terminal state then reports.
    pub run_id: String,
}

/// `POST /benchmarks/{name}/download`: the download job accepted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DownloadAccepted {
    /// The job's id, under `/jobs/{id}`.
    pub job_id: String,
}

/// `GET /jobs`: every job, in its place, and every fault of the queue's
/// record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct JobListing {
    /// The jobs, by position: the order accepted and as reordered; each
    /// lane's queued jobs in the order its worker takes them.
    pub jobs: Vec<JobSummary>,
    /// The faults of the queue's record that belong to no job — a job file
    /// that does not read, a directory that cannot be listed, a run store
    /// that cannot be listed at start-up — reported, never repaired. A fault
    /// beside a job is on the job, in [`JobSummary::faults`].
    pub faults: Vec<JobFault>,
}

/// One event of `GET /jobs/events`: its name, the SSE `event` field, and
/// its data, the SSE `data` field. The stream sends the two as SSE fields,
/// not as this object; the schema is the map from a name to its data's type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum JobEvent {
    /// A job accepted into the queue.
    Queued(JobSummary),
    /// A job taken by its worker, or a progress tick.
    Running(JobSummary),
    /// A job finished.
    Done(JobSummary),
    /// A job failed.
    Failed(JobSummary),
    /// A job cancelled.
    Cancelled(JobSummary),
    /// A queued job moved by a reorder.
    Reordered(JobSummary),
    /// A fault reported beside a job, which does not change its state: the
    /// job, its faults with the new one last.
    Fault(JobSummary),
    /// The whole queue, sent when the stream cannot replay what a client
    /// missed.
    Resync(JobListing),
}

/// A fault of the queue's record that belongs to no job: a job file the
/// queue could not read, or a directory or store it could not list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct JobFault {
    /// The file or directory.
    pub path: String,
    /// What went wrong, and what the queue did about it.
    pub reason: String,
}

/// One job: what it does and where it stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct JobSummary {
    /// Its id.
    pub id: String,
    /// Its place: a lane's worker takes the queued job with the lowest.
    pub position: u64,
    /// When it was accepted, in milliseconds since the epoch; `null` when
    /// the clock read before it.
    pub created_at_ms: Option<u64>,
    /// What it does.
    pub work: JobWork,
    /// Where it stands.
    pub state: JobStatus,
    /// What went wrong beside it without stopping it — a layout not copied
    /// at launch, latencies left out of its median, a write of its record
    /// that failed — in the order reported. Written into the job's file with
    /// it, so a restart reads them back; one that could not be written says
    /// it is held in memory only, until a later write of the job carries it.
    pub faults: Vec<ReportedFault>,
}

/// A fault beside a job, which did not change its state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(transform = every_property_required)]
pub struct ReportedFault {
    /// What went wrong, and what the queue did about it.
    pub reason: String,
    /// When it was reported, in milliseconds since the epoch; `null` when
    /// the clock read before it.
    pub at_ms: Option<u64>,
}

/// What a job does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[schemars(transform = every_variant_property_required)]
pub enum JobWork {
    /// A run, on the run lane.
    Run {
        /// The run id announced at submission.
        run_id: String,
        /// The pipeline's name in the workspace.
        pipeline: String,
        /// The benchmark.
        benchmark: String,
        /// The `Remote` bindings in force at submission.
        bindings: Vec<ServiceBinding>,
        /// The node a prefix run stops at; `null` for a whole run. The
        /// pipeline is then the parent's name, and the job ran the parent's
        /// document cut at this node.
        up_to: Option<String>,
        /// For a prefix run, the canonical hash of the parent document the
        /// cut was made from at submission; `null` for a whole run. Beside
        /// `up_to`, the prefix's provenance, never part of the run's
        /// identity.
        parent_pipeline_hash: Option<String>,
    },
    /// A benchmark download, on the download lane.
    Download {
        /// The benchmark's selector.
        benchmark: String,
    },
}

/// Where a job stands. A time is in milliseconds since the epoch, `null`
/// when the clock read before it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[schemars(transform = every_variant_property_required)]
pub enum JobStatus {
    /// Waiting for its lane's worker.
    Queued,
    /// Executing: a run counts queries, a download bytes.
    Running {
        /// Queries executed, or bytes received.
        done: u64,
        /// Queries in the benchmark, or bytes in the snapshot; `null` until
        /// the first tick.
        total: Option<u64>,
        /// When the worker took it.
        started_at_ms: Option<u64>,
        /// A run's median query latency so far: the lower median of each
        /// executed query's latency, the sum of its trace's durations — the
        /// figure `GET /runs` lists once it is filed. `null` before the
        /// first query, and for a download.
        median_latency_nanos: Option<u64>,
    },
    /// Finished.
    Done {
        /// The id the run is filed under — computed from what ran; `null`
        /// for a download.
        run_id: Option<String>,
        /// Both ids, when the run is filed under another than the one it
        /// announced.
        id_mismatch: Option<RunIdMismatch>,
        /// When it finished.
        finished_at_ms: Option<u64>,
    },
    /// Failed; `interrupted` for a job found running when the service
    /// started.
    Failed {
        /// What failed.
        error: String,
        /// The node that failed, when one did.
        at_node: Option<String>,
        /// When it failed.
        finished_at_ms: Option<u64>,
        /// How many queries' traces a run kept when it stopped — every query
        /// it executed, the one it failed on included — under
        /// `jobs/<id>/partial/`, which `GET /jobs/{id}/queries` serves. `0`
        /// for a download, a run that executed none, traces that could not
        /// be written, and a job interrupted by a crash, which keeps none it
        /// can vouch for.
        partial_traces: u64,
    },
    /// Cancelled before it finished.
    Cancelled {
        /// When it was cancelled.
        finished_at_ms: Option<u64>,
        /// How many queries' traces a run kept, as for `failed`.
        partial_traces: u64,
    },
}

/// A run filed under another id than it announced: both.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RunIdMismatch {
    /// The id announced at submission.
    pub announced: String,
    /// The id computed from what ran, which the run is filed under.
    pub decided: String,
}

/// [`every_property_required`] applied to each variant of a tagged enum,
/// whose schema is a `oneOf` of one object per variant: a variant's field is
/// serialized whether or not it is `null`, so it is required.
fn every_variant_property_required(schema: &mut schemars::Schema) {
    if let Some(serde_json::Value::Array(variants)) = schema.get_mut("oneOf") {
        for variant in variants {
            if let Ok(variant) = <&mut schemars::Schema>::try_from(variant) {
                every_property_required(variant);
            }
        }
    }
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
