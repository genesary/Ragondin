//! Deterministic retrieval metrics: nDCG@k, recall@k, precision@k, MRR@k
//! (via [`reciprocal_rank_at_k`]) and MAP@k (via [`average_precision_at_k`]).
//!
//! These are what make M2 defensible **without an LLM judge** (ADR-10): they
//! are computed from *qrels* — relevance judgments prepared in advance — and
//! are fully deterministic. Generation metrics arrive in a later milestone.
//!
//! # What these functions are, and are not
//!
//! Each one scores **a single query**. The figure a benchmark reports —
//! "nDCG@10 on SciFact" — is the **mean over queries**, and computing that mean
//! belongs to the harness. Three of the six functions are named accordingly
//! rather than after the aggregate they feed: [`reciprocal_rank`] and
//! [`reciprocal_rank_at_k`] rather than `mrr`, and [`average_precision_at_k`]
//! rather than `map`, because MRR and MAP are themselves *means of these
//! functions* over a query set, and a function that scores one query cannot be
//! a mean. Averaging must also run in a fixed query order, since floating-point
//! addition is not associative.
//!
//! # Inputs
//!
//! A ranked list of ids, best first, and the qrels **for that one query** — a
//! map from id to graded relevance where **`0` means not relevant**. The qrels
//! type itself belongs to `ragondin-benchmarks`, which produces it; this crate takes
//! a borrowed map so that neither crate depends on the other.
//!
//! Being *judged* and being *relevant* are different: an id present in the map
//! with grade `0` was assessed and found irrelevant, and counts for nothing. An
//! id absent from the map was never assessed, and is likewise treated as
//! irrelevant — the standard closed-world assumption of TREC-style evaluation.
//!
//! # The gain function
//!
//! nDCG here uses **linear gain** with a `log2(i + 1)` discount:
//!
//! ```text
//! DCG@k = sum over ranks i in 1..=k of  rel(i) / log2(i + 1)
//! ```
//!
//! This is the Jarvelin-Kekalainen formulation that `trec_eval` implements and
//! that BEIR reports through `pytrec_eval`, so the figures are comparable to
//! published BEIR leaderboards.
//!
//! **Only the gain function actually matters.** The alternative exponential
//! gain (`2^rel - 1`) gives different numbers on graded qrels and would make
//! the M2 leaderboard-reproduction milestone (#33) incomparable to the
//! literature — so if a published BEIR figure ever
//! fails to reproduce, that is the line to suspect. The *discount base* is not
//! a real choice: nDCG is a ratio, and changing the base scales `DCG` and
//! `IDCG` by the same constant, so it cancels. `log2` is written because it is
//! the conventional statement of the formula, not because the result depends
//! on it. (Verified by mutation: swapping `log2` for `ln` changes nothing, and
//! no test can distinguish them.)
//!
//! # The `k` cutoff, and what it divides by
//!
//! Five of the six functions take a `k` — [`reciprocal_rank`] is the exception,
//! matching `trec_eval`'s uncut `recip_rank`. What differs between the other
//! five is the *denominator*, and getting it wrong is the second most common
//! way to diverge from a published score (after the gain function above). Every
//! choice below is `trec_eval`'s, because the point of this crate is to replay
//! a published MTEB/BEIR evaluation and land on the same number:
//!
//! - **nDCG@k** normalizes by the ideal gain of the **first `k` judged
//!   documents**, not by every judged document — the ideal ranking is
//!   truncated at `k` exactly like the run is.
//! - **recall@k** divides by **every** relevant document in the qrels, not
//!   just the first `k` — that denominator is never truncated.
//! - **precision@k** divides by `k` itself, even when `ranked` has fewer than
//!   `k` elements — a short run is penalized, not rewarded.
//! - **MRR@k** discards anything past rank `k` entirely: a relevant document
//!   beyond the cutoff does not exist as far as the metric is concerned. There
//!   is no denominator to get wrong.
//! - **MAP@k** also discards anything past rank `k` from the *sum*, but its
//!   **divisor is the total number of relevant documents**, untruncated —
//!   like recall@k, not like precision@k. `m_map_cut.c` divides by `num_rel`
//!   regardless of the cutoff, so a perfect top-`k` scores `1.0` only when `k`
//!   reaches every relevant document: with 50 relevant documents, a perfect
//!   top-10 scores `0.2`, not `1.0`.
//!
//! That last one is the trap. The recommender-systems literature commonly
//! divides MAP@k by `min(k, R)` instead, which is a defensible metric but a
//! *different* one, and it inflates every score relative to a BEIR leaderboard
//! figure. `average_precision_divides_by_every_relevant_document_even_when_k_is_smaller`
//! pins this; note that any test with `k >= R` is blind to the difference,
//! since `min(k, R)` then collapses to `R`.
//!
//! # Degenerate inputs
//!
//! Every function returns `0.0` rather than an error or a `NaN`: an empty
//! ranking, a `k` of zero, qrels with nothing relevant.
//!
//! This matches `trec_eval`, which scores such a query `0.0` and **counts it in
//! the mean** — `m_ndcg_cut.c` leaves the value at zero when the ideal gain is
//! zero, and `trec_eval.c` increments its query count unconditionally. So a
//! harness should average over every judged query, this crate's zeros included.
//! (A query with *no qrels line at all* is a different matter: `trec_eval`
//! never evaluates it, because it is unjudged rather than judged-and-empty.)

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use ragondin_types::DocId;

