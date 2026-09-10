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
//!   reader inspects the first record itself and drops it only when it
//!   **says** it is a header: its score field must not parse as a `u8` *and*
//!   must spell BEIR's own `score`. A non-numeric score alone is not enough —
//!   a judgment whose score is corrupt looks identical, and calling that a
//!   header discarded a real row without a word, then the query with it,
//!   while the same corruption one line later was a hard error. The stricter
//!   rule costs the file whose third column is spelled otherwise: that is now
//!   a typed error naming the row, which an operator can see and correct,
//!   where the silent drop it replaces could only be found by noticing that a
//!   published metric was wrong.
//! - **`_id` is a string, not a number.** `MED-10` and `4983` are both ids;
//!   leading zeros are significant. They map straight onto `DocId`/`QueryId`
//!   and are never parsed. One `_id` names one record: a repeat in either
//!   JSONL file is a typed error, not a silent second entry.
//! - **`title` may be present, empty, `null`, or absent** — see the title
//!   rule below. `null` and absent both deserialize to `None`, and all of
//!   them, plus a title of nothing but whitespace, are treated as no title.
//!
//! # The title rule
//!
//! `Document.text` is `title`, one space, then `text`, with the **whole
//! result** trimmed — so an empty, absent or whitespace-only title
//! contributes neither itself nor a separator. The title is additionally kept
//! under `metadata["title"]`, trimmed.
//!
//! This is not a matter of taste. BEIR's own evaluation code indexes the
//! concatenation, and every published leaderboard figure is computed that way;
//! indexing the text alone changes nDCG@10 on most BEIR datasets, which would
//! make the M2 exit criterion (#33) irreproducible. `combined_text` therefore
//! mirrors the reference's expression rather than paraphrasing it — see its
//! documentation for the source and for what a paraphrase would cost.

use std::collections::{BTreeMap, BTreeSet};
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

        // Judgments on one side and nothing to evaluate on the other is a
        // mismatch between the two files, not a benchmark. Every id-handling
        // bug this reader has had ended here, loading `Ok` with zero queries,
        // and the failure then showed up as a metric of zero computed over an
        // empty set — a number, in a report, with no error behind it. Empty
        // qrels is a different case and stays a normal load: a split with
        // nothing judged is useless, but it is not corrupt.
        if !qrels.is_empty() && queries.is_empty() {
            return Err(BenchmarkError::NoJudgedQuery {
                path: self.qrels_path(),
                judged: qrels.judgment_count(),
            });
        }

        // The same mismatch seen from the document side, and the harder one to
        // notice without a guard: here the query set is *full*, every query
        // runs, and the report carries a zero over the whole benchmark with
        // nothing looking empty. A `qrels/` directory paired with a corpus from
        // another snapshot produces exactly this. Only the total miss is named,
        // mirroring the query side — a benchmark may legitimately judge a
        // document its corpus does not hold.
        if !qrels.is_empty() && !judges_any_document(&qrels, &corpus) {
            return Err(BenchmarkError::NoJudgedDocument {
                path: self.qrels_path(),
                judged: qrels.judgment_count(),
            });
        }

        Ok(Benchmark::new(corpus, queries, qrels))
    }
}

/// Whether any judgment names a document the corpus actually defines.
///
/// The corpus ids are collected once and the judgments probed against that set,
/// rather than the other way round: a BEIR corpus is far larger than its qrels,
/// so this walks the big collection once and the small one until the first hit.
fn judges_any_document(qrels: &Qrels, corpus: &[Document]) -> bool {
    let defined: BTreeSet<&DocId> = corpus.iter().map(|document| &document.id).collect();
    qrels
        .iter()
        .any(|(_, judgments)| judgments.keys().any(|doc| defined.contains(doc)))
}

