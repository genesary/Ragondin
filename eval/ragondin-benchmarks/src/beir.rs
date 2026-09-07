//! The BEIR adapter: a BEIR dataset directory on disk → a [`Benchmark`].
//!
//! # The layout
//!
//! ```text
//! corpus.jsonl        {"_id": "...", "title": "...", "text": "...", "metadata": {...}}
//! queries.jsonl       {"_id": "...", "text": "...", "metadata": {...}}
//! qrels/test.tsv      TAB-separated, typically WITH a header row: query-id/corpus-id/score
//! qrels/train.tsv     same shape; splits vary by dataset
//! ```
//!
//! # Three things that bite, and what this reader does about them
//!
//! - **The qrels TSV *usually* has a header line, but not always.** Read as
//!   data, a header adds a judgment for a query literally called `query-id`,
//!   which no run will ever answer, silently lowering every mean. But some
//!   qrels files in the wild ship with no header at all, and unconditionally
//!   dropping the first line would then drop a real judgment instead. So the
//!   reader inspects the first record itself: it is a header, and is
//!   dropped, only when its score field does not parse as a `u8` — a real
//!   judgment always has a numeric score.
//! - **`_id` is a string, not a number.** `MED-10` and `4983` are both ids;
//!   leading zeros are significant. They map straight onto `DocId`/`QueryId`
//!   and are never parsed.
//! - **`title` may be present, empty, `null`, or absent** — see the title
//!   rule below. `null` and absent both deserialize to `None`, and both are
//!   then treated exactly like an empty title.
//!
//! # The title rule
//!
//! `Document.text` is set to `title`, one space, then `text` — omitting both
//! the space and the title when the title is empty or absent — and the raw
//! title is additionally kept under `metadata["title"]`.
//!
//! This is not a matter of taste. BEIR's own evaluation code indexes the
//! concatenation, and every published leaderboard figure is computed that way;
//! indexing the text alone changes nDCG@10 on most BEIR datasets, which would
//! make the M2 exit criterion (#33) irreproducible.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use ragondin_types::{DocId, Document, Query, QueryId};
use serde::Deserialize;

use crate::benchmark::{Benchmark, BenchmarkAdapter, Qrels};
use crate::error::BenchmarkError;

/// One line of `corpus.jsonl`.
#[derive(Deserialize)]
struct CorpusRecord {
    #[serde(rename = "_id")]
    id: String,
    /// Absent, JSON `null`, and empty are all the same thing here: `Option`
    /// catches `null` (which a bare `String` with `#[serde(default)]` does
    /// not — `default` only fills in a *missing* key), and both `None` and
    /// `Some(String::new())` are unwrapped to `""` before the title rule ever
    /// sees them.
    #[serde(default)]
    title: Option<String>,
    text: String,
}

/// One line of `queries.jsonl`.
#[derive(Deserialize)]
struct QueryRecord {
    #[serde(rename = "_id")]
    id: String,
    text: String,
}

/// Loads a BEIR dataset from a local directory.
///
/// The path comes from configuration and the dataset is read from disk: this
/// adapter never fetches anything. A published score is attached to a frozen
/// snapshot, and benchmarking against a live source is not reproducible (§9.1).
#[derive(Clone, Debug)]
pub struct BeirAdapter {
    root: PathBuf,
    split: String,
}

impl BeirAdapter {
    /// Reads the dataset at `root`, evaluating on the `test` split.
    ///
    /// `test` is the default because it is the split BEIR leaderboards report.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_split(root, "test")
    }

    /// Reads the dataset at `root`, evaluating on the named split.
    ///
    /// Splits vary by dataset — some ship `dev` and no `test` — so the split is
    /// a parameter rather than a constant.
    pub fn with_split(root: impl Into<PathBuf>, split: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            split: split.into(),
        }
    }

    fn qrels_path(&self) -> PathBuf {
        self.root.join("qrels").join(format!("{}.tsv", self.split))
    }
}