/// The relevance grade of `id`, with an unjudged document counting as `0`.
fn grade(relevance: &BTreeMap<DocId, u8>, id: &DocId) -> u8 {
    relevance.get(id).copied().unwrap_or(0)
}

/// The discount applied at 1-based rank `i`: `log2(i + 1)`.
fn discount(rank: usize) -> f64 {
    ((rank + 1) as f64).log2()
}

/// Normalized discounted cumulative gain over the first `k` results.
///
/// Returns `0.0` when no document is relevant, so the ideal gain is zero and
/// the ratio would otherwise be undefined. See the module documentation for the
/// gain function, which is a deliberate convention choice.
pub fn ndcg_at_k(ranked: &[DocId], relevance: &BTreeMap<DocId, u8>, k: usize) -> f64 {
    // Summed in rank order, so the result is bit-identical run to run. A
    // repeated id is credited once — at its best rank — while still occupying
    // the ranks it takes up, matching `recall_at_k`. Without this a retriever
    // returning one relevant document three times scores above 1.0, which a
    // normalized metric must never do. `trec_eval` rejects such a run outright
    // (`form_res_rels.c`, "duplicate docs"); this crate has no error channel,
    // so it absorbs the duplicate rather than inflating the score. Detecting
    // it belongs upstream, in the conformance suite.
    let mut seen: BTreeSet<&DocId> = BTreeSet::new();
    let dcg: f64 = ranked
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, id)| {
            if seen.insert(id) {
                f64::from(grade(relevance, id)) / discount(i + 1)
            } else {
                0.0
            }
        })
        .sum();

    // Grade-0 documents are filtered for clarity, not correctness: they sort
    // last and contribute nothing, so they can never displace a relevant
    // document inside the cut.
    let mut ideal: Vec<u8> = relevance.values().copied().filter(|g| *g > 0).collect();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    let idcg: f64 = ideal
        .into_iter()
        .take(k)
        .enumerate()
        .map(|(i, g)| f64::from(g) / discount(i + 1))
        .sum();

    if idcg == 0.0 {
        0.0
    } else {
        // `+ 0.0` normalizes negative zero: `f64::sum` folds from `-0.0`, so an
        // empty cut yields `-0.0`, and `-0.0 / x` stays `-0.0`. It compares
        // equal to `0.0`, so no assertion would catch it — but it is what gets
        // serialized into the run store.
        dcg / idcg + 0.0
    }
}

