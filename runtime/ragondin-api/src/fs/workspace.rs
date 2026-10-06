//! The workspace: one directory holding everything `ragondin ui` reads and
//! writes (the design document § 6), and the paths derived from its root.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::settings_file;

/// The settings file, at the root.
const SETTINGS_FILE: &str = "workspace.toml";
/// The files an operating system's file manager leaves in a folder it merely
/// showed. A folder holding only these is empty to [`Workspace::open`].
const OS_METADATA: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];
/// The subfolder suggested when a refused folder holds no workspace below it.
const SUGGESTED: &str = "workspace";
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
///   layouts/                      layouts copied at launch, by canonical hash
///   runs/                         the run store, as `bench --store <root>/runs` writes it
///   jobs/                         the queue's state
///   cache/                        derived data, reconstructible
///   datasets/                     benchmarks, when workspace.toml names no other directory
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
    runs: PathBuf,
    created: bool,
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
    /// The folder holds files but no `workspace.toml`: it is not a
    /// workspace, and a new one is created only in a missing or empty
    /// folder ([`Workspace::open`] says what counts as empty). Refused
    /// untouched: nothing was created.
    #[error(
        "{} holds files but no workspace.toml, so it is not a workspace, and a new workspace \
         is created only in a missing or empty folder; {}",
        path.display(),
        match suggestion {
            Some(suggestion) => format!("did you mean {}?", suggestion.display()),
            None => "name an existing workspace, or an empty folder".to_owned(),
        }
    )]
    NotAWorkspace {
        /// The folder.
        path: PathBuf,
        /// A subfolder that is the workspace the caller likely meant: the
        /// first, by name, that holds a `workspace.toml`, or else one called
        /// `workspace`.
        suggestion: Option<PathBuf>,
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
    /// anything is created. A folder without one becomes a new workspace only
    /// when it is missing or empty; one that holds anything else is refused
    /// with [`WorkspaceError::NotAWorkspace`], untouched. **Empty** means it
    /// holds nothing but the file manager's metadata (`.DS_Store`,
    /// `Thumbs.db`, `desktop.ini`) and directories the workspace itself
    /// creates — its layout's and its store's — so a `runs/` that `bench
    /// --store <root>/runs` wrote first, or a workspace whose
    /// `workspace.toml` an interrupted first open never wrote, still opens.
    /// Then each missing directory is created, and a missing
    /// `workspace.toml` written with nothing set; [`Workspace::created`]
    /// reports that. An existing workspace is left as it is.
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
        let mut workspace = Self {
            root: root.as_ref().to_path_buf(),
            runs: runs.as_ref().to_path_buf(),
            created: false,
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
        } else {
            workspace.refuse_unless_empty()?;
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
            workspace.created = true;
        }
        Ok(workspace)
    }

    /// Refuses a root that holds anything a new workspace may not be created
    /// beside. A missing root is empty.
    fn refuse_unless_empty(&self) -> Result<(), WorkspaceError> {
        let io = |source| WorkspaceError::Io {
            path: self.root.clone(),
            source,
        };
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => return Err(io(source)),
        };
        let own = [
            self.pipelines(),
            self.layouts(),
            self.runs(),
            self.jobs(),
            self.cache(),
            self.default_datasets(),
        ];
        for entry in entries {
            let path = entry.map_err(io)?.path();
            let metadata = path.file_name().is_some_and(|name| {
                OS_METADATA
                    .iter()
                    .any(|metadata| name == std::ffi::OsStr::new(metadata))
            });
            if !metadata && !(path.is_dir() && own.contains(&path)) {
                return Err(WorkspaceError::NotAWorkspace {
                    path: self.root.clone(),
                    suggestion: self.suggestion(),
                });
            }
        }
        Ok(())
    }

    /// The subfolder of a refused root the caller likely meant: the first, by
    /// name, that holds a `workspace.toml`, or else `workspace/`.
    fn suggestion(&self) -> Option<PathBuf> {
        let mut subfolders: Vec<PathBuf> = fs::read_dir(&self.root)
            .ok()?
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|path| path.is_dir())
            .collect();
        subfolders.sort();
        subfolders
            .iter()
            .find(|path| path.join(SETTINGS_FILE).is_file())
            .cloned()
            .or_else(|| Some(self.root.join(SUGGESTED)).filter(|path| path.is_dir()))
    }

    /// Whether this open created the workspace: `workspace.toml` was
    /// missing and has been written. The caller says so — the library
    /// prints nothing.
    pub fn created(&self) -> bool {
        self.created
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
        jobs_of(&self.root)
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

/// The queue's directory in the workspace at `root`: where the router reads
/// and writes its jobs, given the workspace's root alone.
pub(crate) fn jobs_of(root: &Path) -> PathBuf {
    root.join(JOBS)
}
