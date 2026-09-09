//! Reciprocal Rank Fusion.

use std::collections::BTreeMap;

use async_trait::async_trait;
use ragondin_contracts::{ComponentError, Fusion, FusionParams};
use ragondin_types::{ChunkId, ScoredChunk};

/// The `k` the literature uses, and the one the original paper reports.
pub const DEFAULT_K: usize = 60;

/// Fuses ranked lists by reciprocal rank.
///
/// A chunk's fused score is `sum over the legs that ranked it of
/// 1 / (k + rank)`, with `rank` 1-based, and the result is sorted by descending
/// score. The **incoming scores are not read at all**: a BM25 score and a
/// cosine similarity are on incomparable scales, and reading only the position
/// is what lets the two be combined without calibrating either.
///
/// `k` damps the top ranks, so a chunk placed well by several legs can outrank
/// one placed first by a single leg. It is **constructor configuration**
/// (`ragondin-contracts`: a params struct carries only what varies per call),
/// and a [`usize`], which is what keeps the contract's "finite scores" clause
/// unbreakable: `k + rank >= 1` for every `k`, so the division has no edge case
/// to guard and this component needs no failure mode of its own.
///
/// **Ties break by chunk id, ascending.** Equal fused scores are the common
/// case rather than a rarity — every chunk holding the same ranks ties — and an
/// order left to the sort would make a run irreproducible.
pub struct ReciprocalRankFusion {
    k: usize,
}

impl ReciprocalRankFusion {
    /// Fuses with the given `k`.
    pub fn new(k: usize) -> Self {
        Self { k }
    }
}

impl Default for ReciprocalRankFusion {
    fn default() -> Self {
        Self::new(DEFAULT_K)
    }
}

