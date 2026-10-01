//! Temporary empty backends: a stopgap, so that the three endpoints
//! `ragondin-api` serves answer against a real run store before the file
//! backends exist.
//!
//! Each answers a listing with nothing and anything else with "not available
//! in this build yet", naming the issue that replaces it. **They carry no
//! behaviour and must not grow any**: each is deleted, not extended, by the
//! issue it names. The workspace on disk (#342) replaces all three: the
//! pipelines and the settings with its file backends, and the registry with
//! `ragondin-api`'s `FsRegistry`, which exists but needs what that issue
//! settles — the datasets directory the settings name, and the one call to
//! `FsRegistry::sweep_staging` at startup before any download runs.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use async_trait::async_trait;
use ragondin_api::{
    ApiError, BenchmarkEntry, PipelineEntry, PipelineFile, PipelineSource, ProgressSink, Registry,
    Revision, Settings, WorkspaceSettings,
};

fn not_yet(what: &str, issue: &str) -> ApiError {
    ApiError::BackendFailed {
        detail: format!("{what} is not available in this build yet ({issue})"),
    }
}

/// No pipeline: `pipelines/` is read by the workspace's issue (#342).
pub struct NoPipelines;

#[async_trait]
impl PipelineSource for NoPipelines {
    async fn list(&self) -> Result<Vec<PipelineEntry>, ApiError> {
        Ok(Vec::new())
    }

    async fn read(&self, _name: &str) -> Result<PipelineFile, ApiError> {
        Err(not_yet("reading a pipeline", "#342"))
    }

    async fn write(
        &self,
        _name: &str,
        _document: &str,
        _layout: Option<&str>,
        _expected: Option<&Revision>,
    ) -> Result<Revision, ApiError> {
        Err(not_yet("writing a pipeline", "#342"))
    }
}

/// No benchmark: `FsRegistry` is wired by the workspace's issue (#342).
pub struct NoBenchmarks;

#[async_trait]
impl Registry for NoBenchmarks {
    async fn benchmarks(&self) -> Result<Vec<BenchmarkEntry>, ApiError> {
        Ok(Vec::new())
    }

    async fn verify(&self, _name: &str) -> Result<BenchmarkEntry, ApiError> {
        Err(not_yet("verifying a benchmark", "#342"))
    }

    async fn download(
        &self,
        _name: &str,
        _progress: ProgressSink,
        _cancel: Arc<AtomicBool>,
    ) -> Result<BenchmarkEntry, ApiError> {
        Err(not_yet("downloading a benchmark", "#342"))
    }

    async fn import(&self, _name: &str, _path: &Path) -> Result<BenchmarkEntry, ApiError> {
        Err(not_yet("importing a benchmark", "#342"))
    }
}

/// Settings read from nothing: no datasets directory, no binding.
/// `workspace.toml` is read by the workspace's issue (#342).
pub struct NoSettings;

#[async_trait]
impl WorkspaceSettings for NoSettings {
    async fn read(&self) -> Result<Settings, ApiError> {
        Ok(Settings {
            datasets: PathBuf::new(),
            services: Vec::new(),
        })
    }

    async fn write(&self, _settings: Settings) -> Result<(), ApiError> {
        Err(not_yet("writing the settings", "#342"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_not_yet(error: &ApiError, issue: &str) -> bool {
        matches!(error, ApiError::BackendFailed { detail }
            if detail.contains("not available in this build yet") && detail.contains(issue))
    }

    #[tokio::test]
    async fn the_pipeline_source_lists_nothing_and_reads_and_writes_nothing() {
        assert_eq!(NoPipelines.list().await, Ok(Vec::new()));
        let read = NoPipelines
            .read("hybrid")
            .await
            .expect_err("nothing to read");
        assert!(is_not_yet(&read, "#342"), "{read:?}");
        let write = NoPipelines
            .write("hybrid", "pipeline: {}", None, None)
            .await
            .expect_err("nothing is written");
        assert!(is_not_yet(&write, "#342"), "{write:?}");
    }

    #[tokio::test]
    async fn the_registry_knows_no_benchmark_and_fetches_none() {
        assert_eq!(NoBenchmarks.benchmarks().await, Ok(Vec::new()));
        let verify = NoBenchmarks.verify("beir/scifact").await.expect_err("none");
        assert!(is_not_yet(&verify, "#342"), "{verify:?}");
        let download = NoBenchmarks
            .download(
                "beir/scifact",
                Arc::new(|_| {}),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .expect_err("none");
        assert!(is_not_yet(&download, "#342"), "{download:?}");
        let import = NoBenchmarks
            .import("mine", Path::new("/data/mine"))
            .await
            .expect_err("none");
        assert!(is_not_yet(&import, "#342"), "{import:?}");
    }

    #[tokio::test]
    async fn the_settings_are_empty_and_not_written() {
        assert_eq!(
            NoSettings.read().await,
            Ok(Settings {
                datasets: PathBuf::new(),
                services: Vec::new(),
            })
        );
        let write = NoSettings
            .write(Settings {
                datasets: PathBuf::from("/data"),
                services: Vec::new(),
            })
            .await
            .expect_err("nothing is written");
        assert!(is_not_yet(&write, "#342"), "{write:?}");
    }
}
