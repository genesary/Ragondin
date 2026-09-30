//! The datasets directory: putting a pinned snapshot on disk, checking what is
//! there against the identity a run carries, and importing a local corpus.
//!
//! Every check here is a statement about digests: "verified" means the loaded
//! dataset digests, through [`dataset_version`], to the value expected — the
//! same function a run's identity hashes, so a verified dataset is exactly the
//! one a run over it records. Nothing in this module computes a dataset digest
//! any other way.
//!
//! **A failure leaves nothing behind.** A download or an import is assembled
//! in a staging directory beside its destination, named with a leading `.`,
//! and renamed into place only once every check passed; on any error the
//! staging directory is removed before the error returns. So a dataset's
//! directory exists only when what it holds verified.
//!
//! This is the one place the crate fetches over the network, and it fetches
//! only to put a frozen snapshot on disk: nothing is ever read from the network
//! during evaluation (`ARCHITECTURE.md` § Local constraints).

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::identity::dataset_version;
use crate::manifest::{manifest, Format, ManifestEntry};
use crate::{Benchmark, BenchmarkAdapter};
use crate::{BenchmarkError, CarriedPieces, SquadAdapter};

/// The file that marks a directory as a local import, and records what it
/// was imported as.
pub const LOCAL_MARKER: &str = "ragondin-local.json";

/// How long a download waits for a connection, and then for each read. A
/// server that stops answering fails the download instead of holding it.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// How far a download is: bytes received of the snapshot's total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Bytes received so far, every file together.
    pub received: u64,
    /// The snapshot's size, as the manifest states it.
    pub total: u64,
}

/// A dataset on disk whose digest is the one expected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    /// What it digests to.
    pub dataset_version: String,
    /// The ground truth it carries.
    pub carries: CarriedPieces,
}

/// What a dataset directory holds, against the digest expected of it.
#[derive(Debug)]
pub enum DiskState {
    /// There is no such directory.
    Absent,
    /// It loads, and digests to the value expected.
    Verified(Verified),
    /// It loads, and digests to another value.
    Differs {
        /// The digest expected.
        expected: String,
        /// The digest it has.
        found: String,
        /// The ground truth it carries: it loaded, so this is known.
        carries: CarriedPieces,
    },
    /// It does not load.
    Unreadable {
        /// The adapter's error.
        error: BenchmarkError,
    },
}

/// Loads the dataset at `dir` with `format` and compares its
/// `dataset_version` with `expected`.
///
/// The dataset is loaded whole to be digested; nothing is cached.
pub fn verify(dir: &Path, format: Format, expected: &str) -> DiskState {
    if !dir.exists() {
        return DiskState::Absent;
    }
    match format.load(dir) {
        Err(error) => DiskState::Unreadable { error },
        Ok(benchmark) => {
            let found = dataset_version(&benchmark);
            if found == expected {
                DiskState::Verified(Verified {
                    dataset_version: found,
                    carries: benchmark.carries(),
                })
            } else {
                DiskState::Differs {
                    expected: expected.to_owned(),
                    found,
                    carries: benchmark.carries(),
                }
            }
        }
    }
}

/// Why a download was refused. Each names the manifest entry.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DownloadError {
    /// The dataset's directory already exists. A download never overwrites
    /// one: what is there may be a user's, and a failure must not remove it.
    #[error("{entry}: {} already exists; remove it to download again", path.display())]
    Occupied {
        /// The entry.
        entry: String,
        /// The directory.
        path: PathBuf,
    },
    /// A file's path would land outside the dataset's directory.
    #[error("{entry}: {path} is not a path inside the dataset's directory")]
    InvalidPath {
        /// The entry.
        entry: String,
        /// The path the entry gives.
        path: String,
    },
    /// A file could not be fetched: no connection, a status other than
    /// success, a transfer cut short.
    #[error("{entry}: fetching {url} failed: {reason}")]
    Fetch {
        /// The entry.
        entry: String,
        /// The URL.
        url: String,
        /// What failed, with every cause.
        reason: String,
    },
    /// A file's bytes digest to another value than the manifest's.
    #[error("{entry}: {file} digests to {found}, the manifest pins {expected}")]
    FileDigest {
        /// The entry.
        entry: String,
        /// The file's path in the snapshot.
        file: String,
        /// The manifest's SHA-256.
        expected: String,
        /// The SHA-256 of what was received.
        found: String,
    },
    /// Every file verified, and the snapshot does not load.
    #[error("{entry}: the snapshot does not load")]
    Load {
        /// The entry.
        entry: String,
        /// The adapter's error.
        #[source]
        source: BenchmarkError,
    },
    /// The snapshot loads, and its `dataset_version` is not the manifest's.
    #[error("{entry}: the snapshot's dataset_version is {found}, the manifest pins {expected}")]
    DatasetVersion {
        /// The entry.
        entry: String,
        /// The manifest's `dataset_version`.
        expected: String,
        /// The loaded snapshot's.
        found: String,
    },
    /// The datasets directory could not be written.
    #[error("{entry}: cannot write {}", path.display())]
    Io {
        /// The entry.
        entry: String,
        /// The path.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
}

