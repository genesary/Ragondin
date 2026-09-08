//! [`Reranker`] conformance.

use ragondin_contracts::{RerankParams, Reranker};

use crate::{
    checks::{
        check_no_duplicate_ids, check_no_fabricated_ids, check_ranking, check_top_k,
        check_zero_top_k_rejected,
    },
    failure::ConformanceFailure,
    fixtures::{ids, query, ranked},
};

const COMPONENT: &str = "Reranker";

/// Checks that `make`'s rerankers honour the [`Reranker`] contract.
///
/// - **No fabricated ids, and no duplicates**: the output is a subset of the
///   chunks handed in, each at most once. A reranker reorders; it has no
///   corpus of its own to draw from. With an empty input that same check is
///   what reports any output at all.
/// - **At most `top_k`** results, and the **ranking contract** — descending,
///   finite scores.
/// - A **`top_k` of zero is rejected** as an invalid request.
///
/// **There is deliberately no lower bound.** A reranker that returns fewer
/// results than it was given — or none at all — is conformant: a cross-encoder
/// with a score threshold may legitimately reject every candidate, and
/// certainly may reject the suite's synthetic text. Nothing in
/// `ragondin-contracts` obliges a reranker to return anything, so nothing here
/// does either.
pub async fn check_reranker_conformance(
    make: impl Fn() -> Box<dyn Reranker>,
) -> Result<(), ConformanceFailure> {
    let reranker = make();
    let query = query();

    let context = "rerank of an empty chunk list";
    let reordered = reranker
        .rerank(&query, Vec::new(), &RerankParams::new(5))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_no_fabricated_ids(COMPONENT, context, &reordered, &[])?;

    let chunks = ranked("candidate", 3);
    let context = "rerank with top_k=2";
    let reordered = reranker
        .rerank(&query, chunks.clone(), &RerankParams::new(2))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_no_fabricated_ids(COMPONENT, context, &reordered, &ids(&chunks))?;
    check_no_duplicate_ids(COMPONENT, context, &reordered)?;
    check_top_k(COMPONENT, context, &reordered, 2)?;
    check_ranking(COMPONENT, context, &reordered)?;

    let reranker = make();
    check_zero_top_k_rejected(
        COMPONENT,
        "rerank with top_k=0",
        reranker
            .rerank(&query, ranked("candidate", 3), &RerankParams::new(0))
            .await,
    )
}

/// [`check_reranker_conformance`], panicking on the first broken check.
pub async fn assert_reranker_conformance(make: impl Fn() -> Box<dyn Reranker>) {
    if let Err(failure) = check_reranker_conformance(make).await {
        panic!("{failure}");
    }
}
