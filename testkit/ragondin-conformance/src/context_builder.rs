//! [`ContextBuilder`] conformance.

use ragondin_contracts::{ContextBuilder, ContextParams};
use ragondin_types::Context;

use crate::{
    checks::{
        check_identity_non_empty, check_identity_stable, check_invalid_request,
        check_no_duplicate_ids, check_no_fabricated_ids,
    },
    failure::ConformanceFailure,
    fixtures::{ids, query, ranked},
};

const COMPONENT: &str = "ContextBuilder";

/// The budget of every well-formed call. Any non-zero budget is well formed;
/// what it counts is the builder's own, so this one may keep every chunk the
/// suite offers or none of them, and either is conformant.
const BUDGET: usize = 4096;

/// Checks that `make`'s builders honour the [`ContextBuilder`] contract.
///
/// The suite supplies every chunk a builder is handed, so — as for a
/// `Fusion` — it knows the whole of the input the context's provenance must
/// come from:
///
/// - **A well-formed call succeeds**, and that includes **zero chunks**
///   (ADR-C19): the answer is the empty context. Its `chunks` must be empty,
///   which is reported as a fabrication, since with nothing offered any placed
///   chunk is one; its `text` is unconstrained, since a template may render a
///   header over no passages.
/// - **No fabricated ids, and no duplicates** in `Context.chunks`: a builder
///   selects, orders and renders the chunks it was handed, each at most once.
/// - A **zero budget is rejected** as an invalid request, as a zero `top_k`
///   is.
/// - The **identity is non-empty**, and **stable across two calls** — two on
///   one instance, and one on a second instance the same constructor built
///   (ADR-C31 § 4, ADR-C32 § 4). A failure to report one at all is a
///   well-formed call failing.
///
/// **There is deliberately no "budget respected" check.** The budget's unit is
/// the implementation's own — characters, tokens, chunks — and the contract
/// fixes that there is a cap, never what it counts (ADR-C31 § 2). The suite
/// cannot measure a context against a cap whose unit it does not know, so any
/// such check would certify a unit of its own choosing. Nor is there a lower
/// bound: a builder may keep none of the chunks it is handed.
///
/// **Nothing about content.** Neither `text` nor the order of the placed
/// chunks is judged: a suite that does not know the component cannot judge
/// what it rendered.
pub async fn check_context_builder_conformance(
    make: impl Fn() -> Box<dyn ContextBuilder>,
) -> Result<(), ConformanceFailure> {
    let builder = make();
    let query = query();

    let context = "build over zero chunks";
    let built = builder
        .build(&query, Vec::new(), &ContextParams::new(BUDGET))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_no_fabricated_ids(COMPONENT, context, &placed(&built), &[])?;

    let chunks = ranked("passage", 3);
    let context = "build over three chunks";
    let built = builder
        .build(&query, chunks.clone(), &ContextParams::new(BUDGET))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_no_fabricated_ids(COMPONENT, context, &placed(&built), &ids(&chunks))?;
    check_no_duplicate_ids(COMPONENT, context, &placed(&built))?;

    check_invalid_request(
        COMPONENT,
        "zero budget rejected",
        "build with a budget of zero",
        builder
            .build(&query, ranked("passage", 3), &ContextParams::new(0))
            .await,
    )?;

    let context = "model_identity";
    let first = builder
        .model_identity()
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_identity_non_empty(COMPONENT, context, &first)?;

    let context = "model_identity, called again on the same instance";
    let again = builder
        .model_identity()
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_identity_stable(COMPONENT, context, &first, &again)?;

    let context = "model_identity, called on a second instance";
    let other = make()
        .model_identity()
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_identity_stable(COMPONENT, context, &first, &other)
}

/// The ids a context placed, in its order.
fn placed(context: &Context) -> Vec<&str> {
    context
        .chunks
        .iter()
        .map(|chunk| chunk.id.as_str())
        .collect()
}

/// [`check_context_builder_conformance`], panicking on the first broken check.
pub async fn assert_context_builder_conformance(make: impl Fn() -> Box<dyn ContextBuilder>) {
    if let Err(failure) = check_context_builder_conformance(make).await {
        panic!("{failure}");
    }
}
