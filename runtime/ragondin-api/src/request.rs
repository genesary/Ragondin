//! Every request body the API reads. Like [`response`](crate::response),
//! this module names no other crate of the workspace: a request type is this
//! crate's own, and the description assembles its schema from the derive.
//!
//! Each refuses a field it does not know (`deny_unknown_fields`), as it
//! refuses one it lacks: `request_invalid` either way, so a misspelled field
//! is reported rather than dropped. The schema says so with
//! `additionalProperties: false`, which the UI's type generator reads as the
//! closed object TypeScript gives anyway. `Layout`, which is a request body
//! and a response body alike, is in `response.rs`, and refuses one too.

use schemars::JsonSchema;
use serde::Deserialize;

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
