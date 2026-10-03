//! In-memory fakes of every trait the router consumes, and the plumbing the
//! handler and layer tests share: a router over the fakes, a request builder
//! that passes the `Host` check, and a body reader.
//!
//! The router is tested against the fakes, the way the design document § 9
//! asks; the file backends are tested on their own, and `datasets` holds what
//! the registry's tests share.

#![allow(dead_code)] // each test binary uses a different part of this module

pub mod calibration;
pub mod datasets;
pub mod runs;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, Response};
use ragondin_api::{
    content_type_for, router, ApiError, Asset, Assets, Backends, BenchmarkEntry, Capabilities,
    FamilyCapabilities, Job, JobState, Launcher, Layout, LoadedDataset, Pairing, PinnedBenchmark,
    PipelineFile, PipelineSource, Precondition, ProgressSink, Registry, Revision, RunDataset,
    Server, ServerConfig, ServiceBinding, ServiceIdentity, Settings, Submission, WorkspaceSettings,
};
use ragondin_benchmarks::identity::dataset_version;
use ragondin_benchmarks::Benchmark;
use ragondin_experiments::{FileSystemRunStore, Run, RunId, RunStore, RunStoreError};
use ragondin_pipeline::{LogicalNode, LogicalPipeline};

/// The address every test router serves, and so the `Host` a request names.
pub const SERVED: &str = "127.0.0.1:7878";
/// The origin a browser page served by [`SERVED`] sends.
pub const OWN_ORIGIN: &str = "http://127.0.0.1:7878";
/// The build identity every test router reports.
pub const BUILD: &str = "ragondin 0.0.0+test";
/// The id of the run stored under `tests/fixtures/runs`.
pub const FIXTURE_RUN: &str = "b41e0752792e728f5dd893043b42d2a2d71f0b0039157a177e0a267e0420ea6f";

/// A run store held in memory: what `GET /runs` lists is what this holds.
#[derive(Default)]
pub struct FakeRunStore {
    runs: Mutex<BTreeMap<String, Run>>,
    unreadable: Mutex<BTreeMap<String, String>>,
}

impl FakeRunStore {
    pub fn holding(runs: impl IntoIterator<Item = Run>) -> Self {
        let store = Self::default();
        for run in runs {
            store.save(&run).expect("the fake store keeps every run");
        }
        store
    }

    /// Makes `id` listed but unreadable, as a torn directory is in the file
    /// backend.
    pub fn tear(&self, id: &str, reason: &str) {
        self.unreadable
            .lock()
            .unwrap()
            .insert(id.to_owned(), reason.to_owned());
    }
}

impl RunStore for FakeRunStore {
    fn save(&self, run: &Run) -> Result<(), RunStoreError> {
        self.runs
            .lock()
            .unwrap()
            .entry(run.id.to_string())
            .or_insert_with(|| run.clone());
        Ok(())
    }

    fn load(&self, id: &RunId) -> Result<Run, RunStoreError> {
        let key = id.to_string();
        if self.unreadable.lock().unwrap().contains_key(&key) {
            return Err(RunStoreError::Incomplete {
                path: PathBuf::from(key),
            });
        }
        self.runs
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or(RunStoreError::NotFound { id: *id })
    }

    fn ids(&self) -> Result<Vec<RunId>, RunStoreError> {
        let mut ids: Vec<String> = self.runs.lock().unwrap().keys().cloned().collect();
        ids.extend(self.unreadable.lock().unwrap().keys().cloned());
        ids.sort();
        ids.dedup();
        Ok(ids.iter().map(|id| id.parse().unwrap()).collect())
    }
}

/// What [`FakeLauncher`] reports as the identity a probe read, followed by
/// the binding and the served model it was asked about.
pub const PROBED_IDENTITY: &str = "fake-identity";

/// The one node parameter [`FakeLauncher::check_document`] refuses, as the
/// binary refuses a key no component reads.
pub const UNREAD_KEY: &str = "unread_by_anything";

/// The six families the binary binds names in, as the fake checks them.
const FAMILIES: [&str; 6] = [
    "retriever",
    "fusion",
    "reranker",
    "context_builder",
    "generator",
    "embedder",
];

/// A launcher that answers capabilities, checks a binding the way the binary
/// words it — `--remote` and the argument first — and probes by address:
/// port 1 is unreachable, any other answers [`PROBED_IDENTITY`].
pub struct FakeLauncher {
    pub capabilities: Capabilities,
}

