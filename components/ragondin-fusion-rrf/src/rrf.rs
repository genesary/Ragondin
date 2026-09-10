//! Reciprocal Rank Fusion.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use ragondin_contracts::{ComponentError, Fusion, FusionParams};
use ragondin_types::{ChunkId, ScoredChunk};

/// The `k` the literature uses, and the one the original paper reports.
pub const DEFAULT_K: usize = 60;

/// The largest `k` this component honours; a larger one is clamped to it.
///
/// A rank's contribution is `1 / (k + rank + 1)`, accumulated in `f64`. Two
/// consecutive divisors around `k` produce reciprocals that differ by roughly
/// `1/k` in relative terms, so as `k` grows they converge — and once they round
/// to the same `f64`, every rank in a leg scores identically, the ascending-id
/// tiebreak decides an order the ranks were supposed to, and a single leg comes
/// back **sorted by id** instead of in the order it arrived. That is the one
/// property `check_fusion_conformance` states a fusion must preserve.
///
/// `2^32` keeps a relative gap of about `2^-32` between consecutive
/// reciprocals — some two million `f64` ULPs, so they separate unambiguously
/// rather than marginally. Setting the bound at the point where they *begin* to
/// collide (`2^53`) is not enough: there the gap is a single ULP, and
/// neighbouring ranks round together in a way that depends on where they fall.
///
/// For scale, the conventional `k` is 60, so this is some seventy million times
/// any value with a meaning to preserve.
///
/// Clamped rather than rejected: a `k` out here carries no intent, and making
/// `new` fallible would put a `Result` on every call site for a case none of
/// them reaches.
pub const MAX_K: usize = MAX_K_U64 as usize;

const MAX_K_U64: u64 = 1 << 32;

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
/// unbreakable: `k + rank >= 1` for every `k`, and `k` is clamped to [`MAX_K`]
/// at construction so the divisor cannot leave the range where consecutive
/// ranks still separate. The division has no edge case to guard, and this
/// component needs no failure mode of its own: every `fuse` call succeeds.
///
/// **Ties break by chunk id, ascending.** Equal fused scores are the common
/// case rather than a rarity — every chunk holding the same ranks ties — and an
/// order left to the sort would make a run irreproducible.
pub struct ReciprocalRankFusion {
    k: usize,
}

impl ReciprocalRankFusion {
    /// Fuses with the given `k`, clamped to [`MAX_K`].
    pub fn new(k: usize) -> Self {
        // `min` against a `u64` bound so this is correct on a 32-bit target
        // too, where every `usize` is already below it and nothing is clamped.
        let k = u64::try_from(k)
            .map(|k| k.min(MAX_K_U64))
            .unwrap_or(MAX_K_U64) as usize;
        Self { k }
    }

    /// The `k` in force, after clamping.
    pub fn k(&self) -> usize {
        self.k
    }
}

impl Default for ReciprocalRankFusion {
    fn default() -> Self {
        Self::new(DEFAULT_K)
    }
}

#[async_trait]
impl Fusion for ReciprocalRankFusion {
    /// An id repeated **within a single leg** contributes once for that leg,
    /// at its first position. Counting both occurrences would roughly double
    /// that chunk's fused score and float it to the top, invisibly: the fused
    /// output still holds distinct ids and passes every downstream check, so
    /// the only symptom is a wrong number.
    ///
    /// This is guarded rather than left to the caller, because the contract
    /// does not forbid the input: `check_no_duplicate_ids` is called from the
    /// `Fusion` and `Reranker` suites only, never from `Retriever` or
    /// `VectorStore`, so a fully conformant leg may repeat an id.
    ///
    /// Skipping a duplicate does not shift the ranks after it — positions are
    /// read from the leg as given, so the chunks around it keep the ranks they
    /// actually occupy.
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
            // One contribution per chunk per leg. A leg naming a chunk twice
            // would otherwise add both reciprocals, roughly doubling that
            // chunk's fused score and floating it to the top -- invisibly,
            // since the fused output still holds distinct ids and passes every
            // downstream check. `check_no_duplicate_ids` is called from the
            // `Fusion` and `Reranker` suites only, never from `Retriever` or
            // `VectorStore`, so a fully conformant leg may repeat an id and
            // this cannot be left to the caller's good behaviour.
            let mut counted: BTreeSet<ChunkId> = BTreeSet::new();

