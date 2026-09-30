//! Converting an [`ExecutionTrace`] into the [`TraceDocument`] a run stores.
//!
//! The trace is the executor's **return value** (INV-10), and this is the one
//! place a run record gets one: nothing here reads a log, and the harness emits
//! no per-node telemetry of its own.
//!
//! # Why the conversion is written by hand
//!
//! `ExecutionTrace` belongs to `ragondin-engine`, which is internal and not an
//! API boundary (INV-2) — its shape is meant to move. Deriving `Serialize` on
//! it would make every stored run a hostage of that shape, and would put the
//! engine's internals in a file format. So the harness, which is the crate that
//! knows both the engine and the run store, translates between them, and this
//! module is where that translation is legible and changeable.
//!
//! It translates into [`Trace`], the stored document's one definition, which
//! lives in `ragondin-experiments` beside [`TraceDocument`] (ADR-C36 § 2); the
//! document is that trace's rendering, and a reader parses it back through the
//! same type. What the JSON looks like is decided there, not here. No engine
//! type crosses into that crate: this module maps each one field by field.

use ragondin_engine::{ExecutionTrace, RankedChunk, ValueSummary};
use ragondin_experiments::{Trace, TraceChunk, TraceDocument, TraceNode, TraceSummary};

/// Renders what the executor returned for one query.
pub(crate) fn render(trace: &ExecutionTrace) -> TraceDocument {
    TraceDocument::from(Trace {
        nodes: trace
            .nodes
            .iter()
            .map(|node| TraceNode {
                node: node.node.clone(),
                inputs: node.inputs.iter().map(summary).collect(),
                output: node.output.as_ref().map(summary),
                // Nanoseconds, as an integer: a duration rendered as a float
                // would round, and this field is read by a replay view that
                // shows where a run spent its time.
                duration_nanos: node.duration.as_nanos() as u64,
                error: node.error.clone(),
            })
            .collect(),
    })
}

/// One named chunk — of a ranking or of a context, converted alike.
///
/// The score is widened from the engine's `f32` to `f64` — lossless, and the
/// reason a stored score shows more digits than the component returned. A
/// non-finite score would render as `null`: the ranking contract makes one
/// unreachable from a conforming component, and the document is opaque to the
/// store, so nothing here refuses it; a reader parsing it back reports it.
fn ranked_chunk(hit: &RankedChunk) -> TraceChunk {
    TraceChunk {
        chunk: hit.chunk.clone(),
        document: hit.document.clone(),
        score: f64::from(hit.score),
    }
}

/// Converts one edge value's summary, variant for variant.
///
/// An output's chunks are **named**, in the order the node returned them, and
/// an input's are counted (ADR-C28); a context and an answer are named where
/// they were produced and sized where they were consumed (ADR-C31 § 5). The
/// engine records each side as its own variant, and so does [`TraceSummary`],
/// so the mapping is one to one and adds nothing.
fn summary(value: &ValueSummary) -> TraceSummary {
    match value {
        ValueSummary::Query { id } => TraceSummary::Query { id: id.clone() },
        ValueSummary::Chunks { count } => TraceSummary::Chunks {
            count: *count as u64,
        },
        ValueSummary::RankedChunks { chunks } => TraceSummary::RankedChunks {
            chunks: chunks.iter().map(ranked_chunk).collect(),
        },
        ValueSummary::ContextSize { count, text_bytes } => TraceSummary::ContextSize {
            count: *count as u64,
            text_bytes: *text_bytes as u64,
        },
        ValueSummary::Context { chunks, text } => TraceSummary::Context {
            chunks: chunks.iter().map(ranked_chunk).collect(),
            text: text.clone(),
        },
        ValueSummary::AnswerSize { text_bytes } => TraceSummary::AnswerSize {
            text_bytes: *text_bytes as u64,
        },
        ValueSummary::Answer { text } => TraceSummary::Answer { text: text.clone() },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ragondin_engine::NodeTrace;
    use ragondin_pipeline::NodeId;
    use ragondin_types::{ChunkId, DocId, QueryId};
    use serde_json::{json, Value};

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

    #[test]
    fn a_score_renders_as_the_widened_f32_it_always_did() {
        // 0.1 has no exact `f32`; its nearest one, widened to `f64`, prints as
        // 0.10000000149011612. The hand-built renderer wrote `json!(score)` of
        // the `f32` itself, so every run stored so far holds that spelling, and
        // the typed shape must write it byte for byte.
        let document = render(&ExecutionTrace {
            nodes: vec![NodeTrace {
                output: Some(ValueSummary::RankedChunks {
                    chunks: vec![ranked("c-1", "doc-a", 0.1)],
                }),
                ..node(None)
            }],
        });

        let score = &document.as_value()["nodes"][0]["output"]["chunks"]["ranked"][0]["score"];
        assert_eq!(score, &json!(0.1f32));
        assert_eq!(
            serde_json::to_string(score).expect("renders"),
            "0.10000000149011612"
        );
    }

    #[test]
    fn a_rendered_trace_parses_back_into_the_shape_it_was_rendered_through() {
        // The writer and the reader compile against one definition (ADR-C36
        // § 2): what this module stores, `Trace::try_from` reads.
        let document = render(&ExecutionTrace {
            nodes: vec![node(None), node(Some("refused"))],
        });

        let trace = Trace::try_from(&document).expect("a rendered trace parses");

        assert_eq!(trace.nodes.len(), 2);
        assert_eq!(trace.nodes[1].error.as_deref(), Some("refused"));
        assert_eq!(TraceDocument::from(trace), document);
    }
}