/// Reads a JSONL file line by line, applying `parse` to each non-blank line.
///
/// Line-by-line rather than whole-file: a BEIR corpus runs to millions of
/// records, and holding the parsed file *and* its text in memory at once is
/// avoidable. Blank lines are skipped — a trailing newline is not a record.
fn read_jsonl<T, R>(
    path: &Path,
    mut parse: impl FnMut(T, usize) -> Result<R, BenchmarkError>,
) -> Result<Vec<R>, BenchmarkError>
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
        parsed.push(parse(record, index + 1)?);
    }

    Ok(parsed)
}

/// Records `id` as seen, or reports it as a duplicate naming `line`.
///
/// One id names one record. Two records under one id disagree about what that
/// id is, and either choice is a guess — so this rejects rather than
/// deduplicating, unlike a repeated `(query, document)` pair in the qrels,
/// which merely restates a judgment.
fn claim_id(
    seen: &mut BTreeSet<String>,
    id: &str,
    path: &Path,
    line: usize,
) -> Result<(), BenchmarkError> {
    if seen.insert(id.to_string()) {
        return Ok(());
    }
    Err(BenchmarkError::DuplicateId {
        path: path.to_path_buf(),
        line,
        id: id.to_string(),
    })
}

fn read_corpus(path: &Path) -> Result<Vec<Document>, BenchmarkError> {
    let mut seen = BTreeSet::new();
    read_jsonl(path, |record: CorpusRecord, line| {
        // The raw title goes into the concatenation, because that is the
        // string BEIR builds; the trimmed one goes into the metadata, because
        // that is ours to define. See `combined_text`.
        let raw_title = record.title.unwrap_or_default();
        let title = raw_title.trim();
        let mut metadata = BTreeMap::new();
        // Only when non-empty *after trimming*: an empty title is the absence
        // of a title, storing "" would make absent and empty indistinguishable
        // while preserving nothing, and a title of only whitespace is
        // semantically absent — the same all-or-nothing rule the ids follow.
        if !title.is_empty() {
            metadata.insert("title".to_string(), title.to_string());
        }
        // Trimmed for the same reason `read_qrels` trims: an id must be
        // treated identically wherever it appears, or a corpus id and the
        // qrels corpus-id naming the same document stop matching and every
        // judgment silently misses.
        let id = record.id.trim();
        claim_id(&mut seen, id, path, line)?;

        Ok(Document {
            id: DocId::new(id),
            text: combined_text(&raw_title, &record.text),
            metadata,
        })
    })
}

/// The title rule, in one place so it can be pinned by one test.
///
/// This mirrors the reference implementation's expression rather than
/// paraphrasing it. BEIR concatenates in
/// `beir/retrieval/models/util.py::extract_corpus_sentences`:
///
/// ```python
/// (doc["title"] + sep + doc["text"]).strip()   # sep defaults to " "
/// ```
///
/// So: join with one space unconditionally, then trim the whole result — do
/// **not** special-case the empty title, and do not trim the parts. The join
/// contributes the separator that the trim then removes when the title is
/// empty, absent, or nothing but whitespace, which is why no branch is needed.
///
/// Written this way so that `Document.text` is byte-identical to the string
/// BEIR indexes, rather than merely equivalent once a tokenizer has collapsed
/// the whitespace. #33 compares against published figures; a claim that can be
/// checked is worth more there than one that has to be argued.
fn combined_text(title: &str, text: &str) -> String {
    format!("{title} {text}").trim().to_string()
}

