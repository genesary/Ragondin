//! What `POST /compare` adds to `ragondin-experiments`' metric table and
//! configuration matrix: the runs aligned by stage, with the pairs a person
//! drew by hand; the bins of the per-query deltas; a node's median latency.
//! Pure functions over values the handler has already read.

use std::collections::BTreeMap;

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

/// The lower median of `durations`: of the two middle values over an even
/// count, the lower, so it is a duration that occurred. `None` for none.
pub(crate) fn median(mut durations: Vec<u64>) -> Option<u64> {
    durations.sort_unstable();
    let middle = durations.len().checked_sub(1)? / 2;
    durations.get(middle).copied()
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

    #[test]
    fn the_median_is_the_lower_middle_value() {
        assert_eq!(median(vec![]), None);
        assert_eq!(median(vec![7]), Some(7));
        assert_eq!(median(vec![30, 10, 20]), Some(20));
        assert_eq!(median(vec![40, 10, 30, 20]), Some(20));
    }
}
