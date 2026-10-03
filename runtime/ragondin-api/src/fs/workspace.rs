//! The workspace: one directory holding everything `ragondin ui` reads and
//! writes (the design document § 6), and the paths derived from its root.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::settings_file;

/// The settings file, at the root.
const SETTINGS_FILE: &str = "workspace.toml";
/// The directories a workspace holds. `runs/` is the store's default place;
/// a workspace opened with a store elsewhere creates that one instead.
const PIPELINES: &str = "pipelines";
const LAYOUTS: &str = "layouts";
const RUNS: &str = "runs";
const JOBS: &str = "jobs";
const CACHE: &str = "cache";
const DATASETS: &str = "datasets";

/// A workspace on disk, opened: its root, and every path derived from it.
///
/// ```text
/// <root>/
///   workspace.toml                the datasets directory and the services — never hashed
///   pipelines/<name>.yaml         a pipeline document, the source of truth
///   pipelines/<name>.layout.json  its layout, never in its hash
///   layouts/                      layouts copied at launch, by run
///   runs/                         the run store, as `bench --store <root>/runs` writes it
///   jobs/                         the queue's state
///   cache/                        derived data, reconstructible
///   datasets/                     benchmarks, when workspace.toml names no other directory
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
    runs: PathBuf,
}

/// Why a workspace did not open.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// `workspace.toml` is not TOML, or is outside its schema. Reported,
    /// never repaired: nothing was created or rewritten.
    #[error("{}:{line}: {reason}", path.display())]
    Malformed {
        /// The file.
        path: PathBuf,
        /// The line, from 1.
        line: usize,
        /// What is wrong on it.
        reason: String,
    },
    /// A file or directory of the workspace could not be read or created.
    #[error("{}: {source}", path.display())]
    Io {
        /// What could not be read or created.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: io::Error,
    },
}

impl Workspace {
    /// Opens the workspace at `root`, its runs in `<root>/runs`.
    ///
    /// `workspace.toml` is read first, and a malformed one is refused before
    /// anything is created; then each missing directory is created, and a
    /// missing `workspace.toml` written with nothing set. An existing
    /// workspace is left as it is.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = root.as_ref();
        Self::open_with_store(root, root.join(RUNS))
    }

    /// Opens the workspace at `root` with its run store at `runs` — where
    /// `ragondin ui --store` named one that is not `<root>/runs`.
    pub fn open_with_store(
        root: impl AsRef<Path>,
        runs: impl AsRef<Path>,
    ) -> Result<Self, WorkspaceError> {
        let workspace = Self {
            root: root.as_ref().to_path_buf(),
            runs: runs.as_ref().to_path_buf(),
        };
        let settings = workspace.settings_file();
        let existing = match fs::read_to_string(&settings) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(WorkspaceError::Io {
                    path: settings,
                    source,
                })
            }
        };
        if let Some(text) = &existing {
            settings_file::parse(text).map_err(|error| WorkspaceError::Malformed {
                path: settings.clone(),
                line: error.line,
                reason: error.reason,
            })?;
        }
        for directory in [
            workspace.pipelines(),
            workspace.layouts(),
            workspace.runs(),
            workspace.jobs(),
            workspace.cache(),
            workspace.default_datasets(),
        ] {
            fs::create_dir_all(&directory).map_err(|source| WorkspaceError::Io {
                path: directory.clone(),
                source,
            })?;
        }
        if existing.is_none() {
            fs::write(&settings, settings_file::empty()).map_err(|source| WorkspaceError::Io {
                path: settings.clone(),
                source,
            })?;
        }
        Ok(workspace)
    }

    /// The workspace directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `workspace.toml`.
    pub fn settings_file(&self) -> PathBuf {
        self.root.join(SETTINGS_FILE)
    }

    /// `pipelines/`: the documents and their layouts.
    pub fn pipelines(&self) -> PathBuf {
        self.root.join(PIPELINES)
    }

    /// `layouts/`: layouts copied at launch.
    pub fn layouts(&self) -> PathBuf {
        self.root.join(LAYOUTS)
    }

    /// The run store's directory: `runs/`, unless opened with another.
    pub fn runs(&self) -> PathBuf {
        self.runs.clone()
    }

    /// `jobs/`: the queue's state.
    pub fn jobs(&self) -> PathBuf {
        self.root.join(JOBS)
    }

    /// `cache/`: derived data.
    pub fn cache(&self) -> PathBuf {
        self.root.join(CACHE)
    }

    /// `datasets/`: where benchmarks are read when `workspace.toml` names no
    /// other directory.
    pub fn default_datasets(&self) -> PathBuf {
        self.root.join(DATASETS)
    }
}
