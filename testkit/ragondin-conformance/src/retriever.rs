//! [`Retriever`] conformance.

use ragondin_contracts::{RetrieveParams, Retriever};

use crate::{
    checks::{check_ranking, check_top_k, check_zero_top_k_rejected},
    failure::ConformanceFailure,
    fixtures::query,
};

const COMPONENT: &str = "Retriever";

/// Checks that `make`'s retrievers honour the [`Retriever`] contract.
///
/// The suite knows nothing about the corpus a retriever was built over, so it
/// asserts only what holds whatever that corpus contains:
///
/// - **at most `top_k` results**, for several values of `top_k`;
/// - the **ranking contract** — descending, finite scores;
/// - a **well-formed query never fails**. This is where "an empty corpus
///   yields an empty result, not an error" lands: a retriever with nothing to
///   return says so with an empty list, and the suite reaches that case
///   naturally, since its query matches nothing in a real corpus.
/// - a **`top_k` of zero is rejected** as an invalid request.
///
/// It never asserts that results are *non-empty*: no query the suite could
/// invent is guaranteed to match a corpus it has never seen.
pub async fn check_retriever_conformance(
    make: impl Fn() -> Box<dyn Retriever>,
) -> Result<(), ConformanceFailure> {
    let retriever = make();
    let query = query();

    for top_k in [1usize, 5] {
        let context = format!("retrieve with top_k={top_k}");
        let hits = retriever
            .retrieve(&query, &RetrieveParams::new(top_k))
            .await
            .map_err(|error| ConformanceFailure::from_call(COMPONENT, &context, &error))?;
        check_top_k(COMPONENT, &context, &hits, top_k)?;
        check_ranking(COMPONENT, &context, &hits)?;
    }

    let retriever = make();
    check_zero_top_k_rejected(
        COMPONENT,
        "retrieve with top_k=0",
        retriever.retrieve(&query, &RetrieveParams::new(0)).await,
    )
}

/// [`check_retriever_conformance`], panicking on the first broken check.
///
/// The form a component crate calls from its own test.
pub async fn assert_retriever_conformance(make: impl Fn() -> Box<dyn Retriever>) {
    if let Err(failure) = check_retriever_conformance(make).await {
        panic!("{failure}");
    }
}
