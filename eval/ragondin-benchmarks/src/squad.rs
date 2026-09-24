//! The SQuAD v1.1 adapter: a SQuAD file on disk → a [`Benchmark`] carrying
//! qrels and reference answers.
//!
//! # The layout
//!
//! ```text
//! dev-v1.1.json   {"version": "1.1", "data": [
//!                   {"title": "...", "paragraphs": [
//!                     {"context": "...", "qas": [
//!                       {"id": "...", "question": "...",
//!                        "answers": [{"answer_start": 0, "text": "..."}, ...]}]}]}]}
//! ```
//!
//! # How the quadruple comes out of it (ADR-C30 § 2)
//!
//! - **Corpus**: each paragraph is one `Document`. Its `DocId` is
//!   `<article title>#<paragraph index within the article>`, counted from 0 in
//!   file order; its text is the paragraph's `context`; its `metadata` holds
//!   the article title under `title`.
//! - **Queries**: each question, under the dataset's own question id.
//! - **Qrels**: each question judges the paragraph it was asked over, at
//!   grade 1 — one relevant document per question.
//! - **Reference answers**: `answers[].text`, in file order, repeats kept.
//!
//! Everything is kept verbatim — titles, contexts, questions, ids, answers.
//! BEIR's reader trims ids because the same id appears in several files and
//! must match itself across them; here every id is either the dataset's own or
//! derived inside this one file, so there is nothing to match and nothing to
//! trim.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

use ragondin_types::{DocId, Document, Query, QueryId};
use serde::Deserialize;

use crate::benchmark::{Benchmark, BenchmarkAdapter, Qrels, ReferenceAnswers};
use crate::error::BenchmarkError;

/// The whole file. `version` is not read: the adapter applies no rule that
/// depends on it, and the file is chosen by name.
#[derive(Deserialize)]
struct SquadFile {
    data: Vec<Article>,
}

#[derive(Deserialize)]
struct Article {
    title: String,
    paragraphs: Vec<Paragraph>,
}

#[derive(Deserialize)]
struct Paragraph {
    context: String,
    qas: Vec<Question>,
}

#[derive(Deserialize)]
struct Question {
    id: String,
    question: String,
    /// `Option` so that an absent key and JSON `null` both reach the
    /// no-answer check and become a typed [`BenchmarkError::NoReferenceAnswer`]
    /// naming the question, instead of a parse error that names only a line.
    #[serde(default)]
    answers: Option<Vec<Answer>>,
}

/// One answer. `answer_start` is not read: the answer is scored as text, never
/// located in the paragraph.
#[derive(Deserialize)]
struct Answer {
    text: String,
}

/// Loads a SQuAD v1.1 file from a local directory.
///
/// Like [`BeirAdapter`](crate::BeirAdapter), it reads a directory the user
/// prepared and never fetches anything: a published score is attached to a
/// frozen snapshot (§9.1).
#[derive(Clone, Debug)]
pub struct SquadAdapter {
    root: PathBuf,
    file_name: String,
}

impl SquadAdapter {
    /// Reads `dev-v1.1.json` in `root`, the file ADR-C30 § 2 pins.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_file(root, "dev-v1.1.json")
    }

    /// Reads the named file in `root`.
    ///
    /// The file is the only selection this adapter makes — SQuAD's train set is
    /// a separate file, not a split inside one — so it is a parameter, shaped
    /// as [`BeirAdapter::with_split`](crate::BeirAdapter::with_split) is.
    pub fn with_file(root: impl Into<PathBuf>, file_name: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            file_name: file_name.into(),
        }
    }

    fn path(&self) -> PathBuf {
        self.root.join(&self.file_name)
    }
}

impl BenchmarkAdapter for SquadAdapter {
    fn load(&self) -> Result<Benchmark, BenchmarkError> {
        let path = self.path();
        let bytes = std::fs::read(&path).map_err(|source| BenchmarkError::Io {
            path: path.clone(),
            source,
        })?;
        // One JSON document, read whole: the dev file is under 5 MB, and the
        // format has no line structure a streaming reader could follow. The
        // line `serde_json` reports is still the physical line of the fault.
        let file: SquadFile =
            serde_json::from_slice(&bytes).map_err(|source| BenchmarkError::MalformedJson {
                path: path.clone(),
                line: source.line(),
                source,
            })?;

        let duplicate = |id: String| BenchmarkError::DuplicateRecord {
            path: path.clone(),
            id,
        };

        let mut corpus = Vec::new();
        let mut queries = Vec::new();
        let mut qrels = Qrels::new();
        let mut reference_answers = ReferenceAnswers::new();
        let mut doc_ids = BTreeSet::new();
        let mut query_ids = BTreeSet::new();

        for article in file.data {
            for (index, paragraph) in article.paragraphs.into_iter().enumerate() {
                let doc_id = format!("{}#{index}", article.title);
                if !doc_ids.insert(doc_id.clone()) {
                    return Err(duplicate(doc_id));
                }

                for question in paragraph.qas {
                    if !query_ids.insert(question.id.clone()) {
                        return Err(duplicate(question.id));
                    }
                    let answers: Vec<String> = question
                        .answers
                        .unwrap_or_default()
                        .into_iter()
                        .map(|answer| answer.text)
                        .collect();
                    // Every dev question carries an answer, so a missing list
                    // is a corrupt or wrong file (ADR-C30 § 2), and absent,
                    // `null` and `[]` are all that same fault.
                    if answers.is_empty() {
                        return Err(BenchmarkError::NoReferenceAnswer {
                            path: path.clone(),
                            id: question.id,
                        });
                    }

                    let query_id = QueryId::new(question.id);
                    qrels.insert(query_id.clone(), DocId::new(doc_id.as_str()), 1);
                    reference_answers.insert(query_id.clone(), answers);
                    queries.push(Query {
                        id: query_id,
                        text: question.question,
                    });
                }

                corpus.push(Document {
                    id: DocId::new(doc_id),
                    text: paragraph.context,
                    metadata: BTreeMap::from([("title".to_string(), article.title.clone())]),
                });
            }
        }

        Ok(Benchmark::new(corpus, queries, qrels).with_reference_answers(reference_answers))
    }
}
