//! The [`Registry`] over a benchmark manifest and a datasets directory.
//!
//! Every verdict, download rule and import is `ragondin-benchmarks`'
//! (`ragondin_benchmarks::datasets`): this backend finds the entry a name
//! means, moves the work onto a blocking thread — each call loads a dataset
//! whole or writes one — supplies the one thing that crate leaves out, the
//! HTTP transport, and converts what comes back into the API's types
//! (`convert.rs`). It computes no digest of its own.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ragondin_benchmarks::datasets::{
    self, local_entries, Body, Controls, DiskState, Fetcher, Progress,
};
use ragondin_benchmarks::identity::dataset_version;
use ragondin_benchmarks::manifest::{Format, ManifestEntry};
use tokio::runtime::Handle;

use super::memo::{self, DatasetMemo};
use crate::backends::{
    DownloadProgress, LoadedDataset, PinnedBenchmark, ProgressSink, Registry, RunDataset,
};
use crate::convert;
use crate::error::ApiError;
use crate::response::BenchmarkEntry;

/// How long a fetch waits for a connection, and then for each read. A server
/// that stops sending fails the read; one that trickles fails the download's
/// overall deadline (`Controls::deadline_for`).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// The benchmarks a manifest names and a datasets directory holds.
///
/// A manifest entry `<format>/<dir>` lives at `<datasets>/<dir>`; a local
/// import at `<datasets>/<name>`, marked by
/// [`LOCAL_MARKER`](ragondin_benchmarks::datasets::LOCAL_MARKER). The binary
/// passes `ragondin_benchmarks::manifest::manifest()`; a test passes entries a
/// local server can serve. The manifest given is the one whose entries'
/// directories an import may not take.
///
/// It keeps the datasets [`Registry::dataset`] verified loaded between
/// calls — a bounded number, the least recently used dropped first — and
/// loads and digests one again whenever a file under its directory changes;
/// `memo.rs` says how many, and how a change is seen. Clones share what is
/// kept.
#[derive(Clone, Debug)]
pub struct FsRegistry {
    datasets: PathBuf,
    manifest: Arc<Vec<ManifestEntry>>,
    loaded: Arc<DatasetMemo>,
}

impl FsRegistry {
    /// A registry over `datasets`, obtaining what `manifest` names.
    pub fn new(datasets: PathBuf, manifest: Vec<ManifestEntry>) -> Self {
        Self {
            datasets,
            manifest: Arc::new(manifest),
            loaded: Arc::new(DatasetMemo::new(memo::CAPACITY, memo::RACY_MARGIN)),
        }
    }

    /// The directory it reads and writes.
    pub fn datasets(&self) -> &Path {
        &self.datasets
    }

    /// Removes the staging directories an interrupted download or import
    /// left, and returns how many. Call it at startup, before any download
    /// or import runs: a running one's staging directory has the same shape.
    ///
    /// # Errors
    ///
    /// `backend_failed` when the directory cannot be read or cleaned.
    pub fn sweep_staging(&self) -> Result<usize, ApiError> {
        datasets::sweep_staging(&self.datasets).map_err(|error| ApiError::BackendFailed {
            detail: format!("sweeping the datasets directory: {error}"),
        })
    }

    fn entry(&self, name: &str) -> Option<ManifestEntry> {
        self.manifest
            .iter()
            .find(|entry| entry.name == name)
            .cloned()
    }

    /// Every benchmark and the digest it is pinned to — the manifest's
    /// entries in manifest order, then the imports — read from the manifest
    /// and the import records alone: nothing is loaded. The one definition
    /// of the pinning both `dataset` and `pinned` answer by.
    fn pins(&self) -> Result<Vec<Pin>, ApiError> {
        let mut pins: Vec<Pin> = self
            .manifest
            .iter()
            .map(|entry| Pin {
                name: entry.name.clone(),
                dir: self.datasets.join(entry.dir()),
                format: entry.format,
                dataset_version: entry.dataset_version.clone(),
            })
            .collect();
        pins.extend(
            local_entries(&self.datasets)
                .map_err(backend_failed)?
                .into_iter()
                // An import whose record cannot be read names no digest, so
                // it is pinned to none; the listing reports it as unreadable.
                .flatten()
                .map(|local| Pin {
                    name: local.selector(),
                    dir: self.datasets.join(&local.name),
                    format: local.format,
                    dataset_version: local.dataset_version,
                }),
        );
        Ok(pins)
    }

