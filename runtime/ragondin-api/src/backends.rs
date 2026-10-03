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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use ragondin_benchmarks::identity::CorpusIndex;
use ragondin_benchmarks::Benchmark;
use ragondin_experiments::{Run, RunId, RunStore, TraceDocument};
use ragondin_types::QueryId;

use crate::error::ApiError;
use ragondin_pipeline::LogicalPipeline;

use crate::response::{BenchmarkEntry, Capabilities, Layout, Pairing, ServiceBinding};

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

/// Whether `name` is a case alias of the stored pipeline `stored`: the same
/// name in another ASCII case, and not the same spelling. A source refuses
/// such a name (`request_invalid`), because on a filesystem that ignores case
/// the two are one file (`ARCHITECTURE.md` § The pipelines). The one place
/// the rule is written: the file backend refuses by it, and
/// `lineage::Index::held` reads a recorded name by it.
pub(crate) fn case_alias(stored: &str, name: &str) -> bool {
    stored != name && stored.eq_ignore_ascii_case(name)
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
    /// `request_invalid` for a name that is not one file name, or that
    /// differs from a stored one only in case; `pipeline_invalid` for a
    /// document that does not validate; `precondition_failed` when the stored
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

    /// The manual pairing between the pipelines `pipeline` and `other`, in
    /// whichever direction it was kept, oriented from `pipeline` — each
    /// pair's `node` in `pipeline`, its `other` in `other` — or `None` when
    /// they have none. UI metadata like a layout, never in a hash (INV-8).
    ///
    /// # Errors
    ///
    /// `pipeline_not_found` when either pipeline is not there;
    /// `backend_failed` for a pairing this build cannot read, or one that
    /// names other pipelines than the two it is kept for.
    async fn read_pairing(&self, pipeline: &str, other: &str) -> Result<Option<Pairing>, ApiError>;

    /// Keeps `pairing` for its two pipelines, replacing any in either
    /// direction.
    ///
    /// # Errors
    ///
    /// `pipeline_not_found` when either pipeline is not there.
    async fn write_pairing(&self, pairing: &Pairing) -> Result<(), ApiError>;

    /// Removes the pairing between `pipeline` and `other`, in either
    /// direction; removing none is not an error.
    ///
    /// # Errors
    ///
    /// `pipeline_not_found` when either pipeline is not there.
    async fn delete_pairing(&self, pipeline: &str, other: &str) -> Result<(), ApiError>;
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
    /// A document is stored, at any revision — `If-Match: *` (RFC 9110
    /// § 13.1.1).
    Exists,
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

    /// Every benchmark the registry knows, with the `dataset_version` it is
    /// pinned to — the manifest's entries, then the imports — **loading
    /// nothing**: naming the benchmarks a run's digest is pinned to is not
    /// verifying them. The pinning is [`dataset`](Self::dataset)'s: a
    /// manifest entry is pinned to its `dataset_version` whether or not it
    /// is on disk, an import to the one it recorded, and an import whose
    /// record cannot be read is pinned to nothing and is left out.
    ///
    /// # Errors
    ///
    /// `backend_failed` when the datasets directory cannot be read.
    async fn pinned(&self) -> Result<Vec<PinnedBenchmark>, ApiError>;
}

/// A benchmark and the digest it is pinned to ([`Registry::pinned`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedBenchmark {
    /// The benchmark's selector, `<format>/<dir>`.
    pub name: String,
    /// The `dataset_version` it is pinned to.
    pub dataset_version: String,
}

