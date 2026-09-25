//! Rendering an [`ExecutionTrace`] into the [`TraceDocument`] a run stores.
//!
//! The trace is the executor's **return value** (INV-10), and this is the one
//! place a run record gets one: nothing here reads a log, and the harness emits
//! no per-node telemetry of its own.
//!
//! # Why the rendering is written by hand
//!
//! `ExecutionTrace` belongs to `ragondin-engine`, which is internal and not an
//! API boundary (INV-2) — its shape is meant to move. Deriving `Serialize` on
//! it would make every stored run a hostage of that shape, and would put the
//! engine's internals in a file format. So the harness, which is the crate that
//! knows both the engine and the run store, translates between them, and this
//! module is where that translation is legible and changeable.

use ragondin_engine::{ExecutionTrace, RankedChunk, ValueSummary};
use ragondin_experiments::TraceDocument;
use serde_json::{json, Value};

/// Renders what the executor returned for one query.
pub(crate) fn render(trace: &ExecutionTrace) -> TraceDocument {
    let nodes: Vec<Value> = trace
        .nodes
        .iter()
        .map(|node| {
            json!({
                "node": node.node.as_str(),
                "inputs": node.inputs.iter().map(summary).collect::<Vec<_>>(),
                "output": node.output.as_ref().map(summary),
                // Nanoseconds, as an integer: a duration rendered as a float
                // would round, and this field is read by a replay view that
                // shows where a run spent its time.
                "duration_nanos": node.duration.as_nanos() as u64,
                "error": node.error,
            })
        })
        .collect();

    TraceDocument::new(json!({ "nodes": nodes }))
}

/// One named chunk — of a ranking or of a context, rendered alike.
fn ranked_chunk(hit: &RankedChunk) -> Value {
    json!({
        "chunk": hit.chunk.as_str(),
        "document": hit.document.as_str(),
        "score": hit.score,
    })
}

