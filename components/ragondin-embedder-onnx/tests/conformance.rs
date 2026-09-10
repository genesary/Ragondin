//! The suite every implementation of a contract must pass (ADR-C6), plus the
//! behaviour that is this component's own.
//!
//! Behind `onnx` because the component is: with the feature off the crate
//! exports nothing to test, so the whole file compiles away rather than
//! failing to build.
//!
//! Every test here runs against the committed fixtures described in
//! `tests/fixtures/generate.py`: a tokenizer over a 38-word vocabulary and a
//! graph that gathers one row per token. Nothing is downloaded, and the model
//! is arithmetic small enough that what a failure accuses is this crate rather
//! than ONNX Runtime.
#![cfg(feature = "onnx")]

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use ragondin_conformance::{assert_embedder_conformance, RolePrefixes};
use ragondin_contracts::{ComponentError, EmbedParams, EmbedRole, Embedder};
use ragondin_embedder_onnx::{EmbedderError, OnnxEmbedder, OnnxEmbedderConfig};
use ragondin_types::Embedding;

/// The hidden size of the fixture graphs, and therefore the dimensionality
/// every vector below has.
const HIDDEN: usize = 8;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn config_over(model: &str) -> OnnxEmbedderConfig {
    OnnxEmbedderConfig::new(fixture(model), fixture("tokenizer.json"))
}

/// The asymmetric configuration, which is the one that exercises ADR-C17: the
/// two roles prepend different text, so a vector that ignores the role is
/// visibly wrong.
fn asymmetric() -> OnnxEmbedderConfig {
    config_over("tiny-embedder.onnx").with_prefixes("query: ", "passage: ")
}

fn embedder() -> OnnxEmbedder {
    OnnxEmbedder::new(asymmetric()).expect("the fixture model and tokenizer must load")
}

fn nonzero(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("a test's own constant is not zero")
}

async fn embed(embedder: &OnnxEmbedder, texts: &[&str], role: EmbedRole) -> Vec<Embedding> {
    let owned: Vec<String> = texts.iter().map(|text| (*text).to_string()).collect();
    embedder
        .embed(&owned, &EmbedParams::new(role))
        .await
        .expect("a well-formed batch must embed")
}

#[tokio::test]
async fn honours_the_embedder_contract() {
    assert_embedder_conformance(|| Box::new(embedder()), RolePrefixes::Distinct).await;
}

/// A symmetric model is configured with no prefix on either side rather than
/// special-cased (ADR-C17), so the suite has to pass in that configuration too
/// — under `Undeclared`, which is what the caller is entitled to declare when
/// the two roles are meant to answer alike.
#[tokio::test]
async fn a_symmetric_configuration_is_conformant() {
    assert_embedder_conformance(
        || {
            Box::new(
                OnnxEmbedder::new(config_over("tiny-embedder.onnx"))
                    .expect("the fixture model and tokenizer must load"),
            )
        },
        RolePrefixes::Undeclared,
    )
    .await;
}

/// The determinism this component owes a reproducible run: the same text, the
/// same model, the same vector — bit for bit, not within a tolerance.
///
/// `run_id` is content-addressed over `model_hashes`
/// (`docs/system-architecture.md` §7.1), so two runs that agree on the model
/// must agree on its output or the address names two different things.
#[tokio::test]
async fn the_same_text_embeds_identically_every_time() {
    let one = embedder();
    let first = embed(&one, &["the cat sat on the mat"], EmbedRole::Passage).await;

    for _ in 0..4 {
        let again = embed(&one, &["the cat sat on the mat"], EmbedRole::Passage).await;
        assert_eq!(first, again, "one text must embed to one vector, always");
    }

    // And across sessions, not only within one: a fresh model load must not
    // move the numbers either.
    let another = embedder();
    let reloaded = embed(&another, &["the cat sat on the mat"], EmbedRole::Passage).await;
    assert_eq!(first, reloaded, "a reloaded model must answer identically");
}

/// Batching is an implementation detail behind the trait
/// (`docs/code-architecture.md` §11.2), which is only true if the batch size
/// cannot be read off the answer.
#[tokio::test]
async fn the_batch_size_does_not_change_the_answer() {
    let texts = [
        "a cat",
        "the dog sat on the mat entirely",
        "alpha beta gamma",
        "one",
        "another text of rather more words than the first one",
    ];

    let mut answers = Vec::new();
    for batch_size in [1, 2, 5, 512] {
        let embedder = OnnxEmbedder::new(asymmetric().with_batch_size(nonzero(batch_size)))
            .expect("the fixture model and tokenizer must load");
        answers.push(embed(&embedder, &texts, EmbedRole::Passage).await);
    }

    for (index, answer) in answers.iter().enumerate().skip(1) {
        assert_eq!(
            &answers[0], answer,
            "batch size {index} must not change a vector"
        );
    }
}

