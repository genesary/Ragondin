//! The workspace's `cache/`: reconstructible derived data, never a truth
//! (the design document § 6). Deleting it changes no response.
//!
//! Two files per run, under `cache/<run_id>/`, each keyed the same way:
//!
//! - `derived.json`: the per-query scores and the per-node metrics
//!   (`derived.rs`). It is written only once the dataset on disk has been
//!   loaded and found to digest to the run's `dataset_version`.
//! - `latency.json`: the run's median query latency, which `GET /runs`
//!   lists. It is read from the run's traces alone, so it is written whether
//!   or not the run's dataset is on disk, and nothing about the dataset is
//!   verified for it.
//!
//! The key names everything a file's figures were computed from — the run's
//! `dataset_version`, a digest of the run's own content (its traces and its
//! metrics), and the build that computed them — so that a file is used only
//! when all of them still hold. No clock is consulted:
//!
//! - a dataset changed on disk no longer verifies, so its cache is never read;
//!   one restored to the run's version verifies again, and the file is valid
//!   again;
//! - a run deleted and launched again keeps its id, since the id digests the
//!   inputs, but a nondeterministic component may give it other traces — the
//!   content digest tells the two apart;
//! - another build may score differently, so the build is in the key, and
//!   [`ServerConfig::build`](crate::ServerConfig::build) must change with
//!   every change to the code.
//!
//! A file that does not parse, or was written under another key, is a miss
//! and is overwritten: it is derived data, so rebuilding it loses nothing —
//! the opposite of a stored run, which is reported, never repaired. **A cache
//! that cannot be read or written fails nothing**: the figures are computed
//! anyway and served, and the failure is reported beside them, so a read-only
//! workspace stays usable and a broken cache is never silent.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use ragondin_experiments::Run;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::derived::NodeFigures;

/// The layout this build writes, of either file. A file under another is a
/// miss.
const FORMAT: u32 = 2;

/// The domain separator of the run-content digest. `v2` streams each trace
/// (`content_digest`); a file keyed under `v1` is a miss.
const CONTENT_DOMAIN: &str = "ragondin-api/cache-run-content/v2";

/// What a cache file is keyed on: everything its figures were computed from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Key {
    format: u32,
    build: String,
    run: String,
    dataset_version: String,
    content: String,
}

impl Key {
    /// The key of `run`'s figures, computed by `build` against the dataset
    /// digesting to the run's `dataset_version`.
    pub(crate) fn of(build: &str, run: &Run) -> Self {
        Self {
            format: FORMAT,
            build: build.to_owned(),
            run: run.id.to_string(),
            dataset_version: run.inputs.dataset_version.clone(),
            content: content_digest(run),
        }
    }
}

/// A digest of what the figures are read from inside the run: every trace
/// document, by query, and every metric, by name.
///
/// Each trace goes in through its JSON text, whose objects are ordered maps
/// in this build, written straight into the hasher rather than built as a
/// string first: building the string was a third of the key's cost
/// (§ *The cache: a choice made here* has the figures). Streamed, the text's
/// length is not known before it is written, so it is not length-prefixed
/// as the encoder's fields are; it is closed by `0xFF` instead, a byte no
/// UTF-8 text holds, so where one trace ends is never in doubt. The query
/// ids and the metrics are written as `ragondin_benchmarks::identity`'s
/// encoder writes a field and a count: a length, little-endian, then the
/// bytes.
fn content_digest(run: &Run) -> String {
    let mut hasher = Sha256::new();
    field(&mut hasher, CONTENT_DOMAIN.as_bytes());
    count(&mut hasher, run.traces.len());
    for (query, trace) in &run.traces {
        field(&mut hasher, query.as_str().as_bytes());
        serde_json::to_writer(&mut hasher, trace.as_value())
            .expect("a JSON value always serializes, and a hasher never fails a write");
        hasher.update([0xFF]);
    }
    count(&mut hasher, run.metrics.len());
    for (name, value) in run.metrics.iter() {
        field(&mut hasher, name.as_bytes());
        field(&mut hasher, &value.to_bits().to_le_bytes());
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        // Infallible: writing to a `String` cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// A count, as a little-endian `u64` whatever the machine's width.
fn count(hasher: &mut Sha256, count: usize) {
    hasher.update((count as u64).to_le_bytes());
}

/// A byte string, preceded by its length.
fn field(hasher: &mut Sha256, bytes: &[u8]) {
    count(hasher, bytes.len());
    hasher.update(bytes);
}

/// What `derived.json` holds for one run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    key: Key,
    figures: StoredFigures,
}

/// What `latency.json` holds for one run: its median query latency, `None`
/// when no trace of it reads — an answer cached like any other.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct LatencyEntry {
    key: Key,
    median_query_latency_nanos: Option<u64>,
}

/// The per-query scores and the per-node metrics of a run whose dataset
/// verified.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Figures {
    /// Per query id, its scores at the run's output.
    pub(crate) queries: BTreeMap<String, BTreeMap<String, f64>>,
    /// Per node, in the canonical order.
    pub(crate) nodes: Vec<NodeFigures>,
}

