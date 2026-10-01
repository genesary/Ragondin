//! A pipeline document loaded from text: [`parse_document`], the one
//! definition of the load every caller runs — `LocalFile` over a file's
//! contents, the API over a request's body, the experiment plane over a
//! stored run's configuration — and [`incompatible_wiring`], the one
//! rendering of an edge of the wrong kind.

use ragondin_config::{incompatible_wiring, parse_document, DocumentError};
use ragondin_pipeline::{NodeId, ValidationError, ValueKind};

const VALID: &str = "pipeline:\n  inputs: [question]\n  nodes:\n    - id: lexical\n      component: retriever\n      impl: bm25\n      inputs: [question]\n      params: { top_k: 10 }\n";

#[test]
fn a_valid_document_lowers_to_its_logical_pipeline() {
    let pipeline = parse_document(VALID).expect("valid");

    assert_eq!(pipeline.inputs(), &[NodeId::new("question")]);
    assert_eq!(
        parse_document(&format!("# a comment\n{VALID}"))
            .unwrap()
            .content_hash(),
        pipeline.content_hash()
    );
}

#[test]
fn a_version_this_build_cannot_read_is_its_own_diagnosis() {
    let error = parse_document("version: 99\npipeline:\n  inputs: [q]\n  nodes: []\n").unwrap_err();

    let DocumentError::UnsupportedSchemaVersion(source) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(source.found(), 99);
    assert_eq!(
        error.to_string(),
        "the configuration is written in a schema version this build cannot read"
    );
}

#[test]
fn text_outside_the_wire_schema_is_malformed_with_the_deserializer_s_location() {
    let error = parse_document("pipeline:\n  inputs: [q\n  nodes: []\n").unwrap_err();

    let DocumentError::Malformed(source) = &error else {
        panic!("{error:?}");
    };
    assert!(source.to_string().contains("line 3 column 8"), "{source}");
    assert_eq!(error.to_string(), "could not parse configuration");
}

#[test]
fn a_graph_the_pass_refuses_is_invalid_with_the_pass_s_verdict_intact() {
    let error = parse_document(&VALID.replace(
        "inputs: [question]\n      params",
        "inputs: [nowhere]\n      params",
    ))
    .unwrap_err();

    let DocumentError::Invalid(source) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        source,
        &ValidationError::DanglingInput {
            node: NodeId::new("lexical"),
            missing: NodeId::new("nowhere"),
        }
    );
    assert_eq!(error.to_string(), "configuration is not a valid pipeline");
}

fn mismatch(port: usize, expected: Option<ValueKind>) -> ValidationError {
    ValidationError::KindMismatch {
        consumer: NodeId::new("ranked"),
        port,
        producer: NodeId::new("legs"),
        expected,
        found: ValueKind::Chunks,
    }
}

#[test]
fn the_wiring_report_names_its_subject_the_edge_the_kind_expected_and_the_kind_found() {
    assert_eq!(
        incompatible_wiring("`pipeline.yaml`", &mismatch(0, Some(ValueKind::Query))).as_deref(),
        Some(
            "`pipeline.yaml` wires two nodes incompatibly\n  \
             edge: `legs` feeds `ranked` at port 0\n  \
             expected: query\n  \
             found: chunks"
        )
    );
}

#[test]
fn a_port_that_does_not_exist_is_reported_as_an_edge_that_should_not_exist() {
    // `expected: None` is a consumer whose variant declares no port at that
    // position, so the fault is the edge rather than the kind on it.
    assert_eq!(
        incompatible_wiring("the configuration", &mismatch(2, None)).as_deref(),
        Some(
            "the configuration wires two nodes incompatibly\n  \
             edge: `legs` feeds `ranked` at port 2\n  \
             expected: nothing — `ranked` declares no port at position 2\n  \
             found: chunks"
        )
    );
}

#[test]
fn every_other_verdict_has_no_wiring_report() {
    assert_eq!(
        incompatible_wiring(
            "the configuration",
            &ValidationError::InputArity { declared: 0 }
        ),
        None
    );
}
