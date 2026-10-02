//! The `Registry` file backend keeps a verified dataset in memory between
//! requests, and re-verifies it whenever its files change: per-node and
//! replay requests on one run load and digest the dataset once, and a byte
//! changed on disk is never served from memory.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::http::StatusCode;
use ragondin_api::fs::FsRegistry;
use ragondin_api::{
    ApiError, BenchmarkEntry, LoadedDataset, PinnedBenchmark, ProgressSink, Registry, RunDataset,
};
use ragondin_benchmarks::manifest::Format;
use ragondin_experiments::Run;
use serde_json::Value;
use support::datasets::{beir_mini_entry, benchmark_fixture, copy_dir, scratch, version_of};
use support::runs::{documents, generation_trace, run_over, GENERATION};
use support::{app_over, get, json, send, FakeRunStore};

const NAME: &str = "beir/mini";
const DIR: &str = "mini";

/// A generation run over the miniature BEIR fixture.
fn the_run() -> Run {
    let benchmark = Format::Beir
        .load(&benchmark_fixture("beir-mini"))
        .expect("the fixture loads");
    run_over(
        0x51,
        GENERATION,
        &benchmark,
        vec![(
            "q-1",
            generation_trace(
                "q-1",
                documents(&["4983", "MED-10"]),
                documents(&["MED-10", "4983", "MED-12"]),
                2,
                "on the mat",
            ),
        )],
        &[("mrr", 0.5), ("ndcg@10", 0.5), ("recall@10", 0.5)],
    )
}

/// A workspace whose datasets directory holds the fixture, and a registry
/// whose manifest pins it.
fn workspace(test: &str) -> (PathBuf, FsRegistry) {
    let workspace = scratch(test);
    let datasets = workspace.join("datasets");
    let fixture = benchmark_fixture("beir-mini");
    copy_dir(&fixture, &datasets.join(DIR));
    let version = version_of(Format::Beir, &fixture);
    let registry = FsRegistry::new(
        datasets,
        vec![beir_mini_entry(NAME, "https://example.invalid", &version)],
    );
    (workspace, registry)
}

/// Waits until every file the fixture wrote is older than the registry's
/// racy-timestamp margin (two seconds, `fs/memo.rs`): before that a verified
/// dataset is served but not kept, since a same-size rewrite within one tick
/// of a coarse filesystem clock would leave its stamps unchanged.
async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(2_200)).await;
}

/// The file backend, keeping every verified dataset it hands out: two
/// answers that are one allocation are one load. Each is kept alive here, so
/// a freed allocation cannot be reused by a second load and pass for the
/// first.
#[derive(Clone)]
struct Recording {
    inner: FsRegistry,
    verified: Arc<Mutex<Vec<Arc<LoadedDataset>>>>,
}

impl Recording {
    fn over(inner: FsRegistry) -> Self {
        Self {
            inner,
            verified: Arc::default(),
        }
    }

    fn handed_out(&self) -> Vec<Arc<LoadedDataset>> {
        self.verified.lock().unwrap().clone()
    }
}

#[async_trait]
impl Registry for Recording {
    async fn benchmarks(&self) -> Result<Vec<BenchmarkEntry>, ApiError> {
        self.inner.benchmarks().await
    }

    async fn verify(&self, name: &str) -> Result<BenchmarkEntry, ApiError> {
        self.inner.verify(name).await
    }