/// [`Figures`] as the file holds them: every figure as the bits of its
/// double. A decimal would be read back by `serde_json`'s default reader to
/// within an ulp of what was written, and a cached answer would then differ
/// from a computed one; the bits make the round trip exact, so a response is
/// the same with the cache or without it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct StoredFigures {
    queries: BTreeMap<String, BTreeMap<String, u64>>,
    nodes: Vec<StoredNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct StoredNode {
    node: String,
    produces_ranking: bool,
    judged_queries: u64,
    metrics: Option<BTreeMap<String, u64>>,
}

fn to_bits(figures: &BTreeMap<String, f64>) -> BTreeMap<String, u64> {
    figures
        .iter()
        .map(|(name, value)| (name.clone(), value.to_bits()))
        .collect()
}

fn from_bits(figures: &BTreeMap<String, u64>) -> BTreeMap<String, f64> {
    figures
        .iter()
        .map(|(name, bits)| (name.clone(), f64::from_bits(*bits)))
        .collect()
}

impl StoredFigures {
    fn from(figures: &Figures) -> Self {
        Self {
            queries: figures
                .queries
                .iter()
                .map(|(query, scores)| (query.clone(), to_bits(scores)))
                .collect(),
            nodes: figures
                .nodes
                .iter()
                .map(|node| StoredNode {
                    node: node.node.clone(),
                    produces_ranking: node.produces_ranking,
                    judged_queries: node.judged_queries,
                    metrics: node.metrics.as_ref().map(to_bits),
                })
                .collect(),
        }
    }

    fn figures(&self) -> Figures {
        Figures {
            queries: self
                .queries
                .iter()
                .map(|(query, scores)| (query.clone(), from_bits(scores)))
                .collect(),
            nodes: self
                .nodes
                .iter()
                .map(|node| NodeFigures {
                    node: node.node.clone(),
                    produces_ranking: node.produces_ranking,
                    judged_queries: node.judged_queries,
                    metrics: node.metrics.as_ref().map(from_bits),
                })
                .collect(),
        }
    }
}

/// The run's derived-data file under the workspace.
fn file(workspace: &Path, run: &str) -> PathBuf {
    workspace.join("cache").join(run).join("derived.json")
}

/// The run's latency file under the workspace.
fn latency_file(workspace: &Path, run: &str) -> PathBuf {
    workspace.join("cache").join(run).join("latency.json")
}

fn failed(path: &Path, what: &str, error: impl std::fmt::Display) -> String {
    format!("{what} the cache file {}: {error}", path.display())
}

/// The cached figures under `key`, if a file holds them. `Err` is why the
/// file could not be read; a file that does not parse, or holds another key,
/// is a miss, not an error.
pub(crate) fn read(workspace: &Path, key: &Key) -> Result<Option<Figures>, String> {
    Ok(read_file::<Entry>(&file(workspace, &key.run))?
        .filter(|entry| &entry.key == key)
        .map(|entry| entry.figures.figures()))
}

/// Writes `figures` under `key`, whole or not at all (`write_file`). `Err`
/// is why it could not.
pub(crate) fn write(workspace: &Path, key: &Key, figures: &Figures) -> Result<(), String> {
    let entry = Entry {
        key: key.clone(),
        figures: StoredFigures::from(figures),
    };
    write_file(&file(workspace, &key.run), &entry)
}

/// The cached median query latency under `key`, if a file holds it: the
/// outer `None` is a miss, the inner one a run with no trace that reads.
/// `Err` and a miss mean what they mean for [`read`].
pub(crate) fn read_latency(workspace: &Path, key: &Key) -> Result<Option<Option<u64>>, String> {
    Ok(
        read_file::<LatencyEntry>(&latency_file(workspace, &key.run))?
            .filter(|entry| &entry.key == key)
            .map(|entry| entry.median_query_latency_nanos),
    )
}

/// Writes a run's median query latency under `key`, as [`write`] writes its
/// figures.
pub(crate) fn write_latency(
    workspace: &Path,
    key: &Key,
    latency: Option<u64>,
) -> Result<(), String> {
    let entry = LatencyEntry {
        key: key.clone(),
        median_query_latency_nanos: latency,
    };
    write_file(&latency_file(workspace, &key.run), &entry)
}

/// The entry `path` holds, `None` when there is no file or it does not parse
/// as one. `Err` is why it could not be read.
fn read_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failed(path, "reading", error)),
    };
    Ok(serde_json::from_slice(&bytes).ok())
}

