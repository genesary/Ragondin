//! What `POST /compare` adds to `ragondin-experiments`' metric table and
//! configuration matrix: the runs aligned by stage, with the pairs a person
//! drew by hand; and the bins of the per-query deltas. Pure functions over
//! values the handler has already read. A node's median latency is taken by
//! `ragondin_experiments::lower_median`, the one median of durations.

use std::collections::BTreeMap;

use ragondin_experiments::Direction;
use ragondin_pipeline::NodeId;

use crate::response::{DeltaBinName, NodePair};
use crate::stages::{Stage, Stages};

/// One run's column, for the alignment.
pub(crate) struct Column<'a> {
    /// Its pipeline's stages.
    pub(crate) stages: &'a Stages,
    /// The pairs drawn by hand between the baseline's pipeline and this
    /// run's: each pair's `node` in the baseline's, its `other` in this
    /// run's. Empty for the baseline itself.
    pub(crate) pairs: &'a [NodePair],
}

/// One stage across the runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) stage: Stage,
    /// The label of the first pair drawn by hand into the row that has one.
    pub(crate) label: Option<String>,
    /// Whether a pair drawn by hand placed a node in it.
    pub(crate) manual: bool,
    /// Whether the automatic pairing at this stage is a guess.
    pub(crate) guessed: bool,
    /// Each run's nodes at this stage, by id, each with whether a pair drawn
    /// by hand placed it; `None` for "no stage here".
    pub(crate) cells: Vec<Option<Vec<(NodeId, bool)>>>,
}

/// The runs' stages, aligned: one row per stage at least one run has, in
/// pipeline order, the baseline's column first.
///
/// A pair drawn by hand moves the other run's node out of the stage its kind
/// gives it and into the stage of the baseline's node it is paired with, and
/// that row then reads as manual, under the pair's label. Only the stages a
/// node's kind decides take part — the legs, after fusion, after rerank: the
/// final ranking and the answer are the walk's. A pair naming a node with
/// none of them is passed over; `POST /compare` refuses to keep one.
pub(crate) fn align(columns: &[Column<'_>]) -> Vec<Row> {
    let Some(baseline) = columns.first() else {
        return Vec::new();
    };
    let mut cells: Vec<BTreeMap<Stage, Vec<(NodeId, bool)>>> = columns
        .iter()
        .map(|column| {
            column
                .stages
                .nodes
                .iter()
                .map(|(stage, nodes)| {
                    (
                        *stage,
                        nodes.iter().map(|node| (node.clone(), false)).collect(),
                    )
                })
                .collect()
        })
        .collect();
    let mut manual: BTreeMap<Stage, Option<String>> = BTreeMap::new();
    for (index, column) in columns.iter().enumerate().skip(1) {
        for pair in column.pairs {
            let (node, other) = (NodeId::new(&pair.node), NodeId::new(&pair.other));
            let (Some(target), Some(from)) = (
                baseline.stages.pairable_stage_of(&node),
                column.stages.pairable_stage_of(&other),
            ) else {
                continue;
            };
            if let Some(nodes) = cells[index].get_mut(&from) {
                nodes.retain(|(id, _)| id != &other);
            }
            let nodes = cells[index].entry(target).or_default();
            if !nodes.iter().any(|(id, _)| id == &other) {
                nodes.push((other, true));
            }
            nodes.sort();
            if let Some(nodes) = cells[0].get_mut(&target) {
                for (id, by_hand) in nodes.iter_mut() {
                    if id == &node {
                        *by_hand = true;
                    }
                }
            }
            let label = manual.entry(target).or_default();
            if label.is_none() {
                label.clone_from(&pair.label);
            }
        }
    }
    let guessed = columns.iter().any(|column| column.stages.guessed);
    Stage::ALL
        .into_iter()
        .filter_map(|stage| {
            let row: Vec<Option<Vec<(NodeId, bool)>>> = cells
                .iter()
                .map(|column| {
                    column
                        .get(&stage)
                        .filter(|nodes| !nodes.is_empty())
                        .cloned()
                })
                .collect();
            row.iter().any(Option::is_some).then(|| Row {
                stage,
                label: manual.get(&stage).cloned().flatten(),
                manual: manual.contains_key(&stage),
                guessed: guessed
                    && !manual.contains_key(&stage)
                    && matches!(stage, Stage::AfterFusion | Stage::AfterRerank),
                cells: row,
            })
        })
        .collect()
}

/// Per metric, the best value among `nodes` and the node it is from, by the
/// metric catalogue's one rule for which way a metric improves
/// (`Direction::of`, `ragondin-metrics`); the first node wins a tie. A name
/// the catalogue gives no direction has no best: nothing says which way it
/// improves. A stage cell's `best` in `POST /compare`, and the value a matrix
/// cell's gain is taken over.
pub(crate) fn best<'a>(
    nodes: impl IntoIterator<Item = (&'a str, &'a BTreeMap<String, f64>)>,
) -> BTreeMap<String, (&'a str, f64)> {
    let mut best: BTreeMap<String, (&'a str, f64)> = BTreeMap::new();
    for (node, metrics) in nodes {
        for (metric, value) in metrics {
            let Some(direction) = Direction::of(metric) else {
                continue;
            };
            let better = best.get(metric).is_none_or(|(_, held)| match direction {
                Direction::HigherIsBetter => value > held,
                Direction::LowerIsBetter => value < held,
            });
            if better {
                best.insert(metric.clone(), (node, *value));
            }
        }
    }
    best
}

/// What a node's gain over the previous ranking stage is, or why it has
/// none — four different things a bare "no gain" would run together.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Gain {
    /// Per metric, the node's value minus the best value of the nearest
    /// stage before its own. A metric missing on either side has none.
    Over(BTreeMap<String, f64>),
    /// The node is at the first ranking stage its pipeline has — a retrieval
    /// leg: nothing comes before it, and its value stands alone.
    FirstStage,
    /// The pipeline's stages are a guess (`Stages::guessed`: two fusions, two
    /// rerankers, or a reranker upstream of the fusion), so which stage came
    /// before this one is too; no gain is served as fact.
    Ambiguous,
    /// The node is at no ranking stage: the answer, or a node the derivation
    /// places nowhere.
    Unstaged,
}