    async fn download(
        &self,
        name: &str,
        progress: ProgressSink,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<BenchmarkEntry, ApiError> {
        self.inner.download(name, progress, cancel).await
    }

    async fn import(&self, name: &str, path: &Path) -> Result<BenchmarkEntry, ApiError> {
        self.inner.import(name, path).await
    }

    async fn pinned(&self) -> Result<Vec<PinnedBenchmark>, ApiError> {
        self.inner.pinned().await
    }

    async fn dataset(&self, version: &str) -> Result<RunDataset, ApiError> {
        let found = self.inner.dataset(version).await?;
        if let RunDataset::Verified { dataset, .. } = &found {
            self.verified.lock().unwrap().push(Arc::clone(dataset));
        }
        Ok(found)
    }
}

async fn fetch(workspace: &Path, registry: &Recording, run: &Run, path: &str) -> Value {
    let response = send(
        app_over(
            FakeRunStore::holding([run.clone()]),
            Arc::new(registry.clone()),
            workspace,
        ),
        get(&format!("/api/v1/runs/{}{path}", run.id)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    json(response).await
}

#[tokio::test(flavor = "multi_thread")]
async fn consecutive_requests_on_one_run_load_and_digest_its_dataset_once() {
    let (workspace, registry) = workspace("memo_once");
    let registry = Recording::over(registry);
    settle().await;
    let run = the_run();

    for path in ["/queries", "/queries", "/trace/q-1", "/trace/q-1"] {
        let body = fetch(&workspace, &registry, &run, path).await;
        let check = body.get("ground_truth").or(body.get("passages")).unwrap();
        assert_eq!(check["status"], "verified", "{path}");
    }

    let handed_out = registry.handed_out();
    assert_eq!(handed_out.len(), 4);
    for (at, dataset) in handed_out.iter().enumerate() {
        assert!(
            Arc::ptr_eq(dataset, &handed_out[0]),
            "request {at} was answered from another load"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn one_byte_changed_between_requests_is_re_verified_and_differs() {
    let (workspace, registry) = workspace("memo_one_byte");
    let registry = Recording::over(registry);
    settle().await;
    let run = the_run();
    let listing = fetch(&workspace, &registry, &run, "/queries").await;
    assert_eq!(listing["ground_truth"]["status"], "verified");
    let trace = fetch(&workspace, &registry, &run, "/trace/q-1").await;
    assert_eq!(trace["passages"]["status"], "verified");

    let corpus = workspace.join("datasets").join(DIR).join("corpus.jsonl");
    let original = fs::read_to_string(&corpus).unwrap();
    let altered = original.replacen("The cat sat", "The bat sat", 1);
    assert_eq!(
        altered.len(),
        original.len(),
        "one byte, not a length change"
    );
    fs::write(&corpus, &altered).unwrap();

    let listing = fetch(&workspace, &registry, &run, "/queries").await;
    assert_eq!(listing["ground_truth"]["status"], "dataset_differs");
    assert!(listing["queries"][0]["scores"]
        .as_object()
        .is_none_or(serde_json::Map::is_empty));
    let trace = fetch(&workspace, &registry, &run, "/trace/q-1").await;
    assert_eq!(trace["passages"]["status"], "dataset_differs");

    // Restored, it verifies again — loaded afresh, not the first load.
    fs::write(&corpus, &original).unwrap();
    let listing = fetch(&workspace, &registry, &run, "/queries").await;
    assert_eq!(listing["ground_truth"]["status"], "verified");
    let handed_out = registry.handed_out();
    assert_eq!(
        handed_out.len(),
        3,
        "verified before, and after the restore"
    );
    assert!(
        Arc::ptr_eq(&handed_out[0], &handed_out[1]),
        "one load before"
    );
    assert!(
        !Arc::ptr_eq(&handed_out[2], &handed_out[0]),
        "re-verified after the change, not served from memory"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dataset_removed_between_requests_is_absent() {
    let (workspace, registry) = workspace("memo_removed");
    let registry = Recording::over(registry);
    let run = the_run();
    let listing = fetch(&workspace, &registry, &run, "/queries").await;
    assert_eq!(listing["ground_truth"]["status"], "verified");

    fs::remove_dir_all(workspace.join("datasets").join(DIR)).unwrap();

    let listing = fetch(&workspace, &registry, &run, "/queries").await;
    assert_eq!(listing["ground_truth"]["status"], "dataset_absent");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dataset_just_written_is_served_but_not_kept() {
    let (workspace, registry) = workspace("memo_racy");
    let registry = Recording::over(registry);
    let run = the_run();
    // Written again just before the requests, same content, so the files are
    // within the margin however long the runner took since the copy.
    let queries = workspace.join("datasets").join(DIR).join("queries.jsonl");
    fs::write(&queries, fs::read(&queries).unwrap()).unwrap();

    for _ in 0..2 {
        let listing = fetch(&workspace, &registry, &run, "/queries").await;
        assert_eq!(listing["ground_truth"]["status"], "verified");
    }

    let handed_out = registry.handed_out();
    assert!(
        !Arc::ptr_eq(&handed_out[0], &handed_out[1]),
        "files within the margin are loaded on every request"
    );
}
