//! A pipeline document checked as `ragondin validate` checks a file: its
//! schema version peeked, the text parsed into the hand-maintained wire
//! schema, the graph validated, and the canonical logical form hashed —
//! `ragondin-pipeline`'s functions, called on the text, the same three steps
//! `ragondin-config`'s `LocalFile` runs over a file's contents (INV-9: the
//! document is parsed into `RawPipeline`, never into an internal type; INV-8:
//! the hash is the canonical form's).
//!
//! One refusal is added, and only one: a parameter holding a service's
//! address (ADR-C32 § 1). `ragondin validate` does not make it — the
//! composition root does, when `bench` reads the node — and a document the
//! editor saves must not carry one into the workspace.
//!
//! The words are the CLI's, without the file path a request does not have:
//! the cause `ragondin validate` prints under `caused by:`, and for an edge
//! of the wrong kind, the three lines of its incompatible-wiring report.
//! `bin/ragondin`'s `tests/ui.rs` compares the two renderings over the same
//! bytes.

use ragondin_pipeline::{
    peek_schema_version, validate, LogicalNode, NodeId, ParamValue, RawPipeline,
    SchemaVersionPeekError, ValidationError, ValueKind,
};

use crate::error::ApiError;
use crate::response::{EdgeLocation, Location};

/// The URI schemes a parameter value is refused as an address under: the
/// ones a `Remote` service is reached by, or would be once TLS is decided.
const ADDRESS_SCHEMES: [&str; 4] = ["http://", "https://", "grpc://", "grpcs://"];

/// The content hash of `document`'s canonical logical form, or why it is
/// not a pipeline, as `pipeline_invalid`.
pub(crate) fn check(document: &str) -> Result<String, ApiError> {
    if let Err(SchemaVersionPeekError::Unsupported(source)) =
        peek_schema_version(serde_yaml::Deserializer::from_str(document))
    {
        return Err(invalid(
            format!(
                "the configuration is written in a schema version this build cannot read: {source}"
            ),
            unlocated(),
        ));
    }
    let raw: RawPipeline = serde_yaml::from_str(document).map_err(|error| {
        // The deserializer's message carries the line and the column; a
        // `Location` names nodes and edges, which a syntax error has none of.
        invalid(
            format!("could not parse configuration: {error}"),
            unlocated(),
        )
    })?;
    let pipeline = validate(raw).map_err(|error| refused(&error))?;
    refuse_addresses(pipeline.nodes())?;
    Ok(pipeline.content_hash().to_string())
}

fn invalid(detail: String, location: Location) -> ApiError {
    ApiError::PipelineInvalid { detail, location }
}

fn unlocated() -> Location {
    Location {
        node: None,
        edge: None,
    }
}

fn at_node(node: &NodeId) -> Location {
    Location {
        node: Some(node.as_str().to_owned()),
        edge: None,
    }
}

/// The validation pass's verdict, located where its error names a node.
fn refused(error: &ValidationError) -> ApiError {
    let location = match error {
        ValidationError::KindMismatch {
            consumer,
            port,
            producer,
            expected,
            found,
        } => {
            return invalid(
                incompatible_wiring(consumer, *port, producer, *expected, *found),
                Location {
                    node: Some(consumer.as_str().to_owned()),
                    edge: Some(EdgeLocation {
                        from: producer.as_str().to_owned(),
                        to: consumer.as_str().to_owned(),
                        port: *port as u64,
                    }),
                },
            )
        }
        ValidationError::UnknownComponent { node, .. }
        | ValidationError::NonFiniteParam { node, .. }
        | ValidationError::DanglingInput { node, .. } => at_node(node),
        ValidationError::DuplicateId { id } | ValidationError::InputCollidesWithNode { id } => {
            at_node(id)
        }
        ValidationError::Cycle { nodes } => match nodes.first() {
            Some(node) => at_node(node),
            None => unlocated(),
        },
        ValidationError::InputArity { .. } => unlocated(),
    };
    invalid(
        format!("configuration is not a valid pipeline: {error}"),
        location,
    )
}