/// The fraction of relevant documents that appear in the first `k` results.
///
/// A document repeated in `ranked` is credited once. Returns `0.0` when nothing
/// is relevant.
pub fn recall_at_k(ranked: &[DocId], relevance: &BTreeMap<DocId, u8>, k: usize) -> f64 {
    let total = relevance.values().filter(|g| **g > 0).count();
    if total == 0 {
        return 0.0;
    }
    let found: BTreeSet<&DocId> = ranked
        .iter()
        .take(k)
        .filter(|id| grade(relevance, id) > 0)
        .collect();
    found.len() as f64 / total as f64
}

/// The fraction of the first `k` results that are relevant.
///
/// The denominator is always `k`, even when `ranked` has fewer than `k`
/// elements — matching `trec_eval`'s `P_k`, which penalizes a run for
/// returning too few results rather than rewarding it. A document repeated
/// in `ranked` is credited once. Returns `0.0` when `k` is `0`.
pub fn precision_at_k(ranked: &[DocId], relevance: &BTreeMap<DocId, u8>, k: usize) -> f64 {
    if k == 0 {
        return 0.0;
    }
    let found: BTreeSet<&DocId> = ranked
        .iter()
        .take(k)
        .filter(|id| grade(relevance, id) > 0)
        .collect();
    found.len() as f64 / k as f64
}

/// The reciprocal of the rank of the first relevant document, or `0.0` if none
/// appears.
///
/// **This is not MRR.** MRR is the mean of this over a query set; see the module
/// documentation.
///
/// There is no `k`, matching `trec_eval`'s uncut `recip_rank`. For a cutoff,
/// use [`reciprocal_rank_at_k`].
pub fn reciprocal_rank(ranked: &[DocId], relevance: &BTreeMap<DocId, u8>) -> f64 {
    ranked
        .iter()
        .position(|id| grade(relevance, id) > 0)
        .map_or(0.0, |i| 1.0 / (i + 1) as f64)
}

/// [`reciprocal_rank`], but only the first `k` results are considered — a
/// relevant document past the cutoff does not count. Returns `0.0` when `k`
/// is `0`. This is the per-query value that MRR@k averages over a query set.
pub fn reciprocal_rank_at_k(ranked: &[DocId], relevance: &BTreeMap<DocId, u8>, k: usize) -> f64 {
    let cutoff = k.min(ranked.len());
    reciprocal_rank(&ranked[..cutoff], relevance)
}

