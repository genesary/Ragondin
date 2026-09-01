//! Guards the crate's **public surface**.
//!
//! `rag-pipeline` is a stable API boundary (INV-1): breaking its public API is
//! a deliberate, versioned act. The unit tests live inside the modules and
//! reach their types through `use super::*`, so they stay green even if the
//! re-exports at the crate root are removed. This test compiles only against
//! the surface an external consumer actually sees.

use rag_pipeline::{
    ExtensionNode, FusionNode, LogicalNode, NodeId, ParamValue, Params, RerankerNode, RetrieverNode,
};

#[test]
fn every_node_type_is_reachable_from_the_crate_root() {
    let mut params = Params::new();
    params.insert("k".to_string(), ParamValue::Int(10));

    let nodes = [
        LogicalNode::Retriever(RetrieverNode {
            id: NodeId::new("bm25_leg"),
            implementation: "bm25".to_string(),
            inputs: vec![NodeId::new("question")],
            params: params.clone(),
        }),
        LogicalNode::Fusion(FusionNode {
            id: NodeId::new("rrf"),
            implementation: "reciprocal_rank_fusion".to_string(),
            inputs: vec![NodeId::new("bm25_leg")],
            params: params.clone(),
        }),
        LogicalNode::Reranker(RerankerNode {
            id: NodeId::new("cross_encoder"),
            implementation: "bge_reranker".to_string(),
            inputs: vec![NodeId::new("rrf")],
            params: params.clone(),
        }),
        LogicalNode::Extension(ExtensionNode {
            id: NodeId::new("my_technique"),
            kind: "hyde".to_string(),
            inputs: vec![NodeId::new("question")],
            params,
        }),
    ];

    let ids: Vec<&str> = nodes.iter().map(|node| node.id().as_str()).collect();
    assert_eq!(
        ids,
        vec!["bm25_leg", "rrf", "cross_encoder", "my_technique"]
    );
    assert_eq!(nodes[1].inputs(), &[NodeId::new("bm25_leg")]);
}
