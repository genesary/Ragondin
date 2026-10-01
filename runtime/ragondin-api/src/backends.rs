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
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use ragondin_benchmarks::Benchmark;
use ragondin_experiments::{RunId, RunStore};

use crate::error::ApiError;
use crate::response::{BenchmarkEntry, Capabilities, Layout, ServiceBinding};

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
/// Locally, `pipelines/<name>.yaml` beside `<name>.layout.json`
/// ([`FsPipelines`](crate::fs::FsPipelines)); in a cluster, custom resources
/// with the layout as an annotation. The document is the source of truth and
/// is never rewritten by the API: what `write` receives is what is stored,
/// byte for byte, and what `read` returns is what is stored. The layout is
/// beside it and never part of it, so writing one changes neither the
/// document's revision nor its hash.
#[async_trait]
pub trait PipelineSource: Send + Sync {
    /// Every pipeline in the workspace, by name.
    async fn list(&self) -> Result<Vec<PipelineFile>, ApiError>;

    /// One pipeline's document, and the revision it was read at.
    ///
    /// # Errors
    ///
    /// `pipeline_not_found` for a name that names no pipeline.
    async fn read(&self, name: &str) -> Result<PipelineFile, ApiError>;

    /// Stores `document` as the pipeline `name`, when it validates and
    /// `precondition` holds, and returns what is now stored.
    ///
    /// # Errors
    ///
    /// `request_invalid` for a name that is not one file name;
    /// `pipeline_invalid` for a document that does not validate, or that
    /// carries a service's address; `precondition_failed` when the stored
    /// revision is not the one `precondition` expects — the editor's guard
    /// against overwriting a file changed under it. Nothing is written on
    /// any of them.
    async fn write(
        &self,
        name: &str,
        document: &str,
        precondition: &Precondition,
    ) -> Result<PipelineFile, ApiError>;

    /// The layout beside the pipeline `name`, or `None` when it has none.
    ///
    /// # Errors
    ///
    /// `pipeline_not_found` when the pipeline itself is not there.
    async fn read_layout(&self, name: &str) -> Result<Option<Layout>, ApiError>;

    /// Stores `layout` beside the pipeline `name`, replacing any.
    ///
    /// # Errors
    ///
    /// `pipeline_not_found` when the pipeline itself is not there.
    async fn write_layout(&self, name: &str, layout: &Layout) -> Result<(), ApiError>;
}

/// A pipeline's document, as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineFile {
    /// The pipeline's name: its file stem locally.
    pub name: String,
    /// The document, verbatim.
    pub document: String,
    /// The revision it was read at.
    pub revision: Revision,
    /// When it was last modified.
    pub modified: SystemTime,
}

/// What a write expects of what is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Precondition {
    /// The stored document is at this revision — `If-Match`.
    Matches(Revision),
    /// Nothing is stored under the name — `If-None-Match: *`, a creation.
    Absent,
    /// The request stated neither, which a write refuses: an editor that
    /// does not say what it read cannot be kept from overwriting a change.
    Unstated,
}

/// An opaque token that changes whenever a pipeline's document does — an
/// etag. The editor compares two; nothing else reads one.
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

    /// Fetches a benchmark the manifest names and returns it once its digests
    /// verified — each file's size and SHA-256, then the loaded dataset's
    /// `dataset_version`. A download that fails, is cancelled or runs out of
    /// time leaves nothing on disk; one that succeeds is `ready`.
    ///
    /// The queue that schedules it (the design document § 7) reads its
    /// progress from `progress`, called after every chunk, and cancels it by
    /// setting `cancel` — the harness's cancellation shape. Cancellation is
    /// observed when a chunk arrives or a file starts, so against a stalled
    /// server it takes effect only when the transport gives up: the 60 s read
    /// timeout, or up to 30 s while connecting.
    ///
    /// # Errors
    ///
    /// `benchmark_not_found` for a name the manifest does not hold,
    /// `benchmark_exists` when its directory is already there — before the
    /// download, or put there by another that finished first;
    /// `download_failed` for a fetch that failed, a file of the wrong size or
    /// digest, a snapshot of the wrong `dataset_version`, or a deadline
    /// passed; `download_cancelled` once `cancel` is set; `backend_failed`
    /// for a manifest defect or a disk that cannot be written.
    async fn download(
        &self,
        name: &str,
        progress: ProgressSink,
        cancel: Arc<AtomicBool>,
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

    /// The dataset a run was evaluated on, located by the `dataset_version`
    /// the run recorded, and loaded when the disk still holds exactly it.
    ///
    /// A run names no benchmark, only the digest of the one it was evaluated
    /// on; so a backend locates it as the benchmark *pinned* to that digest —
    /// a manifest entry whose `dataset_version` it is, or an import that
    /// recorded it — and then reads what the disk holds under that name.
    /// Nothing is ever resolved by closeness: a digest no benchmark is pinned
    /// to is [`RunDataset::Unknown`], whatever is on disk (ADR-C36 § 4).
    ///
    /// # Errors
    ///
    /// `backend_failed` when the datasets directory cannot be read. A dataset
    /// that is absent, differs or does not load is an answer, not an error.
    async fn dataset(&self, dataset_version: &str) -> Result<RunDataset, ApiError>;
}

/// What the registry holds for the `dataset_version` a run recorded.
#[derive(Clone, Debug)]
pub enum RunDataset {
    /// The benchmark pinned to the digest is on disk, loads, and digests to
    /// it: the run's own dataset, loaded.
    Verified {
        /// The benchmark's selector, `<format>/<dir>`.
        name: String,
        /// The dataset, loaded whole.
        benchmark: Arc<Benchmark>,
    },
    /// The benchmark pinned to the digest is not on disk.
    Absent {
        /// The benchmark's selector.
        name: String,
    },
    /// The benchmark pinned to the digest is on disk and digests to another
    /// value.
    Differs {
        /// The benchmark's selector.
        name: String,
        /// What the dataset on disk digests to.
        found: String,
    },
    /// The benchmark pinned to the digest is on disk and does not load.
    Unreadable {
        /// The benchmark's selector.
        name: String,
        /// The adapter's error, with its causes.
        error: String,
    },
    /// No benchmark the registry knows is pinned to the digest.
    Unknown,
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
/// Locally, `workspace.toml` ([`FsSettings`](crate::fs::FsSettings)); in a
/// cluster, the deployment's bindings, read-only. A backend stores what it is
/// given: whether a binding is one the composition root would accept is
/// [`Launcher::check_binding`]'s to say, before it is written.
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

    /// Whether the composition root would accept `family`/`name` bound to
    /// `uri` — the refusals `ragondin bench --remote` applies to one
    /// argument, in its words.
    ///
    /// # Errors
    ///
    /// `binding_refused`, with the composition root's refusal.
    fn check_binding(&self, family: &str, name: &str, uri: &str) -> Result<(), ApiError>;

    /// Reads the identity of the `Remote` service bound as `family`/`name`
    /// at `uri` — the same read the composition root makes before a run, for
    /// `served_model` where the family reports an identity per served model
    /// (ADR-C32 § 4).
    ///
    /// # Errors
    ///
    /// `binding_refused` for a binding [`check_binding`](Self::check_binding)
    /// refuses; `request_invalid` for a family that reports no identity, or
    /// one that needs a served model and was given none;
    /// `impl_not_in_build` in a build that cannot construct a `Remote`
    /// component; `service_unreachable` when the service did not answer.
    async fn probe(
        &self,
        family: &str,
        name: &str,
        uri: &str,
        served_model: Option<&str>,
    ) -> Result<ServiceIdentity, ApiError>;

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
