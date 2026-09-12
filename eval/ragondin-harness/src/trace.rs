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

use ragondin_engine::{ExecutionTrace, ValueSummary};
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

/// Renders one edge value's summary.
fn summary(value: &ValueSummary) -> Value {
    match value {
        ValueSummary::Query { id } => json!({"query": {"id": id.as_str()}}),
        ValueSummary::Chunks { count } => json!({"chunks": {"count": count}}),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ragondin_engine::NodeTrace;
    use ragondin_pipeline::NodeId;
    use ragondin_types::QueryId;

    use super::*;

    fn node(error: Option<&str>) -> NodeTrace {
        NodeTrace {
            node: NodeId::new("leg"),
            inputs: vec![ValueSummary::Query {
                id: QueryId::new("q-1"),
            }],
            output: error.is_none().then_some(ValueSummary::Chunks { count: 3 }),
            duration: Duration::from_micros(1500),
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn a_node_renders_what_it_received_what_it_produced_and_how_long_it_took() {
        let document = render(&ExecutionTrace {
            nodes: vec![node(None)],
        });

        assert_eq!(
            document.as_value(),
            &json!({
                "nodes": [{
                    "node": "leg",
                    "inputs": [{"query": {"id": "q-1"}}],
                    "output": {"chunks": {"count": 3}},
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
    fn an_empty_trace_renders_an_empty_node_list() {
        // A plan that failed before any node ran still returns a trace, and a
        // run record that dropped it would be missing the only evidence there
        // is that nothing ran.
        let document = render(&ExecutionTrace::default());

        assert_eq!(document.as_value(), &json!({"nodes": []}));
    }
}
