//! The API's description: an OpenAPI-shaped JSON document assembled from the
//! declared operations and the response types' `schemars` schemas, kept as a
//! golden file at `api/v1.json` (ADR-C36 § 2).
//!
//! It exists so that a change to the API is a reviewed diff, and so that the
//! UI's types can be generated from it. It is deliberately minimal — paths,
//! methods, the parameters of each in the path, the query string and the
//! request headers, the request and response schema of each, the problem
//! schema of every error, and the schemas under `components/schemas` — and
//! no OpenAPI
//! generator crate builds it (ADR-C36 § 6 admits none). `just
//! gen-api-description` rewrites the file; `tests/description.rs` fails when
//! it is stale.

use std::collections::BTreeMap;

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde_json::{json, Map, Value};

use crate::request::{
    CompareRequest, ImportRequest, PipelineDocument, ProbeRequest, ServiceAddress,
};
use crate::response::{
    BenchmarkListing, Comparison, PipelineDetail, PipelineLayout, PipelineListing, PipelineMatrix,
    PipelineValidated, PipelineWritten, ProbeResult, Problem, QueryTrace, RunDetail, RunListing,
    RunQueries, ServiceListing, Workspace,
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
    /// `description` when present. Its parameters are not stated here: the
    /// path's are read off [`path`](Self::path), and the query's and the
    /// headers' off the route's handler — the `ApiQuery` and `ApiHeaders`
    /// types it takes, which the route list records (ADR-C37 § 5).
    pub description: Option<&'static str>,
}

/// How a query or header type gives its schema: [`schema_of`] at that type.
pub(crate) type Parameters = fn(&mut SchemaGenerator) -> Schema;

/// `T`'s own schema, as a route's query or header type declares it.
pub(crate) fn schema_of<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
    T::json_schema(generator)
}

/// Where a parameter travels, besides the path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Query,
    Header,
}

impl Place {
    fn name(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Header => "header",
        }
    }
}

