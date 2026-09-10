//! Acceptance tests for the BEIR adapter, against a miniature checked-in
//! dataset in real BEIR layout.
//!
//! The fixture is committed, never fetched: a benchmark run must be
//! reproducible from the repository alone, and a test that reaches the network
//! is a test that fails on someone else's machine (§9.1 — freeze the snapshot).

use std::fs;
use std::path::PathBuf;

use ragondin_benchmarks::{BeirAdapter, BenchmarkAdapter, BenchmarkError};
use ragondin_types::{DocId, QueryId};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beir-mini")
}

/// Builds a throwaway BEIR dataset under the OS temp dir for a test that needs
/// a shape the checked-in fixture doesn't cover (a headerless qrels file, a
/// BOM, a null title, ...).
///
/// Adding `tempfile` is out of scope for this crate (dependencies are declared
/// only at the workspace root), so this hand-rolls the same idea: a directory
/// named with both the process id and the test's own name, so parallel test
/// binaries — and parallel test *functions* within one binary — never collide
/// on the same path. It is removed at the start (in case a previous run was
/// killed before its own cleanup ran) and best-effort at the end.
fn write_dataset(test_name: &str, corpus: &str, queries: &str, qrels_test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ragondin-benchmarks-beir-{}-{test_name}",
        std::process::id()
    ));

    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("qrels")).expect("create dataset root and qrels dir");
    fs::write(root.join("corpus.jsonl"), corpus).expect("write corpus.jsonl");
    fs::write(root.join("queries.jsonl"), queries).expect("write queries.jsonl");
    fs::write(root.join("qrels").join("test.tsv"), qrels_test).expect("write qrels/test.tsv");

    root
}

