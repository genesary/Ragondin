//! Conformance, and the ranking conformance deliberately does not check.
//!
//! `ragondin-conformance` knows nothing about the model behind a `Reranker`,
//! so it asserts only what holds whatever that model contains: no fabricated
//! ids, no duplicates, `top_k` respected, descending finite scores, a zero
//! `top_k` rejected. *Which* chunk comes first is quality, and quality is
//! checked here — against the fixture cross-encoder in `fixture/`, whose score
//! for a pair is its lexical overlap and therefore predictable exactly.

#![cfg(feature = "onnx")]

mod fixture;

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use ragondin_conformance::assert_reranker_conformance;
use ragondin_contracts::{ComponentError, RerankParams, Reranker};
use ragondin_reranker_onnx::{ModelError, OnnxReranker, OnnxRerankerConfig};
use ragondin_types::{Chunk, ChunkId, DocId, Query, QueryId, ScoredChunk};

fn config() -> OnnxRerankerConfig {
    let fixture = fixture::cross_encoder();
    OnnxRerankerConfig::new(&fixture.model, &fixture.tokenizer)
}

fn reranker() -> OnnxReranker {
    OnnxReranker::new(config()).expect("the fixture cross-encoder loads")
}

fn query() -> Query {
    Query {
        id: QueryId::new("q-1"),
        text: "how do ragondins build their burrows".to_string(),
    }
}

fn scored(id: &str, text: &str, score: f32) -> ScoredChunk {
    ScoredChunk {
        chunk: Chunk {
            id: ChunkId::new(id),
            text: text.to_string(),
            document_id: DocId::new("d-1"),
        },
        score,
    }
}

/// Three candidates the fixture cross-encoder separates: the answer, something
/// that shares two words with the query, and something unrelated. Retrieval
/// ranked them in exactly the wrong order, so a reranker that returns its input
/// untouched passes none of the assertions below.
fn candidates() -> Vec<ScoredChunk> {
    vec![
        scored(
            "c-sonnet",
            "a sonnet has fourteen lines and a fixed rhyme",
            0.9,
        ),
        scored("c-banks", "how the river banks build", 0.8),
        scored(
            "c-burrows",
            "ragondins build their burrows in river banks",
            0.1,
        ),
    ]
}

fn ids(hits: &[ScoredChunk]) -> Vec<&str> {
    hits.iter().map(|hit| hit.chunk.id.as_str()).collect()
}

#[tokio::test]
async fn the_reranker_conforms() {
    // `None`: configured with no served-model name, it serves the model it
    // loaded and nothing else (ADR-C32 § 4).
    assert_reranker_conformance(|| Box::new(reranker()), None).await;
}