/// Fetches `entry`'s snapshot into `<datasets>/<dir>`, verifying each file's
/// SHA-256 as it lands and then the loaded snapshot's `dataset_version`, and
/// returns what it verified as.
///
/// Synchronous: it drives its own single-threaded runtime for the HTTP client,
/// so it must not be called from an async task — a caller in one moves it to a
/// blocking thread. `progress` receives the bytes received after every chunk.
///
/// # Errors
///
/// [`DownloadError`]; on any of them nothing is left under `datasets` that
/// this call created.
pub fn download(
    entry: &ManifestEntry,
    datasets: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<Verified, DownloadError> {
    let name = entry.name.as_str();
    let destination = datasets.join(entry.dir());
    if destination.exists() {
        return Err(DownloadError::Occupied {
            entry: name.to_owned(),
            path: destination,
        });
    }
    let io_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| DownloadError::Io {
            entry: name.to_owned(),
            path,
            source,
        }
    };
    fs::create_dir_all(datasets).map_err(io_error(datasets))?;
    let staging = Staging::create(datasets.join(format!(".{}.download", entry.dir())))
        .map_err(io_error(datasets))?;

    fetch_all(entry, staging.path(), progress)?;

    let benchmark = entry
        .format
        .load(staging.path())
        .map_err(|source| DownloadError::Load {
            entry: name.to_owned(),
            source,
        })?;
    let found = dataset_version(&benchmark);
    if found != entry.dataset_version {
        return Err(DownloadError::DatasetVersion {
            entry: name.to_owned(),
            expected: entry.dataset_version.clone(),
            found,
        });
    }
    staging
        .publish(&destination)
        .map_err(io_error(&destination))?;
    Ok(Verified {
        dataset_version: found,
        carries: benchmark.carries(),
    })
}

/// Fetches every file of `entry` into `staging`, refusing the first whose
/// digest is not the manifest's.
fn fetch_all(
    entry: &ManifestEntry,
    staging: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), DownloadError> {
    let name = entry.name.as_str();
    let fetch_error = |url: &str, reason: String| DownloadError::Fetch {
        entry: name.to_owned(),
        url: url.to_owned(),
        reason,
    };
    // One runtime per download, current-thread: the client needs one, and
    // this crate's API stays synchronous (`ARCHITECTURE.md` says why).
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| DownloadError::Io {
            entry: name.to_owned(),
            path: staging.to_path_buf(),
            source,
        })?;
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .map_err(|error| fetch_error("", causes(&error)))?;

    let total = entry.size_bytes();
    let mut received = 0u64;
    for file in &entry.files {
        let relative = inside(&file.path).ok_or_else(|| DownloadError::InvalidPath {
            entry: name.to_owned(),
            path: file.path.clone(),
        })?;
        let target = staging.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|source| DownloadError::Io {
                entry: name.to_owned(),
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let found = runtime
            .block_on(fetch(&client, &file.url, &target, &mut |bytes| {
                received += bytes;
                progress(Progress { received, total });
            }))
            .map_err(|failure| match failure {
                Failure::Http(reason) => fetch_error(&file.url, reason),
                Failure::Io(source) => DownloadError::Io {
                    entry: name.to_owned(),
                    path: target.clone(),
                    source,
                },
            })?;
        if found != file.sha256 {
            return Err(DownloadError::FileDigest {
                entry: name.to_owned(),
                file: file.path.clone(),
                expected: file.sha256.clone(),
                found,
            });
        }
    }
    Ok(())
}

enum Failure {
    Http(String),
    Io(io::Error),
}

