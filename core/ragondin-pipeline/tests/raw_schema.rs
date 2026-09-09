//! Golden test: the reference configuration from the documentation must parse.
//!
//! `docs/system-architecture.md` §5.1 is the only place the project shows a
//! pipeline as a user writes it. If that example stops deserializing, the
//! documented format and the implemented one have diverged — which is the
//! failure INV-9 exists to prevent.

use ragondin_pipeline::{validate, LogicalNode, NodeId, RawParamValue, RawPipeline, SchemaVersion};

const REFERENCE: &str = include_str!("fixtures/hybrid-retrieval.yaml");

fn reference() -> RawPipeline {
    serde_yaml::from_str(REFERENCE).expect("the documented reference pipeline must deserialize")
}

#[test]
fn the_reference_pipeline_from_the_documentation_deserializes() {
    let doc = reference();

    assert_eq!(
        doc.version,
        SchemaVersion::CURRENT,
        "§5.1 writes no `version:`, which reads as the version this build writes"
    );
    assert_eq!(
        doc.pipeline.inputs,
        vec!["question".to_string()],
        "§5.1 declares the one input a serving graph takes (ADR-C18)"
    );

    let ids: Vec<&str> = doc.pipeline.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["transform", "dense", "sparse", "fuse"]);

    let families: Vec<&str> = doc
        .pipeline
        .nodes
        .iter()
        .map(|n| n.component.as_str())
        .collect();
    assert_eq!(
        families,
        vec!["query_transform", "retriever", "retriever", "fusion"]
    );

    let impls: Vec<&str> = doc
        .pipeline
        .nodes
        .iter()
        .map(|n| n.implementation.as_str())
        .collect();
    assert_eq!(impls, vec!["hyde", "qdrant_dense", "bm25", "rrf"]);

    let transform = &doc.pipeline.nodes[0];
    assert_eq!(
        transform.inputs,
        vec!["question".to_string()],
        "`transform` consumes the pipeline's declared input"
    );
    assert!(
        transform.params.is_empty(),
        "`transform` declares no params"
    );

    let dense = &doc.pipeline.nodes[1];
    assert_eq!(dense.inputs, vec!["transform".to_string()]);
    assert_eq!(dense.params["top_k"], RawParamValue::Int(50));

    let fuse = &doc.pipeline.nodes[3];
    assert_eq!(
        fuse.inputs,
        vec!["dense".to_string(), "sparse".to_string()],
        "edge order is significant"
    );
}

#[test]
fn the_reference_pipeline_round_trips_through_yaml() {
    let doc = reference();
    let back: RawPipeline = serde_yaml::from_str(&serde_yaml::to_string(&doc).unwrap()).unwrap();
    assert_eq!(back, doc);
}

#[test]
fn a_dangling_input_reference_parses_here_and_is_rejected_later() {
    // Referential integrity belongs to validation (#9). This level must not
    // pre-empt it, or a user's malformed file stops producing a diagnosis and
    // starts producing a parse error.
    let doc: RawPipeline = serde_yaml::from_str(
        "pipeline:\n  nodes:\n    - id: fuse\n      component: fusion\n      impl: rrf\n      inputs: [nobody]\n",
    )
    .expect("an unresolved reference is not a parse failure");
    assert_eq!(doc.pipeline.nodes[0].inputs, vec!["nobody".to_string()]);
}

#[test]
fn the_reference_pipeline_validates_once_its_transform_is_written_as_an_extension() {
    // The fixture is parsed by the tests above; this is the one that runs it
    // through `validate`, so §5.1 is checked against the validator rather than
    // only against the parser.
    //
    // One substitution is required and it is the document's, not the
    // fixture's: §5.1 writes `component: query_transform`, and `lower_node`
    // knows four family names — `retriever`, `fusion`, `reranker`,
    // `extension` — so `query_transform` is an `UnknownComponent`. A query
    // transform is written as an `Extension` today. Applying that here
    // programmatically, rather than transcribing an edited copy of §5.1 into
    // a test, is what keeps the document, the fixture and the assertion from
    // drifting into three different pipelines.
    let source = REFERENCE.replace("component: query_transform", "component: extension");
    let raw: RawPipeline = serde_yaml::from_str(&source).expect("the fixture must deserialize");

    let logical = validate(raw).expect("§5.1's checked-in pipeline must validate");

    assert_eq!(
        logical.inputs(),
        &[NodeId::new("question")],
        "§5.1 declares the one input a serving graph takes (ADR-C18)"
    );

    let transform = logical
        .nodes()
        .iter()
        .find(|n| n.id().as_str() == "transform")
        .expect("transform must be present");
    assert!(
        matches!(transform, LogicalNode::Extension(_)),
        "the query transform is the escape hatch between the declared input and the legs"
    );
    assert_eq!(
        transform.inputs(),
        &[NodeId::new("question")],
        "the transform consumes the declared input, which is what ADR-C18 made possible"
    );
}