            for (index, hit) in leg.into_iter().enumerate() {
                if !counted.insert(hit.chunk.id.clone()) {
                    continue;
                }
                // Saturating is belt and braces: `k` is already clamped to
                // `MAX_K` at construction, so `k + index + 1` cannot overflow
                // for any leg that fits in memory. Saturation alone would not
                // be enough anyway -- it keeps the score finite while letting
                // every rank collapse onto one reciprocal, which is the failure
                // `MAX_K` exists to prevent.
                let divisor = self.k.saturating_add(index).saturating_add(1);
                let contribution = 1.0 / divisor as f64;
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

    /// Relative, for scores far below the absolute tolerance above: at `1e-20`
    /// an absolute comparison passes whatever the code returned, zero included.
    fn assert_close_relative(got: f32, expected: f32) {
        assert!(
            (got - expected).abs() <= expected.abs() * 1e-6,
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

    /// The largest `k` there is. `k + rank` computed as a plain sum overflows
    /// here — a panic in debug, `inf` in release — and either way the contract's
    /// "finite scores" clause is broken by a value the constructor accepts.
    /// Saturating the sum keeps every score finite; every rank then collapses
    /// onto the same reciprocal, which is the answer a `k` that large asks for.
    #[tokio::test]
    async fn a_k_at_the_top_of_the_range_still_yields_finite_scores() {
        let fusion = ReciprocalRankFusion::new(MAX_K);
        let fused = fuse(
            &fusion,
            vec![
                vec![scored("a", 0.9), scored("b", 0.8)],
                vec![scored("b", 0.7), scored("c", 0.6)],
            ],
        )
        .await;

        for hit in &fused {
            assert!(
                hit.score.is_finite(),
                "{} scored {}",
                hit.chunk.id.as_str(),
                hit.score
            );
        }

        // At `MAX_K` a rank still contributes about 1/2^32 = 2.3283064e-10 --
        // consecutive ranks differ, which is what the bound is chosen to keep,
        // but far below what `f32` can tell apart, so the reported scores of
        // `a` and `c` coincide while their order comes from the `f64` sum.
        // `b`, ranked by both legs, scores twice that.
        assert_eq!(ids(&fused), ["b", "a", "c"]);
        assert_close_relative(fused[0].score, 4.656_613e-10);
        assert_close_relative(fused[1].score, 2.328_306_4e-10);
        assert_close_relative(fused[2].score, 2.328_306_4e-10);
    }

    /// An id repeated **within one leg** is counted once for that leg, at its
    /// first position, and the ranks after it are unaffected.
    /// A single leg comes back in the order it went in — at **every** `k` the
    /// constructor accepts, not only realistic ones.
    ///
    /// That property is the conformance suite's, stated on
    /// `check_fusion_conformance`: "A single leg comes back in the order it
    /// went in." A `k` large enough that consecutive divisors stop being
    /// distinct `f64` values collapses every rank onto one score, the
    /// ascending-id tiebreak takes over, and one leg comes back *sorted* —
    /// reversed, for ids in descending order. The suite does not catch it:
    /// its own fixture ids are already ascending, so the tiebreak happens to
    /// reproduce the input.
    #[tokio::test]
    async fn a_single_leg_keeps_its_order_at_the_largest_k_accepted() {
        let fusion = ReciprocalRankFusion::new(usize::MAX);
        let fused = fuse(
            &fusion,
            vec![vec![scored("z", 0.9), scored("m", 0.5), scored("a", 0.1)]],
        )
        .await;

        assert_eq!(
            ids(&fused),
            ["z", "m", "a"],
            "a leg's own order survives, and is not replaced by an id sort"
        );
    }

    /// `k` beyond the representable range is clamped, not taken literally.
    #[test]
    fn a_k_beyond_the_representable_range_is_clamped() {
        assert_eq!(ReciprocalRankFusion::new(usize::MAX).k(), MAX_K);
        assert_eq!(
            ReciprocalRankFusion::new(60).k(),
            60,
            "an ordinary k is kept"
        );
    }

    /// A chunk named twice by one leg is counted once for that leg.
    ///
    /// The earlier reading was that no guard is needed because a well-behaved
    /// retriever never emits a duplicate. The conformance suite does not
    /// require that of one: `check_no_duplicate_ids` is called from the
    /// `Fusion` and `Reranker` suites only, never from `Retriever` or
    /// `VectorStore`. So a fully conformant leg may repeat an id, and counting
    /// both occurrences roughly doubles that chunk's fused score and floats it
    /// to the top — while the fused output still holds distinct ids and passes
    /// every downstream check. The only symptom is a wrong number.
    #[tokio::test]
    async fn a_chunk_named_twice_by_one_leg_is_counted_once_for_that_leg() {
        let fusion = ReciprocalRankFusion::new(0);
        let fused = fuse(
            &fusion,
            vec![
                vec![scored("dup", 0.9), scored("other", 0.8), scored("dup", 0.7)],
                vec![scored("other", 0.6)],
            ],
        )
        .await;

        // `dup` is ranked first by leg one and nowhere else: 1/1.
        // `other` is ranked second by leg one and first by leg two: 1/2 + 1/1.
        assert_eq!(ids(&fused), ["other", "dup"]);
        assert_close(fused[0].score, 1.5);
        assert_close(fused[1].score, 1.0);
    }

    #[tokio::test]
    async fn an_id_repeated_within_one_leg_counts_only_its_first_position() {
        let fusion = ReciprocalRankFusion::default();
        let fused = fuse(
            &fusion,
            vec![vec![scored("a", 0.9), scored("a", 0.8), scored("b", 0.7)]],
        )
        .await;

        // `a` is counted at its first position only: 1/61, not 1/61 + 1/62.
        // `b` keeps the position it actually occupies -- the skipped duplicate
        // does not shift the ranks after it -- so 1/63.
        assert_eq!(ids(&fused), ["a", "b"]);
        assert_close(fused[0].score, 0.016_393_442);
        assert_close(fused[1].score, 0.015_873_016);
    }

    /// An empty leg is not a rank-0 leg: it contributes nothing, and does not
    /// shift the positions the other legs are read at.
    #[tokio::test]
    async fn an_empty_leg_among_non_empty_ones_contributes_nothing() {
        let fusion = ReciprocalRankFusion::default();
        let fused = fuse(
            &fusion,
            vec![
                Vec::new(),
                vec![scored("a", 0.9), scored("b", 0.8)],
                Vec::new(),
            ],
        )
        .await;

        assert_eq!(ids(&fused), ["a", "b"]);
        assert_close(fused[0].score, 0.016_393_443); // 1/61
        assert_close(fused[1].score, 0.016_129_032); // 1/62
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
