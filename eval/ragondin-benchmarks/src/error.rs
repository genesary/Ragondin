//! The one error type every adapter returns.

use std::path::PathBuf;

use thiserror::Error;

/// What can go wrong while loading a benchmark from disk.
///
/// One type for every adapter, not one per format: every adapter returns it
/// through the one [`BenchmarkAdapter::load`](crate::BenchmarkAdapter::load)
/// signature, so the caller that loads a dataset handles a failure the same way
/// whichever dataset failed, and a per-adapter error would make that a match on
/// N types. `#[non_exhaustive]` so a future adapter can add the variant its
/// format needs without breaking callers.
///
/// Every variant carries the offending path, and the line-oriented ones carry
/// the line number: a dataset is thousands of records long, and an error that
/// does not say *where* is an error the reader has to reproduce by hand.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BenchmarkError {
    /// A file of the dataset could not be read.
    ///
    /// The variant a missing `qrels/<split>.tsv` produces — the most common
    /// real failure, since splits vary by dataset.
    #[error("cannot read {path}")]
    Io {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying filesystem error.
        #[source]
        source: std::io::Error,
    },

    /// A JSONL line is not valid JSON, or lacks a field the format requires.
    #[error("{path}:{line}: malformed JSON record")]
    MalformedJson {
        /// The file the line came from.
        path: PathBuf,
        /// The 1-based line number.
        line: usize,
        /// The underlying parse error.
        #[source]
        source: serde_json::Error,
    },

    /// The qrels hold judgments, but not one of them names a query the dataset
    /// defines — so there is nothing a run could be scored against.
    ///
    /// Loading `Ok` here would be worse than failing: every mean metric would
    /// be computed over an empty query set and reported as a number, and the
    /// mismatch between the two files would surface as a bad score rather than
    /// as a bad dataset. Three separate parsing bugs on this reader produced
    /// exactly this state — a swallowed first row, a one-sided trim, a byte
    /// welded onto the first id — so the symptom is named at the load.
    ///
    /// The path is the qrels file, since that is the side whose ids are being
    /// compared against the query set.
    #[error("{path}: {judged} judgments, none naming a query this dataset defines")]
    NoJudgedQuery {
        /// The qrels file whose ids matched no query.
        path: PathBuf,
        /// How many `(query, document)` judgments it holds.
        judged: usize,
    },

    /// The qrels hold judgments, but not one of them names a document the
    /// corpus defines — so no ranked list can ever satisfy one.
    ///
    /// The document-side mirror of [`BenchmarkError::NoJudgedQuery`], and the
    /// harder of the two to diagnose without it: the query set is *full*, every
    /// query runs, and the report carries nDCG@10 = 0 over the whole benchmark
    /// with nothing looking empty anywhere. It is the shape a `qrels/`
    /// directory taken from a different snapshot than `corpus.jsonl` produces,
    /// or a corpus mirror that prefixes or re-cases its ids.
    ///
    /// Only a **total** mismatch is named. A benchmark may legitimately judge a
    /// document its corpus does not hold — `trec_eval` counts such a judgment
    /// in the denominator — so a partial mismatch stays a normal load, exactly
    /// as it does on the query side.
    ///
    /// The path is the qrels file, whose ids are the side being compared.
    #[error("{path}: {judged} judgments, none naming a document this corpus defines")]
    NoJudgedDocument {
        /// The qrels file whose corpus-ids matched no document.
        path: PathBuf,
        /// How many `(query, document)` judgments it holds.
        judged: usize,
    },

    /// One `_id` names two records in the same JSONL file.
    ///
    /// A repeated query id double-weights that query in a macro-average *and*
    /// inflates `Benchmark::queries().len()`, which is documented as the
    /// correct denominator of a mean over a run — so one duplicate moves the
    /// headline number twice over, in the same direction, silently. A repeated
    /// corpus id lets one judgment be satisfied twice within a single ranked
    /// list.
    ///
    /// Rejected rather than deduplicated: two records under one id disagree
    /// about what that id *is*, and picking one is a guess. This is unlike a
    /// repeated `(query, document)` pair in the qrels, which restates a
    /// judgment and is resolved last-wins.
    #[error("{path}:{line}: duplicate id {id:?}")]
    DuplicateId {
        /// The file holding both records.
        path: PathBuf,
        /// The 1-based line of the second occurrence.
        line: usize,
        /// The id that appeared twice.
        id: String,
    },

    /// One id names two records in a dataset that is a single JSON document
    /// rather than one record per line — a SQuAD file — so there is no line
    /// to name.
    ///
    /// The whole-document counterpart of [`BenchmarkError::DuplicateId`], and
    /// rejected for the same reason: two records under one id disagree about
    /// what that id is. In a SQuAD file the id is a question id, or a paragraph's
    /// derived `DocId` — two articles sharing a title would give their
    /// paragraphs the same ids.
    #[error("{path}: duplicate id {id:?}")]
    DuplicateRecord {
        /// The file holding both records.
        path: PathBuf,
        /// The id that appeared twice.
        id: String,
    },

    /// A query's record in a reference-answer source lists no answer.
    ///
    /// Raised by both adapters that read references: `SquadAdapter`, for a
    /// question whose `answers` is absent, `null` or empty, and
    /// `BeirAdapter`'s reference path, for an `answers.jsonl` line whose list
    /// is empty. A missing reference in a dataset that states one for every
    /// query means a corrupt or wrong file, not an unjudged query (ADR-C30
    /// § 2). Scoring it as unjudged would quietly shrink the generation
    /// family's judged set.
    ///
    /// One variant for both, with the line optional, because the fault is the
    /// same and only the file's shape differs: a JSONL file has a line to
    /// name, while a SQuAD file is one JSON document read whole and has none.
    #[error(
        "{}{}: query {id:?} has no reference answer",
        .path.display(),
        .line.map(|line| format!(":{line}")).unwrap_or_default()
    )]
    NoReferenceAnswer {
        /// The file holding the record.
        path: PathBuf,
        /// The 1-based line of the record in a line-oriented file; `None` for
        /// a file read as one JSON document.
        line: Option<usize>,
        /// The query whose answer list is absent or empty.
        id: String,
    },

    /// A line of a reference-answer file names a query the dataset does not
    /// define.
    ///
    /// Its references could be scored against nothing; the likelier cause is
    /// an answers file taken from another dataset or another snapshot.
    #[error("{path}:{line}: id {id:?} names no query of this dataset")]
    UnknownQuery {
        /// The reference-answer file.
        path: PathBuf,
        /// The 1-based line number.
        line: usize,
        /// The id that names no query.
        id: String,
    },

    /// A delimited record does not have the shape the format requires.
    #[error("{path}:{line}: malformed record: {reason}")]
    MalformedRecord {
        /// The file the record came from.
        path: PathBuf,
        /// The 1-based line number.
        line: usize,
        /// What was wrong with it, in the reader's own words.
        reason: String,
    },
}