/// The gain of `node` over the previous ranking stage of its pipeline: its
/// value minus the best value (`best`) of the nearest stage before its own
/// that the pipeline has — for the fusion, the best retrieval leg; for the
/// reranker, the fusion, or the best leg when no fusion comes before it. By
/// stage, never by node: the stages are `stages`, the derivation
/// `POST /compare` aligns runs by, so the two screens cannot disagree on what
/// came before what — and where that derivation guessed, Compare says
/// `confidence: low` and this says [`Gain::Ambiguous`].
pub(crate) fn gain<'a>(
    stages: &Stages,
    node: &NodeId,
    metrics_of: impl Fn(&NodeId) -> Option<&'a BTreeMap<String, f64>>,
) -> Gain {
    const RANKING: [Stage; 4] = [
        Stage::RetrievalLegs,
        Stage::AfterFusion,
        Stage::AfterRerank,
        Stage::FinalRanking,
    ];
    let Some(own) = RANKING.iter().position(|stage| {
        stages
            .nodes
            .get(stage)
            .is_some_and(|nodes| nodes.contains(node))
    }) else {
        return Gain::Unstaged;
    };
    // The final ranking is the walk's node, at a kind stage already unless
    // the pipeline ends on another ranking node: the stages before it are the
    // kind stages.
    let Some(previous) = RANKING[..own]
        .iter()
        .rev()
        .find_map(|stage| stages.nodes.get(stage))
    else {
        return Gain::FirstStage;
    };
    if stages.guessed {
        return Gain::Ambiguous;
    }
    let Some(value) = metrics_of(node) else {
        return Gain::Over(BTreeMap::new());
    };
    let best = best(
        previous
            .iter()
            .filter_map(|id| metrics_of(id).map(|metrics| (id.as_str(), metrics))),
    );
    Gain::Over(
        value
            .iter()
            .filter_map(|(metric, value)| {
                best.get(metric)
                    .map(|(_, held)| (metric.clone(), value - held))
            })
            .collect(),
    )
}

