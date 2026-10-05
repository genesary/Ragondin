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
//! in a staging directory of its own beside its destination — named with a
//! leading `.` and unique to the attempt — and renamed into place only once
//! every check passed; on any error the staging directory is removed before
//! the error returns. So a dataset's directory exists only when what it holds
//! verified, however many attempts overlap.
//!
//! **This crate does not speak HTTP.** [`download`] is handed a [`Fetcher`] —
//! the transport — and applies every rule itself: the size cap, the digest,
//! cancellation, the deadline, the `dataset_version`, the publish step. The
//! experiment plane's API supplies the HTTP fetcher, so the harness and the
//! binary, which depend on this crate and never download, carry no HTTP or
//! TLS stack (`ARCHITECTURE.md` § Putting a dataset on disk).

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::identity::dataset_version;
use crate::manifest::{Format, ManifestEntry, ManifestFile};
use crate::{Benchmark, BenchmarkAdapter};
use crate::{BenchmarkError, CarriedPieces, SquadAdapter};

/// The file that marks a directory as a local import, and records what it
/// was imported as.
pub const LOCAL_MARKER: &str = "ragondin-local.json";

/// The longest name an import accepts.
pub const MAX_NAME_LENGTH: usize = 64;

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

// ---------------------------------------------------------------------------
// The transport seam
// ---------------------------------------------------------------------------

/// The transport a download is handed: it fetches one URL and feeds what
/// arrives into the [`Body`].
///
/// It calls [`Body::announce`] when the length is known before the bytes (an
/// HTTP `Content-Length`), then [`Body::write`] for each chunk, and stops as
/// soon as either returns [`Stopped`]. It returns `Err` with its own reason
/// when the transfer fails. Whatever it returns after a `Stopped`, the
/// download reports the reason the body recorded — a transport that ignores
/// a refusal cannot turn it into a success.
pub trait Fetcher {
    /// Fetches `url` into `body`.
    ///
    /// # Errors
    ///
    /// Why the transfer failed, with every cause, in the transport's words.
    fn fetch(&mut self, url: &str, body: &mut Body<'_>) -> Result<(), String>;
}

/// The download refused to take more: the transfer must stop. The reason is
/// recorded in the [`Body`] and reported by [`download`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("the download stopped the transfer")]
pub struct Stopped;

/// Why a body stopped a transfer.
#[derive(Debug)]
enum Stop {
    TooLarge { seen: u64 },
    Cancelled,
    Deadline,
    Io(io::Error),
}

/// Where a transport writes one file: hashed, counted against the manifest's
/// size, written to the staging directory, and checked for cancellation and
/// the deadline after every chunk.
pub struct Body<'a> {
    limit: u64,
    out: File,
    hasher: Sha256,
    received: u64,
    before: u64,
    total: u64,
    progress: &'a mut dyn FnMut(Progress),
    cancelled: &'a AtomicBool,
    started: Instant,
    deadline: Duration,
    stop: Option<Stop>,
}

impl Body<'_> {
    /// The length the transport announces before the bytes. More than the
    /// manifest's size is refused before a byte is written.
    ///
    /// # Errors
    ///
    /// [`Stopped`] when the transfer must stop.
    pub fn announce(&mut self, length: u64) -> Result<(), Stopped> {
        if self.stop.is_some() {
            return Err(Stopped);
        }
        if length > self.limit {
            return self.halt(Stop::TooLarge { seen: length });
        }
        Ok(())
    }

    /// One chunk of the file.
    ///
    /// # Errors
    ///
    /// [`Stopped`] when the download was cancelled, outlived its deadline,
    /// received more than the manifest's size, or could not write.
    pub fn write(&mut self, chunk: &[u8]) -> Result<(), Stopped> {
        if self.stop.is_some() {
            return Err(Stopped);
        }
        if self.cancelled.load(Ordering::Relaxed) {
            return self.halt(Stop::Cancelled);
        }
        if self.started.elapsed() > self.deadline {
            return self.halt(Stop::Deadline);
        }
        let seen = self.received + chunk.len() as u64;
        if seen > self.limit {
            return self.halt(Stop::TooLarge { seen });
        }
        if let Err(error) = self.out.write_all(chunk) {
            return self.halt(Stop::Io(error));
        }
        self.hasher.update(chunk);
        self.received = seen;
        (self.progress)(Progress {
            received: self.before + self.received,
            total: self.total,
        });
        Ok(())
    }

    fn halt(&mut self, stop: Stop) -> Result<(), Stopped> {
        self.stop = Some(stop);
        Err(Stopped)
    }
}