#[test]
fn loads_the_corpus_queries_and_qrels_of_the_test_split() {
    let benchmark = BeirAdapter::new(fixture_root())
        .load()
        .expect("the fixture must load");

    assert_eq!(
        benchmark.corpus().len(),
        5,
        "every corpus line is a document"
    );

    // Four queries are in queries.jsonl; only three are judged in test.tsv,
    // and evaluating the fourth would drag every mean metric toward zero.
    let query_ids: Vec<&str> = benchmark.queries().iter().map(|q| q.id.as_str()).collect();
    assert_eq!(query_ids, vec!["q-1", "q-2", "0042"]);

    let qrels = benchmark.qrels();
    assert_eq!(qrels.judged_query_count(), 3);
    assert_eq!(qrels.judgment_count(), 4);

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
fn a_leading_zero_id_survives_as_a_string_on_every_side() {
    // "4983" already pins that an id is not parsed as a number, but a
    // round-trip through an integer would return "4983" unchanged, so it
    // cannot tell parsing from not-parsing. "0042" can: parsed as a number it
    // comes back "42", and the judgment, the document and the query would then
    // be filed under three ids that no longer agree. The fixture judges 0042
    // against a document of the same id, so the doc, the query and the qrels
    // are each checked against the literal string.
    let benchmark = BeirAdapter::new(fixture_root())
        .load()
        .expect("the fixture must load");

    let doc = benchmark
        .corpus()
        .iter()
        .find(|d| d.id == DocId::new("0042"))
        .expect("0042 is in the fixture corpus");
    assert_eq!(doc.id.as_str(), "0042", "the leading zero must survive");

    let query = benchmark
        .queries()
        .iter()
        .find(|q| q.id == QueryId::new("0042"))
        .expect("0042 is a judged query in the test split");
    assert_eq!(query.id.as_str(), "0042", "the leading zero must survive");

    let relevance = benchmark
        .qrels()
        .for_query(&QueryId::new("0042"))
        .expect("0042 is judged");
    assert_eq!(
        relevance.get(&DocId::new("0042")),
        Some(&3),
        "the judgment must be reachable under the unparsed id on both sides"
    );
    assert_eq!(
        relevance.get(&DocId::new("42")),
        None,
        "no id may have been normalized through a number"
    );
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
    assert_eq!(benchmark.qrels().judgment_count(), 4, "4 data rows, not 5");
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

    assert_eq!(pairs, vec![("q-1", 2), ("q-2", 1), ("0042", 1)]);
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

#[test]
fn a_headerless_qrels_file_keeps_its_first_judgment() {
    // No header row at all: the real first (and only usable) line is data.
    // The old unconditional `has_headers(true)` swallowed it, taking "q1"
    // down with it via the queries-filtered-by-qrels rule.
    let root = write_dataset(
        "headerless_qrels_keeps_first_judgment",
        "{\"_id\": \"d1\", \"text\": \"doc one\"}\n{\"_id\": \"d2\", \"text\": \"doc two\"}\n",
        "{\"_id\": \"q1\", \"text\": \"query one\"}\n{\"_id\": \"q2\", \"text\": \"query two\"}\n",
        "q1\td1\t1\nq2\td2\t2\n",
    );

    let benchmark = BeirAdapter::new(&root)
        .load()
        .expect("a headerless qrels file must still load");

    let query_ids: Vec<&str> = benchmark.queries().iter().map(|q| q.id.as_str()).collect();
    assert_eq!(
        query_ids,
        vec!["q1", "q2"],
        "q1 must not be silently dropped for lack of a header row"
    );
    assert_eq!(benchmark.qrels().judgment_count(), 2);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_stray_space_around_an_id_does_not_empty_the_query_set() {
    // Only the score used to be trimmed. A trailing space on the query id
    // filed the judgment under QueryId("q1 "), which matches no real query,
    // so "q1" vanished from queries() — and, with one query in the file,
    // emptied the query set entirely.
    let root = write_dataset(
        "stray_space_around_id",
        "{\"_id\": \"d1\", \"text\": \"doc one\"}\n",
        "{\"_id\": \"q1\", \"text\": \"query one\"}\n",
        "query-id\tcorpus-id\tscore\nq1 \td1 \t1 \n",
    );

    let benchmark = BeirAdapter::new(&root)
        .load()
        .expect("a stray space around an id must not fail the load");

    assert_eq!(
        benchmark
            .queries()
            .iter()
            .map(|q| q.id.as_str())
            .collect::<Vec<_>>(),
        vec!["q1"],
        "q1 must survive once every field is trimmed, not just the score"
    );
    let judgments = benchmark
        .qrels()
        .for_query(&QueryId::new("q1"))
        .expect("q1 must be judged under its trimmed id");
    assert_eq!(judgments.get(&DocId::new("d1")), Some(&1));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_null_title_is_treated_the_same_as_an_absent_one() {
    // `#[serde(default)]` on a `String` only covers an absent key, not a JSON
    // `null` value; a null title used to abort the whole load with
    // MalformedJson, an unlisted fourth state beyond present/empty/absent.
    let root = write_dataset(
        "null_title",
        "{\"_id\": \"d1\", \"title\": null, \"text\": \"hello world\"}\n",
        "{\"_id\": \"q1\", \"text\": \"query one\"}\n",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\n",
    );

    let benchmark = BeirAdapter::new(&root)
        .load()
        .expect("a null title must load like an absent one");

    let doc = benchmark
        .corpus()
        .iter()
        .find(|d| d.id == DocId::new("d1"))
        .expect("d1 is in the dataset");
    assert_eq!(doc.text, "hello world", "no title, so no leading space");
    assert_eq!(doc.metadata.get("title"), None);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_utf8_bom_on_the_first_line_of_a_jsonl_file_does_not_fail_the_load() {
    // A BOM can only appear at the very start of a file, attached to the
    // first record's first byte; unstripped, `serde_json` sees an invalid
    // leading character and the whole file fails, though every line is
    // otherwise valid JSON.
    let root = write_dataset(
        "utf8_bom_on_first_line",
        "\u{feff}{\"_id\": \"d1\", \"text\": \"hello\"}\n",
        "\u{feff}{\"_id\": \"q1\", \"text\": \"query one\"}\n",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\n",
    );

    let benchmark = BeirAdapter::new(&root)
        .load()
        .expect("a leading BOM must not fail the load");

    assert_eq!(benchmark.corpus().len(), 1);
    assert_eq!(benchmark.corpus()[0].text, "hello");
    assert_eq!(benchmark.queries().len(), 1);
    assert_eq!(benchmark.queries()[0].id, QueryId::new("q1"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_utf8_bom_on_a_headerless_qrels_file_does_not_corrupt_the_first_query_id() {
    // The BOM sits on the first byte of the first *data* row here, with no
    // header row to absorb it: unstripped, the first judgment would be filed
    // under QueryId("\u{feff}q1"), which matches no query, and "q1" would
    // vanish from the run. This pins that the BOM never reaches the id — the
    // property the reader owes its caller — wherever the stripping happens.
    let root = write_dataset(
        "utf8_bom_on_headerless_qrels",
        "{\"_id\": \"d1\", \"text\": \"doc one\"}\n{\"_id\": \"d2\", \"text\": \"doc two\"}\n",
        "{\"_id\": \"q1\", \"text\": \"query one\"}\n{\"_id\": \"q2\", \"text\": \"query two\"}\n",
        "\u{feff}q1\td1\t1\nq2\td2\t2\n",
    );

    let benchmark = BeirAdapter::new(&root)
        .load()
        .expect("a BOM on a headerless qrels file must not fail the load");

    let query_ids: Vec<&str> = benchmark.queries().iter().map(|q| q.id.as_str()).collect();
    assert_eq!(
        query_ids,
        vec!["q1", "q2"],
        "the BOM must not be carried into the first query id"
    );
    assert_eq!(benchmark.qrels().judgment_count(), 2);
    assert!(
        benchmark.qrels().for_query(&QueryId::new("q1")).is_some(),
        "q1 must be judged under its own id, not under a BOM-prefixed one"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_malformed_qrels_row_after_a_blank_line_reports_its_real_line_number() {
    // The blank line at physical line 3 is skipped by the csv reader, not
    // counted; the malformed row is physical line 4. Hand-computed
    // `index + 2` arithmetic assumes no skipped records and reports line 3.
    let root = write_dataset(
        "malformed_row_after_blank_line",
        "{\"_id\": \"d1\", \"text\": \"doc one\"}\n{\"_id\": \"d2\", \"text\": \"doc two\"}\n",
        "{\"_id\": \"q1\", \"text\": \"query one\"}\n{\"_id\": \"q2\", \"text\": \"query two\"}\n",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\n\nq2\td2\tbad\n",
    );

    let error = BeirAdapter::new(&root)
        .load()
        .expect_err("a non-numeric score must be a typed error");

    match &error {
        BenchmarkError::MalformedRecord { line, .. } => {
            assert_eq!(
                *line, 4,
                "the blank line must not shift the reported line number"
            );
        }
        other => panic!("expected a typed MalformedRecord error, got {other:?}"),
    }

    let _ = fs::remove_dir_all(&root);
}

/// Loads a `test.tsv` file exactly as given, bypassing `write_dataset`'s
/// normalization, so CRLF and a missing trailing newline reach the reader
/// byte-for-byte. Reuses a minimal but sufficient corpus/queries pair.
fn write_qrels_dataset(test_name: &str, qrels_test: &str) -> PathBuf {
    write_dataset(
        test_name,
        "{\"_id\": \"d1\", \"text\": \"doc one\"}\n{\"_id\": \"d2\", \"text\": \"doc two\"}\n",
        "{\"_id\": \"q1\", \"text\": \"query one\"}\n{\"_id\": \"q2\", \"text\": \"query two\"}\n",
        qrels_test,
    )
}

fn expect_malformed_line(root: &PathBuf) -> usize {
    expect_malformed(root).0
}

fn expect_malformed(root: &PathBuf) -> (usize, String) {
    let error = BeirAdapter::new(root)
        .load()
        .expect_err("a malformed qrels row must be a typed error");

    match error {
        BenchmarkError::MalformedRecord { line, reason, .. } => (line, reason),
        other => panic!("expected a typed MalformedRecord error, got {other:?}"),
    }
}

#[test]
fn a_malformed_qrels_row_immediately_after_the_header_reports_line_2() {
    let root = write_qrels_dataset(
        "malformed_row_after_header",
        "query-id\tcorpus-id\tscore\nq1\td1\tbad\n",
    );

    assert_eq!(
        expect_malformed_line(&root),
        2,
        "the bad row is the second physical line"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_malformed_qrels_row_after_several_blank_lines_reports_its_true_line() {
    let root = write_qrels_dataset(
        "malformed_row_after_several_blank_lines",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\n\n\n\nq2\td2\tbad\n",
    );

    // Header (1), data (2), three blank lines (3,4,5), bad row is line 6.
    assert_eq!(
        expect_malformed_line(&root),
        6,
        "three skipped blank lines must not shift the reported line number"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_malformed_qrels_row_as_the_last_line_with_no_trailing_newline_reports_its_true_line() {
    // Deliberately no trailing newline on the final line.
    let root = write_qrels_dataset(
        "malformed_last_line_no_trailing_newline",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\nq2\td2\tbad",
    );

    assert_eq!(
        expect_malformed_line(&root),
        3,
        "a malformed last line with no trailing newline is still physical line 3"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_crlf_qrels_file_reports_the_true_line_of_a_malformed_row() {
    let root = write_qrels_dataset(
        "crlf_qrels_malformed_row",
        "query-id\tcorpus-id\tscore\r\nq1\td1\t1\r\nq2\td2\tbad\r\n",
    );

    assert_eq!(
        expect_malformed_line(&root),
        3,
        "CRLF line endings must not shift the reported line number"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_crlf_qrels_file_with_a_blank_line_reports_the_true_line_of_a_malformed_row() {
    let root = write_qrels_dataset(
        "crlf_qrels_blank_line_malformed_row",
        "query-id\tcorpus-id\tscore\r\nq1\td1\t1\r\n\r\nq2\td2\tbad\r\n",
    );

    assert_eq!(
        expect_malformed_line(&root),
        4,
        "a blank CRLF line must not shift the reported line number"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_qrels_row_with_too_few_fields_reports_the_true_line_and_never_zero() {
    // Previously asserted only the line number, when the row was still
    // rejected via `record.get(2)` returning `None` ("missing score"). A
    // 2-field row now fails the explicit field-count check first, so this
    // also pins the new reason text to make sure that's the path taken.
    let root = write_qrels_dataset(
        "too_few_fields",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\nq2\tonly-two-columns\n",
    );

    let (line, reason) = expect_malformed(&root);
    assert_eq!(line, 3, "the short row is physical line 3");
    assert_ne!(line, 0, "0 is not a physical line in a 1-based file");
    assert!(
        reason.contains("expected 3 fields") && reason.contains('2'),
        "expected a field-count reason, got {reason:?}"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_qrels_data_row_with_a_stray_extra_field_is_a_typed_error() {
    // Previously loaded silently, discarding the 4th column: a per-line
    // `csv::Reader` has no previous row to compare field counts against, so
    // the whole-file reader's implicit rejection was lost when parsing moved
    // to one line at a time.
    let root = write_qrels_dataset(
        "data_row_stray_extra_field",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\nq2\td2\t1\tstray-4th-column\n",
    );

    let (line, reason) = expect_malformed(&root);
    assert_eq!(line, 3, "the malformed row is physical line 3");
    assert!(
        reason.contains("expected 3 fields") && reason.contains('4'),
        "expected a field-count reason, got {reason:?}"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_four_column_qrels_header_is_a_typed_error() {
    // A header whose own column count disagrees with the data rows means the
    // file's shape is wrong regardless of which row notices it first, so the
    // field-count check must fire on the header record too.
    let root = write_qrels_dataset(
        "four_column_header",
        "query-id\tcorpus-id\tscore\textra\nq1\td1\t1\n",
    );

    let (line, reason) = expect_malformed(&root);
    assert_eq!(line, 1, "the malformed header is physical line 1");
    assert!(
        reason.contains("expected 3 fields") && reason.contains('4'),
        "expected a field-count reason, got {reason:?}"
    );

    let _ = fs::remove_dir_all(&root);
}

/// Qrels that name no query the dataset defines is a typed error, not an
/// `Ok` benchmark with nothing to evaluate.
///
/// This is the symptom three separate bugs on this branch produced — a
/// swallowed headerless first row, a one-sided trim, a BOM welded onto the
/// first id — each of which ended with judgments loaded, every query filtered
/// away, and no error anywhere. The guard names the symptom itself, so the
/// fourth cause of it is diagnosed at the load rather than at the score.
#[test]
fn qrels_matching_no_query_at_all_is_a_typed_error_naming_the_qrels_file() {
    let root = write_dataset(
        "qrels-name-unknown-queries",
        "{\"_id\": \"d1\", \"text\": \"doc one\"}\n",
        "{\"_id\": \"q1\", \"text\": \"query one\"}\n",
        "query-id\tcorpus-id\tscore\nq-other\td1\t1\nq-also-other\td1\t2\n",
    );

    let error = BeirAdapter::new(&root)
        .load()
        .expect_err("qrels that match no query must not load Ok with zero queries");

    match &error {
        BenchmarkError::NoJudgedQuery { path, judged } => {
            assert!(
                path.ends_with("qrels/test.tsv"),
                "the error must name the qrels file, got {path:?}"
            );
            assert_eq!(*judged, 2, "both judgments must be counted");
        }
        other => panic!("expected a typed NoJudgedQuery error, got {other:?}"),
    }

    let message = error.to_string();
    assert!(
        message.contains("test.tsv") && message.contains('2'),
        "the message must name the qrels path and the judgment count, got {message:?}"
    );

    let _ = fs::remove_dir_all(&root);
}

/// The mirror image: empty qrels must stay a normal load.
///
/// A guard written as "no queries is an error" rather than "judgments but no
/// queries is an error" would reject a dataset that simply has nothing judged
/// in the split, which is a legitimate — if useless — file, not a corruption.
#[test]
fn an_empty_qrels_file_still_loads_with_no_queries_and_no_error() {
    let root = write_dataset(
        "empty-qrels-loads",
        "{\"_id\": \"d1\", \"text\": \"doc one\"}\n",
        "{\"_id\": \"q1\", \"text\": \"query one\"}\n",
        "query-id\tcorpus-id\tscore\n",
    );

    let benchmark = BeirAdapter::new(&root)
        .load()
        .expect("a qrels file with a header and no judgments is not a corrupt file");

    assert!(benchmark.qrels().is_empty());
    assert!(benchmark.queries().is_empty());
    assert_eq!(benchmark.corpus().len(), 1);

    let _ = fs::remove_dir_all(&root);
}

/// Ids are trimmed on **both** sides — the qrels TSV and the JSONL files.
///
/// Trimming only the qrels side is the shape that silently breaks a dataset
/// whose ids carry the same surrounding whitespace in every file: the qrels
/// id becomes `q1` while the query id stays `q1 `, they stop matching, and
/// `read_queries` filters the query out. Untrimmed on both sides used to
/// match; trimmed on both sides matches too. One side only is the broken case
/// — and on a dataset with more than one query it drops only the affected
/// ones, which the `NoJudgedQuery` guard cannot see. Trimming is the fix; the
/// guard is only the last line.
#[test]
fn ids_carrying_the_same_whitespace_in_every_file_still_match() {
    let root = write_dataset(
        "symmetric-whitespace-ids",
        "{\"_id\": \"d1 \", \"text\": \"body\"}\n",
        "{\"_id\": \"q1 \", \"text\": \"question\"}\n",
        "query-id\tcorpus-id\tscore\nq1 \td1 \t1\n",
    );

    let benchmark = BeirAdapter::new(&root).load().expect("the dataset loads");

    assert_eq!(
        benchmark.queries().len(),
        1,
        "the judged query must survive the qrels filter"
    );
    assert_eq!(benchmark.queries()[0].id, QueryId::new("q1"));
    assert_eq!(
        benchmark.corpus()[0].id,
        DocId::new("d1"),
        "a corpus id must be trimmed the same way a qrels corpus-id is, or a \
         retrieved document can never match its own judgment"
    );

    let relevance = benchmark
        .qrels()
        .for_query(&QueryId::new("q1"))
        .expect("the query is judged");
    assert_eq!(
        relevance.get(&DocId::new("d1")),
        Some(&1),
        "the judgment must be reachable under the same id the corpus carries"
    );

    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------- review findings

/// The header heuristic must not swallow a *corrupt* first data row.
///
/// `is_first_record && score.is_err()` cannot tell BEIR's literal `score`
/// header from a judgment whose score is malformed, so a headerless file
/// whose first row is corrupt lost that judgment without a word — and the
/// query with it, via the qrels filter. The identical corruption one line
/// later was already a hard error, so corruption was tolerated on line 1 and
/// nowhere else. This is the "judgment lost, query filtered out, `Ok`
/// returned" state `NoJudgedQuery` exists for, reached past the guard.
#[test]
fn a_corrupt_first_qrels_row_is_an_error_not_a_header() {
    let root = write_qrels_dataset("corrupt-first-row", "q1\td1\tNOT_A_GRADE\nq2\td2\t2\n");

    let (line, reason) = expect_malformed(&root);
    assert_eq!(line, 1, "the corrupt row is the first physical line");
    assert!(
        reason.contains("NOT_A_GRADE"),
        "the offending value must reach the operator: {reason}"
    );

    let _ = fs::remove_dir_all(&root);
}

/// The deviation the header heuristic exists for must survive the fix above:
/// a genuinely headerless file still loads, keeping its first judgment.
#[test]
fn a_headerless_qrels_file_still_keeps_its_first_judgment() {
    let root = write_qrels_dataset("headerless-still-loads", "q1\td1\t1\nq2\td2\t2\n");

    let benchmark = BeirAdapter::new(&root).load().expect("the dataset loads");

    assert_eq!(
        benchmark.qrels().judgment_count(),
        2,
        "no row of a headerless file may be dropped"
    );

    let _ = fs::remove_dir_all(&root);
}

/// `NoJudgedQuery` guards the query side of the corpus/qrels agreement. The
/// document side had no guard: a `qrels/` directory taken from a different
/// snapshot than `corpus.jsonl` — or a mirror that re-cases or prefixes doc
/// ids — loaded `Ok` over a **full** query set and reported nDCG@10 = 0.
/// Nothing looks empty in that state, which makes it harder to diagnose than
/// the one the existing guard already names.
#[test]
fn qrels_naming_no_document_of_the_corpus_is_a_typed_error() {
    let root = write_dataset(
        "qrels-match-no-document",
        "{\"_id\": \"d1\", \"text\": \"body\"}\n",
        "{\"_id\": \"q1\", \"text\": \"question\"}\n",
        "query-id\tcorpus-id\tscore\nq1\tPREFIX-d1\t1\n",
    );

    let error = BeirAdapter::new(&root)
        .load()
        .expect_err("judgments that name no document score nothing");

    match error {
        BenchmarkError::NoJudgedDocument { judged, .. } => assert_eq!(judged, 1),
        other => panic!("expected NoJudgedDocument, got {other:?}"),
    }

    let _ = fs::remove_dir_all(&root);
}

/// A partial document mismatch stays a normal load, mirroring the query-side
/// guard: a benchmark may legitimately judge a document its corpus does not
/// hold, and `trec_eval` counts such a judgment in the denominator. Only the
/// *total* mismatch is a corrupt dataset.
#[test]
fn a_partial_document_mismatch_still_loads() {
    let root = write_dataset(
        "partial-document-mismatch",
        "{\"_id\": \"d1\", \"text\": \"body\"}\n",
        "{\"_id\": \"q1\", \"text\": \"question\"}\n",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\nq1\tGONE\t1\n",
    );

    let benchmark = BeirAdapter::new(&root).load().expect("the dataset loads");
    assert_eq!(benchmark.qrels().judgment_count(), 2);

    let _ = fs::remove_dir_all(&root);
}

/// A repeated `_id` double-weights a query in the macro-average *and* inflates
/// the denominator `Benchmark::queries().len()` is documented to be. Contrast
/// `Qrels::insert`, where the repeated-pair case is explicitly last-wins: the
/// JSONL readers had no equivalent rule.
#[test]
fn a_duplicate_query_id_is_a_typed_error() {
    let root = write_dataset(
        "duplicate-query-id",
        "{\"_id\": \"d1\", \"text\": \"body\"}\n",
        "{\"_id\": \"q1\", \"text\": \"one\"}\n{\"_id\": \"q1\", \"text\": \"again\"}\n",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\n",
    );

    let error = BeirAdapter::new(&root)
        .load()
        .expect_err("one id names one query");

    match error {
        BenchmarkError::DuplicateId { id, line, .. } => {
            assert_eq!(id, "q1");
            assert_eq!(line, 2, "the second occurrence is the offending line");
        }
        other => panic!("expected DuplicateId, got {other:?}"),
    }

    let _ = fs::remove_dir_all(&root);
}

/// The corpus side of the same rule: two documents under one `DocId` let a
/// single judgment be satisfied twice in one ranked list.
#[test]
fn a_duplicate_document_id_is_a_typed_error() {
    let root = write_dataset(
        "duplicate-document-id",
        "{\"_id\": \"d1\", \"text\": \"one\"}\n{\"_id\": \"d1\", \"text\": \"again\"}\n",
        "{\"_id\": \"q1\", \"text\": \"question\"}\n",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\n",
    );

    let error = BeirAdapter::new(&root)
        .load()
        .expect_err("one id names one document");

    match error {
        BenchmarkError::DuplicateId { id, .. } => assert_eq!(id, "d1"),
        other => panic!("expected DuplicateId, got {other:?}"),
    }

    let _ = fs::remove_dir_all(&root);
}

/// A whitespace-only title is semantically absent, so it must contribute
/// neither itself nor a separating space — the rule the module documents.
/// `title.is_empty()` tested the untrimmed string, so `"   "` counted as a
/// title and welded four leading spaces onto the indexed text, and stored
/// whitespace under `metadata["title"]`. Ids on this branch are trimmed
/// everywhere for exactly this reason.
#[test]
fn a_whitespace_only_title_is_treated_as_absent() {
    let root = write_dataset(
        "whitespace-only-title",
        "{\"_id\": \"d1\", \"title\": \"   \", \"text\": \"body\"}\n",
        "{\"_id\": \"q1\", \"text\": \"question\"}\n",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\n",
    );

    let benchmark = BeirAdapter::new(&root).load().expect("the dataset loads");
    let document = &benchmark.corpus()[0];

    assert_eq!(
        document.text, "body",
        "no title, and so no separating space"
    );
    assert!(
        !document.metadata.contains_key("title"),
        "whitespace is not a title worth preserving"
    );

    let _ = fs::remove_dir_all(&root);
}

/// A title with surrounding whitespace keeps its content and loses the
/// padding, so the concatenation has exactly one separating space.
#[test]
fn a_padded_title_is_trimmed_before_the_concatenation() {
    let root = write_dataset(
        "padded-title",
        "{\"_id\": \"d1\", \"title\": \"  The Title  \", \"text\": \"body\"}\n",
        "{\"_id\": \"q1\", \"text\": \"question\"}\n",
        "query-id\tcorpus-id\tscore\nq1\td1\t1\n",
    );

    let benchmark = BeirAdapter::new(&root).load().expect("the dataset loads");
    let document = &benchmark.corpus()[0];

    assert_eq!(document.text, "The Title body");
    assert_eq!(
        document.metadata.get("title").map(String::as_str),
        Some("The Title")
    );

    let _ = fs::remove_dir_all(&root);
}