impl Default for FakeLauncher {
    fn default() -> Self {
        Self {
            capabilities: Capabilities {
                families: vec![
                    FamilyCapabilities {
                        family: "generator".to_owned(),
                        local: vec!["stub_generator".to_owned()],
                    },
                    FamilyCapabilities {
                        family: "retriever".to_owned(),
                        local: vec!["bm25".to_owned(), "stub_retriever".to_owned()],
                    },
                ],
                remote: true,
            },
        }
    }
}

#[async_trait]
impl Launcher for FakeLauncher {
    fn capabilities(&self) -> Capabilities {
        self.capabilities.clone()
    }

    fn check_binding(&self, family: &str, name: &str, uri: &str) -> Result<(), ApiError> {
        let argument = format!("`--remote {family}/{name}={uri}`");
        if !FAMILIES.contains(&family) {
            return Err(ApiError::BindingRefused {
                detail: format!("{argument}: `{family}` is not a family a name can be bound in"),
            });
        }
        if !uri.starts_with("http://") {
            return Err(ApiError::BindingRefused {
                detail: format!("{argument}: `{uri}` is not an `http://` URI"),
            });
        }
        Ok(())
    }

    fn check_document(
        &self,
        pipeline: &LogicalPipeline,
        _bindings: &[ServiceBinding],
    ) -> Result<(), ApiError> {
        for node in pipeline.nodes() {
            let params = match node {
                LogicalNode::Retriever(node) => &node.params,
                LogicalNode::Fusion(node) => &node.params,
                LogicalNode::Reranker(node) => &node.params,
                LogicalNode::ContextBuilder(node) => &node.params,
                LogicalNode::Generator(node) => &node.params,
                LogicalNode::Extension(node) => &node.params,
            };
            if params.contains_key(UNREAD_KEY) {
                let id = node.id().as_str();
                return Err(ApiError::PipelineInvalid {
                    detail: format!(
                        "node `{id}`: `{UNREAD_KEY}` is not a key the fake component reads: \
                         refused rather than hashed as inert"
                    ),
                    location: ragondin_api::Location {
                        node: Some(id.to_owned()),
                        edge: None,
                    },
                });
            }
        }
        Ok(())
    }

    async fn probe(
        &self,
        family: &str,
        name: &str,
        uri: &str,
        served_model: Option<&str>,
    ) -> Result<ServiceIdentity, ApiError> {
        self.check_binding(family, name, uri)?;
        if uri.ends_with(":1") {
            return Err(ApiError::ServiceUnreachable {
                uri: uri.to_owned(),
                reason: "connection refused".to_owned(),
                last_identity: None,
            });
        }
        let served = served_model
            .map(|model| format!("#{model}"))
            .unwrap_or_default();
        Ok(ServiceIdentity {
            identity: format!("{PROBED_IDENTITY}:{family}/{name}@{uri}{served}"),
        })
    }

    async fn identity(&self, _submission: &Submission) -> Result<RunId, ApiError> {
        Ok(FIXTURE_RUN.parse().unwrap())
    }

    async fn execute(&self, _job: Job) -> JobState {
        JobState::Cancelled
    }
}

/// Settings held in memory.
pub struct FakeSettings {
    pub settings: Mutex<Settings>,
}

impl Default for FakeSettings {
    fn default() -> Self {
        Self {
            settings: Mutex::new(Settings {
                datasets: PathBuf::from("/workspace/datasets"),
                services: vec![ServiceBinding {
                    family: "generator".to_owned(),
                    name: "qwen".to_owned(),
                    uri: "http://127.0.0.1:50051".to_owned(),
                }],
            }),
        }
    }
}

#[async_trait]
impl WorkspaceSettings for FakeSettings {
    async fn read(&self) -> Result<Settings, ApiError> {
        Ok(self.settings.lock().unwrap().clone())
    }

    async fn bind(&self, binding: ServiceBinding) -> Result<Settings, ApiError> {
        let mut settings = self.settings.lock().unwrap();
        match settings
            .services
            .iter_mut()
            .find(|bound| bound.family == binding.family && bound.name == binding.name)
        {
            Some(bound) => bound.uri = binding.uri,
            None => settings.services.push(binding),
        }
        Ok(settings.clone())
    }

    async fn unbind(&self, family: &str, name: &str) -> Result<Option<Settings>, ApiError> {
        let mut settings = self.settings.lock().unwrap();
        let before = settings.services.len();
        settings
            .services
            .retain(|bound| !(bound.family == family && bound.name == name));
        Ok((settings.services.len() != before).then(|| settings.clone()))
    }

