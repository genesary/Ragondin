//! The pure parts of `GET /pipelines/{name}/matrix`: which run is the most
//! recent, and the order the rows read in.

use std::cmp::Ordering;

use ragondin_pipeline::{LogicalNode, LogicalPipeline};

/// What "most recent" reads of a run: its own start time, and its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Recency<'a> {
    pub(crate) started_at_ms: Option<u64>,
    pub(crate) id: &'a str,
}

/// The most-recent-run rule: the greatest `started_at_ms` first, a run whose
/// start is unknown after every run whose start is known — so it is never
/// chosen over one — and ties, unknown included, broken by run id, lowest
/// first, so the choice is deterministic. It is the rule the Runs screen
/// sorts by (`byMostRecent` in `ui/src/runs/model.ts`), and the server's one
/// definition of it. The time is the run's own record (`Run::times`), never
/// one read from the store's backend.
pub(crate) fn most_recent_first(a: Recency<'_>, b: Recency<'_>) -> Ordering {
    match (a.started_at_ms, b.started_at_ms) {
        (Some(a_time), Some(b_time)) if a_time != b_time => b_time.cmp(&a_time),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        _ => a.id.cmp(b.id),
    }
}

/// The pipeline's nodes in topological order — every node after the nodes it
/// reads — ties among the nodes ready at once broken by the canonical order
/// (by id), so the matrix reads like the pipeline and the same pipeline always
/// reads the same way.
pub(crate) fn topological(pipeline: &LogicalPipeline) -> Vec<&LogicalNode> {
    let nodes = pipeline.nodes();
    let mut placed: Vec<&LogicalNode> = Vec::with_capacity(nodes.len());
    let is_placed = |placed: &[&LogicalNode], id| placed.iter().any(|node| node.id() == id);
    // A validated pipeline is acyclic, so each pass places at least one node;
    // the canonical order is the id order, so the first ready node is the
    // lowest id.
    while placed.len() < nodes.len() {
        let ready = nodes.iter().find(|node| {
            !is_placed(&placed, node.id())
                && node.inputs().iter().all(|input| {
                    is_placed(&placed, input) || !nodes.iter().any(|other| other.id() == input)
                })
        });
        match ready {
            Some(node) => placed.push(node),
            // Unreachable for a validated pipeline; never loop on another.
            None => break,
        }
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stages::tests::{lowered, HYBRID_RERANK_GEN};

    fn sorted<'a>(mut runs: Vec<Recency<'a>>) -> Vec<&'a str> {
        runs.sort_by(|a, b| most_recent_first(*a, *b));
        runs.into_iter().map(|run| run.id).collect()
    }

    fn at(id: &str, started_at_ms: Option<u64>) -> Recency<'_> {
        Recency { started_at_ms, id }
    }

    /// The Runs screen's case, `ui/src/runs/model.test.ts`, run for run:
    /// one rule, read the same on both sides.
    #[test]
    fn most_recent_first_unknown_last_ties_by_id() {
        assert_eq!(
            sorted(vec![
                at("3", None),
                at("2", Some(1000)),
                at("9", Some(2000)),
                at("1", None),
                at("4", Some(2000)),
            ]),
            ["4", "9", "2", "1", "3"]
        );
    }

    #[test]
    fn a_run_with_a_time_comes_before_one_without_whichever_id_is_greater() {
        assert_eq!(sorted(vec![at("1", None), at("9", Some(5))]), ["9", "1"]);
        assert_eq!(sorted(vec![at("9", None), at("1", Some(5))]), ["1", "9"]);
    }

    #[test]
    fn two_runs_without_a_time_are_ordered_by_id() {
        assert_eq!(sorted(vec![at("b", None), at("a", None)]), ["a", "b"]);
    }

    #[test]
    fn the_rows_read_like_the_pipeline_inputs_before_what_reads_them() {
        let pipeline = lowered(HYBRID_RERANK_GEN);
        let order: Vec<&str> = topological(&pipeline)
            .into_iter()
            .map(|node| node.id().as_str())
            .collect();
        assert_eq!(
            order,
            ["bm25", "dense", "rrf", "rerank", "concat", "generate"]
        );
    }
}
