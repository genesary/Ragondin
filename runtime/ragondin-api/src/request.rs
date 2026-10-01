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

/// `PUT /pipelines/{name}` and `POST /pipelines/validate`: a pipeline
/// document, as text.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PipelineDocument {
    /// The YAML document. Stored byte for byte when it is written: never
    /// re-serialized.
    pub document: String,
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