impl BenchmarkAdapter for BeirAdapter {
    fn load(&self) -> Result<Benchmark, BenchmarkError> {
        let qrels = read_qrels(&self.qrels_path())?;
        let corpus = read_corpus(&self.root.join("corpus.jsonl"))?;
        let queries = read_queries(&self.root.join("queries.jsonl"), &qrels)?;

        Ok(Benchmark::new(corpus, queries, qrels))
    }
}

/// Reads a JSONL file line by line, applying `parse` to each non-blank line.
///
/// Line-by-line rather than whole-file: a BEIR corpus runs to millions of
/// records, and holding the parsed file *and* its text in memory at once is
/// avoidable. Blank lines are skipped — a trailing newline is not a record.
fn read_jsonl<T, R>(path: &Path, mut parse: impl FnMut(T) -> R) -> Result<Vec<R>, BenchmarkError>
where
    T: for<'de> Deserialize<'de>,
{
    let file = File::open(path).map_err(|source| BenchmarkError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut parsed = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let mut line = line.map_err(|source| BenchmarkError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        // A UTF-8 BOM, if present at all, is attached to the very first byte
        // of the file — never to any later line — so it is only ever worth
        // checking for on the first record.
        if index == 0 {
            if let Some(stripped) = line.strip_prefix('\u{feff}') {
                line = stripped.to_string();
            }
        }
        if line.trim().is_empty() {
            continue;
        }
        let record: T =
            serde_json::from_str(&line).map_err(|source| BenchmarkError::MalformedJson {
                path: path.to_path_buf(),
                line: index + 1,
                source,
            })?;
        parsed.push(parse(record));
    }

    Ok(parsed)
}

fn read_corpus(path: &Path) -> Result<Vec<Document>, BenchmarkError> {
    read_jsonl(path, |record: CorpusRecord| {
        let title = record.title.unwrap_or_default();
        let mut metadata = BTreeMap::new();
        // Only when non-empty: an empty title is the *absence* of a title, and
        // storing "" would make absent and empty indistinguishable downstream
        // while preserving nothing.
        if !title.is_empty() {
            metadata.insert("title".to_string(), title.clone());
        }
        Document {
            id: DocId::new(record.id),
            text: combined_text(&title, &record.text),
            metadata,
        }
    })
}

/// The title rule, in one place so it can be pinned by one test.
fn combined_text(title: &str, text: &str) -> String {
    if title.is_empty() {
        text.to_string()
    } else {
        format!("{title} {text}")
    }
}

/// Reads the query set, keeping only the queries judged in the loaded split.
///
/// `queries.jsonl` holds every query of the dataset across all splits, while
/// `qrels/test.tsv` holds only the test ones. Evaluating a test run over train
/// queries would score them against no judgments at all and drag every mean
/// metric toward zero. File order is preserved among the queries kept, so the
/// run is reproducible.
fn read_queries(path: &Path, qrels: &Qrels) -> Result<Vec<Query>, BenchmarkError> {
    let all = read_jsonl(path, |record: QueryRecord| Query {
        id: QueryId::new(record.id),
        text: record.text,
    })?;

    Ok(all
        .into_iter()
        .filter(|query| qrels.for_query(&query.id).is_some())
        .collect())
}

/// Reads `qrels/<split>.tsv`.
///
/// BEIR ships a `query-id/corpus-id/score` header, and consuming it as data
/// invents a judgment for a query nothing will ever answer — but a header row
/// is not guaranteed: some qrels files in the wild ship without one. So the
/// first parsed record is inspected by hand: it is a header, and is skipped,
/// only when its score field does not parse as a `u8` — a real judgment row
/// always has a numeric score, and BEIR's header row has the literal `score`
/// there. Columns are taken by position, not by header name, because the
/// order is fixed across BEIR while the spelling is not.
///
/// The file is read one physical line at a time — like `read_jsonl` — rather
/// than handed whole to a single `csv::Reader`. `csv`'s own record positions
/// cannot be mapped back to physical file lines once blank lines are skipped
/// (they aren't counted) or the file is CRLF (the line/byte accounting shifts
/// again), so no arithmetic on those positions is reliable. The physical line
/// is what an operator needs anyway: it's the number to open the file and
/// look at the offending row, and counting it directly — the same way
/// `BufReader::lines()` already lets `read_jsonl` do it — sidesteps the
/// mismatch entirely instead of trying to correct it after the fact.
fn read_qrels(path: &Path) -> Result<Qrels, BenchmarkError> {
    let file = File::open(path).map_err(|source| BenchmarkError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut qrels = Qrels::new();
    let mut is_first_record = true;

    for (index, line) in BufReader::new(file).lines().enumerate() {
        let mut line = line.map_err(|source| BenchmarkError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let physical_line = index + 1;

        // A UTF-8 BOM, if present at all, is attached to the very first byte
        // of the file — never to any later line — so it is only ever worth
        // checking for on the first line. `csv::Reader` used to strip this
        // transparently when it was fed the whole file; now that each line
        // goes through its own reader, that's this function's job.
        if index == 0 {
            if let Some(stripped) = line.strip_prefix('\u{feff}') {
                line = stripped.to_string();
            }
        }

        if line.trim().is_empty() {
            continue;
        }

        // A per-line reader, not a shared one: each line is parsed on its
        // own, so a short row on one line can never be rejected for having
        // fewer fields than a different row elsewhere in the file.
        let mut line_reader = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .has_headers(false)
            .from_reader(line.as_bytes());

        let record = line_reader
            .records()
            .next()
            .transpose()
            .map_err(|source| BenchmarkError::MalformedRecord {
                path: path.to_path_buf(),
                line: physical_line,
                reason: source.to_string(),
            })?
            // An empty-after-trim check above already filters out blank
            // lines, so a genuinely empty record here would mean the line
            // was whitespace the trim check didn't catch (it can't be) —
            // this is unreachable in practice, but a missing record is
            // still reported with the correct physical line rather than
            // panicking.
            .ok_or_else(|| BenchmarkError::MalformedRecord {
                path: path.to_path_buf(),
                line: physical_line,
                reason: "empty record".to_string(),
            })?;

        let malformed = |reason: String| BenchmarkError::MalformedRecord {
            path: path.to_path_buf(),
            line: physical_line,
            reason,
        };

        let query_id = record
            .get(0)
            .ok_or_else(|| malformed("missing query-id".to_string()))?;
        let corpus_id = record
            .get(1)
            .ok_or_else(|| malformed("missing corpus-id".to_string()))?;
        let raw_score = record
            .get(2)
            .ok_or_else(|| malformed("missing score".to_string()))?;

        // `u8` rejects a negative and an out-of-range grade for free. BEIR
        // grades are small non-negative integers; anything else is a corrupt
        // file, not a judgment to guess at.
        let parsed_score: Result<u8, _> = raw_score.trim().parse();

        if is_first_record && parsed_score.is_err() {
            // The first record's score isn't numeric: this is the header
            // row (BEIR's literal `score`), not a judgment. Skip it.
            is_first_record = false;
            continue;
        }
        is_first_record = false;

        let grade = parsed_score
            .map_err(|_| malformed(format!("score {raw_score:?} is not a grade in 0..=255")))?;

        // Trim every field, not just the score: an untrimmed id that differs
        // from the "real" id by only whitespace parses fine, inserts fine,
        // and then matches nothing when `queries()` filters by qrels — a
        // silent empty result with no error anywhere. Trimming only the
        // score is exactly the shape that produces that silent mismatch.
        let query_id = query_id.trim();
        let corpus_id = corpus_id.trim();

        qrels.insert(QueryId::new(query_id), DocId::new(corpus_id), grade);
    }

    Ok(qrels)
}