#[tokio::test]
async fn the_chunk_that_answers_the_query_ranks_first() {
    let reordered = reranker()
        .rerank(&query(), candidates(), &RerankParams::new(3))
        .await
        .unwrap();

    assert_eq!(ids(&reordered), ["c-burrows", "c-banks", "c-sonnet"]);
    assert!(
        reordered[0].score > reordered[1].score && reordered[1].score > reordered[2].score,
        "the cross-encoder must separate the three, not merely order them: {:?}",
        reordered.iter().map(|hit| hit.score).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn top_k_keeps_the_best_rather_than_the_first() {
    let reordered = reranker()
        .rerank(&query(), candidates(), &RerankParams::new(1))
        .await
        .unwrap();

    assert_eq!(ids(&reordered), ["c-burrows"]);
}

#[tokio::test]
async fn equally_scored_chunks_are_ordered_by_chunk_id() {
    // Identical text, so the model cannot separate them. Handed in reversed,
    // so input order and id order disagree and only one of them can explain
    // the result.
    let text = "ragondins build their burrows in river banks";
    let reordered = reranker()
        .rerank(
            &query(),
            vec![scored("c-b", text, 0.1), scored("c-a", text, 0.9)],
            &RerankParams::new(2),
        )
        .await
        .unwrap();

    assert_eq!(ids(&reordered), ["c-a", "c-b"]);
    assert_eq!(reordered[0].score, reordered[1].score);
}

#[tokio::test]
async fn the_batch_size_changes_nothing_about_the_result() {
    // Batching is where a score loses its chunk. With a batch size of two over
    // five candidates the batches are uneven, so an implementation that
    // mis-associates scores across a batch boundary cannot agree with one that
    // scores them all at once.
    let mut candidates = candidates();
    candidates.push(scored(
        "c-quarter",
        "the price of tin fell last quarter",
        0.5,
    ));
    candidates.push(scored("c-lines", "how do the lines of a sonnet build", 0.4));

    let one_batch = reranker()
        .rerank(&query(), candidates.clone(), &RerankParams::new(5))
        .await
        .unwrap();

    let mut config = config();
    config.batch_size = NonZeroUsize::new(2).unwrap();
    let three_batches = OnnxReranker::new(config)
        .expect("the fixture cross-encoder loads")
        .rerank(&query(), candidates, &RerankParams::new(5))
        .await
        .unwrap();

    assert_eq!(ids(&one_batch), ids(&three_batches));
    assert_eq!(
        one_batch.iter().map(|hit| hit.score).collect::<Vec<_>>(),
        three_batches
            .iter()
            .map(|hit| hit.score)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_model_with_two_scores_per_pair_is_refused() {
    // The failure this guards is silent, which is why it is worth a test: with
    // 2n scores for n pairs, pairing them up positionally hands chunk 0 the
    // first column of pair 0 and chunk 1 the *second* column of pair 0. Every
    // chunk then carries a plausible number belonging to something else.
    let fixture = fixture::cross_encoder();
    let reranker = OnnxReranker::new(OnnxRerankerConfig::new(
        &fixture.two_headed_model,
        &fixture.tokenizer,
    ))
    .expect("a two-headed model still loads; it is the call that refuses it");

    let error = reranker
        .rerank(&query(), candidates(), &RerankParams::new(3))
        .await
        .unwrap_err();

    assert!(
        matches!(error, ComponentError::Backend(_)),
        "a model that is not a cross-encoder is a backend failure, not the caller's fault: {error}"
    );
    assert_eq!(
        error.to_string(),
        "backend failure: the model returned 6 scores for 3 pairs"
    );
}

/// Twenty in-vocabulary words the query does not contain, then the four it
/// does. The fixture cross-encoder scores lexical overlap, so where the tail
/// survives truncation is readable straight off the score: 4 matches plus the
/// one `[SEP]` both segments share, against that `[SEP]` alone.
fn a_long_passage_answering_at_the_end() -> &'static str {
    "tin price sonnet lines rhyme quarter fell sharply fixed fourteen \
     has and of a the tin price sonnet lines rhyme \
     ragondins build their burrows"
}

async fn score_of_the_long_passage(max_sequence_length: usize) -> f32 {
    let mut config = config();
    config.max_sequence_length = NonZeroUsize::new(max_sequence_length).unwrap();
    let reranker = OnnxReranker::new(config).expect("the fixture cross-encoder loads");

    let reordered = reranker
        .rerank(
            &query(),
            vec![scored("c-long", a_long_passage_answering_at_the_end(), 0.5)],
            &RerankParams::new(1),
        )
        .await
        .unwrap();
    reordered[0].score
}

#[tokio::test]
async fn a_pair_longer_than_max_sequence_length_loses_its_tail() {
    // Nothing else exercises the truncation direction. `LongestFirst` trims the
    // passage, which is the longer segment here, and `Right` takes it off the
    // end — so the four query words parked at the end are what goes, and the
    // score falls to the `[SEP]` both segments share.
    //
    // 20 is the query's six tokens, plus eleven of the passage's twenty-four,
    // plus the three special tokens of a pair.
    assert_eq!(score_of_the_long_passage(512).await, 5.0);
    assert_eq!(score_of_the_long_passage(20).await, 1.0);
}

#[tokio::test]
async fn the_smallest_max_sequence_length_that_leaves_room_for_text_is_accepted() {
    // Four: the `[CLS]` and two `[SEP]`s of a BERT pair, and one token of text.
    // The boundary is worth a test in both directions, because one token either
    // side of it is the difference between a working component and a panic.
    let mut config = config();
    config.max_sequence_length = NonZeroUsize::new(4).unwrap();
    let reranker = OnnxReranker::new(config).expect("four leaves room for one token of text");

    let reordered = reranker
        .rerank(&query(), candidates(), &RerankParams::new(3))
        .await
        .unwrap();

    assert_eq!(reordered.len(), 3);
    assert!(reordered.iter().all(|hit| hit.score.is_finite()));
}

#[tokio::test]
async fn a_max_sequence_length_with_no_room_for_text_is_refused() {
    // `tokenizers` subtracts the special-token count from `max_length` without
    // checking it, in two places. At 1 that underflows: a debug build panics
    // ("attempt to subtract with overflow", tokenizers' `tokenizer/mod.rs`) and
    // a release build wraps to `usize::MAX`, which switches truncation off
    // instead of tightening it. At 3 it does not underflow but leaves no token
    // for text at all. Both are configuration errors, and both must fail the
    // same way in both profiles: here, and at construction.
    for max_sequence_length in [1, 2, 3] {
        let mut config = config();
        config.max_sequence_length = NonZeroUsize::new(max_sequence_length).unwrap();

        let error = OnnxReranker::new(config)
            .err()
            .expect("a limit with no room for text must not build a component");

        assert!(
            matches!(error, ModelError::MaxSequenceLengthTooSmall { .. }),
            "a limit with no room for text is a configuration error: {error}"
        );
        assert_eq!(
            error.to_string(),
            format!(
                "a max_sequence_length of {max_sequence_length} leaves no room for text: \
                 the tokenizer adds 3 special tokens to a pair"
            )
        );
    }
}

#[tokio::test]
async fn an_empty_candidate_list_reranks_to_nothing() {
    // ADR-C19: an empty collection is a valid call and the component does
    // nothing with it. It is not an invalid request, and no model runs.
    let reordered = reranker()
        .rerank(&query(), Vec::new(), &RerankParams::new(5))
        .await
        .unwrap();

    assert!(reordered.is_empty());
}

/// The lowercase hex SHA-256 of a file's bytes, computed here independently of
/// the component, so that the identity's format is pinned rather than echoed.
fn sha256_hex(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).expect("a fixture is readable");
    format!("{:x}", Sha256::digest(bytes))
}

/// A directory of its own under cargo's scratch space for integration tests,
/// emptied first, so a test can rewrite a file at one path.
fn scratch(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("ragondin-reranker-onnx")
        .join(test);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory can be created");
    dir
}

/// A model and a tokenizer at fixed paths in `dir`, holding `model`'s bytes and
/// `tokenizer`.
fn place(dir: &Path, model: &Path, tokenizer: &str) -> OnnxRerankerConfig {
    let (model_at, tokenizer_at) = (dir.join("model.onnx"), dir.join("tokenizer.json"));
    std::fs::copy(model, &model_at).expect("the model fixture copies");
    std::fs::write(&tokenizer_at, tokenizer).expect("the tokenizer is writable");
    OnnxRerankerConfig::new(model_at, tokenizer_at)
}

/// The fixture tokenizer's own text.
fn tokenizer_text() -> String {
    std::fs::read_to_string(&fixture::cross_encoder().tokenizer).expect("the fixture tokenizer")
}

async fn identity_of(config: OnnxRerankerConfig) -> String {
    OnnxReranker::new(config)
        .expect("the fixture cross-encoder loads")
        .model_identity(None)
        .await
        .expect("an ONNX reranker reports its identity for None")
        .as_str()
        .to_string()
}

/// ADR-C32 § 4 fixes the format: the model file's digest, `+`, the tokenizer
/// file's digest, each the lowercase hex SHA-256 of the file's bytes.
#[tokio::test]
async fn the_identity_is_the_digest_of_the_model_then_of_the_tokenizer() {
    let fixture = fixture::cross_encoder();
    let expected = format!(
        "{}+{}",
        sha256_hex(&fixture.model),
        sha256_hex(&fixture.tokenizer)
    );
    assert_eq!(identity_of(config()).await, expected);
}

/// The identity is over the files' contents, never their names: the same
/// bytes at another path are the same model.
#[tokio::test]
async fn the_identity_follows_the_bytes_and_not_the_path() {
    let moved = place(
        &scratch("follows-the-bytes"),
        &fixture::cross_encoder().model,
        &tokenizer_text(),
    );
    assert_eq!(identity_of(moved).await, identity_of(config()).await);
}

#[tokio::test]
async fn the_identity_changes_when_the_model_bytes_change() {
    let fixture = fixture::cross_encoder();
    let dir = scratch("model-bytes-change");
    let before = identity_of(place(&dir, &fixture.model, &tokenizer_text())).await;
    let after = identity_of(place(&dir, &fixture.two_headed_model, &tokenizer_text())).await;
    assert_ne!(
        before, after,
        "another model at the same path is another model"
    );
    assert_eq!(
        before.split('+').nth(1),
        after.split('+').nth(1),
        "the tokenizer's half does not move with the model"
    );
}

/// The tokenizer is what ADR-C32 § 4 adds to the identity: a path alone was
/// hashed before, and a tokenizer rewritten in place changed nothing recorded.
#[tokio::test]
async fn the_identity_changes_when_the_tokenizer_bytes_change() {
    let fixture = fixture::cross_encoder();
    let dir = scratch("tokenizer-bytes-change");
    let original = tokenizer_text();
    let cased = original.replace("\"lowercase\": true", "\"lowercase\": false");
    assert_ne!(original, cased, "the edit must change the tokenizer");
    let before = identity_of(place(&dir, &fixture.model, &original)).await;
    let after = identity_of(place(&dir, &fixture.model, &cased)).await;
    assert_ne!(
        before, after,
        "another tokenizer at the same path is another model"
    );
    assert_eq!(
        before.split('+').next(),
        after.split('+').next(),
        "the model's half does not move with the tokenizer"
    );
}

/// Configured with no served-model name, the component serves none: `None` is
/// the only accepted value, on a call and in `model_identity` (ADR-C32 § 4).
#[tokio::test]
async fn a_served_model_name_is_refused() {
    let reranker = reranker();

    for name in ["bge-reranker", ""] {
        let refused = reranker
            .model_identity(Some(name))
            .await
            .expect_err("an ONNX reranker serves no name");
        assert!(
            matches!(refused, ComponentError::InvalidRequest(_)),
            "{refused}"
        );
    }

    for chunks in [candidates(), Vec::new()] {
        let refused = reranker
            .rerank(
                &query(),
                chunks.clone(),
                &RerankParams::new(3).with_served_model("bge-reranker"),
            )
            .await
            .expect_err("an ONNX reranker serves no name");
        assert!(
            matches!(refused, ComponentError::InvalidRequest(_)),
            "{} chunks: {refused}",
            chunks.len()
        );
    }
}
