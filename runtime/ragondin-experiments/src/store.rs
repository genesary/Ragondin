//! The native run store: [`FileSystemRunStore`] and its [`RunStoreError`].
//!
//! **Native, by decision** (ADR-13): a run here is a content-addressed tuple
//! whose configuration is a graph and whose central artifact is a structured
//! per-node trace, and a conventional experiment tracker models a run as scalar
//! parameters, metrics and opaque blobs. Exporting to such a tracker is a
//! separate, additive piece of the plane; building the store *on* one is what
//! the ADR rejects. No tracker client is a dependency of this crate.
//!
//! # Why the filesystem
//!
//! A directory per run is the chosen v0 backing store: no database to run, no
//! schema to migrate, inspectable with `ls` and `cat`, and trivially copied or
//! committed. The cost is that it answers only one question — *give me this
//! run* — and answers nothing about *which runs match*. When a query or index
//! need appears, this is **internal to the experiment plane and freely
//! swappable** (an embedded database, say): the store's callers name a run by
//! its [`RunId`] and never a path, so the layout below is not an architecture
//! boundary and replacing it breaks no invariant.
//!
//! # The layout
//!
//! ```text
//! <root>/<run id in hex>/
//!     inputs.json    the identity tuple's components
//!     metrics.json   what the run scored
//!     config.yaml    the configuration document, verbatim
//!     traces.json    the per-query execution traces, by query id
//! ```
//!
//! The directory name is the run id, so a run is found without an index, and
//! the id is a digest, so no run id can name a directory outside the root.
//!
//! The traces of a run are one file rather than one file per query. Query ids
//! come from a dataset and are arbitrary strings; spreading them over file
//! names would mean either sanitizing them — and losing the ability to read a
//! trace back by the id it was filed under — or letting a dataset choose paths
//! under the store root.
//!
//! `config.yaml` is named for the one configuration format this build reads
//! (`ragondin-config` parses YAML). The store never parses the document, so
//! the suffix is for whoever opens the directory, not for the store.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ragondin_types::QueryId;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::compare::{compare, RunComparison};
use crate::run::{ConfigDocument, Metrics, Run, RunId, RunInputs, TraceDocument};

const INPUTS_FILE: &str = "inputs.json";
const METRICS_FILE: &str = "metrics.json";
const CONFIG_FILE: &str = "config.yaml";
const TRACES_FILE: &str = "traces.json";

/// A run store backed by a directory tree.
///
/// Writing a run twice is not an error: a run is named by the digest of its
/// inputs, so the second write is the same run and overwrites itself.
#[derive(Clone, Debug)]
pub struct FileSystemRunStore {
    root: PathBuf,
}

impl FileSystemRunStore {
    /// A store rooted at `root`. The directory is created when the first run is
    /// saved; nothing is read or written here.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory the store's runs live under.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Writes a run, creating its directory.
    pub fn save(&self, run: &Run) -> Result<(), RunStoreError> {
        let dir = self.run_dir(&run.id);
        fs::create_dir_all(&dir).map_err(|source| RunStoreError::Io {
            path: dir.clone(),
            source,
        })?;

        write_json(&dir.join(INPUTS_FILE), &run.inputs)?;
        write_json(&dir.join(METRICS_FILE), &run.metrics)?;
        write_text(&dir.join(CONFIG_FILE), run.config.as_str())?;
        write_json(&dir.join(TRACES_FILE), &run.traces)
    }

    /// Reads the run named by `id`.
    pub fn load(&self, id: &RunId) -> Result<Run, RunStoreError> {
        let dir = self.run_dir(id);
        if !dir.is_dir() {
            return Err(RunStoreError::NotFound { id: *id });
        }

        let inputs: RunInputs = read_json(&dir.join(INPUTS_FILE))?;
        let metrics: Metrics = read_json(&dir.join(METRICS_FILE))?;
        let traces: BTreeMap<QueryId, TraceDocument> = read_json(&dir.join(TRACES_FILE))?;
        let config = ConfigDocument::new(read_text(&dir.join(CONFIG_FILE))?);

        Ok(Run {
            id: *id,
            inputs,
            metrics,
            config,
            traces,
        })
    }

    /// Compares two stored runs, metric by metric.
    ///
    /// The comparison a run store is asked for is between two *ids* — that is
    /// what `ragondin compare` takes and what a comparison view links to — so
    /// loading both and diffing them is one call rather than three.
    pub fn compare(&self, left: &RunId, right: &RunId) -> Result<RunComparison, RunStoreError> {
        Ok(compare(&self.load(left)?, &self.load(right)?))
    }

    fn run_dir(&self, id: &RunId) -> PathBuf {
        self.root.join(id.to_string())
    }
}

/// Why a run could not be written or read.
#[derive(Debug, thiserror::Error)]
pub enum RunStoreError {
    /// No run under that id.
    #[error("no run {id} in this store")]
    NotFound {
        /// The id that named nothing.
        id: RunId,
    },
    /// A file could not be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file the failure is about.
        path: PathBuf,
        /// What the filesystem reported.
        source: io::Error,
    },
    /// A record could not be written as JSON, or a file is not the record the
    /// store writes there. Both directions, because both are the same fault:
    /// the bytes on disk and the record in memory do not correspond. A metric
    /// of `NaN` is the writing side of it — JSON has no such number.
    #[error("{}: not a well-formed run record: {source}", path.display())]
    Malformed {
        /// The file the failure is about.
        path: PathBuf,
        /// What serde reported.
        source: serde_json::Error,
    },
}

/// Writes `value` as pretty-printed JSON with a trailing newline — the store is
/// meant to be read and diffed by people, and a one-line file is neither.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), RunStoreError> {
    let mut json =
        serde_json::to_string_pretty(value).map_err(|source| RunStoreError::Malformed {
            path: path.to_path_buf(),
            source,
        })?;
    json.push('\n');
    write_text(path, &json)
}

fn write_text(path: &Path, text: &str) -> Result<(), RunStoreError> {
    fs::write(path, text).map_err(|source| RunStoreError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, RunStoreError> {
    serde_json::from_str(&read_text(path)?).map_err(|source| RunStoreError::Malformed {
        path: path.to_path_buf(),
        source,
    })
}

fn read_text(path: &Path) -> Result<String, RunStoreError> {
    fs::read_to_string(path).map_err(|source| RunStoreError::Io {
        path: path.to_path_buf(),
        source,
    })
}
