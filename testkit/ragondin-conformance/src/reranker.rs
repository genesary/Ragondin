//! [`Reranker`] conformance.

use ragondin_contracts::{RerankParams, Reranker};

use crate::{
    checks::{
        check_identity_non_empty, check_identity_stable, check_no_duplicate_ids,
        check_no_fabricated_ids, check_ranking, check_top_k, check_zero_top_k_rejected,
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
/// - The **identity of `served_model` is non-empty**, and **stable across two
///   calls** — two on one instance, and one on a second instance the same
///   constructor built (ADR-C31 § 4, ADR-C32 § 4). A failure to report one is
///   a well-formed call failing.
///
/// `served_model` is what every call the suite makes asks for — in its
/// [`RerankParams`] and in `model_identity` — and must be what the fixture
/// serves: `None` for a `Local` reranker that loaded one model, a name for a
/// `Remote` one, which refuses `None` (ADR-C32 § 4). The suite cannot know
/// which, so the caller states it, as it states a generator's served model; a
/// wrong statement fails `well-formed call succeeds`. Nor does the suite ask
/// for a model the fixture does not serve: no name is one it could know every
/// fixture refuses.
///
/// **There is deliberately no lower bound.** A reranker that returns fewer
/// results than it was given — or none at all — is conformant: a cross-encoder
/// with a score threshold may legitimately reject every candidate, and
/// certainly may reject the suite's synthetic text. Nothing in
/// `ragondin-contracts` obliges a reranker to return anything, so nothing here
/// does either.
pub async fn check_reranker_conformance(
    make: impl Fn() -> Box<dyn Reranker>,
    served_model: Option<&str>,
) -> Result<(), ConformanceFailure> {
    let reranker = make();
    let query = query();
    let params = |top_k| match served_model {
        None => RerankParams::new(top_k),
        Some(name) => RerankParams::new(top_k).with_served_model(name),
    };

    let context = "rerank of an empty chunk list";
    let reordered = reranker
        .rerank(&query, Vec::new(), &params(5))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_no_fabricated_ids(COMPONENT, context, &ids(&reordered), &[])?;

    let chunks = ranked("candidate", 3);
    let context = "rerank with top_k=2";
    let reordered = reranker
        .rerank(&query, chunks.clone(), &params(2))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_no_fabricated_ids(COMPONENT, context, &ids(&reordered), &ids(&chunks))?;
    check_no_duplicate_ids(COMPONENT, context, &ids(&reordered))?;
    check_top_k(COMPONENT, context, &reordered, 2)?;
    check_ranking(COMPONENT, context, &reordered)?;

    let reranker = make();
    check_zero_top_k_rejected(
        COMPONENT,
        "rerank with top_k=0",
        reranker
            .rerank(&query, ranked("candidate", 3), &params(0))
            .await,
    )?;

    let context = format!("model_identity({served_model:?})");
    let first = reranker
        .model_identity(served_model)
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, &context, &error))?;
    check_identity_non_empty(COMPONENT, &context, &first)?;

    let context = format!("model_identity({served_model:?}), called again on the same instance");
    let again = reranker
        .model_identity(served_model)
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, &context, &error))?;
    check_identity_stable(COMPONENT, &context, &first, &again)?;

    let context = format!("model_identity({served_model:?}), called on a second instance");
    let other = make()
        .model_identity(served_model)
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, &context, &error))?;
    check_identity_stable(COMPONENT, &context, &first, &other)
}

/// [`check_reranker_conformance`], panicking on the first broken check.
pub async fn assert_reranker_conformance(
    make: impl Fn() -> Box<dyn Reranker>,
    served_model: Option<&str>,
) {
    if let Err(failure) = check_reranker_conformance(make, served_model).await {
        panic!("{failure}");
    }
}