/// What the registry holds for the `dataset_version` a run recorded.
#[derive(Clone, Debug)]
pub enum RunDataset {
    /// The benchmark pinned to the digest is on disk, loads, and digests to
    /// it: the run's own dataset, loaded.
    Verified {
        /// The benchmark's selector, `<format>/<dir>`.
        name: String,
        /// The dataset, loaded whole, and the chunk set derived from it.
        /// Shared: a backend may hand the same one to every request while
        /// the disk still holds it (`fs::FsRegistry` does).
        dataset: Arc<LoadedDataset>,
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

/// A dataset that verified, loaded whole, and the chunk set derived from it
/// — built on first use, then kept, so a dataset held between requests is
/// chunked once however many replays read it.
///
/// The chunk set is `CorpusIndex::build` over the corpus, the one derivation
/// the writer and the reader share (ADR-C36 § 4); nothing here decides
/// whether it is the run's — a reader compares its `version` with the run's
/// `index_version` before it shows any text.
pub struct LoadedDataset {
    benchmark: Benchmark,
    index: OnceLock<CorpusIndex>,
}

impl LoadedDataset {
    /// A loaded dataset, its chunk set not yet derived.
    pub fn new(benchmark: Benchmark) -> Self {
        Self {
            benchmark,
            index: OnceLock::new(),
        }
    }

    /// The dataset.
    pub fn benchmark(&self) -> &Benchmark {
        &self.benchmark
    }

    /// The chunk set derived from the corpus, derived on the first call. A
    /// corpus is large, so call it on a blocking thread; a second caller
    /// meanwhile waits for the first derivation rather than repeating it.
    pub fn index(&self) -> &CorpusIndex {
        self.index
            .get_or_init(|| CorpusIndex::build(self.benchmark.corpus()))
    }
}

impl std::fmt::Debug for LoadedDataset {
    // A corpus can be gigabytes: the debug form names its size, not its text.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedDataset")
            .field("documents", &self.benchmark.corpus().len())
            .field("queries", &self.benchmark.queries().len())
            .field("index_built", &self.index.get().is_some())
            .finish()
    }
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

    /// Binds `binding.family`/`binding.name` to `binding.uri`: the address
    /// replaced when the name is bound, the binding added after the others
    /// when it is not. Every other binding, and the datasets directory, are
    /// left as they are. Returns the settings it leaves.
    ///
    /// # Errors
    ///
    /// [`ApiError::BindingRefused`] when the backend cannot store the binding
    /// so that it reads back as the same family and name.
    async fn bind(&self, binding: ServiceBinding) -> Result<Settings, ApiError>;

    /// Removes the binding of `family`/`name`, leaving every other binding
    /// and the datasets directory as they are. Returns the settings it
    /// leaves, or `None` when the name was not bound — and then nothing
    /// changed.
    async fn unbind(&self, family: &str, name: &str) -> Result<Option<Settings>, ApiError>;

    /// Sets the datasets directory — a relative one is read against the
    /// workspace — or, with `None`, clears it so that the backend's default
    /// applies. The bindings are left as they are. Returns the settings it
    /// leaves.
    async fn set_datasets(&self, datasets: Option<PathBuf>) -> Result<Settings, ApiError>;
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
    /// Per family, the local implementation names this build registers,
    /// those it does not and why, and the family's ports
    /// ([`family_ports`](crate::family_ports)); and whether it carries
    /// `remote`.
    fn capabilities(&self) -> Capabilities;

    /// Whether the composition root would accept `family`/`name` bound to
    /// `uri` — the refusals `ragondin bench --remote` applies to one
    /// argument, in its words.
    ///
    /// # Errors
    ///
    /// `binding_refused`, with the composition root's refusal.
    fn check_binding(&self, family: &str, name: &str, uri: &str) -> Result<(), ApiError>;

    /// Whether the composition root would accept `pipeline`'s keys, with the
    /// workspace's `bindings` deciding which names are bound — the key
    /// refusals `ragondin bench` makes before anything is loaded (ADR-C32
    /// § 1): a key no component of the node's nature reads is refused rather
    /// than hashed as inert. A pipeline is checked here before it is stored;
    /// `POST /pipelines/validate` does not call it, as `ragondin validate`
    /// applies none of these checks (ADR-C32 § 2).
    ///
    /// # Errors
    ///
    /// `pipeline_invalid`, in the composition root's words, naming the node
    /// when one is at fault; `binding_refused` for a binding in `bindings`
    /// the composition root refuses.
    fn check_document(
        &self,
        pipeline: &LogicalPipeline,
        bindings: &[ServiceBinding],
    ) -> Result<(), ApiError>;

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
    ///
    /// # Errors
    ///
    /// `ImplNotInBuild`, `ServiceUnreachable` or `PipelineInvalid` for a
    /// submission the composition root cannot run; any other variant is
    /// answered as `backend_failed`.
    async fn identity(&self, submission: &Submission) -> Result<RunId, LauncherError>;

    /// Runs a submission to its end, and returns the run as the harness
    /// assembled it — its id the one computed from what ran, which the queue
    /// compares with the announced one.
    ///
    /// Every query executed is reported to `observer` as it completes, with
    /// its trace (INV-10: a value, never a log line); `cancel` is checked
    /// between queries, and once it is set the call ends with
    /// [`LauncherError::Cancelled`]. Nothing is written to the run store
    /// here: the queue files the returned run, as one block.
    ///
    /// # Errors
    ///
    /// `Execution` naming the node that failed, when one did; `Cancelled`
    /// once `cancel` was honoured; the preparation's refusals as
    /// [`identity`](Self::identity) words them.
    async fn execute(
        &self,
        submission: &Submission,
        observer: Arc<dyn RunObserver>,
        cancel: Cancellation,
    ) -> Result<Run, LauncherError>;
}

/// Why a [`Launcher`] could not announce or execute a run.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LauncherError {
    /// An `impl:` name this build registers no local implementation for, or
    /// a component it cannot construct without `feature`.
    #[error("{}", crate::error::impl_not_in_build(family, implementation, feature.as_deref()))]
    ImplNotInBuild {
        /// The family the node is in.
        family: String,
        /// The `impl:` name.
        implementation: String,
        /// The build feature that would carry it, when one is known.
        feature: Option<String>,
    },
    /// A `Remote` service that did not answer.
    #[error("the service at {uri} did not answer: {reason}")]
    ServiceUnreachable {
        /// The address it was reached at.
        uri: String,
        /// The network error, or what the identity read found.
        reason: String,
    },
    /// A pipeline the composition root refuses, in its words.
    #[error("the pipeline is refused: {detail}")]
    PipelineInvalid {
        /// The refusal.
        detail: String,
        /// The node at fault, when one is.
        node: Option<String>,
    },
    /// Execution failed.
    #[error("{error}")]
    Execution {
        /// What failed, in the words of whatever failed.
        error: String,
        /// The node that failed, when one did.
        at_node: Option<String>,
    },
    /// The cancellation signal was honoured between two queries.
    #[error("the run was cancelled")]
    Cancelled,
}

