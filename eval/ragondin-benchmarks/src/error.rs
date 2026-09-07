//! The one error type every adapter returns.

use std::path::PathBuf;

use thiserror::Error;

/// What can go wrong while loading a benchmark from disk.
///
/// One type for every adapter, not one per format: the harness (#29) handles a
/// load failure the same way whichever dataset failed, and a per-adapter error
/// would make that a match on N types. `#[non_exhaustive]` so a future adapter
/// can add the variant its format needs without breaking callers.
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
