//! [`Generator`] conformance.

use ragondin_contracts::{GenerateParams, Generator};
use ragondin_types::{Context, ContextChunk};

use crate::{
    checks::{check_identity_non_empty, check_identity_stable, check_invalid_request},
    failure::ConformanceFailure,
    fixtures::{query, ranked},
};

const COMPONENT: &str = "Generator";

/// One template per way the grammar on [`Generator`] makes a template
/// malformed, each otherwise well formed so that nothing else is refused.
const MALFORMED_TEMPLATES: [(&str, &str); 3] = [
    (
        "{unknown}",
        "a name other than query or context between braces",
    ),
    ("{context} {", "a `{` no `}` closes"),
    ("{query} }", "a lone `}`"),
];

/// Checks that `make`'s generators honour the [`Generator`] contract.
///
/// `served_model` must be a name the fixture serves, and `template` a template
/// it is expected to accept, in the grammar stated on [`Generator`]: the suite
/// cannot know which model a fixture serves, so the caller states it, as it
/// states a vector store's dimensionality. Both are the caller's claims about
/// its fixture, and a wrong one fails `well-formed call succeeds`.
///
/// - **A well-formed call succeeds** — over a context holding passages, and
///   over the **empty context** (ADR-C19), where an answer of "I do not know"
///   is conformant and a refusal is not.
/// - An **empty served model**, an **empty template** and a **malformed
///   template** are each rejected as an invalid request: each is a refusal of
///   the call's form, which the suite can check without knowing the model.
///   "Malformed" is probed three ways — an unknown placeholder, a `{` no `}`
///   closes, a lone `}` — and a generator must refuse every one.
/// - The **identity of `served_model` is non-empty**, and **stable across two
///   calls** — two on one instance, and one on a second instance the same
///   constructor built (ADR-C31 § 4, ADR-C32 § 4). A failure to report one is
///   a well-formed call failing.
///
/// **Nothing about content.** The answer is never read: a suite that does not
/// know which model it is testing cannot say whether an answer is good, and a
/// generator answering "I do not know" to everything is conformant. Nor does
/// the suite ask for a model the fixture does not serve — the contract refuses
/// one, but no name is one the suite could know every fixture refuses.
pub async fn check_generator_conformance(
    make: impl Fn() -> Box<dyn Generator>,
    served_model: &str,
    template: &str,
) -> Result<(), ConformanceFailure> {
    let generator = make();
    let query = query();
    let params = GenerateParams::new(served_model, template);

    let context = "generate over a context of two passages";
    generator
        .generate(&query, &passages(), &params)
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;

    let context = "generate over an empty context";
    let empty = Context {
        chunks: Vec::new(),
        text: String::new(),
    };
    generator
        .generate(&query, &empty, &params)
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;

    check_invalid_request(
        COMPONENT,
        "empty served_model rejected",
        "generate with an empty served_model",
        generator
            .generate(&query, &passages(), &GenerateParams::new("", template))
            .await,
    )?;

    check_invalid_request(
        COMPONENT,
        "empty template rejected",
        "generate with an empty template",
        generator
            .generate(&query, &passages(), &GenerateParams::new(served_model, ""))
            .await,
    )?;

    for (malformed, why) in MALFORMED_TEMPLATES {
        check_invalid_request(
            COMPONENT,
            "malformed template rejected",
            &format!("generate with the template {malformed:?}, which holds {why}"),
            generator
                .generate(
                    &query,
                    &passages(),
                    &GenerateParams::new(served_model, malformed),
                )
                .await,
        )?;
    }

    let context = format!("model_identity({served_model:?})");
    let first = generator
        .model_identity(served_model)
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, &context, &error))?;
    check_identity_non_empty(COMPONENT, &context, &first)?;

    let context = format!("model_identity({served_model:?}), called again on the same instance");
    let again = generator
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

/// A context a builder could have made from two of the suite's chunks.
fn passages() -> Context {
    let hits = ranked("passage", 2);
    Context {
        text: hits
            .iter()
            .map(|hit| hit.chunk.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        chunks: hits
            .into_iter()
            .map(|hit| ContextChunk {
                id: hit.chunk.id,
                document_id: hit.chunk.document_id,
                score: hit.score,
            })
            .collect(),
    }
}

/// [`check_generator_conformance`], panicking on the first broken check.
pub async fn assert_generator_conformance(
    make: impl Fn() -> Box<dyn Generator>,
    served_model: &str,
    template: &str,
) {
    if let Err(failure) = check_generator_conformance(make, served_model, template).await {
        panic!("{failure}");
    }
}
