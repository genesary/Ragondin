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
//!
//! # A run directory appears whole or not at all
//!
//! [`FileSystemRunStore::save`] writes the four files into a staging directory
//! beside the destination and then renames it into place, which is atomic
//! within one filesystem. That is not tidiness: a caller asks *is this run
//! already stored* to decide whether to execute it at all, and a directory
//! that exists but is half-written would answer that question wrongly. A crash
//! mid-write therefore leaves a `.partial` directory, which is inert — nothing
//! reads one, and the run simply is not in the store.
//!
//! A run already in the store is **left untouched** rather than rewritten. Its
//! id is the digest of its inputs, so a second save of the same id is the same
//! run; rewriting it could only replace it with itself, and `fs::write`
//! truncates, so a crash mid-rewrite would destroy a run that was complete.
//! That is also what makes two processes saving one run safe: they write the
//! same bytes, each into a staging directory of its own, and the first to
//! arrive is the one that stays.
//!
//! An incomplete directory can still be *made* — by a hand that deletes a file
//! under the store root, or by a writer that predates this scheme — so it has
//! a name: [`RunStoreError::Incomplete`], distinct from the [`RunStoreError::Io`]
//! a permission failure produces.

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

    /// Writes a run, and does nothing if that run is already stored.
    ///
    /// The run appears under its id whole or not at all — see the module's
    /// *A run directory appears whole or not at all*.
    pub fn save(&self, run: &Run) -> Result<(), RunStoreError> {
        // Before anything is created, because this is the one fault that would
        // otherwise be discovered on the far side of a durable write: JSON has
        // no non-finite number, `serde_json` writes one as `null`, and `null`
        // does not read back as an `f64`. A run stored that way would be
        // unreadable for good, under an id a caller already believes is done.
        if let Some((metric, _)) = run.metrics.iter().find(|(_, value)| !value.is_finite()) {
            return Err(RunStoreError::NotFinite {
                metric: metric.to_owned(),
            });
        }

        let destination = self.run_dir(&run.id);
        if destination.is_dir() {
            return Ok(());
        }

        let staging = self.staging_dir(&run.id);
        // A staging directory left by a crash of this same process id carries
        // an unfinished run; it is not evidence about this one.
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir_all(&staging).map_err(|source| RunStoreError::Io {
            path: staging.clone(),
            source,
        })?;

        write_json(&staging.join(INPUTS_FILE), &run.inputs)?;
        write_json(&staging.join(METRICS_FILE), &run.metrics)?;
        write_text(&staging.join(CONFIG_FILE), run.config.as_str())?;
        write_json(&staging.join(TRACES_FILE), &run.traces)?;

        if let Err(source) = fs::rename(&staging, &destination) {
            if destination.is_dir() {
                // Another writer stored this same run while this one was
                // staging it. Its bytes are ours.
                let _ = fs::remove_dir_all(&staging);
                return Ok(());
            }
            return Err(RunStoreError::Io {
                path: destination,
                source,
            });
        }

        Ok(())
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

    /// Where a run is assembled before it is renamed into place. Named after
    /// the writing process as well as the run, so two processes saving one run
    /// stage it independently rather than into each other.
    fn staging_dir(&self, id: &RunId) -> PathBuf {
        self.root
            .join(format!(".{id}.{}.partial", std::process::id()))
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
    /// A metric is not a finite number, and JSON has no way to write one.
    ///
    /// Refused on the way in rather than on the way out: `serde_json` writes a
    /// non-finite float as `null`, which does not read back, so a run stored
    /// with one would be permanently unreadable under an id whose existence
    /// says it is done. `NaN` is not exotic here — nDCG over a query with no
    /// relevant document, or a mean over an empty query set, is one.
    #[error("the metric {metric} is not a finite number, and cannot be stored")]
    NotFinite {
        /// The metric whose value could not be written.
        metric: String,
    },
    /// A run's directory exists but a file of the run is missing.
    #[error("{}: this run is incomplete", path.display())]
    Incomplete {
        /// The file that is not there.
        path: PathBuf,
    },
    /// A file could not be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file the failure is about.
        path: PathBuf,
        /// What the filesystem reported.
        source: io::Error,
    },
    /// A file of the run is not the record the store writes there.
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

/// Reads one file of a run. A file that is not there is reported as an
/// incomplete run rather than as an I/O failure: the caller reached a run
/// directory, so *absent* says something about the run, and a caller should
/// not have to read an `io::ErrorKind` to tell a torn run from a permission
/// problem.
fn read_text(path: &Path) -> Result<String, RunStoreError> {
    fs::read_to_string(path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            RunStoreError::Incomplete {
                path: path.to_path_buf(),
            }
        } else {
            RunStoreError::Io {
                path: path.to_path_buf(),
                source,
            }
        }
    })
}