/// The report `ragondin validate` prints for an edge of the wrong kind, from
/// its second line on: the producer first, the direction the value travels.
fn incompatible_wiring(
    consumer: &NodeId,
    port: usize,
    producer: &NodeId,
    expected: Option<ValueKind>,
    found: ValueKind,
) -> String {
    let expected = match expected {
        Some(kind) => kind.to_string(),
        None => format!(
            "nothing — `{}` declares no port at position {port}",
            consumer.as_str()
        ),
    };
    format!(
        "the configuration wires two nodes incompatibly\n  \
         edge: `{}` feeds `{}` at port {port}\n  \
         expected: {expected}\n  \
         found: {found}",
        producer.as_str(),
        consumer.as_str(),
    )
}

/// Refuses a node whose parameters hold a service's address, at any depth
/// of a list: an address is deployment data, bound to an implementation
/// name outside the document, and in a parameter it would enter the hash.
fn refuse_addresses(nodes: &[LogicalNode]) -> Result<(), ApiError> {
    for node in nodes {
        let params = match node {
            LogicalNode::Retriever(node) => &node.params,
            LogicalNode::Fusion(node) => &node.params,
            LogicalNode::Reranker(node) => &node.params,
            LogicalNode::ContextBuilder(node) => &node.params,
            LogicalNode::Generator(node) => &node.params,
            LogicalNode::Extension(node) => &node.params,
        };
        for (key, value) in params {
            if let Some(address) = address_in(value) {
                return Err(invalid(
                    format!(
                        "node `{}`: parameter `{key}` holds the address `{address}`; an address \
                         never enters a pipeline — bind the node's implementation name to it as \
                         a service instead",
                        node.id().as_str()
                    ),
                    at_node(node.id()),
                ));
            }
        }
    }
    Ok(())
}

fn address_in(value: &ParamValue) -> Option<&str> {
    match value {
        ParamValue::String(text) => {
            let lower = text.to_ascii_lowercase();
            ADDRESS_SCHEMES
                .iter()
                .any(|scheme| lower.starts_with(scheme))
                .then_some(text.as_str())
        }
        ParamValue::List(values) => values.iter().find_map(address_in),
        ParamValue::Int(_) | ParamValue::Float(_) | ParamValue::Bool(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "pipeline:\n  inputs: [question]\n  nodes:\n    - id: lexical\n      component: retriever\n      impl: bm25\n      inputs: [question]\n      params: { top_k: 10 }\n";

    fn detail(document: &str) -> (String, Location) {
        match check(document) {
            Err(ApiError::PipelineInvalid { detail, location }) => (detail, location),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_valid_document_hashes_to_its_canonical_form() {
        let hash = check(VALID).expect("valid");

        assert_eq!(hash.len(), 64);
        assert_eq!(check(&format!("# a comment\n{VALID}")).unwrap(), hash);
    }

    #[test]
    fn a_syntax_error_is_reported_unlocated_with_the_deserializer_s_line() {
        let (detail, location) = detail("pipeline: [");

        assert!(
            detail.starts_with("could not parse configuration: "),
            "{detail}"
        );
        assert!(detail.contains("line 1"), "{detail}");
        assert_eq!(location, unlocated());
    }

    #[test]
    fn a_dangling_input_names_its_node() {
        let (detail, location) = detail(&VALID.replace(
            "inputs: [question]\n      params",
            "inputs: [nowhere]\n      params",
        ));

        assert!(detail.contains("`nowhere`"), "{detail}");
        assert_eq!(location.node.as_deref(), Some("lexical"));
    }

    #[test]
    fn an_address_nested_in_a_list_is_refused() {
        let (detail, location) = detail(&VALID.replace(
            "params: { top_k: 10 }",
            "params: { top_k: 10, mirrors: [a, \"GRPC://host:1\"] }",
        ));

        assert!(detail.contains("`mirrors`"), "{detail}");
        assert_eq!(location.node.as_deref(), Some("lexical"));
    }

    #[test]
    fn a_path_or_a_name_is_not_an_address() {
        check(&VALID.replace(
            "params: { top_k: 10 }",
            "params: { top_k: 10, model: /models/bge.onnx, note: \"see http docs\" }",
        ))
        .expect("no address");
    }
}
