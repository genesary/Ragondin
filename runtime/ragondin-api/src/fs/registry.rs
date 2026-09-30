//! The [`Registry`] over a benchmark manifest and a datasets directory.
//!
//! Every verdict, download and import is `ragondin-benchmarks`'
//! (`ragondin_benchmarks::datasets`): this backend finds the entry a name
//! means, moves the work onto a blocking thread — each call loads a dataset
//! whole or writes one — and converts what comes back into the API's types
//! (`convert.rs`). It computes no digest of its own.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use ragondin_benchmarks::datasets::{self, local_entries, DiskState};
use ragondin_benchmarks::manifest::ManifestEntry;

use crate::backends::{DownloadProgress, ProgressSink, Registry};
use crate::convert;
use crate::error::ApiError;
use crate::response::BenchmarkEntry;

/// The benchmarks a manifest names and a datasets directory holds.
///
/// A manifest entry `<format>/<dir>` lives at `<datasets>/<dir>`; a local
/// import at `<datasets>/<name>`, marked by
/// [`LOCAL_MARKER`](ragondin_benchmarks::datasets::LOCAL_MARKER). The binary
/// passes `ragondin_benchmarks::manifest::manifest()`; a test passes entries a
/// local server can serve.
#[derive(Clone, Debug)]
pub struct FsRegistry {
    datasets: PathBuf,
    manifest: Arc<Vec<ManifestEntry>>,
}

impl FsRegistry {
    /// A registry over `datasets`, obtaining what `manifest` names.
    pub fn new(datasets: PathBuf, manifest: Vec<ManifestEntry>) -> Self {
        Self {
            datasets,
            manifest: Arc::new(manifest),
        }
    }

    /// The directory it reads and writes.
    pub fn datasets(&self) -> &Path {
        &self.datasets
    }

    fn entry(&self, name: &str) -> Option<ManifestEntry> {
        self.manifest
            .iter()
            .find(|entry| entry.name == name)
            .cloned()
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
            for local in local_entries(&registry.datasets).map_err(backend_failed)? {
                let dir = registry.datasets.join(&local.name);
                let state = datasets::verify(&dir, local.format, &local.dataset_version);
                listed.extend(convert::local_benchmark(&local, state));
            }
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
            let not_found = || ApiError::BenchmarkNotFound { name: name.clone() };
            let local = local_entries(&registry.datasets)
                .map_err(backend_failed)?
                .into_iter()
                .find(|local| local.selector() == name)
                .ok_or_else(not_found)?;
            let dir = registry.datasets.join(&local.name);
            let state = datasets::verify(&dir, local.format, &local.dataset_version);
            convert::local_benchmark(&local, state).ok_or_else(not_found)
        })
        .await
    }

    async fn download(
        &self,
        name: &str,
        progress: ProgressSink,
    ) -> Result<BenchmarkEntry, ApiError> {
        let entry = self
            .entry(name)
            .ok_or_else(|| ApiError::BenchmarkNotFound {
                name: name.to_owned(),
            })?;
        let datasets = self.datasets.clone();
        blocking(move || {
            let mut report = |step: datasets::Progress| {
                progress(DownloadProgress {
                    received: step.received,
                    total: step.total,
                });
            };
            match datasets::download(&entry, &datasets, &mut report) {
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
        let datasets = self.datasets.clone();
        let name = name.to_owned();
        let path = path.to_path_buf();
        blocking(move || match datasets::import(&datasets, &name, &path) {
            Ok(imported) => Ok(convert::imported(&imported)),
            Err(error) => Err(convert::import_error(&name, error)),
        })
        .await
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
