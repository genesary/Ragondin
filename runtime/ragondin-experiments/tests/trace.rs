//! The trace document's one typed definition (ADR-C36 § 2): [`Trace`] read out
//! of a [`TraceDocument`] and written back into one.
//!
//! Three things are pinned here. Every summary variant survives
//! `Trace → TraceDocument → Trace`. A run stored before the typed shape
//! existed — a directory the harness wrote with its hand-built JSON, kept
//! under `tests/fixtures/` — reads back, parses, and renders to the very bytes
//! on disk. And a document the shape does not describe is reported with the
//! node and the field it went wrong at, never repaired or defaulted.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ragondin_experiments::{
    FileSystemRunStore, RunId, Trace, TraceChunk, TraceDocument, TraceError, TraceNode,
    TraceProblem, TraceSummary,
};
use ragondin_pipeline::NodeId;
use ragondin_types::{ChunkId, DocId, QueryId};
use serde_json::json;

fn chunk(chunk: &str, document: &str, score: f64) -> TraceChunk {
    TraceChunk {
        chunk: ChunkId::new(chunk),
        document: DocId::new(document),
        score,
    }
}

fn node(id: &str, inputs: Vec<TraceSummary>, output: Option<TraceSummary>) -> TraceNode {
    TraceNode {
        node: NodeId::new(id),
        inputs,
        output,
        duration_nanos: 1_500_000,
        error: None,
    }
}

/// One node per summary variant, each on the output side and on an input
/// side — the shape describes a rendering, and the rendering places any
/// variant on either side.
fn every_variant() -> Vec<TraceSummary> {
    vec![
        TraceSummary::Query {
            id: QueryId::new("q-1"),
        },
        TraceSummary::Chunks { count: 2 },
        TraceSummary::RankedChunks {
            chunks: vec![chunk("c-3", "doc-b", 0.5), chunk("c-1", "doc-a", 0.25)],
        },
        TraceSummary::ContextSize {
            count: 2,
            text_bytes: 9,
        },
        TraceSummary::Context {
            chunks: vec![chunk("c-3", "doc-b", 0.5)],
            text: "three\none".to_owned(),
        },
        TraceSummary::AnswerSize { text_bytes: 4 },
        TraceSummary::Answer {
            text: "yes.".to_owned(),
        },
    ]
}

#[test]
fn every_summary_variant_round_trips_through_the_document() {
    for summary in every_variant() {
        let trace = Trace {
            nodes: vec![node("n", vec![summary.clone()], Some(summary.clone()))],
        };

        let document = TraceDocument::from(trace.clone());
        let read = Trace::try_from(&document)
            .unwrap_or_else(|error| panic!("{summary:?} must read back: {error}"));

        assert_eq!(read, trace, "{summary:?} round-trips");
    }
}

#[test]
fn a_failed_node_and_an_empty_trace_round_trip() {
    let failed = Trace {
        nodes: vec![TraceNode {
            error: Some("the component refused the request".to_owned()),
            ..node("leg", vec![TraceSummary::Chunks { count: 0 }], None)
        }],
    };
    for trace in [failed, Trace { nodes: Vec::new() }] {
        let read = Trace::try_from(&TraceDocument::from(trace.clone())).expect("reads back");
        assert_eq!(read, trace);
    }
}

#[test]
fn the_document_is_the_rendering_the_harness_has_always_written() {
    // The expected value is the harness's own golden for a ranking node,
    // copied from `ragondin-harness`'s trace tests: the typed shape writes
    // exactly what the hand-built JSON did, key for key.
    let trace = Trace {
        nodes: vec![node(
            "leg",
            vec![
                TraceSummary::Query {
                    id: QueryId::new("q-1"),
                },
                TraceSummary::Chunks { count: 2 },
            ],
            Some(TraceSummary::RankedChunks {
                chunks: vec![
                    chunk("c-3", "doc-b", 0.5),
                    chunk("c-1", "doc-a", 0.25),
                    chunk("c-2", "doc-a", 0.125),
                ],
            }),
        )],
    };

    assert_eq!(
        TraceDocument::from(trace).as_value(),
        &json!({
            "nodes": [{
                "node": "leg",
                "inputs": [{"query": {"id": "q-1"}}, {"chunks": {"count": 2}}],
                "output": {"chunks": {"count": 3, "ranked": [
                    {"chunk": "c-3", "document": "doc-b", "score": 0.5},
                    {"chunk": "c-1", "document": "doc-a", "score": 0.25},
                    {"chunk": "c-2", "document": "doc-a", "score": 0.125},
                ]}},
                "duration_nanos": 1_500_000u64,
                "error": null,
            }]
        })
    );
}

fn stored_before_typed_trace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stored-before-typed-trace")
}

const STORED_RUN: &str = "b41e0752792e728f5dd893043b42d2a2d71f0b0039157a177e0a267e0420ea6f";