/// Reads the query set, keeping only the queries judged in the loaded split.
///
/// `queries.jsonl` holds every query of the dataset across all splits, while
/// `qrels/test.tsv` holds only the test ones. Evaluating a test run over train
/// queries would score them against no judgments at all and drag every mean
/// metric toward zero. File order is preserved among the queries kept, so the
/// run is reproducible.
///
/// Filtering everything away is not this function's business to report: it
/// returns an empty `Vec` and `load` decides, since only `load` knows whether
/// the qrels held anything to match in the first place.
fn read_queries(path: &Path, qrels: &Qrels) -> Result<Vec<Query>, BenchmarkError> {
    let mut seen = BTreeSet::new();
    let all = read_jsonl(path, |record: QueryRecord, line| {
        // Trimmed to match `read_qrels`. Trimming one side only is worse than
        // trimming neither: it turns a dataset whose ids carry the same
        // whitespace everywhere — which used to match itself — into one whose
        // queries are all filtered out below.
        let id = record.id.trim();
        // Checked before the qrels filter below, so a duplicate is reported
        // whether or not that id happens to be judged in this split: the file
        // is malformed either way, and a rule that only fires on some splits
        // is a rule nobody can rely on.
        claim_id(&mut seen, id, path, line)?;

        Ok(Query {
            id: QueryId::new(id),
            text: record.text,
        })
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
/// only when its score field both fails to parse as a `u8` **and** spells
/// BEIR's own `score`. Failing to parse is not on its own evidence of a
/// header — a judgment whose score is corrupt fails identically, and skipping
/// it dropped a real row in silence while the same corruption on any later
/// line was a hard error.
///
/// Note the asymmetry that buys: the header's *spelling* decides whether the
/// row is a header, while the three columns are still read **by position**,
/// because across BEIR the order is fixed and only the spelling varies. A
/// header spelling its third column otherwise is now a typed error rather
/// than a silent skip, which is the trade: an operator can see and fix an
/// error, where the drop it replaces could only be found by noticing a
/// published number was wrong.
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
///
/// One known consequence of parsing per physical line: a quoted field
/// containing an embedded newline, and a lone `\r` used on its own as a line
/// terminator, parse differently than they would under a whole-file reader,
/// which would see them as part of the same logical record. Both are absent
/// from real BEIR qrels — whose fields are bare ids and small integers, never
/// quoted — so the trade for physical-line accuracy in error messages is
/// worth it.
fn read_qrels(path: &Path) -> Result<Qrels, BenchmarkError> {
    let file = File::open(path).map_err(|source| BenchmarkError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut qrels = Qrels::new();
    let mut is_first_record = true;

    // A leading UTF-8 BOM needs no handling here: `csv::Reader` strips one from
    // the start of whatever it is given, per-line reader included.
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| BenchmarkError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let physical_line = index + 1;

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

        // A shared reader across the whole file used to reject a row whose
        // field count disagreed with the previous row; a per-line reader
        // never sees a previous row to compare against, so that check has to
        // be made explicit here. Applied before the header/data split below,
        // because a header with the wrong number of columns means the file's
        // shape is wrong regardless of which row happens to notice it.
        if record.len() != 3 {
            return Err(malformed(format!(
                "expected 3 fields, found {}",
                record.len()
            )));
        }

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

        // A non-numeric score on the first record is not enough to call it a
        // header: a judgment whose score is *corrupt* looks exactly the same,
        // and skipping it dropped the judgment, then the query with it via the
        // qrels filter, and returned `Ok`. The same corruption one line later
        // was already a hard error, so corruption was tolerated on line 1 and
        // nowhere else. The score column must therefore say what it is —
        // BEIR's header spells it `score` — before the row is discarded.
        if is_first_record
            && parsed_score.is_err()
            && raw_score.trim().eq_ignore_ascii_case("score")
        {
            is_first_record = false;
            continue;
        }
        is_first_record = false;

        let grade = parsed_score
            .map_err(|_| malformed(format!("score {raw_score:?} is not a grade in 0..=255")))?;

        // Trim every field, not just the score: an untrimmed id that differs
        // from the "real" id by only whitespace parses fine, inserts fine,
        // and then matches nothing when `queries()` filters by qrels, so the
        // query set empties out. `load`'s `NoJudgedQuery` guard now catches
        // that state, but only after the fact and only when *every* query
        // misses; trimming here is what stops it happening.
        let query_id = query_id.trim();
        let corpus_id = corpus_id.trim();

        qrels.insert(QueryId::new(query_id), DocId::new(corpus_id), grade);
    }

    Ok(qrels)
}
