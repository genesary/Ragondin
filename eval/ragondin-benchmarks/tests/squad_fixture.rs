//! Acceptance tests for the SQuAD v1.1 adapter, against a miniature,
//! hand-written dataset in the layout of `dev-v1.1.json`.
//!
//! The fixture quotes no text of the real dev file, so it carries no licence
//! notice; it is committed rather than fetched for the reason `beir_fixture.rs`
//! gives — a benchmark test must be reproducible from the repository alone.

use std::fs;
use std::path::PathBuf;

use ragondin_benchmarks::{BenchmarkAdapter, BenchmarkError, CarriedPieces, SquadAdapter};
use ragondin_types::{DocId, QueryId};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/squad-mini")
}

/// Writes one SQuAD-format file under a throwaway directory named after the
/// test, the way `beir_fixture.rs` builds its throwaway datasets.
fn write_squad(test_name: &str, file_name: &str, contents: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ragondin-benchmarks-squad-{}-{test_name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create dataset root");
    fs::write(root.join(file_name), contents).expect("write the SQuAD file");
    root
}

/// A one-question dataset whose question carries `answers_field` verbatim —
/// the JSON after `"answers":`, or nothing at all when it is `None`.
fn one_question(answers_field: Option<&str>) -> String {
    let answers = answers_field
        .map(|field| format!(r#", "answers": {field}"#))
        .unwrap_or_default();
    format!(
        r#"{{"version": "1.1", "data": [{{"title": "T", "paragraphs": [{{"context": "c", "qas": [{{"id": "q-x", "question": "?"{answers}}}]}}]}}]}}"#
    )
}

#[test]
fn loads_paragraphs_questions_qrels_and_references_in_file_order() {
    let benchmark = SquadAdapter::new(fixture_root())
        .load()
        .expect("the fixture must load");

    let doc_ids: Vec<&str> = benchmark.corpus().iter().map(|d| d.id.as_str()).collect();
    assert_eq!(doc_ids, vec!["River_otter#0", "River_otter#1", "Estuary#0"]);

    let query_ids: Vec<&str> = benchmark.queries().iter().map(|q| q.id.as_str()).collect();
    assert_eq!(query_ids, vec!["q0001", "q0002", "q0003", "q0004"]);

    let qrels = benchmark.qrels();
    assert_eq!(qrels.judged_query_count(), 4);
    assert_eq!(qrels.judgment_count(), 4);

    let references = benchmark.reference_answers();
    assert_eq!(references.answered_query_count(), 4);
    assert_eq!(references.answer_count(), 9);

    assert_eq!(benchmark.carries(), CarriedPieces::QrelsAndReferenceAnswers);
}

#[test]
fn a_paragraph_is_a_document_titled_by_its_article() {
    let benchmark = SquadAdapter::new(fixture_root()).load().expect("loads");

    let second = &benchmark.corpus()[1];
    assert_eq!(second.id, DocId::new("River_otter#1"));
    assert_eq!(
        second.text,
        "Otters were once trapped for their fur across the whole of the continent."
    );
    assert_eq!(
        second.metadata.get("title").map(String::as_str),
        Some("River_otter")
    );
    assert_eq!(second.metadata.len(), 1);
}

#[test]
fn a_question_judges_its_own_paragraph_at_grade_one() {
    let benchmark = SquadAdapter::new(fixture_root()).load().expect("loads");

    let q3 = benchmark
        .qrels()
        .for_query(&QueryId::new("q0003"))
        .expect("q0003 is judged");
    assert_eq!(q3.len(), 1);
    assert_eq!(q3.get(&DocId::new("River_otter#1")), Some(&1));

    assert_eq!(benchmark.queries()[2].text, "What were otters trapped for?");
}

#[test]
fn references_keep_file_order_and_repeats() {
    let benchmark = SquadAdapter::new(fixture_root()).load().expect("loads");

    assert_eq!(
        benchmark
            .reference_answers()
            .for_query(&QueryId::new("q0001")),
        Some(
            &[
                "in the bank of a stream".to_string(),
                "bank of a stream".to_string(),
                "in the bank of a stream".to_string(),
            ][..]
        )
    );

    let walked: Vec<(&str, usize, Vec<&str>)> = benchmark
        .iter_with_references()
        .map(|(query, relevance, references)| {
            (
                query.id.as_str(),
                relevance.len(),
                references.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        walked,
        vec![
            (
                "q0001",
                1,
                vec![
                    "in the bank of a stream",
                    "bank of a stream",
                    "in the bank of a stream"
                ]
            ),
            ("q0002", 1, vec!["at dusk"]),
            ("q0003", 1, vec!["their fur", "fur"]),
            ("q0004", 1, vec!["brackish", "brackish", "is brackish"]),
        ]
    );
}

#[test]
fn the_file_name_is_selectable_through_the_constructor() {
    let root = write_squad(
        "file-name",
        "train-v1.1.json",
        &one_question(Some(r#"[{"answer_start": 0, "text": "c"}]"#)),
    );

    let benchmark = SquadAdapter::with_file(&root, "train-v1.1.json")
        .load()
        .expect("the named file loads");
    assert_eq!(benchmark.queries().len(), 1);
    assert_eq!(benchmark.corpus()[0].id, DocId::new("T#0"));

    // The default name is not silently substituted: this directory has none.
    let error = SquadAdapter::new(&root)
        .load()
        .expect_err("dev-v1.1.json is absent");
    match error {
        BenchmarkError::Io { path, .. } => assert_eq!(path, root.join("dev-v1.1.json")),
        other => panic!("expected Io, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_question_with_no_answers_field_is_an_error_naming_the_file_and_the_question() {
    let root = write_squad("absent-answers", "dev-v1.1.json", &one_question(None));

    let error = SquadAdapter::new(&root).load().expect_err("must fail");
    match &error {
        BenchmarkError::NoReferenceAnswer { path, line, id } => {
            assert_eq!(path, &root.join("dev-v1.1.json"));
            // A JSON document read whole has no line to give.
            assert_eq!(*line, None);
            assert_eq!(id, "q-x");
        }
        other => panic!("expected NoReferenceAnswer, got {other:?}"),
    }
    assert!(error.to_string().contains("dev-v1.1.json"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_question_with_an_empty_or_null_answers_list_is_the_same_error() {
    for (name, field) in [("empty-answers", "[]"), ("null-answers", "null")] {
        let root = write_squad(name, "dev-v1.1.json", &one_question(Some(field)));
        let error = SquadAdapter::new(&root).load().expect_err("must fail");
        assert!(
            matches!(&error, BenchmarkError::NoReferenceAnswer { id, .. } if id == "q-x"),
            "{name}: expected NoReferenceAnswer, got {error:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}

#[test]
fn malformed_json_is_an_error_naming_the_file_and_the_line() {
    let root = write_squad(
        "malformed",
        "dev-v1.1.json",
        "{\"version\": \"1.1\",\n\"data\": [ oops ]}",
    );

    let error = SquadAdapter::new(&root).load().expect_err("must fail");
    match &error {
        BenchmarkError::MalformedJson { path, line, .. } => {
            assert_eq!(path, &root.join("dev-v1.1.json"));
            assert_eq!(*line, 2);
        }
        other => panic!("expected MalformedJson, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_repeated_question_id_is_an_error_naming_the_file_and_the_id() {
    let root = write_squad(
        "repeated-question",
        "dev-v1.1.json",
        r#"{"data": [{"title": "T", "paragraphs": [{"context": "c", "qas": [
            {"id": "q-x", "question": "?", "answers": [{"text": "c"}]},
            {"id": "q-x", "question": "?", "answers": [{"text": "c"}]}
        ]}]}]}"#,
    );

    let error = SquadAdapter::new(&root).load().expect_err("must fail");
    match &error {
        BenchmarkError::DuplicateRecord { path, id } => {
            assert_eq!(path, &root.join("dev-v1.1.json"));
            assert_eq!(id, "q-x");
        }
        other => panic!("expected DuplicateRecord, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_repeated_article_title_is_an_error_naming_the_colliding_document_id() {
    // Two articles under one title would give their paragraphs the same
    // `DocId`s, and a judgment would then name two documents at once.
    let root = write_squad(
        "repeated-title",
        "dev-v1.1.json",
        r#"{"data": [
            {"title": "T", "paragraphs": [{"context": "c", "qas": [{"id": "q-1", "question": "?", "answers": [{"text": "c"}]}]}]},
            {"title": "T", "paragraphs": [{"context": "d", "qas": [{"id": "q-2", "question": "?", "answers": [{"text": "d"}]}]}]}
        ]}"#,
    );

    let error = SquadAdapter::new(&root).load().expect_err("must fail");
    assert!(
        matches!(&error, BenchmarkError::DuplicateRecord { id, .. } if id == "T#0"),
        "expected DuplicateRecord for T#0, got {error:?}"
    );
    let _ = fs::remove_dir_all(&root);
}
