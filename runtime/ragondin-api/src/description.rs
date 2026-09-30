//! The API's description: an OpenAPI-shaped JSON document assembled from the
//! declared operations and the response types' `schemars` schemas, kept as a
//! golden file at `api/v1.json` (ADR-C36 § 2).
//!
//! It exists so that a change to the API is a reviewed diff, and so that the
//! UI's types can be generated from it. It is deliberately minimal — paths,
//! methods, the response schema of each, the problem schema of every error,
//! and the schemas under `components/schemas` — and no OpenAPI generator crate
//! builds it (ADR-C36 § 6 admits none). `just gen-api-description` rewrites
//! the file; `tests/description.rs` fails when it is stale.

use std::collections::BTreeMap;

use schemars::generate::SchemaSettings;
use schemars::SchemaGenerator;
use serde_json::{json, Map, Value};

use crate::response::{Problem, QueryTrace, RunDetail, RunListing, RunQueries, Workspace};

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
    /// The query parameters it takes. Its path parameters are read off
    /// [`path`](Self::path).
    pub query: &'static [QueryParameter],
}

/// A query parameter an operation takes: optional, and a positive integer —
/// the only kind any operation takes today.
#[derive(Clone, Copy, Debug)]
pub struct QueryParameter {
    /// Its name.
    pub name: &'static str,
    /// What it does.
    pub description: &'static str,
}

/// Every operation the router serves. `tests/description.rs` checks that each
/// is routed and that each response names a schema the document defines.
pub const OPERATIONS: &[Operation] = &[
    Operation {
        method: "get",
        path: "/workspace",
        summary: "The workspace: its path, its settings, this build and its capabilities.",
        response: "Workspace",
        query: &[],
    },
    Operation {
        method: "get",
        path: "/runs",
        summary: "Every run the store holds, and every one it cannot read.",
        response: "RunListing",
        query: &[],
    },
    Operation {
        method: "get",
        path: "/runs/{id}",
        summary: "One run: its inputs, metrics, configuration, bindings and lowered graph.",
        response: "RunDetail",
        query: &[],
    },
    Operation {
        method: "get",
        path: "/runs/{id}/queries",
        summary: "A run's queries with their scores read from the trace, and its per-node ranking metrics.",
        response: "RunQueries",
        query: &[QueryParameter {
            name: "missing_gold_at",
            description: "Keep only the judged queries with no gold document (grade above 0) in the top k of the output ranking. Needs the run's own dataset: dataset_absent or dataset_differs otherwise.",
        }],
    },
    Operation {
        method: "get",
        path: "/runs/{id}/trace/{query}",
        summary: "One query's trace, node by node, with passage text when the run's own dataset is on disk.",
        response: "QueryTrace",
        query: &[],
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
    generator.subschema_for::<Problem>();
    let schemas: Map<String, Value> = generator.take_definitions(true);

    let mut paths: BTreeMap<&str, Map<String, Value>> = BTreeMap::new();
    for operation in OPERATIONS {
        // Each `{name}` of the path, in order, then the query parameters.
        let mut parameters: Vec<Value> = operation
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
        parameters.extend(operation.query.iter().map(|parameter| {
            json!({
                "name": parameter.name,
                "in": "query",
                "required": false,
                "description": parameter.description,
                "schema": { "type": "integer", "minimum": 1 },
            })
        }));
        let entry = json!({
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