    /// Every import, verified, and every one whose record cannot be read.
    fn locals(&self) -> Result<Vec<(Option<String>, BenchmarkEntry)>, ApiError> {
        let mut listed = Vec::new();
        for local in local_entries(&self.datasets).map_err(backend_failed)? {
            match local {
                Ok(local) => {
                    let dir = self.datasets.join(&local.name);
                    let state = datasets::verify(&dir, local.format, &local.dataset_version);
                    if let Some(entry) = convert::local_benchmark(&local, state) {
                        listed.push((Some(local.selector()), entry));
                    }
                }
                Err(error) => listed.push((None, convert::unreadable_import(&error))),
            }
        }
        Ok(listed)
    }
}

#[async_trait]
impl Registry for FsRegistry {
    async fn benchmarks(&self) -> Result<Vec<BenchmarkEntry>, ApiError> {
        let registry = self.clone();
        blocking(move || {
            let mut listed: Vec<BenchmarkEntry> = registry
                .manifest
                .iter()
                .map(|entry| manifest_state(&registry.datasets, entry))
                .collect();
            listed.extend(registry.locals()?.into_iter().map(|(_, entry)| entry));
            Ok(listed)
        })
        .await
    }

    async fn verify(&self, name: &str) -> Result<BenchmarkEntry, ApiError> {
        let registry = self.clone();
        let name = name.to_owned();
        blocking(move || {
            if let Some(entry) = registry.entry(&name) {
                return Ok(manifest_state(&registry.datasets, &entry));
            }
            registry
                .locals()?
                .into_iter()
                .find(|(selector, _)| selector.as_deref() == Some(name.as_str()))
                .map(|(_, entry)| entry)
                .ok_or(ApiError::BenchmarkNotFound { name })
        })
        .await
    }

    async fn download(
        &self,
        name: &str,
        progress: ProgressSink,
        cancel: Arc<AtomicBool>,
    ) -> Result<BenchmarkEntry, ApiError> {
        let entry = self
            .entry(name)
            .ok_or_else(|| ApiError::BenchmarkNotFound {
                name: name.to_owned(),
            })?;
        let datasets = self.datasets.clone();
        let mut fetcher = Http::new(Handle::current())?;
        blocking(move || {
            let mut report = |step: Progress| {
                progress(DownloadProgress {
                    received: step.received,
                    total: step.total,
                });
            };
            let controls = Controls {
                progress: &mut report,
                cancelled: &cancel,
                deadline: Controls::deadline_for(&entry),
            };
            match datasets::download(&entry, &datasets, &mut fetcher, controls) {
                Ok(verified) => Ok(convert::manifest_benchmark(
                    &entry,
                    DiskState::Verified(verified),
                )),
                Err(error) => Err(convert::download_error(&entry.name, error)),
            }
        })
        .await
    }

    async fn import(&self, name: &str, path: &Path) -> Result<BenchmarkEntry, ApiError> {
        let registry = self.clone();
        let name = name.to_owned();
        let path = path.to_path_buf();
        blocking(move || {
            match datasets::import(&registry.datasets, &name, &path, &registry.manifest) {
                Ok(imported) => Ok(convert::imported(&imported)),
                Err(error) => Err(convert::import_error(&name, error)),
            }
        })
        .await
    }

    async fn dataset(&self, version: &str) -> Result<RunDataset, ApiError> {
        let registry = self.clone();
        let version = version.to_owned();
        blocking(move || {
            // Every benchmark pinned to the digest — the manifest's entries in
            // manifest order, then the imports — each the run's dataset
            // exactly. The first one on disk that verifies is the answer;
            // with none, the first one's state is.
            let pinned = registry
                .pins()?
                .into_iter()
                .filter(|pin| pin.dataset_version == version);
            let mut first = None;
            for Pin {
                name, dir, format, ..
            } in pinned
            {
                let found = registry.loaded.dataset(&version, &dir, name, |name| {
                    load_pinned(name, &dir, format, &version)
                });
                match found {
                    verified @ RunDataset::Verified { .. } => return Ok(verified),
                    other => {
                        first.get_or_insert(other);
                    }
                }
            }
            Ok(first.unwrap_or(RunDataset::Unknown))
        })
        .await
    }

