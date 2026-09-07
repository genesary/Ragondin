//! The BEIR adapter: a BEIR dataset directory on disk → a [`Benchmark`].
//!
//! # The layout
//!
//! ```text
//! corpus.jsonl        {"_id": "...", "title": "...", "text": "...", "metadata": {...}}
//! queries.jsonl       {"_id": "...", "text": "...", "metadata": {...}}
//! qrels/test.tsv      TAB-separated, WITH a header row: query-id/corpus-id/score
//! qrels/train.tsv     same shape; splits vary by dataset
//! ```
//!
//! # Three things that bite, and what this reader does about them
//!
//! - **The qrels TSV has a header line.** Read as data it adds a judgment for a
//!   query literally called `query-id`, which no run will ever answer, silently
//!   lowering every mean. `has_headers(true)` drops it.
//! - **`_id` is a string, not a number.** `MED-10` and `4983` are both ids;
//!   leading zeros are significant. They map straight onto `DocId`/`QueryId`
//!   and are never parsed.
//! - **`title` may be present, empty, or absent** — see the title rule below.
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
    /// Absent and empty are the same thing here, hence `default` rather than
    /// `Option`: the title rule treats both identically.
    #[serde(default)]
    title: String,
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
        let line = line.map_err(|source| BenchmarkError::Io {
            path: path.to_path_buf(),
            source,
        })?;
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
        let mut metadata = BTreeMap::new();
        // Only when non-empty: an empty title is the *absence* of a title, and
        // storing "" would make absent and empty indistinguishable downstream
        // while preserving nothing.
        if !record.title.is_empty() {
            metadata.insert("title".to_string(), record.title.clone());
        }
        Document {
            id: DocId::new(record.id),
            text: combined_text(&record.title, &record.text),
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
/// `has_headers(true)` is the whole point: BEIR ships a `query-id/corpus-id/
/// score` header, and consuming it as data invents a judgment for a query
/// nothing will ever answer. Columns are then taken by position, not by header
/// name, because the order is fixed across BEIR while the spelling is not.
fn read_qrels(path: &Path) -> Result<Qrels, BenchmarkError> {
    let file = File::open(path).map_err(|source| BenchmarkError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(true)
        .from_reader(file);

    let mut qrels = Qrels::new();
    for (index, record) in reader.records().enumerate() {
        // +2: the header is line 1, and `enumerate` is 0-based.
        let line = index + 2;
        let record = record.map_err(|source| BenchmarkError::MalformedRecord {
            path: path.to_path_buf(),
            line,
            reason: source.to_string(),
        })?;

        let malformed = |reason: String| BenchmarkError::MalformedRecord {
            path: path.to_path_buf(),
            line,
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
        let grade: u8 = raw_score
            .trim()
            .parse()
            .map_err(|_| malformed(format!("score {raw_score:?} is not a grade in 0..=255")))?;

        qrels.insert(QueryId::new(query_id), DocId::new(corpus_id), grade);
    }

    Ok(qrels)
}
