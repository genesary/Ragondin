//! [`Embedder`] conformance.

use ragondin_contracts::{EmbedParams, Embedder};

use crate::failure::ConformanceFailure;

const COMPONENT: &str = "Embedder";

/// Checks that `make`'s embedders honour the [`Embedder`] contract.
///
/// - **One vector per input.** A batch that comes back shorter than it went in
///   silently misaligns a corpus from its index: every chunk after the dropped
///   one is stored under the wrong vector, and nothing errors. The same check
///   covers the empty batch, which must embed to no vectors rather than fail.
/// - **One dimensionality per batch.** A ragged batch cannot be searched
///   against a single index.
/// - **Finite components**, for the reason `ragondin-types` gives on
///   [`Embedding`](ragondin_types::Embedding): a non-finite component
///   serializes without error and cannot be read back.
///
/// It does **not** check that the vectors are *good* — that is a benchmark
/// (`ragondin-metrics`), not a contract. Nor does it reject a
/// **zero-dimensional** embedding: `ragondin-types` says an empty embedding is
/// representable, `ragondin-contracts`' own stub treats one as an invalid
/// request, and the suite does not settle a question the contract leaves open
/// (see `ARCHITECTURE.md`).
pub async fn check_embedder_conformance(
    make: impl Fn() -> Box<dyn Embedder>,
) -> Result<(), ConformanceFailure> {
    let embedder = make();

    let batch = vec![
        "a short query".to_string(),
        "a passage of rather more words than the first one".to_string(),
        "another passage entirely".to_string(),
    ];

    for (context, texts) in [
        ("embed of a three-text batch", batch),
        ("embed of an empty batch", Vec::new()),
    ] {
        let vectors = embedder
            .embed(&texts, &EmbedParams::new())
            .await
            .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;

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

    Ok(())
}

/// [`check_embedder_conformance`], panicking on the first broken check.
pub async fn assert_embedder_conformance(make: impl Fn() -> Box<dyn Embedder>) {
    if let Err(failure) = check_embedder_conformance(make).await {
        panic!("{failure}");
    }
}
