//! A pipeline document checked as `ragondin validate` checks a file:
//! `ragondin-config`'s `parse_document`, the one definition of the load, run
//! on the request's text, and the canonical logical form hashed (INV-9: the
//! document is parsed into `RawPipeline`, never into an internal type; INV-8:
//! the hash is the canonical form's).
//!
//! The editor's typed document (ADR-C40) takes the same path: converted to
//! the wire schema, rendered as text by `ragondin-config`'s
//! `render_document`, and that rendering checked like any text.
//!
//! Nothing is added to those checks: `POST /pipelines/validate` answers what
//! `ragondin validate` answers. The composition root's key refusals, which
//! `validate` does not make (ADR-C32 § 2), are `Launcher::check_document`'s,
//! called before a document is stored.
//!
//! What is this module's own is the problem body: the refusal's heading and
//! cause — the CLI's words without the file path a request does not have —
//! and its location. For an edge of the wrong kind the detail is
//! `ragondin-config`'s `incompatible_wiring` report, the one the CLI prints,
//! with "the configuration" where the CLI names the file. `bin/ragondin`'s
//! `tests/ui.rs` compares the two byte for byte.

use ragondin_config::{incompatible_wiring, parse_document, render_document, DocumentError};
use ragondin_pipeline::{LogicalPipeline, NodeId, ValidationError};

use crate::convert;
use crate::error::ApiError;
use crate::response::{EdgeLocation, Location, TypedDocument};

/// The content hash of `document`'s canonical logical form, or why it is
/// not a pipeline, as `pipeline_invalid`. The one place this crate renders a
/// document's hash: every listing, write and lineage reads it here.
pub(crate) fn check(document: &str) -> Result<String, ApiError> {
    Ok(lower(document)?.content_hash().to_string())
}

/// The content hash of the editor's typed document, or why it is not a
/// pipeline, as `pipeline_invalid` (ADR-C40 § 5). The document is converted
/// to the wire schema, rendered as text by `ragondin-config`'s one renderer,
/// and that text goes through [`check`]: the hash is the one of exactly the
/// bytes a write would store, and the load is the one every text takes.
pub(crate) fn check_typed(typed: &TypedDocument) -> Result<String, ApiError> {
    let raw = convert::wire_document(typed)
        .map_err(|unsupported| refusal(DocumentError::UnsupportedSchemaVersion(unsupported)))?;
    let text = render_document(&raw).map_err(|error| ApiError::PipelineInvalid {
        detail: error.to_string(),
        location: unlocated(),
    })?;
    check(&text)
}

/// `document`'s validated logical pipeline, or why it is not one, as
/// `pipeline_invalid`.
pub(crate) fn lower(document: &str) -> Result<LogicalPipeline, ApiError> {
    parse_document(document).map_err(refusal)
}

/// The load's refusal as `pipeline_invalid`, located where it can be.
fn refusal(error: DocumentError) -> ApiError {
    let (detail, location) = match &error {
        DocumentError::Invalid(verdict) => (
            incompatible_wiring("the configuration", verdict).unwrap_or_else(|| headed(&error)),
            located(verdict),
        ),
        // The deserializer's message carries the line and the column; a
        // `Location` names nodes and edges, which these have none of.
        DocumentError::UnsupportedSchemaVersion(_) | DocumentError::Malformed(_) => {
            (headed(&error), unlocated())
        }
    };
    ApiError::PipelineInvalid { detail, location }
}

/// The refusal's heading and its cause, on one line: what `ragondin validate`
/// prints as `error:` and `caused by:`, without the file.
fn headed(error: &DocumentError) -> String {
    // Exhaustive on purpose, though the arms are alike: each variant carries a
    // differently typed cause, and a variant `ragondin-config` adds later must
    // be worded here rather than fall into a catch-all.
    match error {
        DocumentError::UnsupportedSchemaVersion(cause) => format!("{error}: {cause}"),
        DocumentError::Malformed(cause) => format!("{error}: {cause}"),
        DocumentError::Invalid(cause) => format!("{error}: {cause}"),
    }
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

/// Where the validation pass's verdict points: the edge for a kind mismatch,
/// the node wherever the error names one.
fn located(verdict: &ValidationError) -> Location {
    match verdict {
        ValidationError::KindMismatch {
            consumer,
            port,
            producer,
            ..
        } => Location {
            node: Some(consumer.as_str().to_owned()),
            edge: Some(EdgeLocation {
                from: producer.as_str().to_owned(),
                to: consumer.as_str().to_owned(),
                port: *port as u64,
            }),
        },
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