/// What [`Launcher::execute`] reports each query to, as it completes.
///
/// Called from whatever thread executes the run — the binary's dedicated
/// one — so it must not block: the queue's implementation hands the value
/// to its own task and returns.
pub trait RunObserver: Send + Sync {
    /// One more query executed.
    fn query_done(&self, progress: QueryProgress);
}

/// One query executed, as [`RunObserver::query_done`] receives it.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryProgress {
    /// How many queries have executed, this one included: from 1.
    pub position: u64,
    /// How many the run executes.
    pub total: u64,
    /// The query.
    pub query: QueryId,
    /// The time since execution began, as the executor read it. Not a
    /// latency: the queue reads a query's latency from its trace.
    pub elapsed: Duration,
    /// The query's trace, as the run will file it.
    pub trace: TraceDocument,
}

/// The signal that cancels a job: set by the queue, checked by whoever
/// executes. Clones share one flag.
#[derive(Clone, Debug, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    /// A signal not yet set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the signal; it stays set.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether the signal is set.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// The flag itself, as [`Registry::download`] takes it.
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.0)
    }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ragondin_benchmarks::identity::CorpusIndex;
    use ragondin_benchmarks::{Benchmark, Qrels};
    use ragondin_types::{DocId, Document};

    use super::LoadedDataset;

    fn benchmark() -> Benchmark {
        let document = |id: &str, text: &str| Document {
            id: DocId::new(id),
            text: text.to_owned(),
            metadata: BTreeMap::new(),
        };
        Benchmark::new(
            vec![
                document("d-1", "the cat sat"),
                document("d-2", "on the mat"),
            ],
            Vec::new(),
            Qrels::new(),
        )
    }

    #[test]
    fn the_chunk_set_is_the_one_derivation_and_is_derived_once() {
        let loaded = LoadedDataset::new(benchmark());

        let first = loaded.index();
        let second = loaded.index();

        assert_eq!(first, &CorpusIndex::build(benchmark().corpus()));
        assert!(std::ptr::eq(first, second), "built once, then kept");
        assert_eq!(loaded.benchmark(), &benchmark());
    }
}