    async fn pinned(&self) -> Result<Vec<PinnedBenchmark>, ApiError> {
        let registry = self.clone();
        blocking(move || {
            Ok(registry
                .pins()?
                .into_iter()
                .map(|pin| PinnedBenchmark {
                    name: pin.name,
                    dataset_version: pin.dataset_version,
                })
                .collect())
        })
        .await
    }
}

/// A benchmark, where it lives, how it is read, and the digest it is pinned
/// to.
struct Pin {
    name: String,
    dir: PathBuf,
    format: Format,
    dataset_version: String,
}

/// The dataset at `dir`, read as `format` and compared with the digest it is
/// pinned to. Loaded here rather than through `datasets::verify`, which
/// discards what it loaded: the caller needs the dataset itself. The digest
/// is `ragondin_benchmarks::identity`'s, the one definition.
fn load_pinned(name: String, dir: &Path, format: Format, expected: &str) -> RunDataset {
    if !dir.exists() {
        return RunDataset::Absent { name };
    }
    match format.load(dir) {
        Err(error) => RunDataset::Unreadable {
            name,
            error: convert::causes(&error),
        },
        Ok(benchmark) => {
            let found = dataset_version(&benchmark);
            if found == expected {
                RunDataset::Verified {
                    name,
                    dataset: Arc::new(LoadedDataset::new(benchmark)),
                }
            } else {
                RunDataset::Differs { name, found }
            }
        }
    }
}

/// The HTTP transport `datasets::download` is handed: `reqwest`, driven on the
/// server's own runtime from the blocking thread the download runs on.
///
/// Redirects are followed, as `reqwest` does by default (up to ten): a pinned
/// URL on a dataset hub answers with a redirect to its storage, and integrity
/// rests on the manifest's SHA-256, not on the host that served the bytes.
/// TLS verifies against the `webpki-roots` bundle the workspace entry selects,
/// so a network that intercepts TLS with its own authority fails the fetch,
/// and nothing here overrides that.
struct Http {
    client: reqwest::Client,
    runtime: Handle,
}

impl Http {
    fn new(runtime: Handle) -> Result<Self, ApiError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .build()
            .map_err(|error| ApiError::BackendFailed {
                detail: format!("building the HTTP client: {}", convert::causes(&error)),
            })?;
        Ok(Self { client, runtime })
    }
}

impl Fetcher for Http {
    fn fetch(&mut self, url: &str, body: &mut Body<'_>) -> Result<(), String> {
        let client = self.client.clone();
        self.runtime.block_on(async move {
            let http = |error: reqwest::Error| convert::causes(&error);
            let mut response = client
                .get(url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(http)?;
            if let Some(length) = response.content_length() {
                body.announce(length).map_err(|stop| stop.to_string())?;
            }
            while let Some(chunk) = response.chunk().await.map_err(http)? {
                body.write(&chunk).map_err(|stop| stop.to_string())?;
            }
            Ok(())
        })
    }
}

/// A manifest entry, verified against what `datasets` holds for it.
fn manifest_state(datasets: &Path, entry: &ManifestEntry) -> BenchmarkEntry {
    let state = datasets::verify(
        &datasets.join(entry.dir()),
        entry.format,
        &entry.dataset_version,
    );
    convert::manifest_benchmark(entry, state)
}

fn backend_failed(error: std::io::Error) -> ApiError {
    ApiError::BackendFailed {
        detail: format!("reading the datasets directory: {error}"),
    }
}

/// Runs `work` on a blocking thread: every call here reads or writes a whole
/// dataset.
async fn blocking<T, F>(work: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| ApiError::BackendFailed {
            detail: format!("the registry's blocking task failed: {error}"),
        })?
}
