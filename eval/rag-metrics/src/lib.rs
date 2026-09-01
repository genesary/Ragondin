//! Deterministic retrieval metrics.

#[cfg(test)]
mod tests {
    use super::*;
    use rag_types::DocId;
    use std::collections::BTreeMap;

    fn ids(names: &[&str]) -> Vec<DocId> {
        names.iter().map(DocId::new).collect()
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
    fn an_empty_ranking_scores_zero() {
        let empty: Vec<DocId> = Vec::new();
        assert_eq!(ndcg_at_k(&empty, &qrels(), 10), 0.0);
        assert_eq!(recall_at_k(&empty, &qrels(), 10), 0.0);
        assert_eq!(reciprocal_rank(&empty, &qrels()), 0.0);
    }

    #[test]
    fn a_query_with_no_relevant_document_scores_zero() {
        let none: BTreeMap<DocId, u8> = [(DocId::new("d1"), 0)].into_iter().collect();
        let ranked = ids(&["d1", "d2"]);
        assert_eq!(ndcg_at_k(&ranked, &none, 10), 0.0, "IDCG is zero, not NaN");
        assert_eq!(recall_at_k(&ranked, &none, 10), 0.0, "0/0 is reported as 0");
        assert_eq!(reciprocal_rank(&ranked, &none), 0.0);
    }

    #[test]
    fn empty_qrels_score_zero_rather_than_dividing_by_zero() {
        let empty: BTreeMap<DocId, u8> = BTreeMap::new();
        let ranked = ids(&["d1", "d2"]);
        assert!(ndcg_at_k(&ranked, &empty, 10).is_finite());
        assert_eq!(ndcg_at_k(&ranked, &empty, 10), 0.0);
        assert_eq!(recall_at_k(&ranked, &empty, 10), 0.0);
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
    }

    #[test]
    fn k_of_zero_scores_zero() {
        let ranked = ids(&["d1"]);
        assert_eq!(ndcg_at_k(&ranked, &qrels(), 0), 0.0);
        assert_eq!(recall_at_k(&ranked, &qrels(), 0), 0.0);
    }

    #[test]
    fn a_duplicated_document_is_credited_once_by_recall() {
        // A retriever that returns the same id twice must not inflate recall
        // above what it actually found.
        let got = recall_at_k(&ids(&["d1", "d1", "d1"]), &qrels(), 3);
        assert!((got - 1.0 / 3.0).abs() < TOLERANCE, "recall was {got}");
    }

    #[test]
    fn the_same_input_always_yields_the_same_bits() {
        // Determinism is the point (ADR-10): the sum runs in rank order, so
        // repeated evaluation is bit-identical, not merely close.
        let ranked = ids(&["d1", "d2", "d3", "d4", "d5"]);
        let first = ndcg_at_k(&ranked, &qrels(), 10);
        for _ in 0..100 {
            assert_eq!(ndcg_at_k(&ranked, &qrels(), 10).to_bits(), first.to_bits());
        }
    }
}