/// One text alone and the same text inside a batch embed identically.
///
/// This is the mask check. A batch is padded to its longest member, so a short
/// text sits next to positions that carry no token; pooling them in would make
/// a vector depend on which texts happened to be embedded alongside it, and
/// nothing downstream could see it.
#[tokio::test]
async fn a_text_embeds_the_same_alone_as_in_a_padded_batch() {
    let embedder = embedder();
    let alone = embed(&embedder, &["a cat"], EmbedRole::Query).await;
    let batched = embed(
        &embedder,
        &[
            "a cat",
            "the dog sat on the mat rather more entirely than another one",
        ],
        EmbedRole::Query,
    )
    .await;

    assert_eq!(
        alone[0], batched[0],
        "padding must be pooled out, not pooled in"
    );
}

/// What the role does, stated as an equality rather than as a difference.
///
/// The conformance suite can only require that the two roles differ; it does
/// not know the model, so it cannot check that a prefix was applied. Here the
/// prefix is the fixture's own configuration, so it can be: embedding `"cat"`
/// under `Query` must equal embedding `"query: cat"` through an embedder
/// configured with no prefixes at all.
#[tokio::test]
async fn the_role_selects_the_prefix_that_is_prepended() {
    let asymmetric = embedder();
    let symmetric = OnnxEmbedder::new(config_over("tiny-embedder.onnx"))
        .expect("the fixture model and tokenizer must load");

    for (role, prefixed) in [
        (EmbedRole::Query, "query: cat"),
        (EmbedRole::Passage, "passage: cat"),
    ] {
        assert_eq!(
            embed(&asymmetric, &["cat"], role).await,
            embed(&symmetric, &[prefixed], role).await,
            "{role:?} must prepend its own configured prefix and nothing else"
        );
    }
}

/// Vectors come back L2-normalized, so a dot product against them is a cosine
/// similarity. A `VectorStore` scores whatever it is given; normalizing here is
/// what makes two vectors from this embedder comparable in the store's terms.
#[tokio::test]
async fn vectors_are_l2_normalized() {
    let vectors = embed(
        &embedder(),
        &["a cat", "another text entirely", "one"],
        EmbedRole::Passage,
    )
    .await;

    for vector in &vectors {
        let norm: f32 = vector.as_slice().iter().map(|c| c * c).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "expected a unit vector, got a norm of {norm}"
        );
    }
}

/// The dimensionality is the model's, not a configured number: eight, because
/// the fixture graph's embedding table is eight wide.
#[tokio::test]
async fn the_dimensionality_is_the_models_hidden_size() {
    let vectors = embed(&embedder(), &["a cat"], EmbedRole::Passage).await;
    assert_eq!(vectors[0].dim(), HIDDEN);
}

/// A text that tokenizes to no tokens at all embeds to the zero vector.
///
/// The fixture tokenizer adds no `[CLS]`/`[SEP]`, so `""` really does produce
/// an empty sequence — and with nothing to pool, the mean is `0/0`. Left
/// unguarded that is a vector of `NaN`, which serializes without error and
/// cannot be read back (`ragondin-types`), and which the contract's finite-
/// components requirement forbids. It is one vector of the right width, so the
/// batch is still aligned with its input.
///
/// The symmetric configuration, necessarily: a prefix is text, so under an
/// asymmetric one the empty string tokenizes to the prefix's own tokens and
/// this case does not arise.
#[tokio::test]
async fn a_text_that_tokenizes_to_nothing_embeds_to_the_zero_vector() {
    let symmetric = OnnxEmbedder::new(config_over("tiny-embedder.onnx"))
        .expect("the fixture model and tokenizer must load");
    let vectors = embed(&symmetric, &["", "a cat"], EmbedRole::Passage).await;

    assert_eq!(vectors.len(), 2);
    assert_eq!(vectors[0].dim(), HIDDEN);
    assert!(
        vectors[0].as_slice().iter().all(|c| *c == 0.0),
        "an empty sequence pools to zero, not to NaN: {:?}",
        vectors[0]
    );
}

/// Truncation is the tokenizer's, so it happens in token units and is visible
/// as an equality: two texts that share their first four tokens and differ
/// afterwards embed identically once the limit is four.
#[tokio::test]
async fn a_text_longer_than_the_limit_is_truncated() {
    let embedder =
        OnnxEmbedder::new(config_over("tiny-embedder.onnx").with_max_sequence_length(nonzero(4)))
            .expect("the fixture model and tokenizer must load");

    let vectors = embed(
        &embedder,
        &[
            "alpha beta gamma delta",
            "alpha beta gamma delta epsilon zeta eta",
        ],
        EmbedRole::Passage,
    )
    .await;

    assert_eq!(
        vectors[0], vectors[1],
        "everything past the fourth token must be dropped"
    );
}