/// Streams `url` into `target`, hashing as it writes, and returns the SHA-256
/// of what was received.
async fn fetch(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
    on_chunk: &mut dyn FnMut(u64),
) -> Result<String, Failure> {
    let http = |error: reqwest::Error| Failure::Http(causes(&error));
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(http)?;
    let mut out = File::create(target).map_err(Failure::Io)?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response.chunk().await.map_err(http)? {
        out.write_all(&chunk).map_err(Failure::Io)?;
        hasher.update(&chunk);
        on_chunk(chunk.len() as u64);
    }
    out.sync_all().map_err(Failure::Io)?;
    Ok(hex(&hasher.finalize()))
}

/// An error and every cause beneath it, on one line: a `reqwest` error's own
/// text rarely says what failed underneath.
fn causes(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let _ = write!(text, ": {cause}");
        source = cause.source();
    }
    text
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// `path` as a relative path that stays inside its root, or `None`.
fn inside(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    let normal = path
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    (normal && path.components().next().is_some()).then(|| path.to_path_buf())
}

/// A local import, as the datasets directory records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalEntry {
    /// The name the user gave: its directory under the datasets directory.
    pub name: String,
    /// The format it was imported as.
    pub format: Format,
    /// What it digested to when it was imported.
    pub dataset_version: String,
}

impl LocalEntry {
    /// Its benchmark selector, `<format>/<name>`, as `ragondin bench` takes it.
    pub fn selector(&self) -> String {
        format!("{}/{}", self.format.selector(), self.name)
    }
}

/// What an import registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    /// The local entry.
    pub entry: LocalEntry,
    /// The ground truth it carries.
    pub carries: CarriedPieces,
}

/// Why an import was refused.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ImportError {
    /// The name is not one directory name.
    #[error("{name:?} cannot name a dataset: {reason}")]
    InvalidName {
        /// The name given.
        name: String,
        /// Why not.
        reason: &'static str,
    },
    /// A dataset — imported, downloaded, or one the manifest names — already
    /// has the name.
    #[error("{name} is taken: {} is a dataset's directory", path.display())]
    Occupied {
        /// The name given.
        name: String,
        /// The directory it would occupy.
        path: PathBuf,
    },
    /// What was named to import cannot be read: it does not exist, say.
    #[error("cannot read {}", path.display())]
    Source {
        /// What was named.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// The corpus does not load: the adapter's own error.
    #[error("{} does not load", path.display())]
    Load {
        /// What was imported.
        path: PathBuf,
        /// The adapter's error.
        #[source]
        source: BenchmarkError,
    },
    /// A file could not be read or written.
    #[error("cannot import through {}", path.display())]
    Io {
        /// The path.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
}

/// What [`LOCAL_MARKER`] holds.
#[derive(Serialize, Deserialize)]
struct Marker {
    format: String,
    dataset_version: String,
}

/// Imports the corpus at `source`, which carries its own ground truth, as the
/// local entry `name` in `datasets`.
///
/// A directory is read as BEIR — as `beir-qa` when it holds `answers.jsonl` —
/// and a file as SQuAD v1.1. The corpus is loaded, and refused with the
/// adapter's error when it does not load; then the files the adapter read are
/// copied into `<datasets>/<name>` beside [`LOCAL_MARKER`], which records its
/// format and `dataset_version`.
///
/// # Errors
///
/// [`ImportError`]; on any of them nothing is registered.
pub fn import(datasets: &Path, name: &str, source: &Path) -> Result<Imported, ImportError> {
    check_name(name)?;
    let destination = datasets.join(name);
    if destination.exists() || manifest().iter().any(|entry| entry.dir() == name) {
        return Err(ImportError::Occupied {
            name: name.to_owned(),
            path: destination,
        });
    }
    let io_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ImportError::Io { path, source }
    };

    let metadata = fs::metadata(source).map_err(|error| ImportError::Source {
        path: source.to_path_buf(),
        source: error,
    })?;
    let (format, benchmark) = if metadata.is_dir() {
        let format = if source.join("answers.jsonl").exists() {
            Format::BeirQa
        } else {
            Format::Beir
        };
        (format, format.load(source))
    } else {
        (Format::Squad, load_squad_file(source))
    };
    let benchmark = benchmark.map_err(|error| ImportError::Load {
        path: source.to_path_buf(),
        source: error,
    })?;

    fs::create_dir_all(datasets).map_err(io_error(datasets))?;
    let staging =
        Staging::create(datasets.join(format!(".{name}.import"))).map_err(io_error(datasets))?;
    copy_read_files(format, source, staging.path())?;
    let entry = LocalEntry {
        name: name.to_owned(),
        format,
        dataset_version: dataset_version(&benchmark),
    };
    let marker = Marker {
        format: format.selector().to_owned(),
        dataset_version: entry.dataset_version.clone(),
    };
    let marker_path = staging.path().join(LOCAL_MARKER);
    let text = serde_json::to_string_pretty(&marker).expect("a marker always serializes");
    fs::write(&marker_path, text + "\n").map_err(io_error(&marker_path))?;
    staging
        .publish(&destination)
        .map_err(io_error(&destination))?;
    Ok(Imported {
        entry,
        carries: benchmark.carries(),
    })
}

