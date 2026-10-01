//! The stages of a pipeline, by which `POST /compare` aligns runs whose
//! pipelines are built differently (the design document § 3): retrieval
//! legs → after fusion → after rerank → final ranking → answer. Never by node
//! id: two pipelines' nodes need not share a name to be compared.
//!
//! A stage is derived from a node's kind and its position in the lowered
//! graph, and from nothing else:
//!
//! - **Retrieval legs**: every retriever node.
//! - **After fusion**: the fusion node.
//! - **After rerank**: the reranker node.
//! - **Final ranking**: the node whose ranking the retrieval metrics read —
//!   ADR-C30 § 3's walk, `ragondin_experiments::ranking_node`, through
//!   `derived::Outputs`, so the final ranking compared is the one the run was
//!   scored at. Every pipeline has one.
//! - **Answer**: the terminal node, when it produces an answer.
//!
//! Where the graph is ambiguous — two fusions, two rerankers, or a reranker
//! upstream of the fusion — the node furthest from the inputs is taken
//! (ties by id), and the derivation says it guessed, so that a reader is
//! offered the manual pairing.

use std::collections::BTreeMap;

use ragondin_pipeline::{LogicalNode, LogicalPipeline, NodeId};

use crate::derived::Outputs;

/// A stage of a pipeline, in the order a ranking travels through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Stage {
    RetrievalLegs,
    AfterFusion,
    AfterRerank,
    FinalRanking,
    Answer,
}

impl Stage {
    /// Every stage, in order.
    pub(crate) const ALL: [Stage; 5] = [
        Stage::RetrievalLegs,
        Stage::AfterFusion,
        Stage::AfterRerank,
        Stage::FinalRanking,
        Stage::Answer,
    ];

    /// Whether a manual pair may move a node into or out of this stage: the
    /// stages a node's kind decides. The final ranking and the answer are the
    /// walk's, and a pair never moves them.
    pub(crate) fn pairable(self) -> bool {
        matches!(
            self,
            Stage::RetrievalLegs | Stage::AfterFusion | Stage::AfterRerank
        )
    }
}

/// The stages of one pipeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stages {
    /// The nodes at each stage the pipeline has, in the canonical order (by
    /// id); a stage it lacks is absent.
    pub(crate) nodes: BTreeMap<Stage, Vec<NodeId>>,
    /// Whether a stage was picked among several candidates: two fusions, two
    /// rerankers, or a reranker upstream of the fusion.
    pub(crate) guessed: bool,
}

impl Stages {
    /// The stages of `pipeline`.
    pub(crate) fn of(pipeline: &LogicalPipeline) -> Self {
        let depth = depths(pipeline);
        let of_kind = |kind: fn(&LogicalNode) -> bool| -> Vec<&NodeId> {
            pipeline
                .nodes()
                .iter()
                .filter(|node| kind(node))
                .map(LogicalNode::id)
                .collect()
        };
        let deepest = |candidates: &[&NodeId]| -> Option<NodeId> {
            candidates
                .iter()
                .max_by_key(|id| {
                    (
                        depth.get(**id).copied().unwrap_or(0),
                        std::cmp::Reverse(**id),
                    )
                })
                .map(|id| (*id).clone())
        };
        let legs = of_kind(|node| matches!(node, LogicalNode::Retriever(_)));
        let fusions = of_kind(|node| matches!(node, LogicalNode::Fusion(_)));
        let rerankers = of_kind(|node| matches!(node, LogicalNode::Reranker(_)));
        let fusion = deepest(&fusions);
        let rerank = deepest(&rerankers);
        let reranker_feeds_fusion = match &fusion {
            Some(fusion) => rerankers
                .iter()
                .any(|reranker| upstream(pipeline, fusion, reranker)),
            None => false,
        };
        let outputs = Outputs::of(pipeline);

        let mut nodes = BTreeMap::new();
        if !legs.is_empty() {
            nodes.insert(
                Stage::RetrievalLegs,
                legs.into_iter().cloned().collect::<Vec<_>>(),
            );
        }
        for (stage, node) in [
            (Stage::AfterFusion, fusion),
            (Stage::AfterRerank, rerank),
            (Stage::FinalRanking, outputs.ranking),
            (Stage::Answer, outputs.answer),
        ] {
            if let Some(node) = node {
                nodes.insert(stage, vec![node]);
            }
        }
        Self {
            nodes,
            guessed: fusions.len() > 1 || rerankers.len() > 1 || reranker_feeds_fusion,
        }
    }

    /// The first stage a ranking-stage node is at, among those a pair may
    /// move — `None` for a node at none of them.
    pub(crate) fn pairable_stage_of(&self, node: &NodeId) -> Option<Stage> {
        self.nodes
            .iter()
            .find(|(stage, nodes)| stage.pairable() && nodes.contains(node))
            .map(|(stage, _)| *stage)
    }
}

