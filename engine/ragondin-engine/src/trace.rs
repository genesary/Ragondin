//! `ExecutionTrace`: what the executor returns beside its output (INV-10).
//!
//! The trace is **structured business data, not telemetry** (ADR-C9): the
//! platform's differentiating capability is per-node replay, which needs the
//! trace addressable per node and therefore returned rather than logged. No
//! `tracing` macro appears in this crate.
//!
//! What a node received and produced is recorded as a **summary** rather than
//! as the value itself. A [`crate::execute::NodeValue`] carries whole chunks,
//! and a trace that cloned every one of them would grow with the corpus; the
//! summary keeps what per-node replay reads — which node, how much came out,
//! how long it took, and what failed.

use std::time::Duration;

use ragondin_pipeline::NodeId;
use ragondin_types::QueryId;

use crate::execute::NodeValue;

/// What one execution of a plan did, node by node.
///
/// Returned by [`crate::Engine::execute`] **even when execution fails**, so a
/// partial run is inspectable: the nodes that ran are here, and the one that
/// failed is the last of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExecutionTrace {
    /// One entry per node that was executed, in the order the executor ran
    /// them — a topological order over the data-flow edges, never the
    /// canonical order the plan stores its nodes in.
    pub nodes: Vec<NodeTrace>,
}

/// What one node of a plan did.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeTrace {
    /// The node this entry is for.
    pub node: NodeId,
    /// A summary of the value on each of the node's input ports, in port
    /// order.
    pub inputs: Vec<ValueSummary>,
    /// A summary of what the node produced, or `None` when the node failed.
    pub output: Option<ValueSummary>,
    /// How long the node's component took, measured around the call alone.
    pub duration: Duration,
    /// The failure this node reported, rendered.
    ///
    /// Rendered rather than typed because [`crate::ExecError`] is not
    /// `Clone` and the same failure is already returned, typed, beside the
    /// trace. A caller that needs to match on it matches on the returned
    /// error; this field is what per-node replay displays.
    pub error: Option<String>,
}

/// What travelled along one edge, reduced to what a trace records.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueSummary {
    /// A query, named by its id.
    Query {
        /// The query's identifier.
        id: QueryId,
    },
    /// A list of chunks, of this length.
    Chunks {
        /// How many chunks the list held.
        count: usize,
    },
}

impl ValueSummary {
    /// Summarizes an edge value.
    pub(crate) fn of(value: &NodeValue) -> Self {
        match value {
            NodeValue::Query(query) => Self::Query {
                id: query.id.clone(),
            },
            NodeValue::Chunks(chunks) => Self::Chunks {
                count: chunks.len(),
            },
        }
    }
}
