//! "Run up to a node": a prefix of a workspace pipeline, cut on the wire
//! schema and submitted as an ordinary run (ADR-C36 § 7).
//!
//! The prefix up to node *X* is the document whose nodes are *X* and every
//! node *X* reads, transitively, in the order the document lists them, with
//! the pipeline's declared inputs kept; *X* is then its output. The cut is
//! made on `RawPipeline`, the wire schema (INV-9), rendered by
//! `ragondin-config`'s one renderer and loaded back through the one load, so
//! its hash is the canonical form of exactly the text the job snapshots
//! (INV-8) — the same hash `ragondin validate` prints for that text in a
//! file. Nothing is asked of `ragondin-pipeline` beyond its public surface
//! (INV-1).
//!
//! Which nodes a prefix may stop at is read off the parent's lowered graph:
//! never its output (the prefix would be the whole pipeline), never a context
//! builder (a context is scored by nothing, ADR-C30 § 3). And a prefix whose
//! output is not an answer cannot run on a benchmark that carries reference
//! answers (ADR-C30 § 5): [`scorable`] says so at submission, from the
//! ground truth the registry reads off the dataset — what the harness itself
//! would refuse once the run started.

use std::collections::BTreeSet;

use ragondin_config::render_document;
use ragondin_experiments::terminal;
use ragondin_pipeline::{
    produced_kind, LogicalNode, PipelineHash, RawGraph, RawPipeline, ValueKind,
};

use crate::error::ApiError;
use crate::response::{BenchmarkEntry, GroundTruth, Location};
use crate::validation;

/// A workspace document cut at a node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cut {
    /// The cut, rendered: what the job snapshots and the run stores.
    pub(crate) document: String,
    /// The cut's own canonical hash: the run's pipeline hash.
    pub(crate) hash: PipelineHash,
    /// The canonical hash of the document it was cut from.
    pub(crate) parent_hash: PipelineHash,
    /// The kind of value the cut's output produces.
    pub(crate) output: ValueKind,
}

/// `document`, the workspace pipeline `pipeline`, cut at `up_to`.
///
/// # Errors
///
/// `pipeline_invalid` when the document does not validate, or the cut does
/// not — which, the cut keeping everything its nodes read, it always does;
/// `prefix_node_not_found`, `prefix_is_whole_pipeline` and
/// `prefix_ends_in_context`, each naming the node.
pub(crate) fn cut(pipeline: &str, document: &str, up_to: &str) -> Result<Cut, ApiError> {
    let parent = validation::lower(document)?;
    let Some(node) = parent
        .nodes()
        .iter()
        .find(|node| node.id().as_str() == up_to)
    else {
        return Err(ApiError::PrefixNodeNotFound {
            pipeline: pipeline.to_owned(),
            node: up_to.to_owned(),
        });
    };
    if terminal(&parent).is_some_and(|output| output.id() == node.id()) {
        return Err(ApiError::PrefixIsWholePipeline {
            node: up_to.to_owned(),
        });
    }
    if matches!(node, LogicalNode::ContextBuilder(_)) {
        return Err(ApiError::PrefixEndsInContext {
            node: up_to.to_owned(),
        });
    }
    let raw = truncate(&validation::read(document)?, up_to);
    let rendered = render_document(&raw).map_err(|error| ApiError::PipelineInvalid {
        detail: error.to_string(),
        location: Location {
            node: None,
            edge: None,
        },
    })?;
    let hash = validation::lower(&rendered)?.content_hash();
    Ok(Cut {
        document: rendered,
        hash,
        parent_hash: parent.content_hash(),
        output: produced_kind(node),
    })
}

