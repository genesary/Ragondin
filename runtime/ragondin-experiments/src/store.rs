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
//!
//! **Every writer stages into a directory no other writer names.** The staging
//! name carries the writing process *and* a counter drawn once per call, so two
//! threads of one process — this store is `Clone` and `Sync`, so that is an
//! ordinary thing to do — never meet in it. Nothing clears a staging directory
//! on the way in: a name that is unique has nothing to clear, and a `save` that
//! began by emptying a shared path would be reaching into a directory another
//! live writer owns. Concurrent savers of one run therefore each write the same
//! bytes into a place of their own, and the first to rename is the one that
//! stays; the others find the run already there and report success.
//!
//! A staging directory outlives its `save` only if the process dies inside one:
//! an error on the way out removes it. Nothing sweeps the ones a crash leaves,
//! so they accumulate until someone deletes them. They are inert — no run is
//! read from one — and they are marked for a reader as well as for a person:
//! **an entry under the store root whose name begins with `.` is not a run**,
//! which is the convention anything that lists the root must honour. A run id
//! is 64 hex digits, so `.<id>.<pid>-<n>.partial` cannot be parsed as one.
//!
//! An incomplete directory can still be *made* — by a hand that deletes a file
//! under the store root, or by a writer that predates this scheme — so it has
//! a name: [`RunStoreError::Incomplete`], distinct from the [`RunStoreError::Io`]
//! a permission failure produces. [`FileSystemRunStore::save`] reports it too,
//! rather than treating any directory under the id as a stored run: a torn
//! directory is not repaired, because a run's metrics and traces are *not*
//! determined by its id — a judge's scores are not reproducible — so deleting
//! one to write another would destroy the only copy of something. Naming the
//! fault leaves the choice with whoever can make it.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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
    /// *A run directory appears whole or not at all*. `Ok(())` therefore means
    /// *this run is in the store, with all four of its files*; a directory
    /// under the id that is missing one is reported as
    /// [`Incomplete`](RunStoreError::Incomplete) rather than passed over as
    /// stored. Present is as far as that check goes: a file corrupted in place
    /// still reads as a fault at [`load`](Self::load), which is where a record
    /// is parsed.
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
            return complete(&destination);
        }

        // Named for this call alone, so that two writers of one run never meet
        // in it — and therefore never has to be cleared, which would mean
        // emptying a directory another live writer may own.
        let staging = Staging::create(self.staging_dir(&run.id))?;
        let dir = staging.path();

        write_json(&dir.join(INPUTS_FILE), &run.inputs)?;
        write_json(&dir.join(METRICS_FILE), &run.metrics)?;
        write_text(&dir.join(CONFIG_FILE), run.config.as_str())?;
        write_json(&dir.join(TRACES_FILE), &run.traces)?;

        publish(staging, &destination)
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

    /// Where one call assembles a run before renaming it into place.
    ///
    /// Unique per *writer*, not per process: the process id separates two
    /// programs, and the counter separates two calls within one program —
    /// including two threads, since this store is `Clone` and `Sync` and
    /// saving from several at once is an ordinary thing to do. A name nobody
    /// else can compute is what lets `save` create it without first clearing
    /// it.
    fn staging_dir(&self, id: &RunId) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);

        let ticket = NEXT.fetch_add(1, Ordering::Relaxed);
        self.root
            .join(format!(".{id}.{}-{ticket}.partial", std::process::id()))
    }
}

/// A staging directory, removed when it is dropped without being published.
///
/// The four writes below return early on failure, and each of those paths used
/// to leave the directory behind for ever — nothing sweeps the store root. A
/// guard puts the cleanup on the one path that cannot be forgotten.
struct Staging {
    path: PathBuf,
    published: bool,
}

