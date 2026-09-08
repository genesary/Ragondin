//! [`Reranker`] conformance.

use ragondin_contracts::{RerankParams, Reranker};

use crate::{
    checks::{check_no_fabricated_ids, check_ranking, check_top_k, check_zero_top_k_rejected},
    failure::ConformanceFailure,
    fixtures::{ids, query, ranked},
};

const COMPONENT: &str = "Reranker";

/// Checks that `make`'s rerankers honour the [`Reranker`] contract.
///
/// - **No fabricated ids**: the output is a subset of the chunks handed in. A
///   reranker reorders; it has no corpus of its own to draw from.
/// - **At most `top_k`** results.
/// - The **ranking contract** — descending, finite scores.
/// - **Nothing in, nothing out**: an empty chunk list reranks to an empty
///   list, not to an error.
/// - A **`top_k` of zero is rejected** as an invalid request.
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
    // Fabrication first, for the same reason as in the fusion suite.
    check_no_fabricated_ids(COMPONENT, context, &reordered, &[])?;
    if !reordered.is_empty() {
        return Err(ConformanceFailure::new(
            COMPONENT,
            "empty input yields empty output",
            format!("{context}: returned {} results", reordered.len()),
        ));
    }

    let chunks = ranked("candidate", 3);
    let context = "rerank with top_k=2";
    let reordered = reranker
        .rerank(&query, chunks.clone(), &RerankParams::new(2))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_no_fabricated_ids(COMPONENT, context, &reordered, &ids(&chunks))?;
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