#[test]
fn a_run_stored_before_the_typed_shape_loads_parses_and_renders_to_the_same_bytes() {
    // The fixture is a run directory `ragondin-harness` wrote with its
    // hand-built renderer — the stub generation pipeline over four queries,
    // so it holds queries, counted and named chunks, sized and named
    // contexts, and answers. It is read through the store, as any reader
    // would, and never rewritten.
    let root = stored_before_typed_trace();
    let id: RunId = STORED_RUN
        .parse()
        .expect("the fixture is named by a run id");
    let run = FileSystemRunStore::new(&root)
        .load(&id)
        .expect("a run stored before this change still loads");
    assert_eq!(run.traces.len(), 4);

    let mut rerendered = BTreeMap::new();
    for (query, document) in &run.traces {
        let trace = Trace::try_from(document)
            .unwrap_or_else(|error| panic!("the trace of {query:?} parses: {error}"));
        let written = TraceDocument::from(trace);
        assert_eq!(
            &written, document,
            "the trace of {query:?} renders back unchanged"
        );
        rerendered.insert(query.clone(), written);
    }

    // Byte for byte, in the format the store writes: what the typed shape
    // renders is the file already on disk.
    let on_disk = std::fs::read_to_string(root.join(STORED_RUN).join("traces.json"))
        .expect("the fixture's traces are readable");
    let mut written = serde_json::to_string_pretty(&rerendered).expect("renders");
    written.push('\n');
    assert_eq!(written, on_disk);
}

/// What `Trace::try_from` reports for `value`, which must fail.
fn refusal(value: serde_json::Value) -> TraceError {
    match Trace::try_from(&TraceDocument::new(value)) {
        Err(error) => error,
        Ok(trace) => panic!("the document must be refused, read {trace:?}"),
    }
}

fn a_node_with_output(output: serde_json::Value) -> serde_json::Value {
    json!({"nodes": [
        {"node": "retrieve", "inputs": [], "output": null, "duration_nanos": 1, "error": null},
        {"node": "rerank", "inputs": [], "output": output, "duration_nanos": 2, "error": null},
    ]})
}

#[test]
fn an_unknown_summary_shape_is_reported_by_node_and_field() {
    // `chunks` with `ids` in place of `ranked`: neither shape a chunk list is
    // rendered in, and nothing guesses which one was meant.
    let error = refusal(a_node_with_output(
        json!({"chunks": {"count": 1, "ids": ["c-1"]}}),
    ));

    assert_eq!(error.node(), Some(&NodeId::new("rerank")));
    assert_eq!(error.field(), "nodes[1].output.chunks");
    assert_eq!(error.problem(), &TraceProblem::UnknownShape);
    let message = error.to_string();
    assert!(message.contains("rerank"), "{message}");
    assert!(message.contains("nodes[1].output.chunks"), "{message}");
}

#[test]
fn an_unknown_summary_kind_is_reported_by_node_and_field() {
    let error = refusal(a_node_with_output(json!({"table": {"rows": 3}})));

    assert_eq!(error.node(), Some(&NodeId::new("rerank")));
    assert_eq!(error.field(), "nodes[1].output");
    assert_eq!(error.problem(), &TraceProblem::UnknownShape);
}

#[test]
fn a_field_the_shape_does_not_have_is_refused_rather_than_dropped() {
    // Dropping it would make the typed trace render a different document from
    // the one it was read from — a repair by omission.
    let mut value = a_node_with_output(serde_json::Value::Null);
    value["nodes"][0]["branch"] = json!("left");

    let error = refusal(value);

    assert_eq!(error.node(), Some(&NodeId::new("retrieve")));
    assert_eq!(error.field(), "nodes[0].branch");
    assert_eq!(error.problem(), &TraceProblem::Unexpected);
}

#[test]
fn a_missing_field_is_refused_rather_than_defaulted() {
    let mut value = a_node_with_output(serde_json::Value::Null);
    value["nodes"][1]
        .as_object_mut()
        .expect("a node is an object")
        .remove("duration_nanos");

    let error = refusal(value);

    assert_eq!(error.node(), Some(&NodeId::new("rerank")));
    assert_eq!(error.field(), "nodes[1].duration_nanos");
    assert_eq!(error.problem(), &TraceProblem::Missing);
}

#[test]
fn a_value_of_the_wrong_type_is_refused() {
    let error = refusal(a_node_with_output(
        json!({"chunks": {"count": 1, "ranked": [{"chunk": "c-1", "document": "d", "score": null}]}}),
    ));

    assert_eq!(error.field(), "nodes[1].output.chunks.ranked[0].score");
    assert!(
        matches!(error.problem(), TraceProblem::WrongType { .. }),
        "{error}"
    );
}

#[test]
fn a_count_that_disagrees_with_the_chunks_it_counts_is_refused() {
    // The typed shape holds the chunks and derives the count; accepting a
    // disagreeing count would silently rewrite it on the way back out.
    let error = refusal(a_node_with_output(
        json!({"chunks": {"count": 2, "ranked": [{"chunk": "c-1", "document": "d", "score": 1.0}]}}),
    ));

    assert_eq!(error.field(), "nodes[1].output.chunks.count");
    assert_eq!(
        error.problem(),
        &TraceProblem::CountMismatch { count: 2, named: 1 }
    );
}

#[test]
fn a_document_that_is_not_a_trace_at_all_is_reported_at_its_root() {
    let error = refusal(json!({"nodes": [{"node": "sparse", "duration_ms": 3}]}));
    assert_eq!(error.node(), Some(&NodeId::new("sparse")));

    let error = refusal(json!([1, 2, 3]));
    assert_eq!(error.node(), None);
    assert_eq!(error.field(), "");
    assert!(matches!(error.problem(), TraceProblem::WrongType { .. }));
}