/// Each node's distance from the pipeline's inputs: the longest path to it,
/// in edges, a node fed by declared inputs only being at 1.
fn depths(pipeline: &LogicalPipeline) -> BTreeMap<&NodeId, usize> {
    let mut depths: BTreeMap<&NodeId, usize> = BTreeMap::new();
    // A validated pipeline is acyclic, so as many passes as nodes settle
    // every longest path.
    for _ in 0..pipeline.nodes().len() {
        for node in pipeline.nodes() {
            let deepest_input = node
                .inputs()
                .iter()
                .filter_map(|input| depths.get(input).copied())
                .max()
                .unwrap_or(0);
            depths.insert(node.id(), deepest_input + 1);
        }
    }
    depths
}

/// Whether `ancestor` feeds `node`, directly or through other nodes.
fn upstream(pipeline: &LogicalPipeline, node: &NodeId, ancestor: &NodeId) -> bool {
    let mut stack = vec![node];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        let Some(found) = pipeline.nodes().iter().find(|each| each.id() == current) else {
            continue;
        };
        for input in found.inputs() {
            if input == ancestor {
                return true;
            }
            stack.push(input);
        }
    }
    false
}

#[cfg(test)]
pub(crate) mod tests {
    use ragondin_experiments::{lower_configuration, ConfigDocument};

    use super::*;

    pub(crate) const DENSE_ONLY: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
";

    pub(crate) const HYBRID: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [bm25, dense]
";

    pub(crate) const HYBRID_RERANK: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [bm25, dense]
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, rrf]
";

    pub(crate) const HYBRID_RERANK_GEN: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [bm25, dense]
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, rrf]
    - id: concat
      component: context_builder
      impl: concat
      inputs: [question, rerank]
    - id: generate
      component: generator
      impl: answerer
      inputs: [question, concat]
";

    pub(crate) fn lowered(document: &str) -> LogicalPipeline {
        lower_configuration(&ConfigDocument::new(document)).expect("a test pipeline lowers")
    }

    fn ids(names: &[&str]) -> Vec<NodeId> {
        names.iter().map(|name| NodeId::new(*name)).collect()
    }

    fn stages_of(document: &str) -> BTreeMap<Stage, Vec<NodeId>> {
        let stages = Stages::of(&lowered(document));
        assert!(!stages.guessed, "an unambiguous pipeline is not a guess");
        stages.nodes
    }

    #[test]
    fn a_dense_only_pipeline_has_one_leg_which_is_also_its_final_ranking() {
        assert_eq!(
            stages_of(DENSE_ONLY),
            BTreeMap::from([
                (Stage::RetrievalLegs, ids(&["dense"])),
                (Stage::FinalRanking, ids(&["dense"])),
            ])
        );
    }

    #[test]
    fn a_hybrid_pipeline_has_two_legs_and_ends_after_fusion() {
        assert_eq!(
            stages_of(HYBRID),
            BTreeMap::from([
                (Stage::RetrievalLegs, ids(&["bm25", "dense"])),
                (Stage::AfterFusion, ids(&["rrf"])),
                (Stage::FinalRanking, ids(&["rrf"])),
            ])
        );
    }

    #[test]
    fn a_hybrid_rerank_pipeline_ends_after_rerank() {
        assert_eq!(
            stages_of(HYBRID_RERANK),
            BTreeMap::from([
                (Stage::RetrievalLegs, ids(&["bm25", "dense"])),
                (Stage::AfterFusion, ids(&["rrf"])),
                (Stage::AfterRerank, ids(&["rerank"])),
                (Stage::FinalRanking, ids(&["rerank"])),
            ])
        );
    }

    #[test]
    fn a_generating_pipeline_is_ranked_at_the_reranker_and_answers_at_the_generator() {
        assert_eq!(
            stages_of(HYBRID_RERANK_GEN),
            BTreeMap::from([
                (Stage::RetrievalLegs, ids(&["bm25", "dense"])),
                (Stage::AfterFusion, ids(&["rrf"])),
                (Stage::AfterRerank, ids(&["rerank"])),
                (Stage::FinalRanking, ids(&["rerank"])),
                (Stage::Answer, ids(&["generate"])),
            ])
        );
    }

    #[test]
    fn two_fusions_take_the_one_furthest_from_the_inputs_and_say_it_is_a_guess() {
        let stages = Stages::of(&lowered(
            "\
pipeline:
  inputs: [question]
  nodes:
    - id: a
      component: retriever
      impl: bm25
      inputs: [question]
    - id: b
      component: retriever
      impl: dense
      inputs: [question]
    - id: early
      component: fusion
      impl: rrf
      inputs: [a, b]
    - id: late
      component: fusion
      impl: rrf
      inputs: [early, b]
",
        ));

        assert!(stages.guessed);
        assert_eq!(stages.nodes[&Stage::AfterFusion], ids(&["late"]));
    }

    #[test]
    fn a_reranker_upstream_of_the_fusion_is_a_guess() {
        let stages = Stages::of(&lowered(
            "\
pipeline:
  inputs: [question]
  nodes:
    - id: a
      component: retriever
      impl: bm25
      inputs: [question]
    - id: b
      component: retriever
      impl: dense
      inputs: [question]
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, a]
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [rerank, b]
",
        ));

        assert!(stages.guessed);
        assert_eq!(stages.nodes[&Stage::AfterRerank], ids(&["rerank"]));
        assert_eq!(stages.nodes[&Stage::FinalRanking], ids(&["rrf"]));
    }
}
