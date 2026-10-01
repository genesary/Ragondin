//! The walk: which node's ranking a pipeline's retrieval metrics are read
//! from (ADR-C30 § 3), as [`terminal`] and [`ranking_node`].
//!
//! The one definition of that rule, beside the one trace shape ([`Trace`]) and
//! the one configuration lowering ([`lower_configuration`]) this crate already
//! hosts for the writer and the reader alike (ADR-C36 § 2). The harness walks
//! to the ranking it scores when it writes a run's metrics; `ragondin-api`
//! walks to the same node when it recomputes per-query and per-node figures
//! from the stored traces. Both call these functions, so the node a figure is
//! read back at is the node the run was scored at.
//!
//! Only the pipeline's shape is read, through `ragondin-pipeline`'s public
//! surface: whether the node found holds a ranking for a given query is a
//! question about a trace, and each caller asks it of its own trace type.
//!
//! [`Trace`]: crate::Trace
//! [`lower_configuration`]: crate::lower_configuration

use ragondin_pipeline::{LogicalNode, LogicalPipeline, NodeId};

/// Where the walk from a pipeline's terminal node to the ranking behind it
/// stopped.
///
/// The walk goes by port position, never by node name: a generator's context
/// port (`inputs[1]`) names its context builder, and a builder's chunks port
/// (`inputs[1]`) names the node whose output is the ranking.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WalkError {
    /// The pipeline has no single terminal node — no node, or several, that no
    /// other node consumes. The executor refuses such a plan before a query
    /// runs, so after a query has run this is not expected.
    #[error("the pipeline has no single terminal node")]
    NoTerminalNode,

    /// A generator's or a context builder's port the walk reads is absent.
    #[error("node `{}` has no input on port {port}", node.as_str())]
    MissingPort {
        /// The node whose port is absent.
        node: NodeId,
        /// The port position the walk reads.
        port: usize,
    },

    /// The generator's context port names something that is not a context
    /// builder.
    #[error(
        "generator `{}` takes its context from `{}`, which is not a context builder",
        generator.as_str(),
        context.as_str()
    )]
    ContextNotFromBuilder {
        /// The terminal generator.
        generator: NodeId,
        /// What its context port names instead.
        context: NodeId,
    },
}

/// The one node of `pipeline` that no other node consumes, if there is
/// exactly one.
///
/// The executor refuses a plan with zero or several (ADR-C30 § 3 rests on
/// that), so for a pipeline a query has run through this is always `Some`.
pub fn terminal(pipeline: &LogicalPipeline) -> Option<&LogicalNode> {
    let mut terminals = pipeline.nodes().iter().filter(|node| {
        !pipeline
            .nodes()
            .iter()
            .any(|other| other.inputs().contains(node.id()))
    });
    match (terminals.next(), terminals.next()) {
        (Some(terminal), None) => Some(terminal),
        _ => None,
    }
}

/// The node whose output holds the ranking the retrieval metrics read
/// (ADR-C30 § 3), found by port position and never by name.
///
/// - A terminal **generator**: its context port names a context builder, and
///   that builder's chunks port names the ranking.
/// - A terminal **context builder**: the walk enters at its chunks port.
/// - Any other terminal node produces chunks and is its own ranking.
///
/// The node returned may be a pipeline input rather than a node, when a
/// builder's chunks port names one; the caller finds no ranking for it in a
/// trace and says so.
///
/// # Errors
///
/// [`WalkError::NoTerminalNode`] without a single terminal node,
/// [`WalkError::MissingPort`] when a port the walk reads is absent, and
/// [`WalkError::ContextNotFromBuilder`] when a generator's context port names
/// anything but a context builder.
pub fn ranking_node(pipeline: &LogicalPipeline) -> Result<&NodeId, WalkError> {
    let terminal = terminal(pipeline).ok_or(WalkError::NoTerminalNode)?;
    let builder = match terminal {
        LogicalNode::Generator(generator) => {
            let context = port(terminal, CONTEXT_PORT)?;
            match pipeline.nodes().iter().find(|node| node.id() == context) {
                Some(builder @ LogicalNode::ContextBuilder(_)) => builder,
                _ => {
                    return Err(WalkError::ContextNotFromBuilder {
                        generator: generator.id.clone(),
                        context: context.clone(),
                    })
                }
            }
        }
        LogicalNode::ContextBuilder(_) => terminal,
        _ => return Ok(terminal.id()),
    };
    port(builder, CHUNKS_PORT)
}

/// A generator's context port: `Fixed([Query, Context])`.
const CONTEXT_PORT: usize = 1;
/// A context builder's chunks port: `Fixed([Query, Chunks])`.
const CHUNKS_PORT: usize = 1;

