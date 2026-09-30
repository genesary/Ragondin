//! The traits the service consumes, and the values they exchange.
//!
//! The router holds each backend as an `Arc<dyn …>` it was handed
//! ([`Backends`]); nothing here is a static or a global (INV-6's reasoning,
//! applied to the backends by ADR-C36 § 1). The binary picks every backend,
//! so nothing in this crate knows whether it runs on a laptop or in a
//! cluster: a cluster deployment is a set of new implementations of these
//! traits, never a change to the router.
//!
//! `RunStore` is not here: it is the experiment plane's, in
//! `ragondin-experiments`, and the router takes it from there. It is
//! synchronous, and the handlers move each call onto a blocking thread.
//!
//! The four traits below are async, with `async_trait` (frozen decision): a
//! file backend reads the disk and a cluster backend talks to the API server.
//! None of them is a `tower::Service` (INV-11): Tower is the router's
//! envelope, in `layers.rs`, and nothing else.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use ragondin_experiments::{RunId, RunStore};

use crate::error::ApiError;
use crate::response::{BenchmarkEntry, Capabilities, ServiceBinding};

/// Every backend the router consumes, constructed by the binary and passed in.
#[derive(Clone)]
pub struct Backends {
    /// The run store: `GET /runs` and `GET /runs/{id}` read it.
    pub runs: Arc<dyn RunStore>,
    /// The workspace's pipeline documents and their layouts.
    pub pipelines: Arc<dyn PipelineSource>,
    /// The benchmarks: present, available, importable.
    pub registry: Arc<dyn Registry>,
    /// The workspace's deployment settings.
    pub settings: Arc<dyn WorkspaceSettings>,
    /// Everything that needs the composition root: capabilities, the
    /// identity probe, a run's identity, execution.
    pub launcher: Arc<dyn Launcher>,
}

/// The workspace's pipeline documents and their layouts.
///
/// Locally, `pipelines/<name>.yaml` beside `<name>.layout.json`, a backend whose
/// home is this crate's `fs` module, not written yet; in a cluster, custom resources with the
/// layout as an annotation. The document is the source of truth and is never
/// rewritten by the API: what `write` receives is what the file holds.
#[async_trait]
pub trait PipelineSource: Send + Sync {
    /// Every pipeline in the workspace, by name, with its current revision.
    async fn list(&self) -> Result<Vec<PipelineEntry>, ApiError>;

    /// One pipeline's document, its layout if it has one, and the revision
    /// both were read at.
    async fn read(&self, name: &str) -> Result<PipelineFile, ApiError>;

    /// Writes a pipeline's document and, when given, its layout, and returns
    /// the new revision. With `expected`, a backend refuses the write when
    /// the stored revision is another one — the editor's guard against
    /// overwriting a file changed under it.
    async fn write(
        &self,
        name: &str,
        document: &str,
        layout: Option<&str>,
        expected: Option<&Revision>,
    ) -> Result<Revision, ApiError>;
}

/// A pipeline, as a listing names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineEntry {
    /// The pipeline's name: its file stem locally.
    pub name: String,
    /// The revision it is at.
    pub revision: Revision,
}

/// A pipeline's document and layout, read together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineFile {
    /// The document, verbatim.
    pub document: String,
    /// The layout, verbatim, if the pipeline has one.
    pub layout: Option<String>,
    /// The revision both were read at.
    pub revision: Revision,
}

/// An opaque token that changes whenever a pipeline's document or layout
/// does — an etag. The editor compares two; nothing else reads one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Revision(String);

impl Revision {
    /// Wraps a backend's token.
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// The token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The benchmarks: those present with their digests verified, those
/// available from the manifest the binary carries, and those imported.
///
/// Locally, a directory and the manifest `ragondin-benchmarks` holds
/// ([`FsRegistry`](crate::fs::FsRegistry)); in a cluster, an object store and
/// the same manifest. The digest is a benchmark's identity everywhere:
/// "ready" means the dataset on disk digests to the manifest's
/// `dataset_version`, the value a run over it records. Every method reads or
/// writes the datasets and may block on them, so a backend moves that work off
/// the async workers. `conformance::assert_registry_conformance`, behind this
/// crate's `conformance` feature, checks a backend against this contract.
#[async_trait]
pub trait Registry: Send + Sync {
    /// Every benchmark the registry knows — those the manifest names, then
    /// those imported — each with its state.
    async fn benchmarks(&self) -> Result<Vec<BenchmarkEntry>, ApiError>;

    /// One benchmark, by its selector, verified against the digest expected
    /// of it now.
    ///
    /// # Errors
    ///
    /// `benchmark_not_found` for a name the registry does not know.
    async fn verify(&self, name: &str) -> Result<BenchmarkEntry, ApiError>;