    async fn set_datasets(&self, datasets: Option<PathBuf>) -> Result<Settings, ApiError> {
        let mut settings = self.settings.lock().unwrap();
        settings.datasets = datasets.unwrap_or_else(|| PathBuf::from("/workspace/datasets"));
        Ok(settings.clone())
    }
}

/// A workspace with no pipeline in it.
#[derive(Default)]
pub struct FakePipelines;

#[async_trait]
impl PipelineSource for FakePipelines {
    async fn list(&self) -> Result<Vec<PipelineFile>, ApiError> {
        Ok(Vec::new())
    }

    async fn read(&self, name: &str) -> Result<PipelineFile, ApiError> {
        Err(ApiError::PipelineNotFound {
            name: name.to_owned(),
        })
    }

    async fn write(
        &self,
        name: &str,
        _document: &str,
        _precondition: &Precondition,
    ) -> Result<PipelineFile, ApiError> {
        Err(ApiError::BackendFailed {
            detail: format!("the fake workspace keeps no pipeline, {name} included"),
        })
    }

    async fn read_layout(&self, name: &str) -> Result<Option<Layout>, ApiError> {
        Err(ApiError::PipelineNotFound {
            name: name.to_owned(),
        })
    }

    async fn write_layout(&self, name: &str, _layout: &Layout) -> Result<(), ApiError> {
        Err(ApiError::PipelineNotFound {
            name: name.to_owned(),
        })
    }

    async fn read_pairing(&self, name: &str, _other: &str) -> Result<Option<Pairing>, ApiError> {
        Err(ApiError::PipelineNotFound {
            name: name.to_owned(),
        })
    }

    async fn write_pairing(&self, pairing: &Pairing) -> Result<(), ApiError> {
        Err(ApiError::PipelineNotFound {
            name: pairing.pipeline.clone(),
        })
    }

    async fn delete_pairing(&self, name: &str, _other: &str) -> Result<(), ApiError> {
        Err(ApiError::PipelineNotFound {
            name: name.to_owned(),
        })
    }
}

/// A registry with no benchmark in it.
#[derive(Default)]
pub struct FakeRegistry;

#[async_trait]
impl Registry for FakeRegistry {
    async fn benchmarks(&self) -> Result<Vec<BenchmarkEntry>, ApiError> {
        Ok(Vec::new())
    }

    async fn verify(&self, name: &str) -> Result<BenchmarkEntry, ApiError> {
        Err(ApiError::BenchmarkNotFound {
            name: name.to_owned(),
        })
    }

    async fn download(
        &self,
        name: &str,
        _progress: ProgressSink,
        _cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<BenchmarkEntry, ApiError> {
        Err(ApiError::BenchmarkNotFound {
            name: name.to_owned(),
        })
    }

    async fn import(&self, name: &str, _path: &Path) -> Result<BenchmarkEntry, ApiError> {
        Err(ApiError::ImportRefused {
            name: name.to_owned(),
            reason: "the fake registry imports nothing".to_owned(),
        })
    }

    async fn dataset(&self, _dataset_version: &str) -> Result<RunDataset, ApiError> {
        Ok(RunDataset::Unknown)
    }

    async fn pinned(&self) -> Result<Vec<PinnedBenchmark>, ApiError> {
        Ok(Vec::new())
    }
}

/// A workspace holding the given documents, by name, in memory: what
/// `list` and `read` answer. Writes nothing.
#[derive(Default)]
pub struct HeldPipelines {
    pub files: Vec<(String, String)>,
}

#[async_trait]
impl PipelineSource for HeldPipelines {
    async fn list(&self) -> Result<Vec<PipelineFile>, ApiError> {
        Ok(self
            .files
            .iter()
            .map(|(name, document)| PipelineFile {
                name: name.clone(),
                document: document.clone(),
                revision: Revision::new(name.clone()),
                modified: std::time::UNIX_EPOCH,
            })
            .collect())
    }

    async fn read(&self, name: &str) -> Result<PipelineFile, ApiError> {
        match self
            .list()
            .await?
            .into_iter()
            .find(|file| file.name == name)
        {
            Some(file) => Ok(file),
            None => FakePipelines.read(name).await,
        }
    }

    async fn write(
        &self,
        name: &str,
        document: &str,
        precondition: &Precondition,
    ) -> Result<PipelineFile, ApiError> {
        FakePipelines.write(name, document, precondition).await
    }

    async fn read_layout(&self, name: &str) -> Result<Option<Layout>, ApiError> {
        FakePipelines.read_layout(name).await
    }

    async fn write_layout(&self, name: &str, layout: &Layout) -> Result<(), ApiError> {
        FakePipelines.write_layout(name, layout).await
    }

