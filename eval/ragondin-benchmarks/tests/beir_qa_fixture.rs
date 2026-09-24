//! Acceptance tests for the BEIR adapter's reference-answer reading path — a
//! BEIR directory carrying `answers.jsonl` — against a miniature checked-in
//! dataset, `beir-qa-mini`. The M2 fixture, `beir-mini`, has no such file and
//! is not touched here.

use std::fs;
use std::path::PathBuf;

use ragondin_benchmarks::{BeirAdapter, BenchmarkAdapter, BenchmarkError, CarriedPieces};
use ragondin_types::QueryId;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beir-qa-mini")
}

fn m2_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beir-mini")
}

/// A throwaway copy of `beir-qa-mini` whose `answers.jsonl` is `answers`.
fn with_answers(test_name: &str, answers: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ragondin-benchmarks-beir-qa-{}-{test_name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("qrels")).expect("create dataset root");
    for file in ["corpus.jsonl", "queries.jsonl", "qrels/test.tsv"] {
        fs::copy(fixture_root().join(file), root.join(file)).expect("copy fixture file");
    }
    fs::write(root.join("answers.jsonl"), answers).expect("write answers.jsonl");
    root
}

#[test]
fn the_reference_path_loads_answers_beside_the_qrels() {
    let benchmark = BeirAdapter::new(fixture_root())
        .with_reference_answers()
        .load()
        .expect("the fixture must load");

    assert_eq!(benchmark.corpus().len(), 3);
    let query_ids: Vec<&str> = benchmark.queries().iter().map(|q| q.id.as_str()).collect();
    assert_eq!(query_ids, vec!["q-1", "q-2", "q-3"]);
    assert_eq!(benchmark.qrels().judgment_count(), 3);

    let references = benchmark.reference_answers();
    // q-4 has a line but is not in the test split, so it is not in the
    // benchmark; q-3 is in the split and has no line, so it carries nothing.
    assert_eq!(references.answered_query_count(), 2);
    assert_eq!(references.answer_count(), 4);
    assert_eq!(
        references.for_query(&QueryId::new("q-1")),
        Some(
            &[
                "in the bank of a stream".to_string(),
                "bank of a stream".to_string(),
                "in the bank of a stream".to_string(),
            ][..]
        )
    );
    assert_eq!(references.for_query(&QueryId::new("q-3")), None);
    assert_eq!(references.for_query(&QueryId::new("q-4")), None);

    assert_eq!(benchmark.carries(), CarriedPieces::QrelsAndReferenceAnswers);
}

#[test]
fn a_query_with_no_line_iterates_with_no_reference() {
    let benchmark = BeirAdapter::new(fixture_root())
        .with_reference_answers()
        .load()
        .expect("loads");

    let q3 = benchmark
        .iter_with_references()
        .find(|(query, _, _)| query.id.as_str() == "q-3")
        .expect("q-3 is in the split");
    assert!(q3.2.is_empty());
}

#[test]
fn the_plain_path_ignores_answers_jsonl() {
    let benchmark = BeirAdapter::new(fixture_root())
        .load()
        .expect("the plain path loads");

    assert!(benchmark.reference_answers().is_empty());
    assert_eq!(benchmark.carries(), CarriedPieces::QrelsOnly);
}

#[test]
fn the_m2_fixture_stays_reference_free_on_the_plain_path() {
    let benchmark = BeirAdapter::new(m2_fixture_root())
        .load()
        .expect("the M2 fixture loads");
    assert_eq!(benchmark.carries(), CarriedPieces::QrelsOnly);
}

#[test]
fn the_reference_path_requires_answers_jsonl() {
    let error = BeirAdapter::new(m2_fixture_root())
        .with_reference_answers()
        .load()
        .expect_err("beir-mini has no answers.jsonl");
    match error {
        BenchmarkError::Io { path, .. } => {
            assert_eq!(path, m2_fixture_root().join("answers.jsonl"));
        }
        other => panic!("expected Io, got {other:?}"),
    }
}

#[test]
fn an_id_naming_no_query_is_an_error_naming_the_file_and_the_id() {
    let root = with_answers(
        "unknown-id",
        "{\"_id\": \"q-1\", \"answers\": [\"a\"]}\n{\"_id\": \"q-99\", \"answers\": [\"b\"]}\n",
    );

    let error = BeirAdapter::new(&root)
        .with_reference_answers()
        .load()
        .expect_err("must fail");
    match &error {
        BenchmarkError::UnknownQuery { path, line, id } => {
            assert_eq!(path, &root.join("answers.jsonl"));
            assert_eq!(*line, 2);
            assert_eq!(id, "q-99");
        }
        other => panic!("expected UnknownQuery, got {other:?}"),
    }
    let message = error.to_string();
    assert!(message.contains("answers.jsonl") && message.contains("q-99"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_id_on_two_lines_is_an_error_naming_the_file_and_the_id() {
    let root = with_answers(
        "repeated-id",
        "{\"_id\": \"q-1\", \"answers\": [\"a\"]}\n{\"_id\": \"q-1\", \"answers\": [\"b\"]}\n",
    );

    let error = BeirAdapter::new(&root)
        .with_reference_answers()
        .load()
        .expect_err("must fail");
    match &error {
        BenchmarkError::DuplicateId { path, line, id } => {
            assert_eq!(path, &root.join("answers.jsonl"));
            assert_eq!(*line, 2);
            assert_eq!(id, "q-1");
        }
        other => panic!("expected DuplicateId, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_line_listing_no_answer_is_an_error_naming_the_file_and_the_id() {
    let root = with_answers("empty-list", "{\"_id\": \"q-2\", \"answers\": []}\n");

    let error = BeirAdapter::new(&root)
        .with_reference_answers()
        .load()
        .expect_err("must fail");
    match &error {
        BenchmarkError::NoReferenceAnswer { path, line, id } => {
            assert_eq!(path, &root.join("answers.jsonl"));
            assert_eq!(*line, Some(1));
            assert_eq!(id, "q-2");
        }
        other => panic!("expected NoReferenceAnswer, got {other:?}"),
    }
    assert!(error.to_string().contains("answers.jsonl:1:"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_malformed_line_is_an_error_naming_the_file_and_the_line() {
    let root = with_answers(
        "malformed",
        "{\"_id\": \"q-1\", \"answers\": [\"a\"]}\n{\"_id\": \"q-2\"}\n",
    );

    let error = BeirAdapter::new(&root)
        .with_reference_answers()
        .load()
        .expect_err("must fail");
    match &error {
        BenchmarkError::MalformedJson { path, line, .. } => {
            assert_eq!(path, &root.join("answers.jsonl"));
            assert_eq!(*line, 2);
        }
        other => panic!("expected MalformedJson, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_id_known_to_queries_jsonl_but_outside_the_split_is_accepted() {
    // `queries.jsonl` spans every split; an answers line for a train query is
    // a line about a real query, not an unknown id.
    let root = with_answers("other-split", "{\"_id\": \"q-4\", \"answers\": [\"a\"]}\n");

    let benchmark = BeirAdapter::new(&root)
        .with_reference_answers()
        .load()
        .expect("loads");
    assert!(benchmark.reference_answers().is_empty());
    assert_eq!(benchmark.carries(), CarriedPieces::QrelsOnly);
    let _ = fs::remove_dir_all(&root);
}
