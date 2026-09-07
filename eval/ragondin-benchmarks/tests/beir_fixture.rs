//! Acceptance tests for the BEIR adapter, against a miniature checked-in
//! dataset in real BEIR layout.
//!
//! The fixture is committed, never fetched: a benchmark run must be
//! reproducible from the repository alone, and a test that reaches the network
//! is a test that fails on someone else's machine (§9.1 — freeze the snapshot).

use std::path::PathBuf;

use ragondin_benchmarks::{BeirAdapter, BenchmarkAdapter, BenchmarkError};
use ragondin_types::{DocId, QueryId};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beir-mini")
}

#[test]
fn loads_the_corpus_queries_and_qrels_of_the_test_split() {
    let benchmark = BeirAdapter::new(fixture_root())
        .load()
        .expect("the fixture must load");

    assert_eq!(
        benchmark.corpus().len(),
        4,
        "every corpus line is a document"
    );

    // Three queries are in queries.jsonl; only two are judged in test.tsv, and
    // evaluating the third would drag every mean metric toward zero.
    let query_ids: Vec<&str> = benchmark.queries().iter().map(|q| q.id.as_str()).collect();
    assert_eq!(query_ids, vec!["q-1", "q-2"]);

    let qrels = benchmark.qrels();
    assert_eq!(qrels.judged_query_count(), 2);
    assert_eq!(qrels.judgment_count(), 3);

    let q1 = qrels
        .for_query(&QueryId::new("q-1"))
        .expect("q-1 is judged");
    assert_eq!(q1.get(&DocId::new("MED-10")), Some(&2));
    // Judged and irrelevant is not the same as unjudged: grade 0 must survive.
    assert_eq!(q1.get(&DocId::new("MED-12")), Some(&0));

    let q2 = qrels
        .for_query(&QueryId::new("q-2"))
        .expect("q-2 is judged");
    // Ids are opaque strings: "4983" must not have been parsed as a number.
    assert_eq!(q2.get(&DocId::new("4983")), Some(&1));
}

#[test]
fn the_qrels_header_row_is_not_ingested_as_a_judgment() {
    let benchmark = BeirAdapter::new(fixture_root())
        .load()
        .expect("the fixture must load");

    assert!(
        benchmark
            .qrels()
            .for_query(&QueryId::new("query-id"))
            .is_none(),
        "the header row must not become a judgment for a query called query-id"
    );
    assert_eq!(benchmark.qrels().judgment_count(), 3, "3 data rows, not 4");
}

#[test]
fn iteration_pairs_each_query_with_its_own_judgments() {
    let benchmark = BeirAdapter::new(fixture_root())
        .load()
        .expect("the fixture must load");

    let pairs: Vec<(&str, usize)> = benchmark
        .iter()
        .map(|(query, relevance)| (query.id.as_str(), relevance.len()))
        .collect();

    assert_eq!(pairs, vec![("q-1", 2), ("q-2", 1)]);
}

#[test]
fn document_text_is_the_title_then_one_space_then_the_text() {
    let benchmark = BeirAdapter::new(fixture_root())
        .load()
        .expect("the fixture must load");

    let doc = benchmark
        .corpus()
        .iter()
        .find(|d| d.id == DocId::new("MED-10"))
        .expect("MED-10 is in the fixture");

    // BEIR's own evaluation indexes title + text, and every published
    // leaderboard number is computed that way. Indexing text alone moves
    // nDCG@10 on most BEIR datasets.
    assert_eq!(doc.text, "Cats and mats The cat sat on the mat.");
    // The raw title is kept, so the concatenation loses nothing.
    assert_eq!(
        doc.metadata.get("title").map(String::as_str),
        Some("Cats and mats")
    );
}

#[test]
fn an_empty_or_absent_title_contributes_neither_itself_nor_a_space() {
    let benchmark = BeirAdapter::new(fixture_root())
        .load()
        .expect("the fixture must load");

    let empty_title = benchmark
        .corpus()
        .iter()
        .find(|d| d.id == DocId::new("MED-11"))
        .expect("MED-11 is in the fixture");
    assert_eq!(
        empty_title.text,
        "A document whose title is present but empty."
    );
    assert_eq!(empty_title.metadata.get("title"), None);

    let no_title = benchmark
        .corpus()
        .iter()
        .find(|d| d.id == DocId::new("4983"))
        .expect("4983 is in the fixture");
    assert_eq!(no_title.text, "A document with no title field at all.");
    assert_eq!(no_title.metadata.get("title"), None);
}

#[test]
fn a_different_split_is_read_from_its_own_file() {
    let benchmark = BeirAdapter::with_split(fixture_root(), "train")
        .load()
        .expect("the train split must load");

    let query_ids: Vec<&str> = benchmark.queries().iter().map(|q| q.id.as_str()).collect();
    assert_eq!(query_ids, vec!["q-3"]);
    assert_eq!(benchmark.qrels().judgment_count(), 1);
}

#[test]
fn a_missing_qrels_file_is_a_typed_error_naming_the_path() {
    // The fixture ships no dev split — splits vary by dataset, so this is the
    // most common real failure and must not be a panic or a silent empty load.
    let error = BeirAdapter::with_split(fixture_root(), "dev")
        .load()
        .expect_err("the dev split does not exist");

    match &error {
        BenchmarkError::Io { path, .. } => {
            assert!(
                path.ends_with("qrels/dev.tsv"),
                "the error must name the missing file, got {path:?}"
            );
        }
        other => panic!("expected a typed Io error, got {other:?}"),
    }
    assert!(error.to_string().contains("dev.tsv"));
}
