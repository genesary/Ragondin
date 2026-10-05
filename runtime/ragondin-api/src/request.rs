//! Every request body the API reads, and every type its query parameters and
//! request headers are read into. Like [`response`](crate::response), this
//! module names no other crate of the workspace: a request type is this
//! crate's own, and the description assembles its schema from the derive.
//!
//! Each body refuses a field it does not know (`deny_unknown_fields`), as it
//! refuses one it lacks: `request_invalid` either way, so a misspelled field
//! is reported rather than dropped. The schema says so with
//! `additionalProperties: false`, which the UI's type generator reads as the
//! closed object TypeScript gives anyway. `Layout`, which is a request body
//! and a response body alike, is in `response.rs`, and refuses one too.
//!
//! A query parameter type is closed the same way, and its values type
//! themselves (ADR-C37 § 4): `parameter_invalid` for anything it does not
//! read. A header type is not closed, since a request carries headers no
//! handler reads; its fields carry the headers' wire names.

use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::{self, Deserializer, Visitor};
use serde::Deserialize;

/// `GET /runs/{id}/queries`: its query parameters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunQueriesParameters {
    /// Keep only the judged queries with no gold document (grade above 0) in
    /// the top k of the output ranking. It needs the run's own dataset, and
    /// answers dataset_absent or dataset_differs without it.
    pub missing_gold_at: Option<MissingGoldAt>,
}

/// `missing_gold_at`'s value: a positive integer, the depth of the ranking a
/// gold document is looked for in. Its refusal names the parameter, since
/// serde's reason does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissingGoldAt(pub usize);

impl<'de> Deserialize<'de> for MissingGoldAt {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Depth;

        impl Visitor<'_> for Depth {
            type Value = MissingGoldAt;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("`missing_gold_at` as a positive integer")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<MissingGoldAt, E> {
                value
                    .parse::<usize>()
                    .ok()
                    .filter(|depth| *depth > 0)
                    .map(MissingGoldAt)
                    .ok_or_else(|| {
                        E::custom(format!(
                            "`missing_gold_at` is a positive integer, not `{value}`"
                        ))
                    })
            }
        }

        // A query string's values are text: the parse is this type's own.
        deserializer.deserialize_str(Depth)
    }
}

impl JsonSchema for MissingGoldAt {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "MissingGoldAt".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({ "type": "integer", "format": "uint", "minimum": 1 })
    }
}

/// `GET /pipelines/{name}/matrix`: its query parameters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PipelineMatrixParameters {
    /// Also give a column to every benchmark the registry knows that no
    /// counted run ran on, each of its cells not_run_yet. Absent is false.
    pub include_available: Option<IncludeAvailable>,
}

/// `include_available`'s value: `true` or `false`, as text. Its refusal
/// names the parameter, since serde's reason does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncludeAvailable(pub bool);

impl<'de> Deserialize<'de> for IncludeAvailable {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Flag;

        impl Visitor<'_> for Flag {
            type Value = IncludeAvailable;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("`include_available` as `true` or `false`")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<IncludeAvailable, E> {
                match value {
                    "true" => Ok(IncludeAvailable(true)),
                    "false" => Ok(IncludeAvailable(false)),
                    other => Err(E::custom(format!(
                        "`include_available` is `true` or `false`, not `{other}`"
                    ))),
                }
            }
        }

        // A query string's values are text: the parse is this type's own.
        deserializer.deserialize_str(Flag)
    }
}

impl JsonSchema for IncludeAvailable {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "IncludeAvailable".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({ "type": "boolean" })
    }
}

/// `PUT /pipelines/{name}`: the precondition headers a write reads. Each is
/// optional on the wire; the handler says what a write that states neither,
/// or both, is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct PreconditionHeaders {
    /// `"<etag>"` to replace the stored document with that etag — a weak
    /// `W/` tag reads as its strong form — or `*` to replace whatever is
    /// stored.
    #[serde(rename = "If-Match")]
    pub if_match: Option<String>,
    /// `*`, to create the document: nothing may be stored under the name.
    #[serde(rename = "If-None-Match")]
    pub if_none_match: Option<String>,
}

impl crate::extract::HeaderFields for PreconditionHeaders {
    const NAMES: &'static [&'static str] = &["If-Match", "If-None-Match"];
}