/// A model that asks for `token_type_ids` — as a BERT export does — is fed
/// zeros, one segment.
///
/// Asserted as an equality against the two-input fixture: the three-input one
/// adds a large vector for segment 1 and nothing for segment 0, so the two
/// models agree exactly when the segment ids are all zero.
#[tokio::test]
async fn token_type_ids_are_supplied_as_one_segment_of_zeros() {
    let plain = embedder();
    let with_segments = OnnxEmbedder::new(
        config_over("tiny-embedder-token-types.onnx").with_prefixes("query: ", "passage: "),
    )
    .expect("the fixture model and tokenizer must load");

    let texts = ["a cat", "another text entirely"];
    assert_eq!(
        embed(&plain, &texts, EmbedRole::Passage).await,
        embed(&with_segments, &texts, EmbedRole::Passage).await,
        "segment 0 is the only segment there is"
    );
}

/// A model demanding an input this component cannot fill is refused when it is
/// loaded, not when it is called.
///
/// ONNX Runtime's own error for a missing input arrives per call, which would
/// make a wiring mistake look like an intermittent backend failure. The names
/// the component knows are fixed and the model's are readable at load, so the
/// comparison happens once.
#[test]
fn a_model_demanding_an_unknown_input_is_refused_at_construction() {
    let Err(error) = OnnxEmbedder::new(config_over("tiny-embedder-unknown-input.onnx")) else {
        panic!("a model this component cannot drive must not load");
    };

    match error {
        EmbedderError::UnknownModelInput { name } => assert_eq!(name, "temperature"),
        other => panic!("expected UnknownModelInput, got {other:?}"),
    }
}

/// A model that never asks for `input_ids` encodes no text, and so is not an
/// embedder this component can drive.
///
/// Every input it declares is one the component knows how to fill, so the check
/// above passes it. Without this one it would load, run, and answer every text
/// with the same vector.
#[test]
fn a_model_that_does_not_take_input_ids_is_refused() {
    let Err(error) = OnnxEmbedder::new(config_over("tiny-embedder-no-ids.onnx")) else {
        panic!("a model that reads no token ids must not load");
    };

    assert!(
        matches!(error, EmbedderError::NoTokenInput),
        "expected NoTokenInput, got {error:?}"
    );
}

/// A model that pools for itself returns `[batch, hidden]`, and this component
/// pools with the attention mask, so it needs `[batch, sequence, hidden]`.
///
/// The rank is a property of the run rather than of the graph's declared
/// signature — a dynamic axis is `None` until there is a tensor — so this one
/// is a call-time failure, and it reaches the caller as a backend failure the
/// way any other inference failure does.
#[tokio::test]
async fn a_model_whose_output_is_already_pooled_is_refused() {
    let embedder = OnnxEmbedder::new(config_over("tiny-embedder-pooled.onnx"))
        .expect("the model itself loads; what it returns is the problem");

    let error = embedder
        .embed(
            &["a cat".to_string()],
            &EmbedParams::new(EmbedRole::Passage),
        )
        .await
        .expect_err("a two-dimensional output cannot be mask-pooled");

    assert!(
        matches!(error, ComponentError::Backend(_)),
        "expected a backend failure, got {error:?}"
    );
}

/// A path that names no file fails when the component is built. Both halves are
/// configuration, and neither is worth discovering on the first query.
#[test]
fn a_path_that_names_no_file_is_refused_at_construction() {
    let no_model = OnnxEmbedder::new(OnnxEmbedderConfig::new(
        fixture("does-not-exist.onnx"),
        fixture("tokenizer.json"),
    ));
    assert!(
        matches!(no_model, Err(EmbedderError::ModelLoad { .. })),
        "a missing model must be reported as a model load failure"
    );

    let no_tokenizer = OnnxEmbedder::new(OnnxEmbedderConfig::new(
        fixture("tiny-embedder.onnx"),
        fixture("does-not-exist.json"),
    ));
    assert!(
        matches!(no_tokenizer, Err(EmbedderError::TokenizerLoad { .. })),
        "a missing tokenizer must be reported as a tokenizer load failure"
    );
}

/// One embedder, many concurrent calls. `Embedder::embed` takes `&self` and the
/// trait is `Send + Sync`, while an ONNX Runtime session is not safe to run
/// concurrently — so the component serializes its own calls, and the vectors
/// are the same ones a sequential caller would get.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_calls_answer_as_sequential_ones_do() {
    let embedder = std::sync::Arc::new(embedder());
    let expected = embed(&embedder, &["a cat"], EmbedRole::Query).await;

    let mut handles = Vec::new();
    for _ in 0..8 {
        let embedder = std::sync::Arc::clone(&embedder);
        handles.push(tokio::spawn(async move {
            embedder
                .embed(&["a cat".to_string()], &EmbedParams::new(EmbedRole::Query))
                .await
                .expect("a well-formed batch must embed")
        }));
    }

    for handle in handles {
        assert_eq!(handle.await.expect("no task must panic"), expected);
    }
}
