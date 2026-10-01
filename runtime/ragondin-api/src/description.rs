//! The API's description: an OpenAPI-shaped JSON document assembled from the
//! declared operations and the response types' `schemars` schemas, kept as a
//! golden file at `api/v1.json` (ADR-C36 § 2).
//!
//! It exists so that a change to the API is a reviewed diff, and so that the
//! UI's types can be generated from it. It is deliberately minimal — paths,
//! methods, the request and response schema of each, the problem schema of
//! every error, and the schemas under `components/schemas` — and no OpenAPI
//! generator crate builds it (ADR-C36 § 6 admits none). `just
//! gen-api-description` rewrites the file; `tests/description.rs` fails when
//! it is stale.

use std::collections::BTreeMap;

use schemars::generate::SchemaSettings;
use schemars::SchemaGenerator;
use serde_json::{json, Map, Value};

use crate::request::{ImportRequest, PipelineDocument, ProbeRequest, ServiceAddress};
use crate::response::{
    BenchmarkListing, PipelineDetail, PipelineLayout, PipelineListing, PipelineValidated,
    PipelineWritten, ProbeResult, Problem, QueryTrace, RunDetail, RunListing, RunQueries,
    ServiceListing, Workspace,
};

/// One operation the router serves, as the description declares it.
#[derive(Clone, Copy, Debug)]
pub struct Operation {
    /// The HTTP method, lowercase, as OpenAPI spells it.
    pub method: &'static str,
    /// The path under `/api/v1`, with OpenAPI's `{parameter}` syntax.
    pub path: &'static str,
    /// One line saying what it answers.
    pub summary: &'static str,
    /// The schema name of its `200` response body.
    pub response: &'static str,
    /// The schema name of its JSON request body, when it reads one.
    pub request: Option<&'static str>,
    /// What more a reader needs to know, rendered as the operation's
    /// `description` when present. Its path parameters are read off
    /// [`path`](Self::path), and no operation declares a query parameter or
    /// a header: the UI's type generator refuses both until a screen that
    /// sends one extends it (`ui/ARCHITECTURE.md` § The generated types), so
    /// an operation that takes one says so here.
    pub description: Option<&'static str>,
}

/// Every operation the router serves. `tests/description.rs` checks that each
/// is routed and that each response names a schema the document defines.
pub const OPERATIONS: &[Operation] = &[
    Operation {
        method: "get",
        path: "/workspace",
        summary: "The workspace: its path, its settings, this build, its capabilities and its counts.",
        response: "Workspace",
        request: None,
        description: None,
    },
    Operation {
        method: "get",
        path: "/runs",
        summary: "Every run the store holds, and every one it cannot read.",
        response: "RunListing",
        request: None,
        description: None,
    },
    Operation {
        method: "get",
        path: "/runs/{id}",
        summary: "One run: its inputs, metrics, configuration, bindings and lowered graph.",
        response: "RunDetail",
        request: None,
        description: None,
    },
    Operation {
        method: "get",
        path: "/runs/{id}/queries",
        summary: "A run's queries with their scores read from the trace, and its per-node ranking metrics.",
        response: "RunQueries",
        request: None,
        description: Some(
            "Takes one optional query parameter, `missing_gold_at=<k>`, a positive integer: keep only the judged queries with no gold document (grade above 0) in the top k of the output ranking. It needs the run's own dataset, and answers dataset_absent or dataset_differs without it. Any other parameter, or this one twice, is parameter_invalid. Not declared under `parameters`: the UI's type generator refuses query parameters until the screen that first sends one extends it.",
        ),
    },
    Operation {
        method: "get",
        path: "/runs/{id}/trace/{query}",
        summary: "One query's trace, node by node, with passage text when the run's own dataset is on disk.",
        response: "QueryTrace",
        request: None,
        description: None,
    },
    Operation {
        method: "get",
        path: "/pipelines",
        summary: "Every pipeline document: its etag, its hash or why it does not validate.",
        response: "PipelineListing",
        request: None,
        description: None,
    },
    Operation {
        method: "post",
        path: "/pipelines/validate",
        summary: "The canonical hash `ragondin validate` prints for a document, or `pipeline_invalid`, located.",
        response: "PipelineValidated",
        request: Some("PipelineDocument"),
        description: None,
    },
    Operation {
        method: "get",
        path: "/pipelines/{name}",
        summary: "One pipeline document, verbatim, with its etag and its hash or why it does not validate.",
        response: "PipelineDetail",
        request: None,
        description: Some(
            "The etag is also the response's `ETag` header, quoted. Not declared as a header: the UI's type generator reads path parameters only.",
        ),
    },
    Operation {
        method: "put",
        path: "/pipelines/{name}",
        summary: "Stores a document byte for byte when it validates and its precondition holds.",
        response: "PipelineWritten",
        request: Some("PipelineDocument"),
        description: Some(
            "Requires the header `If-Match: \"<etag>\"` to replace the stored document, or `If-None-Match: *` to create one. A stale etag, a creation over an existing document, or neither header is precondition_failed (412), with the current etag in the `ETag` header and the detail, and nothing written. The answer's etag is also its `ETag` header. Not declared as headers: the UI's type generator reads path parameters only.",
        ),
    },
    Operation {
        method: "get",
        path: "/pipelines/{name}/layout",
        summary: "The layout beside a pipeline document, or `null`.",
        response: "PipelineLayout",
        request: None,
        description: None,
    },
    Operation {
        method: "put",
        path: "/pipelines/{name}/layout",
        summary: "Replaces the layout beside a pipeline document; never changes its etag or hash.",
        response: "PipelineLayout",
        request: Some("Layout"),
        description: None,
    },
    Operation {
        method: "get",
        path: "/benchmarks",
        summary: "Every benchmark the registry knows, with its state and licence.",
        response: "BenchmarkListing",
        request: None,
        description: None,
    },
    Operation {
        method: "post",
        path: "/benchmarks/import",
        summary: "Imports a corpus on the server's disk as a local benchmark.",
        response: "BenchmarkEntry",
        request: Some("ImportRequest"),
        description: None,
    },
    Operation {
        method: "get",
        path: "/services",
        summary: "The `Remote` bindings the workspace holds, and what their last probe read.",
        response: "ServiceListing",
        request: None,
        description: None,
    },
    Operation {
        method: "put",
        path: "/services/{family}/{name}",
        summary: "Binds a name to an address, refused in the words `ragondin bench --remote` uses.",
        response: "ServiceListing",
        request: Some("ServiceAddress"),
        description: None,
    },
    Operation {
        method: "delete",
        path: "/services/{family}/{name}",
        summary: "Unbinds a name.",
        response: "ServiceListing",
        request: None,
        description: None,
    },
    Operation {
        method: "post",
        path: "/services/{family}/{name}/probe",
        summary: "Reads a bound service's identity as a run would, or `service_unreachable`.",
        response: "ProbeResult",
        request: Some("ProbeRequest"),
        description: None,
    },
];