/// `POST /pipelines/{name}/rename`'s header: the etag the rename moves, as
/// a write names it. Optional on the wire, as a write's are; a rename that
/// states none is refused, since it would move a document it never read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct RenameHeaders {
    /// `"<etag>"`: the stored document must be at that etag — a weak `W/` tag
    /// reads as its strong form — or `*`, at any.
    #[serde(rename = "If-Match")]
    pub if_match: Option<String>,
}

impl crate::extract::HeaderFields for RenameHeaders {
    const NAMES: &'static [&'static str] = &["If-Match"];
}

/// `POST /pipelines/{name}/rename`: the name the pipeline takes.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameRequest {
    /// The new name: one file name, under the rule every pipeline name
    /// follows, that no stored pipeline has in any case.
    pub to: String,
}

/// `PUT /pipelines/{name}`: a pipeline document, as text or as the editor
/// holds it — one key, naming which (ADR-C40 § 5, § 6).
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PipelineDocument {
    /// The YAML document, stored byte for byte: never re-serialized. What an
    /// import and a fork from a run send.
    Document(String),
    /// The typed document the editor holds. The server converts it to the
    /// wire schema and stores its rendering — the bytes `POST
    /// /pipelines/validate` answers as `rendering` for the same document.
    Typed(crate::response::TypedDocument),
}

/// `POST /pipelines/validate`: a pipeline document, as text or as the editor
/// holds it — one key, naming which (ADR-C40 § 5, § 6).
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ValidationRequest {
    /// The YAML document, checked exactly as `ragondin validate` checks a
    /// file: what an import and the command line send.
    Document(String),
    /// The typed document the editor holds. The server converts it to the
    /// wire schema, renders that as text, and checks the rendering, so the
    /// hash is the one of the bytes a write would store.
    Typed(crate::response::TypedDocument),
}

/// `POST /benchmarks/import`: a corpus on disk, and the name to import it as.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    /// The local benchmark's name: one directory name.
    pub name: String,
    /// The directory holding the corpus and its ground truth, on the
    /// server's disk.
    pub path: String,
}

/// `PUT /services/{family}/{name}`: the address to bind the name to.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceAddress {
    /// The service's address — the scheme `http`, a host and an optional
    /// port, nothing else — as `ragondin bench --remote` takes it.
    pub uri: String,
}

/// `POST /services/{family}/{name}/probe`: what the identity read needs
/// besides the binding.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProbeRequest {
    /// The name the service serves the model under — a node's
    /// `served_model`. An embedder, a reranker or a generator reports an
    /// identity only for one; a context builder takes none. Absent is none.
    pub served_model: Option<String>,
}

/// `POST /compare`: the runs to compare, the baseline among them, and
/// optionally a manual pairing to keep before comparing.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareRequest {
    /// The runs, by id: two to five, each once, the baseline among them. The
    /// answer puts the baseline first and the others in this order.
    pub run_ids: Vec<String>,
    /// The run the others are compared against.
    pub baseline: String,
    /// A manual pairing between the baseline's workspace pipeline and
    /// another compared run's: applied to this comparison, then kept —
    /// replacing any — once the answer is built, so a refused request keeps
    /// nothing. With no pairs, it is removed and the two pipelines pair
    /// automatically again. Absent changes nothing.
    pub pairing: Option<crate::response::Pairing>,
}

/// `POST /runs`: what to run. The bindings are not sent: the workspace's
/// bindings in force — those `GET /services` lists — are snapshotted into
/// the job at submission.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    /// The workspace pipeline's name; its document is snapshotted into the
    /// job at submission, so an edit afterwards changes nothing queued.
    pub pipeline: String,
    /// The benchmark's selector, `<format>/<name>`.
    pub benchmark: String,
    /// The node a prefix run stops after; absent for the whole pipeline.
    pub up_to: Option<String>,
}

/// `PATCH /jobs/{id}`: where to move a queued job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReorderRequest {
    /// Its place among its lane's queued jobs, from 0 — the next taken. A
    /// place past the last moves it last.
    pub position: u64,
}

/// `GET /jobs/events`: the header a reconnecting client sends.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct EventsHeaders {
    /// The id of the last event the client received, to resume after it.
    /// Absent, or one this server no longer holds the events after, and the
    /// stream begins with `resync`.
    #[serde(rename = "Last-Event-ID")]
    pub last_event_id: Option<String>,
}

impl crate::extract::HeaderFields for EventsHeaders {
    const NAMES: &'static [&'static str] = &["Last-Event-ID"];
}