/// Writes `entry` to `path`, whole or not at all: to a file of this writer's
/// own beside the destination, then renamed over it, so a reader never sees
/// half a file and two writers of the same values do not interleave. `Err`
/// is why it could not; the staging file is removed then.
fn write_file<T: Serialize>(path: &Path, entry: &T) -> Result<(), String> {
    let directory = path.parent().expect("the cache file is under a directory");
    std::fs::create_dir_all(directory).map_err(|error| failed(directory, "creating", error))?;
    // One writer per thread at a time, and the handler runs on a blocking
    // thread: the process and the thread name a writer.
    let thread: String = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("a cache file has a UTF-8 name");
    let staging = directory.join(format!(".{stem}.{}.{thread}.partial", std::process::id()));
    let bytes = serde_json::to_vec(entry).map_err(|error| failed(path, "serializing", error))?;
    let written = std::fs::write(&staging, bytes)
        .map_err(|error| failed(&staging, "writing", error))
        .and_then(|()| {
            std::fs::rename(&staging, path)
                .map_err(|error| failed(path, "renaming into place", error))
        });
    if written.is_err() {
        // Best effort: the write's own error is the one reported, and a
        // leftover `.partial` file is never read as the cache.
        let _ = std::fs::remove_file(&staging);
    }
    written
}

#[cfg(test)]
mod cost;

#[cfg(test)]
mod tests {
    use super::*;

    fn key(run: &str) -> Key {
        Key {
            format: FORMAT,
            build: "test".to_owned(),
            run: run.to_owned(),
            dataset_version: "0".repeat(64),
            content: "1".repeat(64),
        }
    }

    fn figures() -> Figures {
        Figures {
            queries: BTreeMap::from([(
                "q".to_owned(),
                BTreeMap::from([("mrr".to_owned(), 1.0 / 3.0)]),
            )]),
            nodes: Vec::new(),
        }
    }

    /// An empty directory of this test's own, under the system's temporary
    /// directory and this process's id.
    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir()
            .join(format!("ragondin-api-cache-{}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn the_key_holds_for_the_same_content_and_changes_with_any_of_it() {
        use ragondin_experiments::TraceDocument;
        use ragondin_types::QueryId;

        let base = cost::run(1, 3, 10);
        assert_eq!(Key::of("b", &base), Key::of("b", &base.clone()));

        type Edit<'a> = (&'a str, &'a dyn Fn(&mut Run));
        let changed = |change: &dyn Fn(&mut Run)| {
            let mut run = base.clone();
            change(&mut run);
            Key::of("b", &run)
        };
        let q1 = QueryId::new("q1");
        let edits: [Edit; 6] = [
            ("one duration", &|run| {
                let mut value = run.traces[&q1].as_value().clone();
                value["nodes"][0]["duration_nanos"] = 7.into();
                run.traces.insert(q1.clone(), TraceDocument::new(value));
            }),
            ("one chunk's score", &|run| {
                let mut value = run.traces[&q1].as_value().clone();
                value["nodes"][0]["output"]["chunks"]["ranked"][0]["score"] = 0.25.into();
                run.traces.insert(q1.clone(), TraceDocument::new(value));
            }),
            ("a query renamed", &|run| {
                let trace = run.traces.remove(&q1).unwrap();
                run.traces.insert(QueryId::new("q1x"), trace);
            }),
            ("a query dropped", &|run| {
                run.traces.remove(&q1);
            }),
            ("one metric", &|run| {
                run.metrics.insert("mrr", 0.25);
            }),
            ("two traces swapped", &|run| {
                let q0 = QueryId::new("q0");
                let first = run.traces[&q0].clone();
                let second = run.traces[&q1].clone();
                run.traces.insert(q0, second);
                run.traces.insert(q1.clone(), first);
            }),
        ];
        for (what, edit) in edits {
            assert_ne!(changed(edit), Key::of("b", &base), "{what} changes the key");
        }
    }

    #[test]
    fn figures_read_back_exactly_under_their_key_and_under_no_other() {
        let workspace = scratch("round_trip");
        write(&workspace, &key("run"), &figures()).unwrap();
        assert_eq!(read(&workspace, &key("run")).unwrap(), Some(figures()));
        let mut other = key("run");
        other.content = "2".repeat(64);
        assert_eq!(read(&workspace, &other).unwrap(), None);
    }

    #[test]
    fn a_latency_reads_back_under_its_key_beside_the_figures_and_under_no_other() {
        let workspace = scratch("latency_round_trip");
        write_latency(&workspace, &key("run"), Some(17_249)).unwrap();
        write(&workspace, &key("run"), &figures()).unwrap();
        assert_eq!(
            read_latency(&workspace, &key("run")).unwrap(),
            Some(Some(17_249))
        );
        assert_eq!(read(&workspace, &key("run")).unwrap(), Some(figures()));
        let mut other = key("run");
        other.build = "another".to_owned();
        assert_eq!(read_latency(&workspace, &other).unwrap(), None);

        // No trace that reads is an answer, and cached as one.
        write_latency(&workspace, &key("none"), None).unwrap();
        assert_eq!(read_latency(&workspace, &key("none")).unwrap(), Some(None));
    }

    #[test]
    fn a_write_that_fails_leaves_no_partial_file_behind() {
        let workspace = scratch("failed_write");
        // A non-empty directory where the file goes: the rename onto it fails.
        let target = file(&workspace, "run");
        std::fs::create_dir_all(target.join("occupied")).unwrap();

        assert!(write(&workspace, &key("run"), &figures()).is_err());

        let left: Vec<String> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["derived.json"], "no `.partial` file is left");
    }
}
