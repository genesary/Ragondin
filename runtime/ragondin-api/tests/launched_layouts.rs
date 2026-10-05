//! The layout copied at launch: `POST /runs` copies `pipelines/<name>.layout.json`
//! to `layouts/<hash>.json`, under the canonical hash of the document the job
//! runs, so that "Fork this run" finds it through `GET /runs/{id}/layout`.
//! Driven over the router against a real workspace on disk — `FsPipelines`,
//! the file run store, the queue's `jobs/` — and a launcher that files each
//! run under its document's hash.

mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::http::StatusCode;
use ragondin_api::fs::{FsPipelines, Workspace};
use ragondin_api::{
    ApiError, BenchmarkEntry, BenchmarkState, Cancellation, Capabilities, GroundTruth, Launcher,
    LauncherError, PinnedBenchmark, ProgressSink, Registry, RunDataset, RunObserver, Server,
    ServiceBinding, ServiceIdentity, Submission,
};
use ragondin_benchmarks::{Benchmark, Qrels};
use ragondin_config::parse_document;
use ragondin_experiments::{FileSystemRunStore, Run, RunId};
use ragondin_pipeline::LogicalPipeline;
use serde_json::{json, Value};
use support::datasets::scratch;
use support::runs::run_over;
use support::{fakes, get, json, router_over, send, write_request, FakeRunStore};

const NAME: &str = "hybrid";
const BENCHMARK: &str = "beir/scifact";

const HYBRID: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: lexical
      component: retriever
      impl: bm25
      inputs: [question]
    - id: semantic
      component: retriever
      impl: dense
      inputs: [question]
    - id: fused
      component: fusion
      impl: rrf
      inputs: [lexical, semantic]
    - id: context
      component: context_builder
      impl: concat
      inputs: [question, fused]
    - id: answer
      component: generator
      impl: answerer
      inputs: [question, context]
";

/// A layout formatted as no serializer would write it — keys out of order,
/// odd spacing — so that a copy that re-serialized it would show.
const LAYOUT: &str = "{ \"nodes\": {\"lexical\": {\"y\": 2.0, \"x\": 1.5},\n  \"fused\": {\"x\": 3.0,   \"y\": -4.25}},\n  \"version\": 1 }\n";

fn hash_of(document: &str) -> String {
    parse_document(document)
        .expect("a test document validates")
        .content_hash()
        .to_string()
}

/// A launcher that announces, and files each run under, the content hash of
/// the document it is handed: what ran is a run of exactly that document.
struct HashingLauncher;

#[async_trait]
impl Launcher for HashingLauncher {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            families: Vec::new(),
            remote: false,
        }
    }

    fn check_binding(&self, _: &str, _: &str, _: &str) -> Result<(), ApiError> {
        Ok(())
    }

    fn check_document(&self, _: &LogicalPipeline, _: &[ServiceBinding]) -> Result<(), ApiError> {
        Ok(())
    }

    async fn probe(
        &self,
        _: &str,
        _: &str,
        uri: &str,
        _: Option<&str>,
    ) -> Result<ServiceIdentity, ApiError> {
        Err(ApiError::ServiceUnreachable {
            uri: uri.to_owned(),
            reason: "probes nothing".to_owned(),
            last_identity: None,
        })
    }

    async fn identity(&self, submission: &Submission) -> Result<RunId, LauncherError> {
        let pipeline = parse_document(&submission.pipeline).map_err(|error| {
            LauncherError::PipelineInvalid {
                detail: error.to_string(),
                node: None,
            }
        })?;
        Ok(RunId::from_digest(*pipeline.content_hash().as_bytes()))
    }

    async fn execute(
        &self,
        submission: &Submission,
        _: Arc<dyn RunObserver>,
        _: Cancellation,
    ) -> Result<Run, LauncherError> {
        let benchmark = Benchmark::new(Vec::new(), Vec::new(), Qrels::new());
        let mut run = run_over(0, &submission.pipeline, &benchmark, Vec::new(), &[]);
        run.id = self.identity(submission).await?;
        Ok(run)
    }
}

/// A registry holding one ready benchmark of qrels, which a prefix ending in
/// a ranking can be scored on.
struct QrelsRegistry;

#[async_trait]
impl Registry for QrelsRegistry {
    async fn benchmarks(&self) -> Result<Vec<BenchmarkEntry>, ApiError> {
        Ok(vec![self.verify(BENCHMARK).await?])
    }

    async fn verify(&self, name: &str) -> Result<BenchmarkEntry, ApiError> {
        Ok(BenchmarkEntry {
            name: name.to_owned(),
            format: "beir".to_owned(),
            state: BenchmarkState::Ready {
                dataset_version: "digest".to_owned(),
            },
            ground_truth: Some(GroundTruth::Qrels),
            licence: None,
            licence_url: None,
        })
    }