    async fn read_pairing(&self, name: &str, other: &str) -> Result<Option<Pairing>, ApiError> {
        FakePipelines.read_pairing(name, other).await
    }

    async fn write_pairing(&self, pairing: &Pairing) -> Result<(), ApiError> {
        FakePipelines.write_pairing(pairing).await
    }

    async fn delete_pairing(&self, name: &str, other: &str) -> Result<(), ApiError> {
        FakePipelines.delete_pairing(name, other).await
    }
}

/// A registry whose entries are pinned to the given digests, as
/// `(selector, dataset_version)`: what `pinned` answers. Loads, lists,
/// downloads and imports nothing.
#[derive(Default)]
pub struct PinningRegistry {
    pub pins: Vec<(String, String)>,
}

#[async_trait]
impl Registry for PinningRegistry {
    async fn benchmarks(&self) -> Result<Vec<BenchmarkEntry>, ApiError> {
        panic!("naming a run's benchmarks verifies no benchmark")
    }

    async fn verify(&self, name: &str) -> Result<BenchmarkEntry, ApiError> {
        FakeRegistry.verify(name).await
    }

    async fn download(
        &self,
        name: &str,
        progress: ProgressSink,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<BenchmarkEntry, ApiError> {
        FakeRegistry.download(name, progress, cancel).await
    }

    async fn import(&self, name: &str, path: &Path) -> Result<BenchmarkEntry, ApiError> {
        FakeRegistry.import(name, path).await
    }

    async fn dataset(&self, _version: &str) -> Result<RunDataset, ApiError> {
        panic!("naming a run's benchmarks loads no dataset")
    }

    async fn pinned(&self) -> Result<Vec<PinnedBenchmark>, ApiError> {
        Ok(self
            .pins
            .iter()
            .map(|(name, dataset_version)| PinnedBenchmark {
                name: name.clone(),
                dataset_version: dataset_version.clone(),
            })
            .collect())
    }
}

/// A registry holding benchmarks in memory, each on "disk" and verified: a
/// run's dataset is found when one of them digests to its `dataset_version`.
/// Lists, downloads and imports nothing.
#[derive(Default)]
pub struct FixtureRegistry {
    held: Vec<(String, Arc<LoadedDataset>)>,
}

impl FixtureRegistry {
    pub fn holding(benchmarks: impl IntoIterator<Item = (String, Benchmark)>) -> Self {
        Self {
            held: benchmarks
                .into_iter()
                .map(|(name, benchmark)| (name, Arc::new(LoadedDataset::new(benchmark))))
                .collect(),
        }
    }
}

#[async_trait]
impl Registry for FixtureRegistry {
    async fn benchmarks(&self) -> Result<Vec<BenchmarkEntry>, ApiError> {
        Ok(Vec::new())
    }

    async fn verify(&self, name: &str) -> Result<BenchmarkEntry, ApiError> {
        FakeRegistry.verify(name).await
    }