/// What `node`'s input port `position` names.
fn port(node: &LogicalNode, position: usize) -> Result<&NodeId, WalkError> {
    node.inputs()
        .get(position)
        .ok_or_else(|| WalkError::MissingPort {
            node: node.id().clone(),
            port: position,
        })
}

#[cfg(test)]
mod tests {
    use ragondin_pipeline::{LogicalPipeline, NodeId};

    use super::{ranking_node, terminal, WalkError};

    /// A `LogicalPipeline` read straight from its in-memory shape: `validate`
    /// refuses the malformed wirings the walk must still name, and the walk
    /// reads nothing but the shape.
    fn forged(json: &str) -> LogicalPipeline {
        serde_json::from_str(json).expect("the forged JSON must match LogicalPipeline's shape")
    }

    const GENERATION: &str = r#"{"inputs":["question"],"nodes":[
        {"Generator":{"id":"answer","implementation":"g","inputs":["question","context"],"params":{}}},
        {"ContextBuilder":{"id":"context","implementation":"c","inputs":["question","leg"],"params":{}}},
        {"Retriever":{"id":"leg","implementation":"r","inputs":["question"],"params":{}}}
    ]}"#;

    const TWO_TERMINALS: &str = r#"{"inputs":["question"],"nodes":[
        {"Retriever":{"id":"a","implementation":"r","inputs":["question"],"params":{}}},
        {"Retriever":{"id":"b","implementation":"r","inputs":["question"],"params":{}}}
    ]}"#;

    #[test]
    fn the_terminal_node_is_the_one_no_other_node_consumes() {
        let pipeline = forged(GENERATION);

        assert_eq!(
            terminal(&pipeline).map(|node| node.id()),
            Some(&NodeId::new("answer"))
        );
    }

    #[test]
    fn a_pipeline_with_several_terminal_nodes_has_none() {
        assert!(terminal(&forged(TWO_TERMINALS)).is_none());
    }

    #[test]
    fn a_chunk_producing_terminal_node_is_its_own_ranking() {
        let pipeline = forged(
            r#"{"inputs":["question"],"nodes":[
                {"Fusion":{"id":"fused","implementation":"f","inputs":["a","b"],"params":{}}},
                {"Retriever":{"id":"a","implementation":"r","inputs":["question"],"params":{}}},
                {"Retriever":{"id":"b","implementation":"r","inputs":["question"],"params":{}}}
            ]}"#,
        );

        assert_eq!(ranking_node(&pipeline), Ok(&NodeId::new("fused")));
    }

    #[test]
    fn a_generator_is_scored_on_the_ranking_that_fed_its_context_builder() {
        assert_eq!(ranking_node(&forged(GENERATION)), Ok(&NodeId::new("leg")));
    }

    #[test]
    fn a_terminal_context_builder_is_scored_on_its_own_chunks_port() {
        let pipeline = forged(
            r#"{"inputs":["question"],"nodes":[
                {"ContextBuilder":{"id":"context","implementation":"c","inputs":["question","leg"],"params":{}}},
                {"Retriever":{"id":"leg","implementation":"r","inputs":["question"],"params":{}}}
            ]}"#,
        );

        assert_eq!(ranking_node(&pipeline), Ok(&NodeId::new("leg")));
    }

    #[test]
    fn a_generator_fed_by_something_other_than_a_context_builder_is_named() {
        let pipeline = forged(
            r#"{"inputs":["question"],"nodes":[
                {"Generator":{"id":"answer","implementation":"g","inputs":["question","leg"],"params":{}}},
                {"Retriever":{"id":"leg","implementation":"r","inputs":["question"],"params":{}}}
            ]}"#,
        );

        assert_eq!(
            ranking_node(&pipeline),
            Err(WalkError::ContextNotFromBuilder {
                generator: NodeId::new("answer"),
                context: NodeId::new("leg"),
            })
        );
    }

    #[test]
    fn a_missing_port_and_a_missing_terminal_are_named() {
        let short = forged(
            r#"{"inputs":["question"],"nodes":[
                {"ContextBuilder":{"id":"context","implementation":"c","inputs":["question"],"params":{}}}
            ]}"#,
        );
        assert_eq!(
            ranking_node(&short),
            Err(WalkError::MissingPort {
                node: NodeId::new("context"),
                port: 1,
            })
        );

        let generator = forged(
            r#"{"inputs":["question"],"nodes":[
                {"Generator":{"id":"answer","implementation":"g","inputs":["question"],"params":{}}}
            ]}"#,
        );
        assert_eq!(
            ranking_node(&generator),
            Err(WalkError::MissingPort {
                node: NodeId::new("answer"),
                port: 1,
            })
        );

        assert_eq!(
            ranking_node(&forged(TWO_TERMINALS)),
            Err(WalkError::NoTerminalNode)
        );
    }
}
