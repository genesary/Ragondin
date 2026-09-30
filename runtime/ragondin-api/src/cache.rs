//! The workspace's `cache/`: reconstructible derived data, never a truth
//! (the design document § 6). Deleting it changes no response.
//!
//! One file per run, `cache/<run_id>/derived.json`: the verdict on the run's
//! chunk set and, when it held, the per-query scores and per-node metrics
//! (`derived.rs`). It is written only once the dataset on disk has been
//! loaded and found to digest to the run's `dataset_version`, and it names
//! what it was computed under — that digest, the chunk set's digest it found,
//! and the build that computed it — so that it is used only when all three
//! still hold. A dataset that changed on disk no longer verifies, so its
//! cache is never read; one restored to the run's version verifies again, and
//! the file is valid again. No clock is consulted.
//!
//! A file that does not parse, or was written under another format or build,
//! is a miss and is overwritten: it is derived data, so rebuilding it loses
//! nothing — the opposite of a stored run, which is reported, never repaired.
//! A file that cannot be read or written for any other reason is
//! `backend_failed`: a cache that silently stopped working would make every
//! request pay for the derivation with nothing to say why.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::derived::NodeFigures;
use crate::error::ApiError;

/// The layout this build writes. A file under another is a miss.
const FORMAT: u32 = 1;

/// What is cached for one run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    format: u32,
    build: String,
    run: String,
    dataset_version: String,
    /// What the chunk set derived from the dataset digests to.
    pub(crate) index_version: String,
    /// The derived figures, present when `index_version` is the run's.
    figures: Option<StoredFigures>,
}

/// The per-query scores and the per-node metrics of a verified run.
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

impl Entry {
    pub(crate) fn new(
        build: &str,
        run: &str,
        dataset_version: &str,
        index_version: String,
        figures: Option<&Figures>,
    ) -> Self {
        Self {
            format: FORMAT,
            build: build.to_owned(),
            run: run.to_owned(),
            dataset_version: dataset_version.to_owned(),
            index_version,
            figures: figures.map(|figures| StoredFigures {
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
            }),
        }
    }

    /// The figures, when the chunk set was the run's.
    pub(crate) fn figures(&self) -> Option<Figures> {
        self.figures.as_ref().map(|stored| Figures {
            queries: stored
                .queries
                .iter()
                .map(|(query, scores)| (query.clone(), from_bits(scores)))
                .collect(),
            nodes: stored
                .nodes
                .iter()
                .map(|node| NodeFigures {
                    node: node.node.clone(),
                    produces_ranking: node.produces_ranking,
                    judged_queries: node.judged_queries,
                    metrics: node.metrics.as_ref().map(from_bits),
                })
                .collect(),
        })
    }
}

/// The run's cache directory under the workspace.
fn directory(workspace: &Path, run: &str) -> PathBuf {
    workspace.join("cache").join(run)
}

fn failed(path: &Path, what: &str, error: std::io::Error) -> ApiError {
    ApiError::BackendFailed {
        detail: format!("{what} the cache file {}: {error}", path.display()),
    }
}

/// The cached entry of `run`, if one was computed by this build under the
/// run's `dataset_version`, and holds figures exactly when the chunk set it
/// found is the run's `index_version`.
pub(crate) fn read(
    workspace: &Path,
    build: &str,
    run: &str,
    dataset_version: &str,
    index_version: &str,
) -> Result<Option<Entry>, ApiError> {
    let path = directory(workspace, run).join("derived.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failed(&path, "reading", error)),
    };
    Ok(serde_json::from_slice::<Entry>(&bytes)
        .ok()
        .filter(|entry| {
            entry.format == FORMAT
                && entry.build == build
                && entry.run == run
                && entry.dataset_version == dataset_version
                && entry.figures.is_some() == (entry.index_version == index_version)
        }))
}

/// Writes `entry` for `run`, whole or not at all: to a file of this writer's
/// own beside the destination, then renamed over it, so a reader never sees
/// half a file and two writers of the same values do not interleave.
pub(crate) fn write(workspace: &Path, run: &str, entry: &Entry) -> Result<(), ApiError> {
    let directory = directory(workspace, run);
    std::fs::create_dir_all(&directory).map_err(|error| failed(&directory, "creating", error))?;
    let path = directory.join("derived.json");
    // One writer per thread at a time, and the handler runs on a blocking
    // thread: the process and the thread name a writer.
    let thread: String = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let staging = directory.join(format!(".derived.{}.{thread}.partial", std::process::id()));
    let bytes = serde_json::to_vec(entry).map_err(|error| ApiError::BackendFailed {
        detail: format!("serializing the cache entry of run {run}: {error}"),
    })?;
    std::fs::write(&staging, bytes).map_err(|error| failed(&staging, "writing", error))?;
    std::fs::rename(&staging, &path).map_err(|error| {
        // Best effort: the rename's error is the one reported, and a
        // leftover `.partial` file is never read as the cache.
        let _ = std::fs::remove_file(&staging);
        failed(&path, "renaming into place", error)
    })
}