/// What a download reports to, and what can stop it.
pub struct Controls<'a> {
    /// Receives bytes received of the snapshot's total after every chunk.
    pub progress: &'a mut dyn FnMut(Progress),
    /// Set from anywhere to cancel: checked after every chunk, and before each
    /// file.
    pub cancelled: &'a AtomicBool,
    /// How long the whole download may take, from its start. A server that
    /// trickles bytes fails at the deadline instead of holding the download.
    pub deadline: Duration,
}

impl Controls<'_> {
    /// The deadline a download of `entry` gets by default: a minute's grace,
    /// plus the time its size takes at 32 KiB/s — a minimum mean throughput.
    pub fn deadline_for(entry: &ManifestEntry) -> Duration {
        Duration::from_secs(60) + Duration::from_secs(entry.size_bytes() / (32 * 1024))
    }
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

/// Why a download was refused. Each names the manifest entry.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DownloadError {
    /// The dataset's directory exists — before the download started, or put
    /// there by another download that finished first. A download never
    /// overwrites one.
    #[error("{entry}: {} already exists; remove it to download again", path.display())]
    Occupied {
        /// The entry.
        entry: String,
        /// The directory.
        path: PathBuf,
    },
    /// A file's path would land outside the dataset's directory: a defect in
    /// the manifest this build carries.
    #[error("{entry}: {path} is not a path inside the dataset's directory")]
    InvalidPath {
        /// The entry.
        entry: String,
        /// The path the entry gives.
        path: String,
    },
    /// The transport failed: no connection, a status other than success, a
    /// transfer cut short.
    #[error("{entry}: fetching {url} failed: {reason}")]
    Fetch {
        /// The entry.
        entry: String,
        /// The URL.
        url: String,
        /// What failed, in the transport's words.
        reason: String,
    },
    /// A file announced, or ran to, more bytes than the manifest's size.
    #[error("{entry}: {file} is {seen} bytes or more, the manifest pins {limit}")]
    TooLarge {
        /// The entry.
        entry: String,
        /// The file's path in the snapshot.
        file: String,
        /// The manifest's size.
        limit: u64,
        /// The length announced, or the bytes counted when the cap was passed.
        seen: u64,
    },
    /// A file ended short of the manifest's size.
    #[error("{entry}: {file} ended after {received} bytes, the manifest pins {expected}")]
    Truncated {
        /// The entry.
        entry: String,
        /// The file's path in the snapshot.
        file: String,
        /// The manifest's size.
        expected: u64,
        /// The bytes received.
        received: u64,
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
    /// The download was cancelled.
    #[error("{entry}: the download was cancelled")]
    Cancelled {
        /// The entry.
        entry: String,
    },
    /// The download outlived its deadline.
    #[error("{entry}: the download took longer than {}s", deadline.as_secs())]
    DeadlineExceeded {
        /// The entry.
        entry: String,
        /// The deadline it had.
        deadline: Duration,
    },
    /// Every file verified, and the snapshot does not load: a defect in the
    /// manifest this build carries, since its digests pinned these bytes.
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
    /// The snapshot digests to the manifest's `dataset_version`, and carries
    /// other pieces than the entry declares: a defect in the manifest this
    /// build carries, whose declared ground truth was shown before the
    /// download.
    #[error("{entry}: the snapshot carries {found:?}, the manifest declares {declared:?}")]
    GroundTruth {
        /// The entry.
        entry: String,
        /// The pieces the manifest entry declares.
        declared: CarriedPieces,
        /// The pieces the loaded snapshot carries.
        found: CarriedPieces,
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

/// Fetches `entry`'s snapshot through `fetcher` into `<datasets>/<dir>`,
/// verifying each file's size and SHA-256 as it lands and then the loaded
/// snapshot's `dataset_version`, and returns what it verified as.
///
/// Synchronous: the fetcher blocks, and a caller in an async task moves the
/// call to a blocking thread.
///
/// # Errors
///
/// [`DownloadError`]; on any of them nothing is left under `datasets` that
/// this call created.
pub fn download(
    entry: &ManifestEntry,
    datasets: &Path,
    fetcher: &mut dyn Fetcher,
    mut controls: Controls<'_>,
) -> Result<Verified, DownloadError> {
    let name = entry.name.as_str();
    let destination = datasets.join(entry.dir());
    let occupied = || DownloadError::Occupied {
        entry: name.to_owned(),
        path: destination.clone(),
    };
    if destination.exists() {
        return Err(occupied());
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
    let staging = Staging::create(datasets, entry.dir(), "download").map_err(io_error(datasets))?;

    let started = Instant::now();
    let total = entry.size_bytes();
    let mut before = 0u64;
    for file in &entry.files {
        fetch_file(
            entry,
            file,
            staging.path(),
            fetcher,
            &mut controls,
            started,
            total,
            before,
        )?;
        before += file.size_bytes;
    }

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
    if benchmark.carries() != entry.carries {
        return Err(DownloadError::GroundTruth {
            entry: name.to_owned(),
            declared: entry.carries,
            found: benchmark.carries(),
        });
    }
    match staging.publish(&destination) {
        Ok(()) => {}
        Err(_) if destination.exists() => return Err(occupied()),
        Err(error) => return Err(io_error(&destination)(error)),
    }
    Ok(Verified {
        dataset_version: found,
        carries: benchmark.carries(),
    })
}

/// Fetches one file into `staging`, and refuses it unless it is exactly the
/// manifest's size and digest.
#[allow(clippy::too_many_arguments)] // one call site, every argument distinct
fn fetch_file(
    entry: &ManifestEntry,
    file: &ManifestFile,
    staging: &Path,
    fetcher: &mut dyn Fetcher,
    controls: &mut Controls<'_>,
    started: Instant,
    total: u64,
    before: u64,
) -> Result<(), DownloadError> {
    let name = entry.name.as_str();
    let cancelled = || DownloadError::Cancelled {
        entry: name.to_owned(),
    };
    let late = |deadline| DownloadError::DeadlineExceeded {
        entry: name.to_owned(),
        deadline,
    };
    if controls.cancelled.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    if started.elapsed() > controls.deadline {
        return Err(late(controls.deadline));
    }
    let relative = inside(&file.path).ok_or_else(|| DownloadError::InvalidPath {
        entry: name.to_owned(),
        path: file.path.clone(),
    })?;
    let target = staging.join(relative);
    let io_error = |path: &Path, source| DownloadError::Io {
        entry: name.to_owned(),
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    }
    let out = File::create(&target).map_err(|source| io_error(&target, source))?;
    let deadline = controls.deadline;
    let mut body = Body {
        limit: file.size_bytes,
        out,
        hasher: Sha256::new(),
        received: 0,
        before,
        total,
        progress: &mut *controls.progress,
        cancelled: controls.cancelled,
        started,
        deadline,
        stop: None,
    };
    let outcome = fetcher.fetch(&file.url, &mut body);
    let Body {
        stop,
        received,
        hasher,
        out,
        ..
    } = body;
    let digest = hex(&hasher.finalize());

    match stop {
        Some(Stop::TooLarge { seen }) => {
            return Err(DownloadError::TooLarge {
                entry: name.to_owned(),
                file: file.path.clone(),
                limit: file.size_bytes,
                seen,
            })
        }
        Some(Stop::Cancelled) => return Err(cancelled()),
        Some(Stop::Deadline) => return Err(late(deadline)),
        Some(Stop::Io(source)) => return Err(io_error(&target, source)),
        None => {}
    }
    if let Err(reason) = outcome {
        return Err(DownloadError::Fetch {
            entry: name.to_owned(),
            url: file.url.clone(),
            reason,
        });
    }
    if received < file.size_bytes {
        return Err(DownloadError::Truncated {
            entry: name.to_owned(),
            file: file.path.clone(),
            expected: file.size_bytes,
            received,
        });
    }
    out.sync_all().map_err(|source| io_error(&target, source))?;
    if digest != file.sha256 {
        return Err(DownloadError::FileDigest {
            entry: name.to_owned(),
            file: file.path.clone(),
            expected: file.sha256.clone(),
            found: digest,
        });
    }
    Ok(())
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

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

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

/// A directory whose [`LOCAL_MARKER`] this build cannot read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkerError {
    /// The directory's name.
    pub name: String,
    /// Why, naming the marker.
    pub reason: String,
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
    /// The name is not one this directory accepts.
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
        /// What was loaded.
        path: PathBuf,
        /// The adapter's error.
        #[source]
        source: BenchmarkError,
    },
    /// The copy digests to another value than the source did: the source
    /// changed while it was imported.
    #[error("{} changed while it was imported: it digested to {expected}, its copy to {found}", path.display())]
    Changed {
        /// What was imported.
        path: PathBuf,
        /// The source's `dataset_version`.
        expected: String,
        /// The copy's.
        found: String,
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
/// local entry `name` in `datasets`. `manifest` is the manifest the caller
/// obtains from: its entries' directories are reserved.
///
/// A directory is read as BEIR — as `beir-qa` when it holds `answers.jsonl` —
/// and a file as SQuAD v1.1. The source is loaded, and refused with the
/// adapter's error when it does not load; the files the adapter read are
/// copied into a staging directory, the copy is loaded and must digest as the
/// source did, and it is published as `<datasets>/<name>` beside
/// [`LOCAL_MARKER`], which records its format and `dataset_version`.
///
/// # Errors
///
/// [`ImportError`]; on any of them nothing is registered.
pub fn import(
    datasets: &Path,
    name: &str,
    source: &Path,
    manifest: &[ManifestEntry],
) -> Result<Imported, ImportError> {
    check_name(name)?;
    let destination = datasets.join(name);
    let occupied = || ImportError::Occupied {
        name: name.to_owned(),
        path: destination.clone(),
    };
    if destination.exists() || manifest.iter().any(|entry| entry.dir() == name) {
        return Err(occupied());
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
    let expected = dataset_version(&benchmark);

    fs::create_dir_all(datasets).map_err(io_error(datasets))?;
    let staging = Staging::create(datasets, name, "import").map_err(io_error(datasets))?;
    copy_read_files(format, source, staging.path())?;
    let copy = format
        .load(staging.path())
        .map_err(|error| ImportError::Load {
            path: staging.path().to_path_buf(),
            source: error,
        })?;
    let found = dataset_version(&copy);
    if found != expected {
        return Err(ImportError::Changed {
            path: source.to_path_buf(),
            expected,
            found,
        });
    }
    let marker = Marker {
        format: format.selector().to_owned(),
        dataset_version: found.clone(),
    };
    let marker_path = staging.path().join(LOCAL_MARKER);
    let text = serde_json::to_string_pretty(&marker).expect("a marker always serializes");
    fs::write(&marker_path, text + "\n").map_err(io_error(&marker_path))?;
    match staging.publish(&destination) {
        Ok(()) => {}
        Err(_) if destination.exists() => return Err(occupied()),
        Err(error) => return Err(io_error(&destination)(error)),
    }
    Ok(Imported {
        entry: LocalEntry {
            name: name.to_owned(),
            format,
            dataset_version: found,
        },
        carries: copy.carries(),
    })
}

/// A name is `[A-Za-z0-9_-][A-Za-z0-9._-]*`, at most [`MAX_NAME_LENGTH`]
/// bytes, not ending in `.`, and not a Windows device name (`CON`, `PRN`,
/// `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, in any case, with or without an
/// extension): one directory name on every platform, never a staging
/// directory's.
fn check_name(name: &str) -> Result<(), ImportError> {
    let refuse = |reason| {
        Err(ImportError::InvalidName {
            name: name.to_owned(),
            reason,
        })
    };
    let allowed = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-';
    let Some(&first) = name.as_bytes().first() else {
        return refuse("it is empty");
    };
    if name.len() > MAX_NAME_LENGTH {
        return refuse("it is longer than 64 bytes");
    }
    if !allowed(first) {
        return refuse("it must begin with a letter, a digit, `_` or `-`");
    }
    if !name.bytes().all(|byte| allowed(byte) || byte == b'.') {
        return refuse("it may hold only letters, digits, `.`, `_` and `-`");
    }
    if name.ends_with('.') {
        return refuse("it must not end with `.`, which Windows strips");
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if device {
        return refuse("it is a device name Windows reserves");
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
    let io_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ImportError::Io { path, source }
    };
    let copy = |from: &Path, to: &Path| fs::copy(from, to).map(drop).map_err(io_error(from));
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

/// Every local import under `datasets`, by name: a [`LocalEntry`] for each
/// marker this build reads, a [`MarkerError`] for each it does not — one bad
/// marker is that directory's problem, not the listing's. A directory without
/// [`LOCAL_MARKER`] is not an import, and a `.`-named one is staging.
///
/// # Errors
///
/// The directory itself cannot be read.
pub fn local_entries(datasets: &Path) -> io::Result<Vec<Result<LocalEntry, MarkerError>>> {
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
        entries.push(read_marker(&name, &marker_path));
    }
    entries.sort_by(|a, b| entry_name(a).cmp(entry_name(b)));
    Ok(entries)
}

fn entry_name(entry: &Result<LocalEntry, MarkerError>) -> &str {
    match entry {
        Ok(entry) => &entry.name,
        Err(error) => &error.name,
    }
}

fn read_marker(name: &str, path: &Path) -> Result<LocalEntry, MarkerError> {
    let invalid = |reason: String| MarkerError {
        name: name.to_owned(),
        reason: format!("{}: {reason}", path.display()),
    };
    let text = fs::read_to_string(path).map_err(|error| invalid(error.to_string()))?;
    let marker: Marker = serde_json::from_str(&text).map_err(|error| invalid(error.to_string()))?;
    let format = Format::from_selector(&marker.format)
        .ok_or_else(|| invalid(format!("no format is named {:?}", marker.format)))?;
    Ok(LocalEntry {
        name: name.to_owned(),
        format,
        dataset_version: marker.dataset_version,
    })
}

// ---------------------------------------------------------------------------
// Staging
// ---------------------------------------------------------------------------

/// Distinguishes the staging directories one process creates.
static ATTEMPT: AtomicU64 = AtomicU64::new(0);

/// Removes the staging directories interrupted downloads and imports left in
/// `datasets` — `.<name>.download-…` and `.<name>.import-…` — and returns how
/// many.
///
/// Call it when no download or import is running, at startup: a running one's
/// staging directory has the same shape.
///
/// # Errors
///
/// The directory cannot be read, or one cannot be removed.
pub fn sweep_staging(datasets: &Path) -> io::Result<usize> {
    let listing = match fs::read_dir(datasets) {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut removed = 0;
    for entry in listing {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && (name.contains(".download-") || name.contains(".import-")) {
            fs::remove_dir_all(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// A directory assembled beside its destination and renamed into place, or
/// removed when dropped unpublished — so an error or a panic leaves nothing.
struct Staging {
    path: PathBuf,
    published: bool,
}

impl Staging {
    /// Creates `<datasets>/.<name>.<kind>-<process>-<attempt>`, a name no
    /// other attempt, in this process or another, uses at the same time.
    fn create(datasets: &Path, name: &str, kind: &str) -> io::Result<Self> {
        loop {
            let attempt = ATTEMPT.fetch_add(1, Ordering::Relaxed);
            let path = datasets.join(format!(".{name}.{kind}-{}-{attempt}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Self {
                        path,
                        published: false,
                    })
                }
                // Left by an earlier process that had the same id.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Renames it to `destination`. Fails when `destination` exists and is
    /// not empty — another attempt published first.
    fn publish(mut self, destination: &Path) -> io::Result<()> {
        if destination.exists() {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
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