impl Staging {
    fn create(path: PathBuf) -> Result<Self, RunStoreError> {
        fs::create_dir_all(&path).map_err(|source| RunStoreError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(Self {
            path,
            published: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.published {
            // Best effort: this runs on a failure path, where a second failure
            // has nothing to add to the one being reported.
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Renames a staged run into its place under the store root.
///
/// Two ways this does not simply succeed, and they are different. The
/// destination may have appeared since `save` looked — another writer stored
/// the same run — and then the rename fails against a non-empty directory
/// whose content is the content this writer staged, so the run is stored and
/// the staged copy is dropped. Anything else is the filesystem refusing, and
/// is reported.
fn publish(mut staging: Staging, destination: &Path) -> Result<(), RunStoreError> {
    match fs::rename(staging.path(), destination) {
        Ok(()) => {
            staging.published = true;
            Ok(())
        }
        Err(source) => {
            if destination.is_dir() {
                Ok(())
            } else {
                Err(RunStoreError::Io {
                    path: destination.to_path_buf(),
                    source,
                })
            }
        }
    }
}

/// `Ok(())` if every file of a run is present under `dir`, and which one is
/// missing otherwise.
fn complete(dir: &Path) -> Result<(), RunStoreError> {
    for member in [INPUTS_FILE, METRICS_FILE, CONFIG_FILE, TRACES_FILE] {
        let path = dir.join(member);
        if !path.is_file() {
            return Err(RunStoreError::Incomplete { path });
        }
    }
    Ok(())
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
    /// says it is done. The per-query metrics do not produce one —
    /// `ragondin-metrics` guards its zero denominators — but the aggregate a
    /// run records has denominators of its own, and a mean over an empty query
    /// set is `0.0 / 0.0`.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of this test's own. `save`'s behaviour is covered from
    /// outside, in `tests/run_store.rs`; what is here is the pair of branches
    /// reachable only when the destination changes underneath a writer, which
    /// no caller of `save` can arrange on purpose.
    /// `CARGO_TARGET_TMPDIR` is given to an integration test and not to this
    /// one, so the directory is named after the process and the test instead —
    /// the same idea, and no `tempfile` dependency for it.
    fn scratch(test_name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ragondin-experiments-publish-{}-{test_name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the scratch directory must be creatable");
        path
    }

    fn staged(root: &Path, name: &str) -> Staging {
        let staging = Staging::create(root.join(name)).expect("staging must be creatable");
        fs::write(staging.path().join(INPUTS_FILE), "{}").expect("a staged file must be writable");
        staging
    }

    #[test]
    fn losing_the_race_to_a_destination_that_now_exists_is_success() {
        let root = scratch("race_lost");
        let staging = staged(&root, "staged");
        let staging_path = staging.path().to_path_buf();

        // What the winner of the race left: a directory under the id, not
        // empty — which is what makes the rename fail rather than succeed.
        let destination = root.join("destination");
        fs::create_dir_all(&destination).expect("the destination must be creatable");
        fs::write(destination.join(INPUTS_FILE), "{}").expect("the winner's file must be writable");

        publish(staging, &destination).expect("the run is stored, whoever stored it");

        assert!(
            !staging_path.exists(),
            "the staged copy is dropped, not left beside the run"
        );
        assert!(destination.join(INPUTS_FILE).is_file());
    }

    #[test]
    fn a_rename_that_fails_for_any_other_reason_is_reported() {
        let root = scratch("rename_refused");
        let staging = staged(&root, "staged");
        let staging_path = staging.path().to_path_buf();

        // No directory to rename into, and none appears: the filesystem is
        // refusing, and a refusal is not a run that someone else stored.
        let destination = root.join("absent-parent").join("destination");

        match publish(staging, &destination) {
            Err(RunStoreError::Io { path, .. }) => assert_eq!(path, destination),
            other => panic!("a refused rename must be reported, got {other:?}"),
        }
        assert!(
            !staging_path.exists(),
            "a failed publish leaves no staging directory behind"
        );
    }

    #[test]
    fn a_staging_directory_is_removed_unless_it_was_published() {
        let root = scratch("staging_guard");
        let path = {
            let staging = staged(&root, "staged");
            staging.path().to_path_buf()
        };
        assert!(!path.exists(), "dropping a staging directory removes it");
    }
}