/// The seven bins, from the worst to the best, with their bounds. The edges
/// are the product's, fixed by `docs/design/2026-09-29-front-end-design.md`
/// § 3. UX and design (`ARCHITECTURE.md` § Compare): the delta's sign, and
/// its absolute magnitude against 0.1 and 0.3, alike for every ranking
/// metric, a bound belonging to the bin nearer zero. Moving them is a change
/// to that document, then to this table and to [`bin_of`].
pub(crate) const BINS: [(DeltaBinName, Option<f64>, Option<f64>); 7] = [
    (DeltaBinName::MuchWorse, None, Some(-0.3)),
    (DeltaBinName::Worse, Some(-0.3), Some(-0.1)),
    (DeltaBinName::SlightlyWorse, Some(-0.1), Some(0.0)),
    (DeltaBinName::Unchanged, Some(0.0), Some(0.0)),
    (DeltaBinName::SlightlyBetter, Some(0.0), Some(0.1)),
    (DeltaBinName::Better, Some(0.1), Some(0.3)),
    (DeltaBinName::MuchBetter, Some(0.3), None),
];

/// The bin a delta falls in. `Unchanged` is a delta of exactly zero: two
/// equal per-query scores, which a ranking left as it was gives exactly.
pub(crate) fn bin_of(delta: f64) -> DeltaBinName {
    let magnitude = delta.abs();
    let (worse, better) = if magnitude > 0.3 {
        (DeltaBinName::MuchWorse, DeltaBinName::MuchBetter)
    } else if magnitude > 0.1 {
        (DeltaBinName::Worse, DeltaBinName::Better)
    } else {
        (DeltaBinName::SlightlyWorse, DeltaBinName::SlightlyBetter)
    };
    if delta < 0.0 {
        worse
    } else if delta > 0.0 {
        better
    } else {
        DeltaBinName::Unchanged
    }
}

