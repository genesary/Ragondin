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
/// dimensionality does not match the index, a `top_k` of zero". A component
/// that answers it with an empty list makes a caller's arithmetic bug look
/// like an empty corpus.
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