    async fn download(
        &self,
        name: &str,
        progress: ProgressSink,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<BenchmarkEntry, ApiError> {
        FakeRegistry.download(name, progress, cancel).await
    }

    async fn import(&self, name: &str, path: &Path) -> Result<BenchmarkEntry, ApiError> {
        FakeRegistry.import(name, path).await
    }

    async fn dataset(&self, version: &str) -> Result<RunDataset, ApiError> {
        Ok(self
            .held
            .iter()
            .find(|(_, dataset)| dataset_version(dataset.benchmark()) == version)
            .map_or(RunDataset::Unknown, |(name, dataset)| {
                RunDataset::Verified {
                    name: name.clone(),
                    dataset: Arc::clone(dataset),
                }
            }))
    }

    async fn pinned(&self) -> Result<Vec<PinnedBenchmark>, ApiError> {
        Ok(self
            .held
            .iter()
            .map(|(name, dataset)| PinnedBenchmark {
                name: name.clone(),
                dataset_version: dataset_version(dataset.benchmark()),
            })
            .collect())
    }
}

/// The router over the given store, and default fakes for the rest.
pub fn app(store: FakeRunStore) -> Server {
    app_with(store, FakeLauncher::default())
}

pub fn app_with(store: FakeRunStore, launcher: FakeLauncher) -> Server {
    app_serving(store, launcher, Arc::new(ragondin_api::NoAssets))
}

/// The router with `assets` served beside the API, as the binary serves the
/// UI's pages.
pub fn app_with_assets(assets: impl Assets + 'static) -> Server {
    app_serving(
        FakeRunStore::default(),
        FakeLauncher::default(),
        Arc::new(assets),
    )
}

fn app_serving(store: FakeRunStore, launcher: FakeLauncher, assets: Arc<dyn Assets>) -> Server {
    serve(
        store,
        Arc::new(FakeRegistry),
        launcher,
        PathBuf::from("/workspace"),
        assets,
    )
}

/// The router over `store` and `registry`, working in `workspace` — where
/// the derived data it computes is cached, under `cache/`.
pub fn app_over(store: FakeRunStore, registry: Arc<dyn Registry>, workspace: &Path) -> Server {
    serve(
        store,
        registry,
        FakeLauncher::default(),
        workspace.to_path_buf(),
        Arc::new(ragondin_api::NoAssets),
    )
}

fn serve(
    store: FakeRunStore,
    registry: Arc<dyn Registry>,
    launcher: FakeLauncher,
    workspace: PathBuf,
    assets: Arc<dyn Assets>,
) -> Server {
    let mut backends = fakes(store);
    backends.registry = registry;
    backends.launcher = Arc::new(launcher);
    router(
        backends,
        ServerConfig {
            served: SERVED.to_owned(),
            build: BUILD.to_owned(),
            workspace,
        },
        assets,
    )
}

/// The default fakes over `store`, for a test to replace one of.
pub fn fakes(store: FakeRunStore) -> Backends {
    Backends {
        runs: Arc::new(store),
        pipelines: Arc::new(FakePipelines),
        registry: Arc::new(FakeRegistry),
        settings: Arc::new(FakeSettings::default()),
        launcher: Arc::new(FakeLauncher::default()),
    }
}

/// The router over `backends`, working in `workspace` — where the derived
/// data it computes is cached, under `cache/`.
pub fn router_over(backends: Backends, workspace: &Path) -> Server {
    router(
        backends,
        ServerConfig {
            served: SERVED.to_owned(),
            build: BUILD.to_owned(),
            workspace: workspace.to_path_buf(),
        },
        Arc::new(ragondin_api::NoAssets),
    )
}

/// The router over `backends`, with no assets.
pub fn app_with_backends(backends: Backends) -> Server {
    router(
        backends,
        ServerConfig {
            served: SERVED.to_owned(),
            build: BUILD.to_owned(),
            workspace: PathBuf::from("/workspace"),
        },
        Arc::new(ragondin_api::NoAssets),
    )
}

/// A `GET` that passes the `Host` check.
pub fn get(path: &str) -> Request<Body> {
    Request::get(path)
        .header("host", SERVED)
        .body(Body::empty())
        .unwrap()
}

/// A state-changing request that passes the `Host` and `Origin` checks,
/// with `body` as JSON and `headers` added.
pub fn write_request(
    method: &str,
    path: &str,
    body: &serde_json::Value,
    headers: &[(&str, &str)],
) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", SERVED)
        .header("origin", OWN_ORIGIN)
        .header("content-type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

pub async fn send(app: Server, request: Request<Body>) -> Response<Body> {
    use tower::ServiceExt;
    app.oneshot(request)
        .await
        .expect("the server is infallible")
}

/// The page [`FakeAssets::built`] holds at `index.html`.
pub const INDEX_PAGE: &str = "<!doctype html><title>fixture</title><div id=\"root\"></div>";

/// An asset table in memory, as `vite build` lays one out, recording every
/// path it was asked for.
#[derive(Clone, Default)]
pub struct FakeAssets {
    files: BTreeMap<&'static str, &'static str>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl FakeAssets {
    /// A built UI: an index page, a script, a stylesheet and an icon.
    pub fn built() -> Self {
        Self {
            files: BTreeMap::from([
                ("index.html", INDEX_PAGE),
                ("assets/index-abc123.js", "console.log(1);"),
                ("assets/index-abc123.css", "body{}"),
                ("favicon.svg", "<svg/>"),
            ]),
            asked: Arc::default(),
        }
    }

    /// Every path the router asked for, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Assets for FakeAssets {
    fn get(&self, path: &str) -> Option<Asset> {
        self.asked.lock().unwrap().push(path.to_owned());
        self.files.get(path).map(|text| Asset {
            bytes: text.as_bytes().into(),
            content_type: content_type_for(path),
        })
    }
}

pub async fn body(response: Response<Body>) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).expect("the body is UTF-8")
}

pub async fn json(response: Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).expect("the body is JSON")
}

/// The run stored under `tests/fixtures/runs`, recorded by the harness.
pub fn fixture_run() -> Run {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runs");
    FileSystemRunStore::new(root)
        .load(&FIXTURE_RUN.parse().unwrap())
        .expect("the fixture run loads")
}