#[async_trait]
impl Fusion for ReciprocalRankFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        // Accumulated in `f64` and narrowed once at the end: the reciprocals
        // are small and numerous, and summing them in `f32` would make the
        // fused score depend on how many legs happened to rank the chunk.
        let mut totals: Vec<(ScoredChunk, f64)> = Vec::new();
        // Position into `totals`, so the output order comes from insertion
        // order and the sort below, never from a map's iteration order.
        let mut seen: BTreeMap<ChunkId, usize> = BTreeMap::new();

        for leg in inputs {
            for (index, hit) in leg.into_iter().enumerate() {
                let contribution = 1.0 / (self.k + index + 1) as f64;
                match seen.get(&hit.chunk.id) {
                    Some(&at) => totals[at].1 += contribution,
                    None => {
                        seen.insert(hit.chunk.id.clone(), totals.len());
                        totals.push((hit, contribution));
                    }
                }
            }
        }

        totals.sort_by(|(left, left_score), (right, right_score)| {
            right_score
                .total_cmp(left_score)
                .then_with(|| left.chunk.id.cmp(&right.chunk.id))
        });

        Ok(totals
            .into_iter()
            .map(|(mut hit, score)| {
                hit.score = score as f32;
                hit
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ragondin_types::{Chunk, ChunkId, DocId};

    fn scored(id: &str, score: f32) -> ScoredChunk {
        ScoredChunk {
            chunk: Chunk {
                id: ChunkId::new(id),
                text: format!("text of {id}"),
                document_id: DocId::new("doc"),
            },
            score,
        }
    }

    fn ids(hits: &[ScoredChunk]) -> Vec<&str> {
        hits.iter().map(|hit| hit.chunk.id.as_str()).collect()
    }

    async fn fuse(
        fusion: &ReciprocalRankFusion,
        inputs: Vec<Vec<ScoredChunk>>,
    ) -> Vec<ScoredChunk> {
        fusion
            .fuse(inputs, &FusionParams::new())
            .await
            .expect("fusing well-formed lists succeeds")
    }

    fn assert_close(got: f32, expected: f32) {
        assert!(
            (got - expected).abs() < 1e-7,
            "expected {expected}, got {got}"
        );
    }

    /// The formula, hand-computed, with `k = 60` and 1-based ranks.
    ///
    /// Leg A ranks `a`, `b`, `c` 1, 2, 3; leg B ranks `b`, `d` 1, 2. So:
    ///
    /// - `b` = 1/62 + 1/61 = 0.016129032 + 0.016393443 = 0.032522475
    /// - `a` = 1/61                                     = 0.016393443
    /// - `d` =         1/62                             = 0.016129032
    /// - `c` = 1/63                                     = 0.015873016
    ///
    /// `b` outranks `a` on two mediocre placements against one good one, which
    /// is the whole point of RRF and the thing a rewrite would most easily lose.
    #[tokio::test]
    async fn scores_and_orders_by_the_reciprocal_rank_formula() {
        let fusion = ReciprocalRankFusion::default();
        let fused = fuse(
            &fusion,
            vec![
                vec![scored("a", 9.0), scored("b", 8.0), scored("c", 7.0)],
                vec![scored("b", 0.4), scored("d", 0.3)],
            ],
        )
        .await;

        assert_eq!(ids(&fused), ["b", "a", "d", "c"]);
        assert_close(fused[0].score, 0.032_522_475);
        assert_close(fused[1].score, 0.016_393_443);
        assert_close(fused[2].score, 0.016_129_032);
        assert_close(fused[3].score, 0.015_873_016);
    }

    /// The incoming scores are on incomparable scales, and RRF reads none of
    /// them: only the position. A leg whose scores are large must not thereby
    /// dominate.
    #[tokio::test]
    async fn incoming_scores_are_ignored() {
        let fusion = ReciprocalRankFusion::default();
        let fused = fuse(
            &fusion,
            vec![
                vec![scored("a", 1e6), scored("b", 1e5)],
                vec![scored("b", 0.002), scored("a", 0.001)],
            ],
        )
        .await;

        assert_eq!(ids(&fused), ["a", "b"]);
        // Both are ranked 1 and 2 once each: 1/61 + 1/62, identically.
        assert_close(fused[0].score, 0.032_522_475);
        assert_close(fused[1].score, 0.032_522_475);
    }

    /// Equal scores are frequent — every chunk holding the same ranks ties —
    /// so the order among them is pinned rather than left to the sort. Without
    /// it a run is not reproducible, which M2 requires.
    #[tokio::test]
    async fn ties_break_by_chunk_id_ascending() {
        let fusion = ReciprocalRankFusion::default();
        let fused = fuse(
            &fusion,
            vec![vec![scored("z", 0.9)], vec![scored("a", 0.1)]],
        )
        .await;

        assert_eq!(
            ids(&fused),
            ["a", "z"],
            "both are ranked 1, so both score 1/61"
        );
        assert_close(fused[0].score, fused[1].score);
    }

    /// `k` damps the influence of the top ranks. It is constructor
    /// configuration, not a per-call parameter, per `ragondin-contracts`.
    #[tokio::test]
    async fn k_is_configurable() {
        let fused = fuse(
            &ReciprocalRankFusion::new(0),
            vec![vec![scored("a", 0.9), scored("b", 0.8)]],
        )
        .await;

        assert_eq!(ids(&fused), ["a", "b"]);
        assert_close(fused[0].score, 1.0);
        assert_close(fused[1].score, 0.5);
    }

    #[test]
    fn the_default_k_is_sixty() {
        assert_eq!(DEFAULT_K, 60);
    }

    /// A chunk seen in several legs is returned once, carrying the chunk of the
    /// first leg that offered it. Some leg's text has to win; which one is
    /// fixed so that the answer does not depend on how the legs were wired.
    #[tokio::test]
    async fn a_repeated_chunk_keeps_the_first_legs_copy() {
        let fusion = ReciprocalRankFusion::default();
        let mut from_the_right = scored("a", 0.1);
        from_the_right.chunk.text = "the right leg's text".to_string();
        let fused = fuse(&fusion, vec![vec![scored("a", 0.9)], vec![from_the_right]]).await;

        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].chunk.text, "text of a");
    }

    #[tokio::test]
    async fn fusing_nothing_yields_nothing() {
        let fusion = ReciprocalRankFusion::default();
        assert!(fuse(&fusion, Vec::new()).await.is_empty());
        assert!(fuse(&fusion, vec![Vec::new(), Vec::new()]).await.is_empty());
    }

    /// The engine holds a `Box<dyn Fusion>` and cannot tell `Local` from
    /// `Remote` (ADR-3), so the vtable is the only path that matters.
    #[tokio::test]
    async fn it_is_callable_through_a_trait_object() {
        let component: Box<dyn Fusion> = Box::new(ReciprocalRankFusion::default());
        let fused = component
            .fuse(vec![vec![scored("a", 0.9)]], &FusionParams::new())
            .await
            .expect("fusing a single leg succeeds");
        assert_eq!(ids(&fused), ["a"]);
    }
}
