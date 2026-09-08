//! [`Fusion`] conformance.

use ragondin_contracts::{Fusion, FusionParams};
use ragondin_types::ScoredChunk;

use crate::{
    checks::{check_no_duplicate_ids, check_no_fabricated_ids, check_ranking},
    failure::ConformanceFailure,
    fixtures::{ids, ranked, scored},
};

const COMPONENT: &str = "Fusion";

/// Checks that `make`'s fusions honour the [`Fusion`] contract.
///
/// Unlike the families whose answers depend on a corpus, a fusion's entire
/// input is the suite's own — so this is the one family where the suite can
/// say what a *correct* answer looks like, and it does:
///
/// - **Fusing nothing yields nothing.** With no ids offered, any result is a
///   fabrication, which is how that case is reported.
/// - **Output ids are a subset of the union of the inputs**, with **no
///   duplicates**. The two legs the suite fuses **share a chunk**, so a fusion
///   that concatenates without merging is caught here — that is a fusion's
///   whole job, and the bug is invisible downstream except as an inflated
///   score.
/// - **A non-empty input yields a non-empty output.** Safe for RRF, CombSUM,
///   CombMNZ and interleaving alike: no corpus, index or model is involved.
/// - **A single leg comes back in the order it went in.** Its scores are
///   distinct, so no tie can arise from the input; a fusion that re-scores is
///   free to change the *scores*, not the *order*.
/// - The **ranking contract** — descending, finite scores.
pub async fn check_fusion_conformance(
    make: impl Fn() -> Box<dyn Fusion>,
) -> Result<(), ConformanceFailure> {
    let fusion = make();

    for (context, inputs) in [
        ("fuse with no input lists", Vec::new()),
        (
            "fuse with two empty input lists",
            vec![Vec::new(), Vec::new()],
        ),
    ] {
        let fused = fusion
            .fuse(inputs, &FusionParams::new())
            .await
            .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
        check_no_fabricated_ids(COMPONENT, context, &fused, &[])?;
    }

    let leg = ranked("leg-a", 3);
    let context = "fuse of a single ranked list";
    let fused = fusion
        .fuse(vec![leg.clone()], &FusionParams::new())
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_no_fabricated_ids(COMPONENT, context, &fused, &ids(&leg))?;
    check_no_duplicate_ids(COMPONENT, context, &fused)?;
    check_non_empty(context, &fused)?;
    check_ranking(COMPONENT, context, &fused)?;
    check_order_preserved(context, &leg, &fused)?;

    // The legs overlap on `shared-0`: a fusion that concatenates rather than
    // merges returns it twice, and nothing but this scenario would notice.
    let left = vec![
        scored("shared-0", 0.9),
        scored("leg-a-1", 0.6),
        scored("leg-a-2", 0.3),
    ];
    let right = vec![scored("shared-0", 0.8), scored("leg-b-1", 0.4)];
    let context = "fuse of two ranked lists sharing a chunk";
    let fused = fusion
        .fuse(vec![left.clone(), right.clone()], &FusionParams::new())
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    let mut union = ids(&left);
    union.extend(ids(&right));
    check_no_fabricated_ids(COMPONENT, context, &fused, &union)?;
    check_no_duplicate_ids(COMPONENT, context, &fused)?;
    check_non_empty(context, &fused)?;
    check_ranking(COMPONENT, context, &fused)
}

fn check_non_empty(context: &str, fused: &[ScoredChunk]) -> Result<(), ConformanceFailure> {
    if fused.is_empty() {
        return Err(ConformanceFailure::new(
            COMPONENT,
            "non-empty input yields output",
            format!("{context}: returned nothing"),
        ));
    }
    Ok(())
}

/// The ids that survived must appear in the order they were given.
///
/// Dropping an input is not checked here — a fusion may legitimately cut a
/// list — so the comparison is against the input restricted to the survivors.
fn check_order_preserved(
    context: &str,
    input: &[ScoredChunk],
    fused: &[ScoredChunk],
) -> Result<(), ConformanceFailure> {
    let got = ids(fused);
    let expected: Vec<&str> = ids(input)
        .into_iter()
        .filter(|id| got.contains(id))
        .collect();
    if got != expected {
        return Err(ConformanceFailure::new(
            COMPONENT,
            "order preserved",
            format!("{context}: returned {got:?}, expected {expected:?}"),
        ));
    }
    Ok(())
}

/// [`check_fusion_conformance`], panicking on the first broken check.
pub async fn assert_fusion_conformance(make: impl Fn() -> Box<dyn Fusion>) {
    if let Err(failure) = check_fusion_conformance(make).await {
        panic!("{failure}");
    }
}
