//! [`Embedder`] conformance.

use ragondin_contracts::{EmbedParams, EmbedRole, Embedder};

use crate::failure::ConformanceFailure;

const COMPONENT: &str = "Embedder";

/// What the caller declares about the fixture it hands the suite.
///
/// The suite does not know the model behind a trait object, so it cannot tell
/// an asymmetric embedder that ignores its [`EmbedRole`] from a symmetric one
/// that is right to. Only the caller knows, and this is where it says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RolePrefixes {
    /// The fixture was configured with **distinct** prefixes per role, so one
    /// text must embed differently under `Query` than under `Passage`. Declare
    /// this and the suite checks it.
    Distinct,
    /// Nothing is declared: a symmetric model, or one whose configuration the
    /// caller does not control. Both roles are still exercised; neither is
    /// required to differ from the other.
    Undeclared,
}

/// Checks that `make`'s embedders honour the [`Embedder`] contract.
///
/// Every check below runs under **both** [`EmbedRole`]s, since an embedder may
/// take a different path per role and a contract broken on one side only is
/// still broken:
///
/// - **One vector per input.** A batch that comes back shorter than it went in
///   silently misaligns a corpus from its index: every chunk after the dropped
///   one is stored under the wrong vector, and nothing errors. The same check
///   covers the empty batch, which must embed to no vectors rather than fail.
/// - **One dimensionality, across every batch and both roles.** A ragged batch
///   cannot be searched against a single index — and neither can a corpus
///   embedded under `Passage` be searched by a query embedded under `Query`
///   into a space of another width. One dimensionality per role is one
///   embedding space per role, which is no retrieval at all; the role-separation
///   check below cannot see it, since vectors of unequal width are unequal.
/// - **Finite components**, for the reason `ragondin-types` gives on
///   [`Embedding`](ragondin_types::Embedding): a non-finite component
///   serializes without error and cannot be read back.
///
/// And, when `prefixes` is [`RolePrefixes::Distinct`]:
///
/// - **The role changes the vector.** One text embedded under the two roles
///   must not come back identical. This catches an implementation that accepts
///   the role and ignores it — the failure ADR-C17 exists to prevent, and the
///   one every other check here passes straight through, since a
///   wrongly-prefixed vector is well-formed in every respect.
///
/// # What it still cannot check
///
/// That a given prefix was the **right** one. The suite does not know the
/// model, so it cannot distinguish an embedder applying E5's `"query: "` from
/// one applying its own invention; it sees only that the two roles differ. Nor
/// can it require them to differ at all unless the caller declares they were
/// configured to: a symmetric model answering both roles alike is correct, and
/// a check a correct implementation could fail is worse than a missing one.
/// The protection against a wrong prefix is the contract's documented
/// statement on [`Embedder`], not a check here.
///
/// It does **not** check that the vectors are *good* — that is a benchmark
/// (`ragondin-metrics`), not a contract. Nor does it reject a
/// **zero-dimensional** embedding: `ragondin-types` says an empty embedding is
/// representable, `ragondin-contracts`' own stub treats one as an invalid
/// request, and the suite does not settle a question the contract leaves open
/// (see `ARCHITECTURE.md`).
pub async fn check_embedder_conformance(
    make: impl Fn() -> Box<dyn Embedder>,
    prefixes: RolePrefixes,
) -> Result<(), ConformanceFailure> {
    let embedder = make();

    // The width of the first vector seen, carried across both roles: an
    // embedder has one embedding space, not one per call.
    let mut expected_dim: Option<usize> = None;

    for role in [EmbedRole::Query, EmbedRole::Passage] {
        let batch = vec![
            "a short query".to_string(),
            "a passage of rather more words than the first one".to_string(),
            "another passage entirely".to_string(),
        ];

        for (what, texts) in [
            ("embed of a three-text batch", batch),
            ("embed of an empty batch", Vec::new()),
        ] {
            let context = format!("{what} under {role:?}");
            let vectors = embedder
                .embed(&texts, &EmbedParams::new(role))
                .await
                .map_err(|error| ConformanceFailure::from_call(COMPONENT, &context, &error))?;

            if vectors.len() != texts.len() {
                return Err(ConformanceFailure::new(
                    COMPONENT,
                    "one vector per input",
                    format!(
                        "{context}: {} texts in, {} vectors out",
                        texts.len(),
                        vectors.len()
                    ),
                ));
            }

            let Some(first) = vectors.first() else {
                continue;
            };
            let dim = first.dim();
            if let Some(odd) = vectors.iter().position(|vector| vector.dim() != dim) {
                return Err(ConformanceFailure::new(
                    COMPONENT,
                    "constant dimensionality",
                    format!(
                        "{context}: vector 0 has {dim} components, vector {odd} has {}",
                        vectors[odd].dim()
                    ),
                ));
            }

            match expected_dim {
                None => expected_dim = Some(dim),
                Some(seen) if seen != dim => {
                    return Err(ConformanceFailure::new(
                        COMPONENT,
                        "constant dimensionality",
                        format!(
                            "{context}: {dim} components, where an earlier call \
                             returned {seen}",
                        ),
                    ));
                }
                Some(_) => {}
            }

            if let Some(odd) = vectors
                .iter()
                .position(|vector| vector.as_slice().iter().any(|c| !c.is_finite()))
            {
                return Err(ConformanceFailure::new(
                    COMPONENT,
                    "finite components",
                    format!("{context}: vector {odd} carries a non-finite component"),
                ));
            }
        }
    }

    // Matched exhaustively rather than compared: `RolePrefixes` is open to a
    // variant this crate has not thought of yet — the doc on `Undeclared`
    // already fuses two distinct facts, a symmetric model and an unknown one —
    // and an `== Distinct` would silently skip the check for it. That is the
    // wildcard failure `EmbedRole` is closed to prevent; the rule is worth no
    // less applied to the enum this suite owns.
    match prefixes {
        RolePrefixes::Distinct => check_roles_are_separated(embedder.as_ref()).await?,
        RolePrefixes::Undeclared => {}
    }

    Ok(())
}

/// One text, both roles: a fixture configured with distinct prefixes owes two
/// distinct vectors.
async fn check_roles_are_separated(embedder: &dyn Embedder) -> Result<(), ConformanceFailure> {
    let texts = vec!["the one text both roles are asked about".to_string()];

    let mut vectors = Vec::new();
    for role in [EmbedRole::Query, EmbedRole::Passage] {
        let context = format!("embed of one text under {role:?}");
        vectors.push(
            embedder
                .embed(&texts, &EmbedParams::new(role))
                .await
                .map_err(|error| ConformanceFailure::from_call(COMPONENT, &context, &error))?,
        );
    }

    if vectors[0] == vectors[1] {
        // Reported by shape, not by content, as every other failure here is:
        // the real fixture this check exists for is an E5- or BGE-class
        // embedder, and printing its vectors would put a thousand floats in a
        // panic message that has already said the only thing that matters —
        // the two are identical.
        let dim = vectors[0].first().map_or(0, |vector| vector.dim());
        return Err(ConformanceFailure::new(
            COMPONENT,
            "role changes the vector",
            format!(
                "distinct per-role prefixes were declared, yet one text embeds \
                 identically under Query and Passage ({dim} components)"
            ),
        ));
    }

    Ok(())
}

/// [`check_embedder_conformance`], panicking on the first broken check.
pub async fn assert_embedder_conformance(
    make: impl Fn() -> Box<dyn Embedder>,
    prefixes: RolePrefixes,
) {
    if let Err(failure) = check_embedder_conformance(make, prefixes).await {
        panic!("{failure}");
    }
}
