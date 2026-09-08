//! [`Fusion`] conformance.

use ragondin_contracts::{Fusion, FusionParams};
use ragondin_types::ScoredChunk;

use crate::{
    checks::{check_no_fabricated_ids, check_ranking},
    failure::ConformanceFailure,
    fixtures::{ids, ranked},
};

const COMPONENT: &str = "Fusion";

/// Checks that `make`'s fusions honour the [`Fusion`] contract.
///
/// - **Fusing nothing yields nothing**: no input lists, or only empty ones,
///   fuses to an empty list rather than to an error.
/// - **Output ids are a subset of the union of the inputs.** A fusion merges;
///   it never introduces a chunk no upstream leg produced.
/// - **A single list is order-preserving up to ties**: with one leg of
///   strictly descending scores, the surviving ids come back in the order they
///   went in.
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
        // Fabrication first: with nothing offered, *any* result is invented,
        // and that is the more precise diagnosis of the two.
        check_no_fabricated_ids(COMPONENT, context, &fused, &[])?;
        if !fused.is_empty() {
            return Err(ConformanceFailure::new(
                COMPONENT,
                "empty inputs yield empty output",
                format!("{context}: returned {} results", fused.len()),
            ));
        }
    }

    let leg = ranked("leg-a", 3);
    let context = "fuse of a single ranked list";
    let fused = fusion
        .fuse(vec![leg.clone()], &FusionParams::new())
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_no_fabricated_ids(COMPONENT, context, &fused, &ids(&leg))?;
    check_ranking(COMPONENT, context, &fused)?;
    check_order_preserved(context, &leg, &fused)?;

    let left = ranked("leg-a", 3);
    let right = ranked("leg-b", 2);
    let context = "fuse of two ranked lists";
    let fused = fusion
        .fuse(vec![left.clone(), right.clone()], &FusionParams::new())
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    let mut union = ids(&left);
    union.extend(ids(&right));
    check_no_fabricated_ids(COMPONENT, context, &fused, &union)?;
    check_ranking(COMPONENT, context, &fused)
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