/// Renders one edge value's summary.
///
/// An output's chunks are **named**, in the order the node returned them, and
/// an input's are counted (ADR-C28). `count` is rendered on both, so a reader
/// of the field does not have to know which side it is looking at; on an
/// output it is the length of `ranked`, which is where the ranking a per-query
/// fixture, a graded-relevance calibration or a replay view reads lives.
///
/// A produced context renders as `{"context": {"chunks": [...], "text": ...}}`
/// and a produced answer as `{"answer": {"text": ...}}` — the shapes ADR-C31
/// § 5 pins, each chunk rendered as a ranked one is. Consumed, a context
/// renders as `{"context": {"count": ..., "text_bytes": ...}}` and an answer
/// as `{"answer": {"text_bytes": ...}}`: sizes only, since the value is named
/// under the node that produced it.
///
/// A score is rendered as the JSON number of its `f32`, widened to `f64` on
/// the way — lossless, and the reason a stored score shows more digits than
/// the component returned. A non-finite score would render as `null`: the
/// ranking contract makes one unreachable from a conforming component, and
/// this document is opaque to the store, so nothing here refuses it.
fn summary(value: &ValueSummary) -> Value {
    match value {
        ValueSummary::Query { id } => json!({"query": {"id": id.as_str()}}),
        ValueSummary::Chunks { count } => json!({"chunks": {"count": count}}),
        ValueSummary::RankedChunks { chunks } => json!({"chunks": {
            "count": chunks.len(),
            "ranked": chunks.iter().map(ranked_chunk).collect::<Vec<_>>(),
        }}),
        ValueSummary::ContextSize { count, text_bytes } => {
            json!({"context": {"count": count, "text_bytes": text_bytes}})
        }
        ValueSummary::Context { chunks, text } => json!({"context": {
            "chunks": chunks.iter().map(ranked_chunk).collect::<Vec<_>>(),
            "text": text,
        }}),
        ValueSummary::AnswerSize { text_bytes } => json!({"answer": {"text_bytes": text_bytes}}),
        ValueSummary::Answer { text } => json!({"answer": {"text": text}}),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ragondin_engine::NodeTrace;
    use ragondin_pipeline::NodeId;
    use ragondin_types::{ChunkId, DocId, QueryId};

    use super::*;

    fn ranked(chunk: &str, document: &str, score: f32) -> RankedChunk {
        RankedChunk {
            chunk: ChunkId::new(chunk),
            document: DocId::new(document),
            score,
        }
    }

    fn node(error: Option<&str>) -> NodeTrace {
        NodeTrace {
            node: NodeId::new("leg"),
            inputs: vec![
                ValueSummary::Query {
                    id: QueryId::new("q-1"),
                },
                ValueSummary::Chunks { count: 2 },
            ],
            output: error.is_none().then(|| ValueSummary::RankedChunks {
                chunks: vec![
                    ranked("c-3", "doc-b", 0.5),
                    ranked("c-1", "doc-a", 0.25),
                    ranked("c-2", "doc-a", 0.125),
                ],
            }),
            duration: Duration::from_micros(1500),
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn a_node_renders_what_it_received_what_it_produced_and_how_long_it_took() {
        let document = render(&ExecutionTrace {
            nodes: vec![node(None)],
        });

        // ADR-C28: the output names its chunks in the node's own order, each
        // with its document and score; an input carries a count and no ids.
        assert_eq!(
            document.as_value(),
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

    #[test]
    fn a_failed_node_renders_its_failure_and_no_output() {
        let document = render(&ExecutionTrace {
            nodes: vec![node(Some("the component refused the request"))],
        });

        let rendered = &document.as_value()["nodes"][0];
        assert_eq!(rendered["output"], Value::Null);
        assert_eq!(rendered["error"], "the component refused the request");
    }

    #[test]
    fn a_context_and_an_answer_render_named_as_outputs_and_sized_as_inputs() {
        // ADR-C31 § 5: a produced context names its chunks and its text, a
        // produced answer its text; consumed, each is rendered by its sizes.
        let document = render(&ExecutionTrace {
            nodes: vec![
                NodeTrace {
                    node: NodeId::new("ctx"),
                    inputs: vec![ValueSummary::Chunks { count: 2 }],
                    output: Some(ValueSummary::Context {
                        chunks: vec![ranked("c-3", "doc-b", 0.5), ranked("c-1", "doc-a", 0.25)],
                        text: "three\none".to_string(),
                    }),
                    duration: Duration::from_nanos(10),
                    error: None,
                },
                NodeTrace {
                    node: NodeId::new("gen"),
                    inputs: vec![
                        ValueSummary::ContextSize {
                            count: 2,
                            text_bytes: 9,
                        },
                        ValueSummary::AnswerSize { text_bytes: 4 },
                    ],
                    output: Some(ValueSummary::Answer {
                        text: "yes.".to_string(),
                    }),
                    duration: Duration::from_nanos(20),
                    error: None,
                },
            ],
        });

        let nodes = &document.as_value()["nodes"];
        assert_eq!(
            nodes[0]["output"],
            json!({"context": {
                "chunks": [
                    {"chunk": "c-3", "document": "doc-b", "score": 0.5},
                    {"chunk": "c-1", "document": "doc-a", "score": 0.25},
                ],
                "text": "three\none",
            }})
        );
        assert_eq!(nodes[1]["output"], json!({"answer": {"text": "yes."}}));
        assert_eq!(
            nodes[1]["inputs"],
            json!([
                {"context": {"count": 2, "text_bytes": 9}},
                {"answer": {"text_bytes": 4}},
            ])
        );
    }

    #[test]
    fn an_empty_trace_renders_an_empty_node_list() {
        // A plan that failed before any node ran still returns a trace, and a
        // run record that dropped it would be missing the only evidence there
        // is that nothing ran.
        let document = render(&ExecutionTrace::default());

        assert_eq!(document.as_value(), &json!({"nodes": []}));
    }
}