    async fn download(
        &self,
        name: &str,
        _: ProgressSink,
        _: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<BenchmarkEntry, ApiError> {
        self.verify(name).await
    }

    async fn import(&self, name: &str, _: &Path) -> Result<BenchmarkEntry, ApiError> {
        self.verify(name).await
    }

    async fn dataset(&self, _: &str) -> Result<RunDataset, ApiError> {
        Ok(RunDataset::Unknown)
    }

    async fn pinned(&self) -> Result<Vec<PinnedBenchmark>, ApiError> {
        Ok(Vec::new())
    }
}

/// A fresh workspace holding [`HYBRID`], and its layout when `layout` is set.
fn workspace(test: &str, layout: Option<&str>) -> Workspace {
    let workspace =
        Workspace::open(scratch(&format!("launched-layout-{test}"))).expect("a workspace opens");
    fs::write(workspace.pipelines().join(format!("{NAME}.yaml")), HYBRID).unwrap();
    if let Some(layout) = layout {
        fs::write(
            workspace.pipelines().join(format!("{NAME}.layout.json")),
            layout,
        )
        .unwrap();
    }
    workspace
}

fn server(workspace: &Workspace) -> Server {
    let mut backends = fakes(FakeRunStore::default());
    backends.runs = Arc::new(FileSystemRunStore::new(workspace.runs()));
    backends.pipelines = Arc::new(FsPipelines::new(workspace));
    backends.registry = Arc::new(QrelsRegistry);
    backends.launcher = Arc::new(HashingLauncher);
    router_over(backends, workspace.root())
}

/// Submits [`NAME`], up to `up_to` when set, and answers the `202`'s body.
async fn launch(app: &Server, up_to: Option<&str>) -> Value {
    let mut body = json!({ "pipeline": NAME, "benchmark": BENCHMARK });
    if let Some(node) = up_to {
        body["up_to"] = json!(node);
    }
    let response = send(
        app.clone(),
        write_request("POST", "/api/v1/runs", &body, &[]),
    )
    .await;
    let status = response.status();
    let body = json(response).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    body
}

/// Waits for the job `accepted` names to be done, for at most 5 s.
async fn done(app: &Server, accepted: &Value) {
    let path = format!("/api/v1/jobs/{}", accepted["job_id"].as_str().unwrap());
    let mut job = Value::Null;
    for _ in 0..500 {
        job = json(send(app.clone(), get(&path)).await).await;
        if job["state"]["kind"] == "done" {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the job never finished: {job}");
}

async fn faults(app: &Server) -> Vec<Value> {
    json(send(app.clone(), get("/api/v1/jobs")).await).await["faults"]
        .as_array()
        .unwrap()
        .clone()
}

fn launched(workspace: &Workspace, hash: &str) -> std::path::PathBuf {
    workspace.layouts().join(format!("{hash}.json"))
}

#[tokio::test]
async fn launching_a_pipeline_with_a_layout_copies_it_byte_for_byte_under_its_hash() {
    let workspace = workspace("whole", Some(LAYOUT));
    let app = server(&workspace);

    launch(&app, None).await;

    let copied = fs::read(launched(&workspace, &hash_of(HYBRID))).expect("the layout was copied");
    assert_eq!(copied, LAYOUT.as_bytes());
    assert!(faults(&app).await.is_empty());
}

/// What "Fork this run" does, through the API: the run's configuration
/// written to the new pipeline `name`, then the layout copied at the run's
/// launch written beside it. Answers the new pipeline's layout.
async fn fork(app: &Server, run: &str, name: &str) -> Value {
    let detail = json(send(app.clone(), get(&format!("/api/v1/runs/{run}"))).await).await;
    let written = send(
        app.clone(),
        write_request(
            "PUT",
            &format!("/api/v1/pipelines/{name}"),
            &json!({ "document": detail["configuration"] }),
            &[("if-none-match", "*")],
        ),
    )
    .await;
    assert_eq!(written.status(), StatusCode::OK);
    let at_launch = json(send(app.clone(), get(&format!("/api/v1/runs/{run}/layout"))).await).await;
    let copied = send(
        app.clone(),
        write_request(
            "PUT",
            &format!("/api/v1/pipelines/{name}/layout"),
            &at_launch["layout"],
            &[],
        ),
    )
    .await;
    assert_eq!(copied.status(), StatusCode::OK);
    let read = send(
        app.clone(),
        get(&format!("/api/v1/pipelines/{name}/layout")),
    )
    .await;
    json(read).await["layout"].clone()
}

#[tokio::test]
async fn a_fork_of_the_launched_run_carries_the_layout_to_its_new_pipeline() {
    let workspace = workspace("fork", Some(LAYOUT));
    let app = server(&workspace);
    let accepted = launch(&app, None).await;
    done(&app, &accepted).await;

    let forked = fork(&app, accepted["run_id"].as_str().unwrap(), "hybrid-fork").await;

    let original: Value = serde_json::from_str(LAYOUT).unwrap();
    assert_eq!(forked, original);
}

#[tokio::test]
async fn a_fork_of_a_prefix_run_carries_the_parent_s_layout_to_its_new_pipeline() {
    let workspace = workspace("fork-prefix", Some(LAYOUT));
    let app = server(&workspace);
    let accepted = launch(&app, Some("fused")).await;
    done(&app, &accepted).await;

    let forked = fork(
        &app,
        accepted["run_id"].as_str().unwrap(),
        "hybrid-fused-fork",
    )
    .await;

    let original: Value = serde_json::from_str(LAYOUT).unwrap();
    assert_eq!(forked, original);
}

#[tokio::test]
async fn a_refused_resubmission_leaves_the_copy_made_at_the_accepted_launch() {
    let workspace = workspace("refused", Some(LAYOUT));
    let app = server(&workspace);
    let accepted = launch(&app, None).await;
    done(&app, &accepted).await;

    // The layout moves on; the same document submitted again is `run_exists`.
    fs::write(
        workspace.pipelines().join(format!("{NAME}.layout.json")),
        r#"{"version":1,"nodes":{"lexical":{"x":99.0,"y":99.0}}}"#,
    )
    .unwrap();
    let response = send(
        app.clone(),
        write_request(
            "POST",
            "/api/v1/runs",
            &json!({ "pipeline": NAME, "benchmark": BENCHMARK }),
            &[],
        ),
    )
    .await;
    let status = response.status();
    let body = json(response).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "run_exists");
    let copied = fs::read(launched(&workspace, &hash_of(HYBRID))).unwrap();
    assert_eq!(copied, LAYOUT.as_bytes());
}

#[tokio::test]
async fn a_prefix_run_copies_its_parent_s_layout_under_the_cut_s_hash() {
    let workspace = workspace("prefix", Some(LAYOUT));
    let app = server(&workspace);

    let accepted = launch(&app, Some("fused")).await;
    done(&app, &accepted).await;

    // The cut's hash is the run's pipeline hash, which a fork reads by.
    let run = accepted["run_id"].as_str().unwrap();
    let detail = json(send(app.clone(), get(&format!("/api/v1/runs/{run}"))).await).await;
    let cut = detail["configuration"].as_str().unwrap();
    assert_ne!(hash_of(cut), hash_of(HYBRID));
    let copied = fs::read(launched(&workspace, &hash_of(cut))).expect("the layout was copied");
    assert_eq!(copied, LAYOUT.as_bytes());
    // Nothing under the parent's hash: the parent was not what ran.
    assert!(!launched(&workspace, &hash_of(HYBRID)).exists());
}

#[tokio::test]
async fn a_pipeline_without_a_layout_launches_and_copies_nothing() {
    let workspace = workspace("absent", None);
    let app = server(&workspace);

    let accepted = launch(&app, None).await;
    done(&app, &accepted).await;

    assert!(!launched(&workspace, &hash_of(HYBRID)).exists());
    assert!(faults(&app).await.is_empty());
    let run = accepted["run_id"].as_str().unwrap();
    let at_launch = json(send(app.clone(), get(&format!("/api/v1/runs/{run}/layout"))).await).await;
    assert_eq!(at_launch, json!({ "layout": null }));
}

#[tokio::test]
async fn a_copy_that_fails_is_reported_and_the_run_still_goes_ahead() {
    let workspace = workspace("blocked", Some(LAYOUT));
    // A file where `layouts/` goes: nothing can be written under it.
    fs::remove_dir_all(workspace.layouts()).unwrap();
    fs::write(workspace.layouts(), b"not a directory").unwrap();
    let app = server(&workspace);

    let accepted = launch(&app, None).await;
    done(&app, &accepted).await;

    let faults = faults(&app).await;
    assert_eq!(faults.len(), 1, "{faults:?}");
    let reason = faults[0]["reason"].as_str().unwrap();
    assert!(reason.contains("layout"), "{reason}");
    assert!(reason.contains(NAME), "{reason}");
    assert!(
        faults[0]["path"]
            .as_str()
            .unwrap()
            .contains(accepted["job_id"].as_str().unwrap()),
        "{faults:?}"
    );
}
