//! `ExecutionTrace`: what the executor returns beside its output (INV-10).
//!
//! The trace is **structured business data, not telemetry** (ADR-C9): the
//! platform's differentiating capability is per-node replay, which needs the
//! trace addressable per node and therefore returned rather than logged. No
//! `tracing` macro appears in this crate.
//!
//! What a node received and produced is recorded as a **summary** rather than
//! as the value itself. A [`crate::execute::NodeValue`] carries whole chunks,
//! text and all, and a trace that cloned every one of them would grow with the
//! corpus; the summary keeps what per-node replay reads — which node, what came
//! out and in what order, how long it took, and what failed.
//!
//! The two sides of a node are summarized differently, and ADR-C28 is why. A
//! node's **output** names the chunks it produced — chunk id, document id,
//! score — in the order it returned them, because that ranking is the record
//! the ADR-10 regression fixture, a graded-relevance calibration and per-node
//! replay all read. A node's **input** stays a count: an input is the output of
//! the node that produced it, already named under that node, so naming it twice
//! would double the trace for no information.

use std::time::Duration;

use ragondin_pipeline::NodeId;
use ragondin_types::{ChunkId, DocId, QueryId};

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
    /// order. A list of chunks is summarized here by its count alone
    /// (ADR-C28).
    pub inputs: Vec<ValueSummary>,
    /// What the node produced, or `None` when the node failed. A list of
    /// chunks is **named** here, in rank order (ADR-C28).
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

/// One chunk of a node's output, as the trace names it.
///
/// The three fields a [`ScoredChunk`](ragondin_types::ScoredChunk) already
/// holds that a ranking is read through — nothing is added to
/// `ragondin-contracts` or `ragondin-types` for this (ADR-C28). The chunk's
/// *text* is deliberately absent: it is the one field that grows with the
/// corpus and the one no reader of a ranking needs.
#[derive(Clone, Debug, PartialEq)]
pub struct RankedChunk {
    /// The chunk, by id.
    pub chunk: ChunkId,
    /// The document the chunk was derived from.
    pub document: DocId,
    /// The score the node gave it, on the node's own scale.
    pub score: f32,
}

/// What travelled along one edge, reduced to what a trace records.
///
/// Chunks have two entries rather than one because the two sides of a node
/// record different things (ADR-C28), and a single variant carrying both a
/// count and an optional list would hold the same fact twice — a count that
/// can disagree with the list beside it. So [`Self::Chunks`] is what an input
/// port records and [`Self::RankedChunks`] what a node produced, and the count
/// of an output is the length of its list.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueSummary {
    /// A query, named by its id.
    Query {
        /// The query's identifier.
        id: QueryId,
    },
    /// A list of chunks, counted and not named: what an **input** port
    /// records.
    Chunks {
        /// How many chunks the list held.
        count: usize,
    },
    /// The chunks a node **produced**, in the order it returned them.
    RankedChunks {
        /// The chunks, in rank order — the node's own order, never sorted
        /// here.
        chunks: Vec<RankedChunk>,
    },
}

impl ValueSummary {
    /// Summarizes a value arriving on one of a node's input ports.
    pub(crate) fn of_input(value: &NodeValue) -> Self {
        match value {
            NodeValue::Query(query) => Self::Query {
                id: query.id.clone(),
            },
            NodeValue::Chunks(chunks) => Self::Chunks {
                count: chunks.len(),
            },
        }
    }

    /// Names a value a node produced.
    pub(crate) fn of_output(value: &NodeValue) -> Self {
        match value {
            NodeValue::Query(query) => Self::Query {
                id: query.id.clone(),
            },
            NodeValue::Chunks(chunks) => Self::RankedChunks {
                chunks: chunks
                    .iter()
                    .map(|hit| RankedChunk {
                        chunk: hit.chunk.id.clone(),
                        document: hit.chunk.document_id.clone(),
                        score: hit.score,
                    })
                    .collect(),
            },
        }
    }
}