fn check_name(name: &str) -> Result<(), ImportError> {
    let refuse = |reason| {
        Err(ImportError::InvalidName {
            name: name.to_owned(),
            reason,
        })
    };
    if name.is_empty() {
        return refuse("it is empty");
    }
    if name.starts_with('.') {
        return refuse("a leading `.` is kept for staging directories");
    }
    if name.contains(['/', '\\']) {
        return refuse("it must be one directory name, without a separator");
    }
    Ok(())
}

fn load_squad_file(file: &Path) -> Result<Benchmark, BenchmarkError> {
    let root = file.parent().unwrap_or(Path::new("."));
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    SquadAdapter::with_file(root, name).load()
}

/// Copies what `format`'s adapter reads from `source` into `staging`: the BEIR
/// files and every qrels split, or the one SQuAD file under the name
/// `SquadAdapter::new` reads.
fn copy_read_files(format: Format, source: &Path, staging: &Path) -> Result<(), ImportError> {
    let copy = |from: &Path, to: &Path| {
        fs::copy(from, to)
            .map(drop)
            .map_err(|source| ImportError::Io {
                path: from.to_path_buf(),
                source,
            })
    };
    match format {
        Format::Squad => copy(source, &staging.join("dev-v1.1.json")),
        Format::Beir | Format::BeirQa => {
            let mut files = vec!["corpus.jsonl", "queries.jsonl"];
            if format == Format::BeirQa {
                files.push("answers.jsonl");
            }
            for file in files {
                copy(&source.join(file), &staging.join(file))?;
            }
            let qrels = source.join("qrels");
            let target = staging.join("qrels");
            let io_error = |path: &Path| {
                let path = path.to_path_buf();
                move |source| ImportError::Io { path, source }
            };
            fs::create_dir_all(&target).map_err(io_error(&target))?;
            for split in fs::read_dir(&qrels).map_err(io_error(&qrels))? {
                let split = split.map_err(io_error(&qrels))?;
                if split
                    .file_type()
                    .map_err(io_error(&split.path()))?
                    .is_file()
                {
                    copy(&split.path(), &target.join(split.file_name()))?;
                }
            }
            Ok(())
        }
    }
}

/// Every local import under `datasets`, by name. A directory without
/// [`LOCAL_MARKER`] is not one, and a `.`-named one is staging.
///
/// # Errors
///
/// The directory cannot be read, or a marker is not one this build wrote.
pub fn local_entries(datasets: &Path) -> io::Result<Vec<LocalEntry>> {
    let mut entries = Vec::new();
    let listing = match fs::read_dir(datasets) {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(entries),
        Err(error) => return Err(error),
    };
    for dir in listing {
        let dir = dir?;
        let name = dir.file_name().to_string_lossy().into_owned();
        let marker_path = dir.path().join(LOCAL_MARKER);
        if name.starts_with('.') || !marker_path.is_file() {
            continue;
        }
        let invalid = |reason: String| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {reason}", marker_path.display()),
            )
        };
        let marker: Marker = serde_json::from_str(&fs::read_to_string(&marker_path)?)
            .map_err(|error| invalid(error.to_string()))?;
        let format = Format::from_selector(&marker.format)
            .ok_or_else(|| invalid(format!("no format is named {:?}", marker.format)))?;
        entries.push(LocalEntry {
            name,
            format,
            dataset_version: marker.dataset_version,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// A directory assembled beside its destination and renamed into place, or
/// removed when dropped unpublished — so an error or a panic leaves nothing.
struct Staging {
    path: PathBuf,
    published: bool,
}

impl Staging {
    /// Creates `path` empty, removing what an interrupted earlier attempt
    /// left there.
    fn create(path: PathBuf) -> io::Result<Self> {
        match fs::remove_dir_all(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::create_dir(&path)?;
        Ok(Self {
            path,
            published: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn publish(mut self, destination: &Path) -> io::Result<()> {
        fs::rename(&self.path, destination)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