/// The parameters `of` declares, as OpenAPI parameters in `place`: one per
/// property of its object schema, `required` as the schema says, with the
/// property's documentation as the parameter's. A query type that is not
/// closed — `additionalProperties` anything but `false`, as a map or a
/// flattened map gives — is refused, since its handler would accept a
/// parameter the description does not declare (ADR-C37 § 5); a header type
/// is not closed, since a request carries headers no handler reads.
fn declare_parameters(
    generator: &mut SchemaGenerator,
    place: Place,
    of: Parameters,
) -> Result<Vec<Value>, String> {
    // The generator's own transforms — OpenAPI's `nullable`, among them —
    // as it applies them to every definition.
    let mut schema = of(generator);
    for transform in generator.transforms_mut() {
        transform.transform(&mut schema);
    }
    let schema = schema.to_value();
    if schema.get("type") != Some(&json!("object")) {
        return Err(format!(
            "a {} parameter type is a struct, and this schema is not one: {schema}",
            place.name()
        ));
    }
    if place == Place::Query && schema.get("additionalProperties") != Some(&Value::Bool(false)) {
        return Err(format!(
            "a query parameter type is closed, `#[serde(deny_unknown_fields)]`, and this \
             schema's `additionalProperties` is not `false`: {schema}"
        ));
    }
    // A closed struct with no field — `NoParameters` — declares none.
    let none = Map::new();
    let properties = match schema.get("properties") {
        Some(Value::Object(properties)) => properties,
        None => &none,
        Some(_) => return Err(format!("`properties` that is not an object: {schema}")),
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Ok(properties
        .iter()
        .map(|(name, property)| {
            let mut property = property.clone();
            let documented = property.as_object_mut().and_then(|property| {
                // An absent parameter is not a `null` one: optional is
                // `required`'s to say.
                property.remove("nullable");
                property.remove("description")
            });
            let mut parameter = json!({
                "name": name,
                "in": place.name(),
                "required": required.contains(&name.as_str()),
                "schema": property,
            });
            if let Some(description) = documented {
                parameter["description"] = description;
            }
            parameter
        })
        .collect())
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
        description: None,
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
        method: "post",
        path: "/compare",
        summary: "Runs of one benchmark against a baseline: the metric table, the parameter matrix, the stages with their pairing, the per-query deltas and their bins, and the latency per node.",
        response: "Comparison",
        request: Some("CompareRequest"),
        description: Some(
            "Two to five runs, each once, the baseline among them. More than five, or runs whose dataset_version differs, is runs_not_comparable (409), naming the ceiling or both versions; there is no comparison across benchmarks. A body's `pairing` — between the baseline's pipeline and another compared run's — is checked first and applied to this comparison, and kept under `pipelines/<pipeline>.pairing/<other>.json` only once the response is built, so a refused request keeps nothing; it is read in both directions after. With no pairs it is removed (\"Reset to automatic\"). A pairing of other pipelines, or a pair naming a node that is not a retriever, fusion or reranker of its pipeline, is request_invalid.",
        ),
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
            "The etag is also the response's `ETag` header, quoted.",
        ),
    },
    Operation {
        method: "put",
        path: "/pipelines/{name}",
        summary: "Stores a document byte for byte when it validates and its precondition holds.",
        response: "PipelineWritten",
        request: Some("PipelineDocument"),
        description: Some(
            "A write states one precondition: `If-Match` to replace the stored document, or `If-None-Match: *` to create one. A stale etag, `If-Match: *` with nothing stored, a creation over an existing document, or neither header is precondition_failed (412), with the current etag in the `ETag` header, the detail and the problem's `etag` member, and nothing written; both headers at once is request_invalid. A document the composition root refuses — a key no component reads — is pipeline_invalid, in `ragondin bench`'s words. The answer's etag is also its `ETag` header.",
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
        path: "/pipelines/{name}/matrix",
        summary: "A pipeline's node × benchmark matrix over its runs: per benchmark the most recent run of its current form or of a prefix of it, each node's figure with its gain over the previous stage, or why the cell is empty.",
        response: "PipelineMatrix",
        request: None,
        description: Some(
            "A run fills a cell when its pipeline hash is the document's current one, under any name, or when it is a prefix of the current document — its launch record's parent_pipeline_hash is the current hash, or else the structural test says so, for every run. A run whose launch record names the pipeline and that is neither fills no cell: it is a feeding run with content_since_changed, its parameter difference against the current document, and a benchmark whose only runs are such runs reads not_run_on_this_version, linking one. Any other run counts nowhere. Each column is the most recent run of the whole current form on its benchmark, or, with none, the most recent prefix — the greatest started_at_ms, a run with no time after every run with one, ties by run id — so `missing` never names a launch that exists. A ranking cell's gain is over_previous_stage, first_stage (a leg), ambiguous (the stage derivation guessed) or unstaged. A feeding run carries its launch record (launched_as) and the documents sharing its hash (pipeline_names) side by side. An empty cell says why: no_qrels, no_reference_answers, not_run_yet (with the benchmark), prefix_stops (with the node the prefix stops at), not_run_on_this_version (with the run), not_scored, unverified, no_figure. `include_available=true` adds a column for every benchmark the registry knows that no counted run ran on.",
        ),
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
    generator.subschema_for::<Comparison>();
    generator.subschema_for::<PipelineListing>();
    generator.subschema_for::<PipelineDetail>();
    generator.subschema_for::<PipelineWritten>();
    generator.subschema_for::<PipelineValidated>();
    generator.subschema_for::<PipelineLayout>();
    generator.subschema_for::<PipelineMatrix>();
    generator.subschema_for::<BenchmarkListing>();
    generator.subschema_for::<ServiceListing>();
    generator.subschema_for::<ProbeResult>();
    generator.subschema_for::<Problem>();
    // The request bodies.
    generator.subschema_for::<PipelineDocument>();
    generator.subschema_for::<ImportRequest>();
    generator.subschema_for::<ServiceAddress>();
    generator.subschema_for::<ProbeRequest>();
    generator.subschema_for::<CompareRequest>();
    // The query and header parameters, before the definitions are taken: a
    // type one of them refers to is defined under its own name too.
    let mut routes = crate::routes::Declared::default();
    crate::routes::api(&mut routes);
    for route in &routes.0 {
        assert!(
            OPERATIONS
                .iter()
                .any(|operation| (operation.method, operation.path) == (route.method, route.path)),
            "{} {} is routed and not described",
            route.method,
            route.path
        );
    }
    let declared: Vec<Vec<Value>> = OPERATIONS
        .iter()
        .map(|operation| {
            let route = routes
                .0
                .iter()
                .find(|route| (route.method, route.path) == (operation.method, operation.path))
                .unwrap_or_else(|| {
                    panic!(
                        "{} {} is described and not routed",
                        operation.method, operation.path
                    )
                });
            let mut declared = Vec::new();
            for (place, of) in [(Place::Query, route.query), (Place::Header, route.headers)] {
                if let Some(of) = of {
                    declared.extend(
                        declare_parameters(&mut generator, place, of).unwrap_or_else(|refused| {
                            panic!("{} {}: {refused}", operation.method, operation.path)
                        }),
                    );
                }
            }
            declared
        })
        .collect();
    let schemas: Map<String, Value> = generator.take_definitions(true);

    let mut paths: BTreeMap<&str, Map<String, Value>> = BTreeMap::new();
    for (operation, declared) in OPERATIONS.iter().zip(declared) {
        // Each `{name}` of the path, in order, then the query's and the
        // headers'.
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
        parameters.extend(declared);
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use schemars::JsonSchema;
    use serde::Deserialize;

    use super::*;

    fn declare<T: JsonSchema>(place: Place) -> Result<Vec<Value>, String> {
        let mut generator: SchemaGenerator = SchemaSettings::openapi3().into_generator();
        declare_parameters(&mut generator, place, schema_of::<T>)
    }

    #[allow(dead_code)]
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct Closed {
        /// How many.
        depth: Option<u32>,
        kind: u8,
    }

    #[allow(dead_code)]
    #[derive(Deserialize, JsonSchema)]
    struct Open {
        depth: Option<u32>,
    }

    /// The shape that loses the checks: every parameter the struct does not
    /// name lands in the map. (Under `deny_unknown_fields` the derived schema
    /// drops the map and reads as closed; serde then refuses an unknown
    /// parameter too, so the closed schema is what the type does.)
    #[allow(dead_code)]
    #[derive(Deserialize, JsonSchema)]
    struct Flattened {
        depth: Option<u32>,
        #[serde(flatten)]
        rest: BTreeMap<String, String>,
    }

    #[test]
    fn a_closed_query_type_declares_each_field_required_as_its_type_says() {
        let declared = declare::<Closed>(Place::Query).expect("a closed struct is declared");
        assert_eq!(
            declared,
            [
                json!({
                    "name": "depth",
                    "in": "query",
                    "required": false,
                    "description": "How many.",
                    "schema": { "type": "integer", "format": "uint32", "minimum": 0 },
                }),
                json!({
                    "name": "kind",
                    "in": "query",
                    "required": true,
                    "schema": { "type": "integer", "format": "uint8", "minimum": 0, "maximum": 255 },
                }),
            ]
        );
    }

    #[test]
    fn a_query_type_that_is_not_closed_cannot_be_declared() {
        let refused = declare::<Open>(Place::Query).unwrap_err();
        assert!(refused.contains("additionalProperties"), "{refused}");
    }

    #[test]
    fn a_map_or_a_flattened_field_cannot_be_declared_as_query_parameters() {
        assert!(declare::<BTreeMap<String, u32>>(Place::Query).is_err());
        assert!(declare::<Flattened>(Place::Query).is_err());
    }

    #[test]
    fn a_header_type_need_not_be_closed() {
        let declared = declare::<Open>(Place::Header).expect("a header type is open");
        assert_eq!(declared[0]["in"], "header");
        assert_eq!(declared[0]["required"], false);
    }
}