/// The description as the golden file holds it: pretty-printed JSON with a
/// trailing newline. Object keys are sorted — `serde_json`'s map is ordered —
/// so the text depends on the API alone.
pub fn render() -> String {
    // `{:#}` is `serde_json`'s pretty printer, and it cannot fail.
    format!("{:#}\n", description())
}

fn description() -> Value {
    let mut generator: SchemaGenerator = SchemaSettings::openapi3().into_generator();
    // Every response type, and the problem body. A type reachable from these
    // is defined too, under its own name.
    generator.subschema_for::<Workspace>();
    generator.subschema_for::<RunListing>();
    generator.subschema_for::<RunDetail>();
    generator.subschema_for::<RunQueries>();
    generator.subschema_for::<QueryTrace>();
    generator.subschema_for::<PipelineListing>();
    generator.subschema_for::<PipelineDetail>();
    generator.subschema_for::<PipelineWritten>();
    generator.subschema_for::<PipelineValidated>();
    generator.subschema_for::<PipelineLayout>();
    generator.subschema_for::<BenchmarkListing>();
    generator.subschema_for::<ServiceListing>();
    generator.subschema_for::<ProbeResult>();
    generator.subschema_for::<Problem>();
    // The request bodies.
    generator.subschema_for::<PipelineDocument>();
    generator.subschema_for::<ImportRequest>();
    generator.subschema_for::<ServiceAddress>();
    generator.subschema_for::<ProbeRequest>();
    let schemas: Map<String, Value> = generator.take_definitions(true);

    let mut paths: BTreeMap<&str, Map<String, Value>> = BTreeMap::new();
    for operation in OPERATIONS {
        // Each `{name}` of the path, in order.
        let parameters: Vec<Value> = operation
            .path
            .split('/')
            .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
            .map(|name| {
                json!({
                    "name": name,
                    "in": "path",
                    "required": true,
                    "schema": { "type": "string" },
                })
            })
            .collect();
        let mut entry = json!({
            "summary": operation.summary,
            "parameters": parameters,
            "responses": {
                "200": {
                    "description": operation.summary,
                    "content": { "application/json": { "schema": {
                        "$ref": format!("#/components/schemas/{}", operation.response),
                    } } },
                },
                "default": {
                    "description": "An error, as application/problem+json.",
                    "content": { "application/problem+json": { "schema": {
                        "$ref": "#/components/schemas/Problem",
                    } } },
                },
            },
        });
        if let Some(description) = operation.description {
            entry["description"] = json!(description);
        }
        if let Some(request) = operation.request {
            entry["requestBody"] = json!({
                "required": true,
                "content": { "application/json": { "schema": {
                    "$ref": format!("#/components/schemas/{request}"),
                } } },
            });
        }
        paths
            .entry(operation.path)
            .or_default()
            .insert(operation.method.to_owned(), entry);
    }

    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "ragondin",
            "version": "v1",
            "description": "The JSON API `ragondin ui` serves to the UI embedded in the same build.",
        },
        "servers": [{ "url": "/api/v1" }],
        "paths": paths,
        "components": { "schemas": schemas },
    })
}
