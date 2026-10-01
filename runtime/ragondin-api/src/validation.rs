//! A pipeline document checked as `ragondin validate` checks a file: its
//! schema version peeked, the text parsed into the hand-maintained wire
//! schema, the graph validated, and the canonical logical form hashed —
//! `ragondin-pipeline`'s functions, called on the text, the same three steps
//! `ragondin-config`'s `LocalFile` runs over a file's contents (INV-9: the
//! document is parsed into `RawPipeline`, never into an internal type; INV-8:
//! the hash is the canonical form's).
//!
//! Nothing is added to those checks: `POST /pipelines/validate` answers what
//! `ragondin validate` answers. The composition root's key refusals, which
//! `validate` does not make (ADR-C32 § 2), are `Launcher::check_document`'s,
//! called before a document is stored.
//!
//! The three steps are written here a third time, beside `LocalFile::load`
//! and `ragondin-experiments`' `lower_configuration`, because
//! `ragondin-config` loads only from a path; one path-free loader there is
//! #375.
//!
//! The words are the CLI's, without the file path a request does not have:
//! the cause `ragondin validate` prints under `caused by:`, and for an edge
//! of the wrong kind, the three lines of its incompatible-wiring report.
//! `bin/ragondin`'s `tests/ui.rs` compares the two renderings over the same
//! bytes.

use ragondin_pipeline::{
    peek_schema_version, validate, LogicalPipeline, NodeId, RawPipeline, SchemaVersionPeekError,
    ValidationError, ValueKind,
};

use crate::error::ApiError;
use crate::response::{EdgeLocation, Location};

/// The content hash of `document`'s canonical logical form, or why it is
/// not a pipeline, as `pipeline_invalid`.
pub(crate) fn check(document: &str) -> Result<String, ApiError> {
    Ok(lower(document)?.content_hash().to_string())
}

/// `document`'s validated logical pipeline, or why it is not one, as
/// `pipeline_invalid`.
pub(crate) fn lower(document: &str) -> Result<LogicalPipeline, ApiError> {
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
    validate(raw).map_err(|error| refused(&error))
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
    fn a_url_valued_parameter_is_a_parameter_like_any_other() {
        // What a key holds is not looked at here: whether the key is read is
        // the composition root's to say, through `Launcher::check_document`.
        check(&VALID.replace(
            "params: { top_k: 10 }",
            "params: { top_k: 10, endpoint: \"http://10.0.0.5:50051\" }",
        ))
        .expect("validates, as `ragondin validate` validates it");
    }
}