/// Average precision over the first `k` results: the sum of
/// [`precision_at_k`]-shaped values taken at each rank where a *new* relevant
/// document appears, divided by the **total** number of relevant documents in
/// the qrels.
///
/// The divisor is deliberately *not* `min(k, number of relevant documents)`.
/// `trec_eval`'s `m_map_cut.c` divides by `num_rel` whatever the cutoff, so a
/// perfect top-`k` scores `1.0` only when `k` reaches every relevant document;
/// with 50 relevant documents, a perfect top-10 scores `0.2`. The `min(k, R)`
/// variant is the recommender-systems convention and would inflate every
/// MAP@k this crate produces relative to a published BEIR/MTEB figure. See the
/// module documentation.
///
/// **This is not MAP.** MAP@k is the mean of this over a query set — the
/// same relationship [`reciprocal_rank`] has to MRR.
///
/// A document repeated in `ranked` is credited at most once, at its first
/// occurrence; a later repeat contributes nothing to the sum, even though it
/// is relevant, because it is not a *new* relevant document found. Returns
/// `0.0` when `k` is `0` or when nothing in `relevance` is relevant.
pub fn average_precision_at_k(ranked: &[DocId], relevance: &BTreeMap<DocId, u8>, k: usize) -> f64 {
    let total_relevant = relevance.values().filter(|g| **g > 0).count();
    if k == 0 || total_relevant == 0 {
        return 0.0;
    }
    let mut seen: BTreeSet<&DocId> = BTreeSet::new();
    let mut relevant_found: usize = 0;
    let sum: f64 = ranked
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, id)| {
            if grade(relevance, id) > 0 && seen.insert(id) {
                relevant_found += 1;
                relevant_found as f64 / (i + 1) as f64
            } else {
                0.0
            }
        })
        .sum();
    // `+ 0.0` normalizes negative zero, exactly as `ndcg_at_k` does: `f64::sum`
    // folds from `-0.0`, so a cut containing no relevant document yields
    // `-0.0`, and `-0.0 / x` stays `-0.0`.
    sum / total_relevant as f64 + 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use ragondin_types::DocId;
    use std::collections::BTreeMap;

    fn ids(names: &[&str]) -> Vec<DocId> {
        names.iter().copied().map(DocId::new).collect()
    }

    /// d1 is highly relevant, d6 is relevant but never retrieved, d4 is
    /// marginally relevant, d2 is judged and *not* relevant.
    fn qrels() -> BTreeMap<DocId, u8> {
        [("d1", 3), ("d2", 0), ("d4", 1), ("d6", 2)]
            .into_iter()
            .map(|(id, grade)| (DocId::new(id), grade))
            .collect()
    }

    const TOLERANCE: f64 = 1e-9;

    /// The six public functions, in the order the determinism test scores
    /// them, so a failure names the culprit rather than an index.
    const METRIC_NAMES: [&str; 6] = [
        "ndcg_at_k",
        "recall_at_k",
        "precision_at_k",
        "reciprocal_rank",
        "reciprocal_rank_at_k",
        "average_precision_at_k",
    ];

    #[test]
    fn ndcg_matches_a_hand_computed_value() {
        // Ranking: d1 d2 d3 d4 d5, cut at 5. Linear gain, log2(i+1) discount.
        //
        //   DCG  = 3/log2(2) + 0 + 0 + 1/log2(5) + 0
        //        = 3 + 0.430676558073393  = 3.430676558073393
        //
        // Ideal ranking of the judged documents is 3, 2, 1:
        //   IDCG = 3/log2(2) + 2/log2(3) + 1/log2(4)
        //        = 3 + 1.261859507142915 + 0.5 = 4.761859507142915
        //
        //   nDCG = 3.430676558073393 / 4.761859507142915
        let got = ndcg_at_k(&ids(&["d1", "d2", "d3", "d4", "d5"]), &qrels(), 5);
        let expected = 3.430676558073393 / 4.761859507142915;
        assert!(
            (got - expected).abs() < TOLERANCE,
            "nDCG@5 was {got}, expected {expected}"
        );
    }

    #[test]
    fn a_perfect_ranking_scores_one() {
        // Every judged document retrieved in ideal order.
        let got = ndcg_at_k(&ids(&["d1", "d6", "d4", "d2"]), &qrels(), 4);
        assert!((got - 1.0).abs() < TOLERANCE, "expected 1.0, got {got}");
    }

    #[test]
    fn ndcg_is_cut_at_k() {
        // d4 sits at rank 4, so cutting at 3 must drop its contribution.
        let ranked = ids(&["d1", "d2", "d3", "d4", "d5"]);
        let at_3 = ndcg_at_k(&ranked, &qrels(), 3);
        let expected = 3.0 / (3.0 + 1.261859507142915 + 0.5);
        assert!(
            (at_3 - expected).abs() < TOLERANCE,
            "nDCG@3 was {at_3}, expected {expected}"
        );
    }

    #[test]
    fn recall_counts_relevant_documents_retrieved() {
        // Three documents are relevant (d1, d4, d6); d2 is judged 0 and so is
        // not. Two of the three appear in the top 5.
        let got = recall_at_k(&ids(&["d1", "d2", "d3", "d4", "d5"]), &qrels(), 5);
        assert!((got - 2.0 / 3.0).abs() < TOLERANCE, "recall@5 was {got}");
    }

    #[test]
    fn recall_is_cut_at_k() {
        let got = recall_at_k(&ids(&["d1", "d2", "d3", "d4", "d5"]), &qrels(), 3);
        assert!((got - 1.0 / 3.0).abs() < TOLERANCE, "recall@3 was {got}");
    }

    #[test]
    fn reciprocal_rank_finds_the_first_relevant_document() {
        // d2 is judged but not relevant, d3 is unjudged: the first relevant
        // document is d1, at rank 3.
        let got = reciprocal_rank(&ids(&["d2", "d3", "d1", "d4"]), &qrels());
        assert!((got - 1.0 / 3.0).abs() < TOLERANCE, "RR was {got}");
    }

    #[test]
    fn a_judged_but_irrelevant_document_does_not_count_as_a_hit() {
        // The distinction the whole metric rests on: `d2` is *in* the qrels,
        // with grade 0. Treating "present in qrels" as "relevant" would score
        // this 1.0.
        let got = reciprocal_rank(&ids(&["d2", "d1"]), &qrels());
        assert!((got - 0.5).abs() < TOLERANCE, "RR was {got}");
    }

    #[test]
    fn reciprocal_rank_at_k_matches_reciprocal_rank_when_k_is_not_a_binding_cutoff() {
        // d1 is the first relevant document, at rank 3; k=4 does not cut it off.
        let ranked = ids(&["d2", "d3", "d1", "d4"]);
        let got = reciprocal_rank_at_k(&ranked, &qrels(), 4);
        assert!((got - 1.0 / 3.0).abs() < TOLERANCE, "RR@4 was {got}");
    }

    #[test]
    fn reciprocal_rank_at_k_ignores_a_relevant_document_past_the_cutoff() {
        // d1 (relevant) sits at rank 3, but the cutoff is 2: it must not count.
        let ranked = ids(&["d2", "d3", "d1", "d4"]);
        let got = reciprocal_rank_at_k(&ranked, &qrels(), 2);
        assert_eq!(
            got, 0.0,
            "RR@2 was {got}, expected 0.0 — d1 is past the cutoff"
        );
    }

    #[test]
    fn reciprocal_rank_at_k_larger_than_the_ranking_behaves_like_the_uncut_version() {
        let ranked = ids(&["d2", "d3", "d1", "d4"]);
        assert_eq!(
            reciprocal_rank_at_k(&ranked, &qrels(), 500),
            reciprocal_rank(&ranked, &qrels())
        );
    }

    #[test]
    fn reciprocal_rank_at_k_of_zero_scores_zero() {
        let ranked = ids(&["d1"]);
        assert_eq!(reciprocal_rank_at_k(&ranked, &qrels(), 0), 0.0);
    }

    #[test]
    fn precision_matches_a_hand_computed_value() {
        // Top 5: d1 (relevant) and d4 (relevant) are the hits; d2, d3, d5 are not.
        let got = precision_at_k(&ids(&["d1", "d2", "d3", "d4", "d5"]), &qrels(), 5);
        assert!(
            (got - 0.4).abs() < TOLERANCE,
            "precision@5 was {got}, expected 0.4"
        );
    }

    #[test]
    fn precision_is_cut_at_k() {
        // Top 3 only: d1 is the sole hit among d1, d2, d3.
        let got = precision_at_k(&ids(&["d1", "d2", "d3", "d4", "d5"]), &qrels(), 3);
        assert!(
            (got - 1.0 / 3.0).abs() < TOLERANCE,
            "precision@3 was {got}, expected {}",
            1.0 / 3.0
        );
    }

    #[test]
    fn precision_denominator_is_always_k_even_if_the_run_returns_fewer_documents() {
        // Only one document is returned, but the cutoff is 5: trec_eval's
        // convention divides by k regardless, so this is 1/5, not 1/1.
        let got = precision_at_k(&ids(&["d1"]), &qrels(), 5);
        assert!(
            (got - 0.2).abs() < TOLERANCE,
            "precision@5 was {got}, expected 0.2"
        );
    }

    #[test]
    fn a_duplicated_document_is_credited_once_by_precision() {
        let got = precision_at_k(&ids(&["d1", "d1", "d1"]), &qrels(), 3);
        assert!(
            (got - 1.0 / 3.0).abs() < TOLERANCE,
            "precision@3 was {got}, expected {}",
            1.0 / 3.0
        );
    }

    #[test]
    fn precision_at_a_cutoff_past_the_ranking_length_still_divides_by_k() {
        // Only 5 documents are returned; asking for precision@500 means "divide
        // the (at most 5) hits by 500", not "treat 500 as if it were 5".
        let ranked = ids(&["d1", "d2", "d3", "d4", "d5"]);
        let at_500 = precision_at_k(&ranked, &qrels(), 500);
        assert!(
            (at_500 - 2.0 / 500.0).abs() < TOLERANCE,
            "precision@500 was {at_500}, expected {}",
            2.0 / 500.0
        );
    }

    #[test]
    fn average_precision_matches_a_hand_computed_value() {
        // d1 is relevant but sits at rank 3, past the k=2 cutoff: it does not
        // contribute to the sum. So the sum is 1/1 + 2/2 = 2, and the divisor
        // is the *total* number of relevant documents, 3 — not the 2 that were
        // reachable within the cut.
        let got = average_precision_at_k(&ids(&["d4", "d6", "d1"]), &qrels(), 2);
        assert!(
            (got - 2.0 / 3.0).abs() < TOLERANCE,
            "AP@2 was {got}, expected {}",
            2.0 / 3.0
        );
    }

    #[test]
    fn average_precision_ignores_a_relevant_document_past_the_cutoff() {
        // Same ranking as the hand-computed test above, but cut at k=1: only
        // d4 counts, so the sum is 1/1 and AP@1 is 1/3. If d1 (rank 3) leaked
        // past the cutoff, the sum would pick up a second term.
        let got = average_precision_at_k(&ids(&["d4", "d6", "d1"]), &qrels(), 1);
        assert!(
            (got - 1.0 / 3.0).abs() < TOLERANCE,
            "AP@1 was {got}, expected {}",
            1.0 / 3.0
        );
    }

    #[test]
    fn average_precision_is_zero_when_no_relevant_document_is_retrieved() {
        // d1, d4 and d6 are relevant, but none of them appear in the ranking:
        // the sum is 0, and the function must not divide 0/0 or panic.
        let got = average_precision_at_k(&ids(&["d2", "d3", "d5"]), &qrels(), 3);
        assert_eq!(got, 0.0, "AP@3 was {got}, expected 0.0");
    }

    #[test]
    fn average_precision_only_credits_ranks_where_a_new_relevant_document_appears() {
        // Rank 1 (d1) and rank 4 (d4) are hits; d2, d3, d5 contribute nothing.
        let got = average_precision_at_k(&ids(&["d1", "d2", "d3", "d4", "d5"]), &qrels(), 5);
        assert!(
            (got - 0.5).abs() < TOLERANCE,
            "AP@5 was {got}, expected 0.5"
        );
    }

    #[test]
    fn average_precision_divides_by_every_relevant_document_even_when_k_is_smaller() {
        // The test that pins the divisor, and the one convention mistake that
        // silently inflates a published MAP@k. Three documents are relevant
        // (d1, d4, d6) and the top 2 are a *perfect* prefix: sum = 1/1 + 2/2 = 2.
        //
        //   trec_eval (`m_map_cut.c`): 2 / 3     = 0.6666…  ← what we implement
        //   RecSys min(k, R):          2 / min(2,3) = 1.0
        //
        // A perfect top-k when k < R must NOT score 1.0: the run was never
        // given room to find every relevant document, and trec_eval charges it
        // for the ones it could not reach. Any test with k >= R is blind here,
        // because min(k, R) collapses to R.
        let got = average_precision_at_k(&ids(&["d1", "d6"]), &qrels(), 2);
        assert!(
            (got - 2.0 / 3.0).abs() < TOLERANCE,
            "AP@2 was {got}, expected {} — a perfect top-2 out of 3 relevant \
             documents is 2/3 under trec_eval, not 1.0",
            2.0 / 3.0
        );
    }

    #[test]
    fn a_degenerate_average_precision_is_positive_zero() {
        // `f64::sum` folds from `-0.0`, and `-0.0 / 3.0` stays `-0.0`. It
        // compares equal to `0.0`, so no `assert_eq!` above can see it — but it
        // is what gets serialized into the run store.
        let empty: Vec<DocId> = Vec::new();
        assert_eq!(
            average_precision_at_k(&empty, &qrels(), 10).to_bits(),
            0.0f64.to_bits()
        );
        assert_eq!(
            average_precision_at_k(&ids(&["d2", "d3"]), &qrels(), 2).to_bits(),
            0.0f64.to_bits()
        );
    }

    #[test]
    fn a_duplicated_document_is_credited_once_by_average_precision() {
        let got = average_precision_at_k(&ids(&["d1", "d1", "d1"]), &qrels(), 3);
        assert!(
            (got - 1.0 / 3.0).abs() < TOLERANCE,
            "AP@3 was {got}, expected {}",
            1.0 / 3.0
        );
    }

    #[test]
    fn an_empty_ranking_scores_zero() {
        let empty: Vec<DocId> = Vec::new();
        assert_eq!(ndcg_at_k(&empty, &qrels(), 10), 0.0);
        assert_eq!(recall_at_k(&empty, &qrels(), 10), 0.0);
        assert_eq!(reciprocal_rank(&empty, &qrels()), 0.0);
        assert_eq!(reciprocal_rank_at_k(&empty, &qrels(), 10), 0.0);
        assert_eq!(precision_at_k(&empty, &qrels(), 10), 0.0);
        assert_eq!(average_precision_at_k(&empty, &qrels(), 10), 0.0);
    }

    #[test]
    fn a_query_with_no_relevant_document_scores_zero() {
        let none: BTreeMap<DocId, u8> = [(DocId::new("d1"), 0)].into_iter().collect();
        let ranked = ids(&["d1", "d2"]);
        assert_eq!(ndcg_at_k(&ranked, &none, 10), 0.0, "IDCG is zero, not NaN");
        assert_eq!(recall_at_k(&ranked, &none, 10), 0.0, "0/0 is reported as 0");
        assert_eq!(reciprocal_rank(&ranked, &none), 0.0);
        assert_eq!(reciprocal_rank_at_k(&ranked, &none, 10), 0.0);
        assert_eq!(
            precision_at_k(&ranked, &none, 10),
            0.0,
            "0 relevant found / 10"
        );
        assert_eq!(
            average_precision_at_k(&ranked, &none, 10),
            0.0,
            "no relevant documents at all"
        );
    }

    #[test]
    fn empty_qrels_score_zero_rather_than_dividing_by_zero() {
        let empty: BTreeMap<DocId, u8> = BTreeMap::new();
        let ranked = ids(&["d1", "d2"]);
        assert!(ndcg_at_k(&ranked, &empty, 10).is_finite());
        assert_eq!(ndcg_at_k(&ranked, &empty, 10), 0.0);
        assert_eq!(recall_at_k(&ranked, &empty, 10), 0.0);
        assert_eq!(reciprocal_rank(&ranked, &empty), 0.0);
        assert_eq!(reciprocal_rank_at_k(&ranked, &empty, 10), 0.0);
        assert_eq!(precision_at_k(&ranked, &empty, 10), 0.0);
        assert_eq!(average_precision_at_k(&ranked, &empty, 10), 0.0);
    }

    #[test]
    fn k_larger_than_the_ranking_uses_the_whole_ranking() {
        let ranked = ids(&["d1", "d2", "d3", "d4", "d5"]);
        assert_eq!(
            ndcg_at_k(&ranked, &qrels(), 5),
            ndcg_at_k(&ranked, &qrels(), 500)
        );
        assert_eq!(
            recall_at_k(&ranked, &qrels(), 5),
            recall_at_k(&ranked, &qrels(), 500)
        );
        assert_eq!(
            reciprocal_rank_at_k(&ranked, &qrels(), 5),
            reciprocal_rank_at_k(&ranked, &qrels(), 500)
        );
        assert_eq!(
            average_precision_at_k(&ranked, &qrels(), 5),
            average_precision_at_k(&ranked, &qrels(), 500)
        );
        // `precision_at_k` is deliberately absent: it divides by `k` itself,
        // so a `k` past the ranking length lowers the score rather than
        // leaving it unchanged. That is the documented convention, not an
        // inconsistency.
    }

    #[test]
    fn k_of_zero_scores_zero() {
        let ranked = ids(&["d1"]);
        assert_eq!(ndcg_at_k(&ranked, &qrels(), 0), 0.0);
        assert_eq!(recall_at_k(&ranked, &qrels(), 0), 0.0);
        assert_eq!(reciprocal_rank_at_k(&ranked, &qrels(), 0), 0.0);
        assert_eq!(precision_at_k(&ranked, &qrels(), 0), 0.0);
        assert_eq!(average_precision_at_k(&ranked, &qrels(), 0), 0.0);
    }

    #[test]
    fn a_duplicated_document_is_credited_once_by_recall() {
        // A retriever that returns the same id twice must not inflate recall
        // above what it actually found.
        let got = recall_at_k(&ids(&["d1", "d1", "d1"]), &qrels(), 3);
        assert!((got - 1.0 / 3.0).abs() < TOLERANCE, "recall was {got}");
    }

    #[test]
    fn ndcg_cut_below_the_number_of_relevant_documents() {
        // The case nDCG@10 hits on most BEIR datasets: more relevant documents
        // exist than the cutoff admits, so the ideal gain must also be
        // truncated at k. Truncating it at the ranking length instead gives
        // 0.4749950106150897 — a 10% error that no other test here can see.
        //
        //   DCG@2  = 1/log2(2) + 2/log2(3) = 1 + 1.261859507142915
        //   IDCG@2 = 3/log2(2) + 2/log2(3) = 3 + 1.261859507142915
        //
        // Cross-checked against pytrec_eval, the library BEIR evaluates with.
        let got = ndcg_at_k(&ids(&["d4", "d6", "d1"]), &qrels(), 2);
        let expected = 0.5307212739772434;
        assert!(
            (got - expected).abs() < TOLERANCE,
            "nDCG@2 was {got}, expected {expected}"
        );
    }

    #[test]
    fn a_duplicated_document_is_credited_once_by_ndcg_too() {
        // A normalized metric may never exceed 1. Crediting a repeat would
        // score this 1.3425 — DCG 6.392789260714372 over IDCG@3
        // 4.761859507142915. The duplicate still occupies its rank; it simply
        // gains nothing.
        let got = ndcg_at_k(&ids(&["d1", "d1", "d1"]), &qrels(), 3);
        assert!(got <= 1.0, "nDCG exceeded 1.0: {got}");
        let expected = 3.0 / (3.0 + 1.261859507142915 + 0.5);
        assert!(
            (got - expected).abs() < TOLERANCE,
            "nDCG was {got}, expected {expected}"
        );
    }

    #[test]
    fn a_degenerate_ndcg_is_positive_zero() {
        // `-0.0 == 0.0`, so no assert_eq! can see this — but it is what gets
        // written into the run store.
        let empty: Vec<DocId> = Vec::new();
        assert_eq!(ndcg_at_k(&empty, &qrels(), 10).to_bits(), 0.0f64.to_bits());
        assert_eq!(
            ndcg_at_k(&ids(&["d1"]), &qrels(), 0).to_bits(),
            0.0f64.to_bits()
        );
    }

    #[test]
    fn the_same_input_always_yields_the_same_bits() {
        // Determinism is the point (ADR-10): every sum runs in rank order, so
        // repeated evaluation is bit-identical, not merely close. ADR-10 claims
        // this of the crate, not of one function, so all six are checked.
        let ranked = ids(&["d1", "d2", "d3", "d4", "d5"]);
        let score_all = || {
            [
                ndcg_at_k(&ranked, &qrels(), 10),
                recall_at_k(&ranked, &qrels(), 10),
                precision_at_k(&ranked, &qrels(), 10),
                reciprocal_rank(&ranked, &qrels()),
                reciprocal_rank_at_k(&ranked, &qrels(), 10),
                average_precision_at_k(&ranked, &qrels(), 10),
            ]
        };
        let first = score_all();
        for _ in 0..100 {
            for (name, (a, b)) in METRIC_NAMES.iter().zip(first.iter().zip(score_all())) {
                assert_eq!(a.to_bits(), b.to_bits(), "{name} is not bit-stable");
            }
        }
    }
}
