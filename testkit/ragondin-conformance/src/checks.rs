//! The checks more than one family shares.
//!
//! Each is `pub(crate)`: a contributor calls one function per family, not a
//! menu of checks, so that "conformant" means the same thing for a built-in and
//! a third-party component (INV-7).

use ragondin_contracts::ComponentError;
use ragondin_types::ScoredChunk;

use crate::failure::ConformanceFailure;

/// The ranking contract.
///
/// `ragondin-contracts` states it on [`Retriever`](ragondin_contracts::Retriever)
/// and delegates its enforcement here: a result is **sorted by descending
/// score** and **every score is finite**. Neither half is checkable by the type
/// system, and both fail silently — nDCG@k reads position, so an unsorted list
/// reports a wrong number rather than an error, and `f32` admits `NaN`, on
/// which the `partial_cmp(…).unwrap()` every implementer writes will panic in
/// somebody else's crate.
///
/// A caller must hand it a list that **can** hold two entries: on one hit the
/// check is vacuous, which is how a store returning ascending distances would
/// otherwise pass.
pub(crate) fn check_ranking(
    component: &'static str,
    context: &str,
    hits: &[ScoredChunk],
) -> Result<(), ConformanceFailure> {
    // Finiteness first: a NaN is not "out of order", it is not a score at all,
    // and reporting it as an ordering fault would send the implementer looking
    // in the wrong place.
    if let Some(hit) = hits.iter().find(|hit| !hit.score.is_finite()) {
        return Err(ConformanceFailure::new(
            component,
            "finite scores",
            format!(
                "{context}: chunk `{}` scored {}, which is not finite",
                hit.chunk.id.as_str(),
                hit.score
            ),
        ));
    }

    for pair in hits.windows(2) {
        if pair[0].score < pair[1].score {
            return Err(ConformanceFailure::new(
                component,
                "descending scores",
                format!(
                    "{context}: chunk `{}` scored {} precedes `{}` scored {}",
                    pair[0].chunk.id.as_str(),
                    pair[0].score,
                    pair[1].chunk.id.as_str(),
                    pair[1].score
                ),
            ));
        }
    }

    Ok(())
}

/// No result may carry an id that was not offered to the component.
///
/// A fabricated id is the failure that turns an evaluation number into
/// fiction: the chunk is scored, ranked and reported, and nothing downstream
/// can tell it was never retrieved.
///
/// With nothing offered, *any* result is a fabrication — which is why the
/// families that own their inputs need no separate "empty in, empty out"
/// check: this one fires first, and with the more precise diagnosis.
pub(crate) fn check_no_fabricated_ids(
    component: &'static str,
    context: &str,
    hits: &[ScoredChunk],
    offered: &[&str],
) -> Result<(), ConformanceFailure> {
    if let Some(hit) = hits
        .iter()
        .find(|hit| !offered.contains(&hit.chunk.id.as_str()))
    {
        return Err(ConformanceFailure::new(
            component,
            "no fabricated ids",
            format!(
                "{context}: returned chunk `{}`, which was not among the {} it was given",
                hit.chunk.id.as_str(),
                offered.len()
            ),
        ));
    }
    Ok(())
}

/// A ranked list ranks each chunk once.
///
/// Checked only where the component's whole input is known to the suite, so
/// that "this id appears twice" is a statement about the component and not
/// about a corpus it was built over. It is the fusion's characteristic bug —
/// concatenating overlapping legs without merging them — and it double-counts
/// in every position-reading metric while erroring nowhere.
pub(crate) fn check_no_duplicate_ids(
    component: &'static str,
    context: &str,
    hits: &[ScoredChunk],
) -> Result<(), ConformanceFailure> {
    let mut seen: Vec<&str> = Vec::with_capacity(hits.len());
    for hit in hits {
        let id = hit.chunk.id.as_str();
        if seen.contains(&id) {
            return Err(ConformanceFailure::new(
                component,
                "no duplicate ids",
                format!("{context}: chunk `{id}` appears more than once"),
            ));
        }
        seen.push(id);
    }
    Ok(())
}

/// `top_k` bounds the answer from above, and only from above.
///
/// There is no lower bound anywhere in this suite. For a retriever the suite
/// knows nothing of the corpus; for a reranker a score threshold may
/// legitimately reject every candidate. Conformance is a floor, not a proof
/// that a component is useful.
pub(crate) fn check_top_k(
    component: &'static str,
    context: &str,
    hits: &[ScoredChunk],
    top_k: usize,
) -> Result<(), ConformanceFailure> {
    if hits.len() > top_k {
        return Err(ConformanceFailure::new(
            component,
            "top_k respected",
            format!("{context}: asked for at most {top_k}, got {}", hits.len()),
        ));
    }
    Ok(())
}

/// A `top_k` of zero is an unmet precondition, not a request for nothing.
///
/// `ComponentError::InvalidRequest`'s own documentation names it as the
/// example: "a precondition of the call is unmet — an embedding whose
/// dimensionality does not match the index, a `top_k` of zero". That sentence
/// is written on the error every component boundary returns, not on one
/// family, so the clause is enforced for every family that takes a `top_k`.
/// A component answering zero with an empty list makes a caller's arithmetic
/// bug look like an empty corpus.
pub(crate) fn check_zero_top_k_rejected(
    component: &'static str,
    context: &str,
    outcome: Result<Vec<ScoredChunk>, ComponentError>,
) -> Result<(), ConformanceFailure> {
    match outcome {
        Err(ComponentError::InvalidRequest(_)) => Ok(()),
        Err(other) => Err(ConformanceFailure::new(
            component,
            "zero top_k rejected",
            format!("{context}: rejected with `{other}`, expected an invalid-request error"),
        )),
        Ok(hits) => Err(ConformanceFailure::new(
            component,
            "zero top_k rejected",
            format!(
                "{context}: returned {} results instead of rejecting",
                hits.len()
            ),
        )),
    }
}