/// `raw`'s nodes up to `up_to`: that node and every node it reads,
/// transitively, in the document's order, with every declared input kept.
fn truncate(raw: &RawPipeline, up_to: &str) -> RawPipeline {
    let mut kept = BTreeSet::new();
    let mut pending = vec![up_to];
    while let Some(id) = pending.pop() {
        if !kept.insert(id) {
            continue;
        }
        if let Some(node) = raw.pipeline.nodes.iter().find(|node| node.id == id) {
            pending.extend(node.inputs.iter().map(String::as_str));
        }
    }
    RawPipeline {
        version: raw.version,
        pipeline: RawGraph {
            inputs: raw.pipeline.inputs.clone(),
            nodes: raw
                .pipeline
                .nodes
                .iter()
                .filter(|node| kept.contains(node.id.as_str()))
                .cloned()
                .collect(),
        },
    }
}

/// Whether a prefix stopping at `up_to`, whose output is of kind `output`,
/// can run on `benchmark`: not when the benchmark carries reference answers
/// and the output is not an answer, which the harness refuses (ADR-C30 § 5).
/// A benchmark whose ground truth was not read — nothing on disk loaded — is
/// left to the launcher, which reads the dataset itself.
///
/// # Errors
///
/// `prefix_not_scorable`, naming the node: ADR-C30 § 5 restated from the
/// same `carries()` and `produced_kind` the harness reads (#468 gives the
/// rule one definition).
pub(crate) fn scorable(
    up_to: &str,
    output: ValueKind,
    benchmark: &BenchmarkEntry,
) -> Result<(), ApiError> {
    let answers = matches!(
        benchmark.ground_truth,
        Some(GroundTruth::ReferenceAnswers | GroundTruth::Both)
    );
    if answers && output != ValueKind::Answer {
        return Err(ApiError::PrefixNotScorable {
            node: up_to.to_owned(),
            benchmark: benchmark.name.clone(),
            kind: output.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::BenchmarkState;
    use crate::stages::tests::{HYBRID, HYBRID_RERANK, HYBRID_RERANK_GEN};

    /// [`HYBRID_RERANK_GEN`] with a retriever, `side`, that only a second
    /// fusion after the reranker reads.
    const WITH_SIDE_LEG: &str = "\
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
    - id: side
      component: retriever
      impl: bm25
      inputs: [question]
    - id: both
      component: fusion
      impl: rrf
      inputs: [rerank, side]
    - id: concat
      component: context_builder
      impl: concat
      inputs: [question, both]
    - id: generate
      component: generator
      impl: answerer
      inputs: [question, concat]
";

    fn hash(document: &str) -> String {
        validation::check(document).expect("a test document validates")
    }

    fn node_ids(document: &str) -> Vec<String> {
        validation::read(document)
            .unwrap()
            .pipeline
            .nodes
            .into_iter()
            .map(|node| node.id)
            .collect()
    }

    #[test]
    fn the_cut_keeps_the_node_its_transitive_inputs_and_the_declared_inputs() {
        let cut = cut("p", HYBRID_RERANK_GEN, "rerank").expect("a prefix");

        assert_eq!(hash(&cut.document), hash(HYBRID_RERANK));
        assert_eq!(cut.parent_hash.to_string(), hash(HYBRID_RERANK_GEN));
        assert_eq!(cut.output, ValueKind::Chunks);
        assert_eq!(hash(&cut_at(HYBRID_RERANK_GEN, "rrf")), hash(HYBRID));
    }

    #[test]
    fn a_node_read_only_by_dropped_nodes_is_dropped_and_the_declared_input_kept() {
        let cut = cut("p", WITH_SIDE_LEG, "rerank").expect("a prefix");

        assert_eq!(node_ids(&cut.document), ["bm25", "dense", "rrf", "rerank"]);
        assert_eq!(
            validation::read(&cut.document).unwrap().pipeline.inputs,
            ["question"]
        );
        assert_eq!(
            node_ids(&cut_at(WITH_SIDE_LEG, "both")),
            ["bm25", "dense", "rrf", "rerank", "side", "both"]
        );
    }

    /// A true diamond: one leg read by two rerankers, whose rankings a fusion
    /// joins, then a context and an answer.
    const DIAMOND: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: lexical
      component: retriever
      impl: bm25
      inputs: [question]
    - id: first
      component: reranker
      impl: cross_encoder
      inputs: [question, lexical]
    - id: second
      component: reranker
      impl: cross_encoder
      inputs: [question, lexical]
    - id: fused
      component: fusion
      impl: rrf
      inputs: [first, second]
    - id: concat
      component: context_builder
      impl: concat
      inputs: [question, fused]
    - id: generate
      component: generator
      impl: answerer
      inputs: [question, concat]
";

    #[test]
    fn a_node_two_kept_nodes_read_is_kept_once_in_a_diamond() {
        assert_eq!(
            node_ids(&cut_at(DIAMOND, "fused")),
            ["lexical", "first", "second", "fused"]
        );
        assert_eq!(node_ids(&cut_at(DIAMOND, "first")), ["lexical", "first"]);
    }

    #[test]
    fn the_truncation_keeps_every_declared_input_however_many() {
        // The wire schema reads two declared inputs; validation then refuses
        // them, so the cut of such a document is refused as the document is.
        // The truncation itself drops none of them.
        let raw = validation::read(
            "\
pipeline:
  inputs: [question, filter]
  nodes:
    - id: lexical
      component: retriever
      impl: bm25
      inputs: [question]
    - id: filtered
      component: retriever
      impl: bm25
      inputs: [filter]
    - id: fused
      component: fusion
      impl: rrf
      inputs: [lexical, filtered]
",
        )
        .expect("the wire schema reads two inputs");

        let cut = truncate(&raw, "lexical");

        assert_eq!(cut.pipeline.inputs, ["question", "filter"]);
        let ids: Vec<&str> = cut.pipeline.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["lexical"]);
    }

    #[test]
    fn the_four_refusals_name_the_node() {
        let refused = |up_to: &str| cut("p", HYBRID_RERANK_GEN, up_to).unwrap_err();

        assert_eq!(
            refused("nowhere"),
            ApiError::PrefixNodeNotFound {
                pipeline: "p".to_owned(),
                node: "nowhere".to_owned()
            }
        );
        assert_eq!(
            refused("question"),
            ApiError::PrefixNodeNotFound {
                pipeline: "p".to_owned(),
                node: "question".to_owned()
            }
        );
        assert_eq!(
            refused("generate"),
            ApiError::PrefixIsWholePipeline {
                node: "generate".to_owned()
            }
        );
        assert_eq!(
            refused("concat"),
            ApiError::PrefixEndsInContext {
                node: "concat".to_owned()
            }
        );

        let entry = |ground_truth| BenchmarkEntry {
            name: "squad/dev".to_owned(),
            format: "squad".to_owned(),
            state: BenchmarkState::Ready {
                dataset_version: "digest".to_owned(),
            },
            ground_truth,
            licence: None,
            licence_url: None,
        };
        assert_eq!(
            scorable("rerank", ValueKind::Chunks, &entry(Some(GroundTruth::Both))),
            Err(ApiError::PrefixNotScorable {
                node: "rerank".to_owned(),
                benchmark: "squad/dev".to_owned(),
                kind: "chunks".to_owned(),
            })
        );
        assert!(scorable(
            "rerank",
            ValueKind::Chunks,
            &entry(Some(GroundTruth::ReferenceAnswers))
        )
        .is_err());
        assert_eq!(
            scorable(
                "rerank",
                ValueKind::Chunks,
                &entry(Some(GroundTruth::Qrels))
            ),
            Ok(())
        );
        assert_eq!(scorable("rerank", ValueKind::Chunks, &entry(None)), Ok(()));
        assert_eq!(
            scorable("gen", ValueKind::Answer, &entry(Some(GroundTruth::Both))),
            Ok(())
        );
    }

    /// The document `document` cut at `up_to` renders.
    fn cut_at(document: &str, up_to: &str) -> String {
        cut("p", document, up_to).expect("a prefix").document
    }
}