    /// Fetches a benchmark the manifest names, reporting progress to
    /// `progress`, and returns it once its digests verified — each file's,
    /// then the loaded dataset's. A download that fails leaves nothing on
    /// disk, and one that succeeds is `ready`.
    ///
    /// It runs to its end once started: the download it drives offers no
    /// cancellation, and the queue that schedules it (the design document
    /// § 7) reads its progress from `progress`.
    ///
    /// # Errors
    ///
    /// `benchmark_not_found` for a name the manifest does not hold,
    /// `benchmark_exists` when its directory is already there,
    /// `download_failed` for a fetch that failed or bytes whose digest is not
    /// the manifest's.
    async fn download(
        &self,
        name: &str,
        progress: ProgressSink,
    ) -> Result<BenchmarkEntry, ApiError>;

    /// Imports the corpus at `path`, which carries its own ground truth, as
    /// the local benchmark `name`, and returns it.
    ///
    /// # Errors
    ///
    /// `import_refused` for a name that is not one directory name, a path
    /// that cannot be read, or a corpus its adapter refuses — the adapter's
    /// error in the detail; `benchmark_exists` for a name already taken.
    async fn import(&self, name: &str, path: &Path) -> Result<BenchmarkEntry, ApiError>;
}

/// Where a download stands: bytes received of the snapshot's total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadProgress {
    /// Bytes received so far.
    pub received: u64,
    /// The snapshot's size, as the manifest states it.
    pub total: u64,
}

/// What a download reports its progress to. Called from the thread the
/// download runs on, after every chunk received.
pub type ProgressSink = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

/// The workspace's deployment settings — data about where things run, never
/// hashed into a run (ADR-C32).
///
/// Locally, `workspace.toml`, a backend whose home is this crate's `fs` module, not written yet; in a
/// cluster, the deployment's bindings, read-only.
#[async_trait]
pub trait WorkspaceSettings: Send + Sync {
    /// The current settings.
    async fn read(&self) -> Result<Settings, ApiError>;

    /// Replaces the settings.
    async fn write(&self, settings: Settings) -> Result<(), ApiError>;
}

/// The workspace's settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The directory benchmarks are read from.
    pub datasets: PathBuf,
    /// The `Remote` bindings: family and name to address.
    pub services: Vec<ServiceBinding>,
}

/// Everything the UI can cause that needs the composition root: the build's
/// capabilities, a `Remote` service's identity, a run's identity at
/// submission, and its execution.
///
/// Implemented by the binary, which alone knows the engine and the
/// components; this trait is the only path from the UI to the data plane
/// (INV-12). A run's identity is announced by [`identity`](Self::identity) at
/// submission and decided by [`execute`](Self::execute) from what actually
/// ran (ADR-C36 § 1).
#[async_trait]
pub trait Launcher: Send + Sync {
    /// Per family, the local implementation names this build registers, and
    /// whether it carries `remote`.
    fn capabilities(&self) -> Capabilities;

    /// Reads the identity of the `Remote` service bound as `family`/`name`
    /// at `uri` — the same read the composition root makes before a run.
    async fn probe(&self, family: &str, name: &str, uri: &str)
        -> Result<ServiceIdentity, ApiError>;

    /// The run id a submission announces, computed from constructed
    /// components and the services' identities, so that an existing run is
    /// refused and an unreachable service fails at submission.
    async fn identity(&self, submission: &Submission) -> Result<RunId, ApiError>;

    /// Runs a job to its end, and returns its terminal state: `Done` with the
    /// id the harness computed from what ran, `Failed`, or `Cancelled`.
    ///
    /// **Provisional shape.** It carries no progress observer and no
    /// cancellation token, which the queue needs to report `Running` and to
    /// cancel between queries. The job model and its queue (#349) settle the
    /// signature, and the binary's implementation over the composition root
    /// (#353) fills it. Nothing calls it yet.
    async fn execute(&self, job: Job) -> JobState;
}

/// What a `Remote` service reported as the identity of the model it serves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceIdentity {
    /// The identity string. Never empty.
    pub identity: String,
}

/// A run asked for: the pipeline, the benchmark, the bindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    /// The pipeline's name in the workspace.
    pub pipeline_name: String,
    /// The pipeline document, snapshotted at submission.
    pub pipeline: String,
    /// The benchmark to evaluate it on.
    pub benchmark: String,
    /// The `Remote` bindings to run it with.
    pub bindings: Vec<ServiceBinding>,
    /// The node to stop after, for a prefix run.
    pub up_to: Option<String>,
}

/// A submission accepted into the queue, with the run id it announced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    /// The job's id.
    pub id: String,
    /// The run id announced at submission.
    pub run_id: RunId,
    /// What was submitted.
    pub submission: Submission,
    /// When it was accepted.
    pub created_at: SystemTime,
}

/// Where a job is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobState {
    /// Waiting for the worker.
    Queued,
    /// Running: `done` of `total` queries executed.
    Running {
        /// Queries executed.
        done: u64,
        /// Queries in the benchmark.
        total: u64,
        /// When execution started.
        started_at: SystemTime,
    },
    /// Finished, and stored under `run_id`.
    Done {
        /// The id the store received: the one computed from what ran.
        run_id: RunId,
    },
    /// Failed.
    Failed {
        /// What failed.
        error: String,
        /// The node that failed, when one did.
        at_node: Option<String>,
    },
    /// Cancelled before it finished.
    Cancelled,
}