/// Each bin of [`BINS`], in order, with the queries whose delta falls in it,
/// in the order given.
pub(crate) fn bins(deltas: &[(String, f64)]) -> Vec<(DeltaBinName, Vec<String>)> {
    BINS.iter()
        .map(|(bin, _, _)| {
            let queries = deltas
                .iter()
                .filter(|(_, delta)| bin_of(*delta) == *bin)
                .map(|(query, _)| query.clone())
                .collect();
            (*bin, queries)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stages::tests::{lowered, DENSE_ONLY, HYBRID, HYBRID_RERANK};

    fn cell(names: &[(&str, bool)]) -> Option<Vec<(NodeId, bool)>> {
        Some(
            names
                .iter()
                .map(|(name, by_hand)| (NodeId::new(*name), *by_hand))
                .collect(),
        )
    }

    #[test]
    fn a_stage_one_run_lacks_is_no_stage_there() {
        let dense = Stages::of(&lowered(DENSE_ONLY));
        let rerank = Stages::of(&lowered(HYBRID_RERANK));

        let rows = align(&[
            Column {
                stages: &dense,
                pairs: &[],
            },
            Column {
                stages: &rerank,
                pairs: &[],
            },
        ]);

        let stages: Vec<Stage> = rows.iter().map(|row| row.stage).collect();
        assert_eq!(
            stages,
            [
                Stage::RetrievalLegs,
                Stage::AfterFusion,
                Stage::AfterRerank,
                Stage::FinalRanking
            ]
        );
        assert_eq!(
            rows[0].cells,
            [
                cell(&[("dense", false)]),
                cell(&[("bm25", false), ("dense", false)])
            ]
        );
        assert_eq!(rows[1].cells, [None, cell(&[("rrf", false)])]);
        assert_eq!(
            rows[3].cells,
            [cell(&[("dense", false)]), cell(&[("rerank", false)])]
        );
        assert!(rows.iter().all(|row| !row.manual && !row.guessed));
    }

    #[test]
    fn a_pair_drawn_by_hand_moves_the_other_node_to_the_baseline_node_s_stage() {
        let hybrid = Stages::of(&lowered(HYBRID));
        let dense = Stages::of(&lowered(DENSE_ONLY));
        let pairs = [NodePair {
            node: "rrf".to_owned(),
            other: "dense".to_owned(),
            label: Some("the best candidates".to_owned()),
        }];

        let rows = align(&[
            Column {
                stages: &hybrid,
                pairs: &[],
            },
            Column {
                stages: &dense,
                pairs: &pairs,
            },
        ]);

        let legs = rows
            .iter()
            .find(|row| row.stage == Stage::RetrievalLegs)
            .unwrap();
        assert_eq!(legs.cells[1], None);
        assert!(!legs.manual);
        let fusion = rows
            .iter()
            .find(|row| row.stage == Stage::AfterFusion)
            .unwrap();
        assert!(fusion.manual);
        assert_eq!(fusion.label.as_deref(), Some("the best candidates"));
        assert_eq!(
            fusion.cells,
            [cell(&[("rrf", true)]), cell(&[("dense", true)])]
        );
        // The final ranking is the walk's, and a pair never moves it.
        let last = rows
            .iter()
            .find(|row| row.stage == Stage::FinalRanking)
            .unwrap();
        assert_eq!(
            last.cells,
            [cell(&[("rrf", false)]), cell(&[("dense", false)])]
        );
    }

    #[test]
    fn an_ambiguous_graph_makes_its_fusion_and_rerank_rows_a_guess() {
        let mut guessed = Stages::of(&lowered(HYBRID_RERANK));
        guessed.guessed = true;
        let dense = Stages::of(&lowered(DENSE_ONLY));

        let rows = align(&[
            Column {
                stages: &dense,
                pairs: &[],
            },
            Column {
                stages: &guessed,
                pairs: &[],
            },
        ]);

        let guesses: Vec<(Stage, bool)> = rows.iter().map(|row| (row.stage, row.guessed)).collect();
        assert_eq!(
            guesses,
            [
                (Stage::RetrievalLegs, false),
                (Stage::AfterFusion, true),
                (Stage::AfterRerank, true),
                (Stage::FinalRanking, false)
            ]
        );
    }

    #[test]
    fn every_delta_falls_in_exactly_one_bin_a_bound_in_the_one_nearer_zero() {
        let cases = [
            (-1.0, DeltaBinName::MuchWorse),
            (-0.3000001, DeltaBinName::MuchWorse),
            (-0.3, DeltaBinName::Worse),
            (-0.1000001, DeltaBinName::Worse),
            (-0.1, DeltaBinName::SlightlyWorse),
            (-1e-12, DeltaBinName::SlightlyWorse),
            (0.0, DeltaBinName::Unchanged),
            (-0.0, DeltaBinName::Unchanged),
            (1e-12, DeltaBinName::SlightlyBetter),
            (0.1, DeltaBinName::SlightlyBetter),
            (0.1000001, DeltaBinName::Better),
            (0.3, DeltaBinName::Better),
            (0.3000001, DeltaBinName::MuchBetter),
            (1.0, DeltaBinName::MuchBetter),
        ];
        for (delta, bin) in cases {
            assert_eq!(bin_of(delta), bin, "{delta}");
        }
        let deltas: Vec<(String, f64)> = cases
            .iter()
            .enumerate()
            .map(|(n, (delta, _))| (format!("q{n}"), *delta))
            .collect();
        let binned = bins(&deltas);
        let names: Vec<DeltaBinName> = binned.iter().map(|(bin, _)| *bin).collect();
        let order: Vec<DeltaBinName> = BINS.iter().map(|(bin, _, _)| *bin).collect();
        assert_eq!(names, order);
        let total: usize = binned.iter().map(|(_, queries)| queries.len()).sum();
        assert_eq!(total, cases.len(), "the bins partition the queries");
        assert_eq!(binned[3].1, ["q6", "q7"]);
    }

    fn figures(rows: &[(&str, f64, f64)]) -> BTreeMap<NodeId, BTreeMap<String, f64>> {
        rows.iter()
            .map(|(node, ndcg, mrr)| {
                (
                    NodeId::new(*node),
                    BTreeMap::from([("ndcg@10".to_owned(), *ndcg), ("mrr".to_owned(), *mrr)]),
                )
            })
            .collect()
    }

    #[test]
    fn the_best_of_several_nodes_is_taken_metric_by_metric() {
        let figures = figures(&[("bm25", 0.3, 0.5), ("dense", 0.4, 0.2)]);
        let best = best(
            figures
                .iter()
                .map(|(node, metrics)| (node.as_str(), metrics)),
        );
        assert_eq!(best["ndcg@10"], ("dense", 0.4));
        assert_eq!(best["mrr"], ("bm25", 0.5));
    }

    #[test]
    fn a_name_the_catalogue_does_not_know_has_no_best() {
        let metrics = BTreeMap::from([
            ("ndcg@10".to_owned(), 0.4),
            ("latency_p50_ms".to_owned(), 12.0),
        ]);
        let best = best([("dense", &metrics)]);
        assert_eq!(best.keys().collect::<Vec<_>>(), ["ndcg@10"]);
    }

    #[test]
    fn a_stage_s_gain_is_over_the_best_node_of_the_stage_before_it() {
        let stages = Stages::of(&lowered(HYBRID_RERANK));
        let figures = figures(&[
            ("bm25", 0.3, 0.5),
            ("dense", 0.4, 0.2),
            ("rrf", 0.5, 0.6),
            ("rerank", 0.7, 0.55),
        ]);
        let gain_of = |node: &str| gain(&stages, &NodeId::new(node), |id| figures.get(id));

        // A leg has no stage before it: the value stands alone.
        assert_eq!(gain_of("bm25"), Gain::FirstStage);
        assert_eq!(gain_of("dense"), Gain::FirstStage);
        // The fusion over the best leg, metric by metric.
        let Gain::Over(rrf) = gain_of("rrf") else {
            panic!("the fusion gains over the legs");
        };
        assert_eq!(rrf["ndcg@10"], 0.5 - 0.4);
        assert_eq!(rrf["mrr"], 0.6 - 0.5);
        // The reranker over the fusion.
        let Gain::Over(rerank) = gain_of("rerank") else {
            panic!("the reranker gains over the fusion");
        };
        assert_eq!(rerank["ndcg@10"], 0.7 - 0.5);
        assert_eq!(rerank["mrr"], 0.55 - 0.6);
    }

    #[test]
    fn a_reranker_with_no_fusion_before_it_gains_over_the_legs() {
        let stages = Stages::of(&lowered(
            "\
pipeline:
  inputs: [question]
  nodes:
    - id: colbert
      component: retriever
      impl: colbert
      inputs: [question]
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, colbert]
",
        ));
        let figures = figures(&[("colbert", 0.4, 0.5), ("rerank", 0.6, 0.7)]);
        let Gain::Over(rerank) = gain(&stages, &NodeId::new("rerank"), |id| figures.get(id)) else {
            panic!("the reranker gains over the leg");
        };
        assert_eq!(rerank["ndcg@10"], 0.6 - 0.4);
    }

    #[test]
    fn a_dense_only_pipeline_s_one_node_has_no_gain() {
        let stages = Stages::of(&lowered(DENSE_ONLY));
        let figures = figures(&[("dense", 0.4, 0.5)]);
        assert_eq!(
            gain(&stages, &NodeId::new("dense"), |id| figures.get(id)),
            Gain::FirstStage
        );
    }

    #[test]
    fn over_a_guessed_derivation_no_gain_is_computed_past_the_legs() {
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
        let figures = figures(&[
            ("a", 0.3, 0.3),
            ("b", 0.4, 0.4),
            ("rerank", 0.5, 0.5),
            ("rrf", 0.6, 0.6),
        ]);
        let gain_of = |node: &str| gain(&stages, &NodeId::new(node), |id| figures.get(id));
        assert_eq!(gain_of("a"), Gain::FirstStage);
        assert_eq!(gain_of("rerank"), Gain::Ambiguous);
        assert_eq!(gain_of("rrf"), Gain::Ambiguous);
    }

    #[test]
    fn a_node_at_no_ranking_stage_is_unstaged() {
        let stages = Stages::of(&lowered(HYBRID_RERANK));
        let figures = figures(&[]);
        assert_eq!(
            gain(&stages, &NodeId::new("nowhere"), |id| figures.get(id)),
            Gain::Unstaged
        );
    }
}
