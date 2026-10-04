//! The job model and its queue: `POST /runs`, `GET /jobs`, `GET /jobs/{id}`,
//! `PATCH` and `DELETE /jobs/{id}`, `GET /jobs/events` and
//! `POST /benchmarks/{name}/download`, driven over the router against a
//! scripted `Launcher` and a workspace in a scratch directory, whose `jobs/`
//! is read back as the queue wrote it.

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::http::StatusCode;
use futures_util::StreamExt;
use ragondin_api::{
    ApiError, BenchmarkEntry, BenchmarkState, Cancellation, Capabilities, DownloadProgress,
    Launcher, LauncherError, PinnedBenchmark, ProgressSink, QueryProgress, Registry, RunDataset,
    RunObserver, Server, ServiceBinding, ServiceIdentity, Submission,
};
use ragondin_experiments::{
    FileSystemRunStore, Run, RunId, RunStore, Trace, TraceDocument, TraceNode,
};
use ragondin_pipeline::{LogicalPipeline, NodeId};
use ragondin_types::QueryId;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use support::datasets::scratch;
use support::{
    fakes, fixture_run, get, json, router_over, send, write_request, FakeRunStore, HeldPipelines,
};

/// The pipeline every submission names, held by the workspace.
const PIPELINE: &str = "fixture";
/// The second pipeline, so that two submissions announce two ids.
const OTHER_PIPELINE: &str = "other";
const BENCHMARK: &str = "beir/mini";

/// The fixture run's configuration: a document that validates.
fn document() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runs");
    let run = std::fs::read_dir(&root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::read_to_string(run.join("config.yaml")).unwrap()
}

/// The id a submission announces: a digest of what was submitted, so that
/// two submissions of different pipelines announce different ids.
fn announced(submission: &Submission) -> RunId {
    let digest = Sha256::digest(
        format!(
            "{}\n{}\n{:?}",
            submission.pipeline_name, submission.benchmark, submission.up_to
        )
        .as_bytes(),
    );
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    hex.parse().unwrap()
}

/// A trace of one node that took `nanos`.
fn trace(nanos: u64) -> TraceDocument {
    TraceDocument::from(Trace {
        nodes: vec![TraceNode {
            node: NodeId::new("answer"),
            inputs: Vec::new(),
            output: None,
            duration_nanos: nanos,
            error: None,
        }],
    })
}

/// What a [`ScriptedLauncher`] does when it executes.
#[derive(Clone, Default)]
struct Script {
    /// One query per entry, each taking that many nanoseconds in its trace.
    queries: Vec<u64>,
    /// The query, from 1, at which execution fails at node `rerank`.
    fail_at: Option<u64>,
    /// The query, from 1, after whose tick execution waits for the gate.
    gate_after: Option<u64>,
    /// The id the returned run carries, when it is not the announced one.
    decided: Option<RunId>,
    /// The query, from 1, after whose tick execution never returns.
    hang_after: Option<u64>,
    /// Every trace reported is one whose shape does not read.
    malformed: bool,
}

/// A `Launcher` that announces [`announced`] and executes its [`Script`],
/// recording what it saw.
struct ScriptedLauncher {
    script: Script,
    /// Released once per wait at `gate_after`.
    gate: tokio::sync::Semaphore,
    /// `execute` calls in progress, and how often one began while another was.
    active: AtomicUsize,
    overlaps: AtomicUsize,
    /// The pipeline name of every `execute` call, in the order they began.
    executed: Mutex<Vec<String>>,
    /// The workspace, to read `jobs/` as `execute` begins.
    workspace: Option<PathBuf>,
    /// The state each job's file held when `execute` was called on it.
    seen_on_disk: Mutex<Vec<Value>>,
    /// Set once execution hangs.
    hung: AtomicBool,
}

impl ScriptedLauncher {
    fn new(script: Script) -> Self {
        Self {
            script,
            gate: tokio::sync::Semaphore::new(0),
            active: AtomicUsize::new(0),
            overlaps: AtomicUsize::new(0),
            executed: Mutex::default(),
            workspace: None,
            seen_on_disk: Mutex::default(),
            hung: AtomicBool::new(false),
        }
    }
}

impl Default for ScriptedLauncher {
    fn default() -> Self {
        Self::new(Script::default())
    }
}

#[async_trait]
impl Launcher for ScriptedLauncher {
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
            reason: "the scripted launcher probes nothing".to_owned(),
            last_identity: None,
        })
    }

    async fn identity(&self, submission: &Submission) -> Result<RunId, LauncherError> {
        Ok(announced(submission))
    }

    async fn execute(
        &self,
        submission: &Submission,
        observer: Arc<dyn RunObserver>,
        cancel: Cancellation,
    ) -> Result<Run, LauncherError> {
        if self.active.fetch_add(1, Ordering::SeqCst) > 0 {
            self.overlaps.fetch_add(1, Ordering::SeqCst);
        }
        self.executed
            .lock()
            .unwrap()
            .push(submission.pipeline_name.clone());
        if let Some(workspace) = &self.workspace {
            for job in job_files(workspace) {
                self.seen_on_disk.lock().unwrap().push(job);
            }
        }
        let result = self.run(submission, observer.as_ref(), &cancel).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

impl ScriptedLauncher {
    async fn run(
        &self,
        submission: &Submission,
        observer: &dyn RunObserver,
        cancel: &Cancellation,
    ) -> Result<Run, LauncherError> {
        let total = self.script.queries.len() as u64;
        let mut traces = BTreeMap::new();
        for (index, nanos) in self.script.queries.iter().enumerate() {
            let position = index as u64 + 1;
            if self.script.fail_at == Some(position) {
                return Err(LauncherError::Execution {
                    error: "the reranker failed".to_owned(),
                    at_node: Some("rerank".to_owned()),
                });
            }
            let query = QueryId::new(format!("q-{position}"));
            let trace = if self.script.malformed {
                TraceDocument::new(json!({ "nodes": "not a list" }))
            } else {
                trace(*nanos)
            };
            traces.insert(query.clone(), trace.clone());
            observer.query_done(QueryProgress {
                position,
                total,
                query,
                elapsed: Duration::from_millis(position),
                trace,
            });
            if cancel.is_cancelled() {
                return Err(LauncherError::Cancelled);
            }
            if self.script.gate_after == Some(position) {
                self.gate.acquire().await.unwrap().forget();
            }
            if self.script.hang_after == Some(position) {
                self.hung.store(true, Ordering::SeqCst);
                std::future::pending::<()>().await;
            }
        }
        let mut run = fixture_run();
        run.id = self.script.decided.unwrap_or_else(|| announced(submission));
        run.traces = traces;
        Ok(run)
    }
}

/// Every job file under `workspace/jobs/`, parsed.
fn job_files(workspace: &Path) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(workspace.join("jobs")) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "json")
                .then(|| serde_json::from_slice(&std::fs::read(path).ok()?).ok())?
        })
        .collect()
}

fn job_file(workspace: &Path, id: &str) -> Value {
    let path = workspace.join("jobs").join(format!("{id}.json"));
    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
}

/// A registry with one benchmark to download, in four chunks, released by
/// its gate when it has one.
#[derive(Default)]
struct DownloadingRegistry {
    downloads: AtomicUsize,
}

#[async_trait]
impl Registry for DownloadingRegistry {
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
        progress: ProgressSink,
        cancel: Arc<AtomicBool>,
    ) -> Result<BenchmarkEntry, ApiError> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        for received in [100, 200, 300, 400] {
            if cancel.load(Ordering::SeqCst) {
                return Err(ApiError::DownloadCancelled {
                    name: name.to_owned(),
                });
            }
            progress(DownloadProgress {
                received,
                total: 400,
            });
        }
        Ok(BenchmarkEntry {
            name: name.to_owned(),
            format: "beir".to_owned(),
            state: BenchmarkState::Ready {
                dataset_version: "digest".to_owned(),
            },
            ground_truth: None,
            licence: None,
            licence_url: None,
        })
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

/// The server over a workspace at `workspace`, its runs in `runs/` there,
/// the two pipelines held, `launcher` executing.
fn server(workspace: &Path, launcher: Arc<ScriptedLauncher>) -> Server {
    server_with(
        workspace,
        launcher,
        Arc::new(DownloadingRegistry::default()),
    )
}

fn server_with(
    workspace: &Path,
    launcher: Arc<ScriptedLauncher>,
    registry: Arc<dyn Registry>,
) -> Server {
    let mut backends = fakes(FakeRunStore::default());
    backends.runs = Arc::new(FileSystemRunStore::new(workspace.join("runs")));
    backends.pipelines = Arc::new(HeldPipelines {
        files: vec![
            (PIPELINE.to_owned(), document()),
            (OTHER_PIPELINE.to_owned(), document()),
        ],
    });
    backends.registry = registry;
    backends.launcher = launcher;
    router_over(backends, workspace)
}

fn submit(pipeline: &str) -> axum::http::Request<axum::body::Body> {
    write_request(
        "POST",
        "/api/v1/runs",
        &json!({ "pipeline": pipeline, "benchmark": BENCHMARK }),
        &[],
    )
}

fn submission(pipeline: &str) -> Submission {
    Submission {
        pipeline_name: pipeline.to_owned(),
        pipeline: document(),
        benchmark: BENCHMARK.to_owned(),
        bindings: Vec::new(),
        up_to: None,
    }
}

/// Submits `pipeline` and returns the job id, asserting the `202`.
async fn accepted(app: &Server, pipeline: &str) -> String {
    let response = send(app.clone(), submit(pipeline)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = json(response).await;
    body["job_id"].as_str().unwrap().to_owned()
}

/// `GET /jobs/{id}`'s body, polled until `until` holds, for at most 5 s.
async fn job_until(app: &Server, id: &str, until: impl Fn(&Value) -> bool) -> Value {
    let mut job = Value::Null;
    for _ in 0..500 {
        job = json(send(app.clone(), get(&format!("/api/v1/jobs/{id}"))).await).await;
        if until(&job) {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the job never reached the state awaited: {job}");
}

fn kind(job: &Value) -> &str {
    job["state"]["kind"].as_str().unwrap_or("")
}

fn finished(job: &Value) -> bool {
    matches!(kind(job), "done" | "failed" | "cancelled")
}

/// One server-sent event: its id, its name and its data, parsed.
#[derive(Clone, Debug)]
struct Event {
    id: String,
    name: String,
    data: Value,
}

/// The events `GET /jobs/events` sends, from `last_event_id` when given,
/// until `until` holds of those received, for at most 5 s.
async fn events(
    app: &Server,
    last_event_id: Option<&str>,
    until: impl Fn(&[Event]) -> bool,
) -> Vec<Event> {
    let mut request = get("/api/v1/jobs/events");
    if let Some(id) = last_event_id {
        request
            .headers_mut()
            .insert("last-event-id", id.parse().unwrap());
    }
    let response = send(app.clone(), request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/event-stream",
        "an event stream"
    );
    let mut stream = response.into_body().into_data_stream();
    let mut text = String::new();
    let mut received = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !until(&received) {
        let chunk = tokio::time::timeout_at(deadline, stream.next())
            .await
            .unwrap_or_else(|_| panic!("the stream stopped short: {received:#?}"))
            .expect("the stream stays open")
            .unwrap();
        text.push_str(std::str::from_utf8(&chunk).unwrap());
        while let Some(end) = text.find("\n\n") {
            let frame: String = text.drain(..end + 2).collect();
            let field = |name: &str| {
                frame
                    .lines()
                    .find_map(|line| line.strip_prefix(&format!("{name}: ")))
                    .map(str::to_owned)
            };
            if let (Some(id), Some(name), Some(data)) = (field("id"), field("event"), field("data"))
            {
                received.push(Event {
                    id,
                    name,
                    data: serde_json::from_str(&data).unwrap(),
                });
            }
        }
    }
    received
}

/// The events about job `id`, by name.
fn names_of(events: &[Event], id: &str) -> Vec<String> {
    events
        .iter()
        .filter(|event| event.data["id"] == id)
        .map(|event| event.name.clone())
        .collect()
}

#[tokio::test]
async fn a_submission_is_announced_queued_run_and_filed_as_one_block() {
    let workspace = scratch("jobs-announced-queued-run-filed");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100, 300, 200],
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));
    let announced_id = announced(&submission(PIPELINE)).to_string();

    // The stream opens before the submission, so it sees the job queued.
    let watcher = {
        let app = app.clone();
        tokio::spawn(async move {
            events(&app, None, |events| {
                events.iter().any(|event| event.name == "done")
            })
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;

    let response = send(app.clone(), submit(PIPELINE)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = json(response).await;
    assert_eq!(body["run_id"], announced_id);
    let id = body["job_id"].as_str().unwrap().to_owned();

    let events = watcher.await.unwrap();
    assert_eq!(
        names_of(&events, &id),
        ["queued", "running", "running", "running", "running", "done"],
        "{events:#?}"
    );
    let ticks: Vec<&Value> = events
        .iter()
        .filter(|event| event.name == "running")
        .map(|event| &event.data["state"]["done"])
        .collect();
    assert_eq!(ticks, [&json!(0), &json!(1), &json!(2), &json!(3)]);
    let done = events.iter().find(|event| event.name == "done").unwrap();
    assert_eq!(done.data["state"]["run_id"], announced_id);
    assert_eq!(done.data["state"]["id_mismatch"], Value::Null);

    let stored = FileSystemRunStore::new(workspace.join("runs"))
        .load(&announced_id.parse().unwrap())
        .expect("the run is filed under its id");
    assert_eq!(stored.traces.len(), 3);

    let file = job_file(&workspace, &id);
    assert_eq!(file["state"]["kind"], "done");
    assert_eq!(file["work"]["run_id"], announced_id);
    assert_eq!(file["work"]["pipeline_name"], PIPELINE);
    assert_eq!(file["work"]["pipeline"], document());
    let history: Vec<&str> = file["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|transition| transition["state"].as_str().unwrap())
        .collect();
    assert_eq!(history, ["queued", "running", "done"]);
}

#[tokio::test]
async fn job_times_are_unix_millis_and_the_live_median_is_the_trace_latency_s_lower_median() {
    let workspace = scratch("jobs-times-and-median");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100, 300, 200],
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let watcher = {
        let app = app.clone();
        tokio::spawn(async move {
            events(&app, None, |events| events.iter().any(|e| e.name == "done")).await
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    let id = accepted(&app, PIPELINE).await;
    let events = watcher.await.unwrap();

    let medians: Vec<&Value> = events
        .iter()
        .filter(|event| event.name == "running" && event.data["state"]["done"] != 0)
        .map(|event| &event.data["state"]["median_latency_nanos"])
        .collect();
    assert_eq!(medians, [&json!(100), &json!(100), &json!(200)]);

    let file = job_file(&workspace, &id);
    let created = file["created_at"].as_u64().expect("an integer");
    assert!(
        created >= before,
        "{created} is milliseconds since the epoch"
    );
    let running = events.iter().find(|event| event.name == "running").unwrap();
    let started = running.data["state"]["started_at_ms"].as_u64().unwrap();
    assert!(started >= created);
    for transition in file["history"].as_array().unwrap() {
        assert!(transition["at"].as_u64().unwrap() >= created, "{file}");
    }
    assert!(file["state"]["finished_at"].as_u64().unwrap() >= started);
}

#[tokio::test]
async fn a_submission_whose_id_exists_is_refused_with_the_run_or_the_job() {
    let workspace = scratch("jobs-id-exists");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100],
        gate_after: Some(1),
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));

    let running = accepted(&app, PIPELINE).await;
    let queued = accepted(&app, OTHER_PIPELINE).await;
    job_until(&app, &running, |job| kind(job) == "running").await;

    for (pipeline, job) in [(PIPELINE, &running), (OTHER_PIPELINE, &queued)] {
        let response = send(app.clone(), submit(pipeline)).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let problem = json(response).await;
        assert_eq!(problem["code"], "run_exists");
        assert_eq!(problem["link"], format!("/api/v1/jobs/{job}"));
    }

    launcher.gate.add_permits(2);
    job_until(&app, &queued, finished).await;
    let run_id = announced(&submission(PIPELINE)).to_string();
    let response = send(app.clone(), submit(PIPELINE)).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let problem = json(response).await;
    assert_eq!(problem["code"], "run_exists");
    assert_eq!(problem["link"], format!("/api/v1/runs/{run_id}"));
}

#[tokio::test]
async fn cancelling_a_queued_job_never_executes_it() {
    let workspace = scratch("jobs-cancel-queued");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100],
        gate_after: Some(1),
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));
    let running = accepted(&app, PIPELINE).await;
    let queued = accepted(&app, OTHER_PIPELINE).await;
    job_until(&app, &running, |job| kind(job) == "running").await;

    let response = send(
        app.clone(),
        write_request(
            "DELETE",
            &format!("/api/v1/jobs/{queued}"),
            &json!(null),
            &[],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cancelled = json(response).await;
    assert_eq!(kind(&cancelled), "cancelled");
    assert_eq!(
        cancelled["state"]["partial_traces"], 0,
        "it executed nothing"
    );

    launcher.gate.add_permits(1);
    job_until(&app, &running, finished).await;
    // The worker had its chance at the queue: nothing else was taken.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(*launcher.executed.lock().unwrap(), [PIPELINE]);
    assert_eq!(job_file(&workspace, &queued)["state"]["kind"], "cancelled");
    assert!(
        !workspace.join("jobs").join(&queued).exists(),
        "no partial traces"
    );
}

#[tokio::test]
async fn cancelling_the_running_job_keeps_its_partial_traces_and_files_no_run() {
    let workspace = scratch("jobs-cancel-running");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100, 200, 300],
        gate_after: Some(1),
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));
    let id = accepted(&app, PIPELINE).await;
    job_until(&app, &id, |job| job["state"]["done"] == 1).await;

    let response = send(
        app.clone(),
        write_request("DELETE", &format!("/api/v1/jobs/{id}"), &json!(null), &[]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    launcher.gate.add_permits(1);

    let job = job_until(&app, &id, finished).await;
    assert_eq!(kind(&job), "cancelled", "{job}");
    let partial: BTreeMap<String, Value> = serde_json::from_slice(
        &std::fs::read(workspace.join("jobs").join(&id).join("partial/traces.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(partial.keys().collect::<Vec<_>>(), ["q-1", "q-2"]);
    assert_eq!(job["state"]["partial_traces"], 2, "{job}");
    assert_eq!(job_file(&workspace, &id)["state"]["partial_traces"], 2);
    assert_eq!(
        FileSystemRunStore::new(workspace.join("runs"))
            .ids()
            .unwrap(),
        Vec::<RunId>::new()
    );
}

#[tokio::test]
async fn a_failing_execution_keeps_the_error_the_node_and_the_partial_traces() {
    let workspace = scratch("jobs-failing-execution");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100, 200, 300],
        fail_at: Some(2),
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));
    let id = accepted(&app, PIPELINE).await;

    let job = job_until(&app, &id, finished).await;
    assert_eq!(kind(&job), "failed", "{job}");
    assert_eq!(job["state"]["error"], "the reranker failed");
    assert_eq!(job["state"]["at_node"], "rerank");
    let file = job_file(&workspace, &id);
    assert_eq!(file["state"]["at_node"], "rerank");
    let partial: BTreeMap<String, Value> = serde_json::from_slice(
        &std::fs::read(workspace.join("jobs").join(&id).join("partial/traces.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(partial.keys().collect::<Vec<_>>(), ["q-1"]);
    assert_eq!(job["state"]["partial_traces"], 1, "{job}");
    assert_eq!(file["state"]["partial_traces"], 1);
    assert_eq!(
        FileSystemRunStore::new(workspace.join("runs"))
            .ids()
            .unwrap(),
        Vec::<RunId>::new()
    );
}

#[tokio::test]
async fn a_decided_id_that_differs_from_the_announced_one_is_reported_and_filed_under_the_decided_id(
) {
    let workspace = scratch("jobs-decided-id-differs");
    let decided: RunId = "00000000000000000000000000000000000000000000000000000000000000dd"
        .parse()
        .unwrap();
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100],
        decided: Some(decided),
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));
    let announced_id = announced(&submission(PIPELINE)).to_string();
    let id = accepted(&app, PIPELINE).await;

    let job = job_until(&app, &id, finished).await;
    assert_eq!(kind(&job), "done", "{job}");
    assert_eq!(job["state"]["run_id"], decided.to_string());
    assert_eq!(
        job["state"]["id_mismatch"],
        json!({ "announced": announced_id, "decided": decided.to_string() })
    );
    assert_eq!(
        job_file(&workspace, &id)["state"]["id_mismatch"]["announced"],
        announced_id
    );
    let store = FileSystemRunStore::new(workspace.join("runs"));
    assert_eq!(store.ids().unwrap(), [decided]);
    let response = send(app.clone(), get(&format!("/api/v1/runs/{announced_id}"))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn two_runs_never_overlap_and_a_download_runs_alongside() {
    let workspace = scratch("jobs-no-overlap-download-alongside");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100, 200],
        gate_after: Some(1),
        ..Script::default()
    }));
    let registry = Arc::new(DownloadingRegistry::default());
    let app = server_with(&workspace, Arc::clone(&launcher), registry.clone());
    let first = accepted(&app, PIPELINE).await;
    let second = accepted(&app, OTHER_PIPELINE).await;
    job_until(&app, &first, |job| kind(job) == "running").await;

    // The run worker is held; the download is not behind it.
    let response = send(
        app.clone(),
        write_request(
            "POST",
            "/api/v1/benchmarks/beir%2Fmini/download",
            &json!(null),
            &[],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let download = json(response).await["job_id"].as_str().unwrap().to_owned();
    let done = job_until(&app, &download, finished).await;
    assert_eq!(kind(&done), "done", "{done}");
    assert_eq!(
        done["work"],
        json!({ "kind": "download", "benchmark": "beir/mini" })
    );
    assert_eq!(registry.downloads.load(Ordering::SeqCst), 1);
    let first_now = json(send(app.clone(), get(&format!("/api/v1/jobs/{first}"))).await).await;
    assert_eq!(kind(&first_now), "running");
    assert_eq!(
        kind(&json(send(app.clone(), get(&format!("/api/v1/jobs/{second}"))).await).await),
        "queued"
    );

    launcher.gate.add_permits(2);
    job_until(&app, &second, finished).await;
    assert_eq!(launcher.overlaps.load(Ordering::SeqCst), 0);
    assert_eq!(
        *launcher.executed.lock().unwrap(),
        [PIPELINE, OTHER_PIPELINE]
    );
}

#[tokio::test]
async fn a_trace_whose_latency_does_not_read_is_left_out_of_the_median_and_reported() {
    let workspace = scratch("jobs-unread-latency");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100, 200],
        malformed: true,
        ..Script::default()
    }));
    let app = server(&workspace, launcher);
    let id = accepted(&app, PIPELINE).await;

    let job = job_until(&app, &id, finished).await;
    assert_eq!(kind(&job), "done", "{job}");
    let listing = json(send(app.clone(), get("/api/v1/jobs")).await).await;
    let faults = listing["faults"].as_array().unwrap();
    assert_eq!(faults.len(), 1, "{listing}");
    assert!(faults[0]["path"]
        .as_str()
        .unwrap()
        .ends_with(&format!("{id}.json")));
    assert!(faults[0]["reason"]
        .as_str()
        .unwrap()
        .contains("2 of its traces"));
}

#[tokio::test]
async fn a_download_s_progress_is_bytes() {
    let workspace = scratch("jobs-download-progress");
    let launcher = Arc::new(ScriptedLauncher::default());
    let app = server(&workspace, launcher);
    let watcher = {
        let app = app.clone();
        tokio::spawn(async move {
            events(&app, None, |events| events.iter().any(|e| e.name == "done")).await
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    let response = send(
        app.clone(),
        write_request(
            "POST",
            "/api/v1/benchmarks/beir%2Fmini/download",
            &json!(null),
            &[],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let events = watcher.await.unwrap();
    let last = events
        .iter()
        .rfind(|event| event.name == "running")
        .unwrap();
    assert_eq!(last.data["state"]["done"], 400);
    assert_eq!(last.data["state"]["total"], 400);
}

#[tokio::test]
async fn reordering_a_queued_job_changes_the_worker_s_order() {
    let workspace = scratch("jobs-reorder");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100],
        gate_after: Some(1),
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));
    let first = accepted(&app, PIPELINE).await;
    job_until(&app, &first, |job| kind(job) == "running").await;
    // A third pipeline name only the fake needs: the workspace holds two, so
    // the first is resubmitted under an id of its own once it is done.
    let second = accepted(&app, OTHER_PIPELINE).await;
    let fixture_up_to = write_request(
        "POST",
        "/api/v1/runs",
        &json!({ "pipeline": PIPELINE, "benchmark": BENCHMARK, "up_to": "context" }),
        &[],
    );
    let response = send(app.clone(), fixture_up_to).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let third = json(response).await["job_id"].as_str().unwrap().to_owned();

    let response = send(
        app.clone(),
        write_request(
            "PATCH",
            &format!("/api/v1/jobs/{third}"),
            &json!({ "position": 0 }),
            &[],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let listing = json(response).await;
    let order: Vec<&str> = listing["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|job| job["id"].as_str().unwrap())
        .collect();
    assert_eq!(order, [first.as_str(), third.as_str(), second.as_str()]);

    // A job that is not queued is not reordered.
    let response = send(
        app.clone(),
        write_request(
            "PATCH",
            &format!("/api/v1/jobs/{first}"),
            &json!({ "position": 0 }),
            &[],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json(response).await["code"], "job_not_queued");

    launcher.gate.add_permits(3);
    job_until(&app, &second, finished).await;
    let executed = launcher.executed.lock().unwrap().clone();
    assert_eq!(executed, [PIPELINE, PIPELINE, OTHER_PIPELINE]);
    // The order is on disk, so it survives a restart.
    assert!(
        job_file(&workspace, &third)["position"].as_u64()
            < job_file(&workspace, &second)["position"].as_u64()
    );
}

#[tokio::test]
async fn an_unknown_job_is_job_not_found() {
    let workspace = scratch("jobs-unknown");
    let app = server(&workspace, Arc::new(ScriptedLauncher::default()));
    for request in [
        get("/api/v1/jobs/nothing"),
        write_request("DELETE", "/api/v1/jobs/nothing", &json!(null), &[]),
        write_request(
            "PATCH",
            "/api/v1/jobs/nothing",
            &json!({ "position": 0 }),
            &[],
        ),
    ] {
        let response = send(app.clone(), request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(json(response).await["code"], "job_not_found");
    }
}

/// A job file as the queue writes one, in `state`, at `position`.
fn stored_job(id: &str, position: u64, state: Value, history: &[&str]) -> Value {
    json!({
        "id": id,
        "position": position,
        "created_at": 1_700_000_000_000u64,
        "work": {
            "kind": "run",
            "run_id": announced(&submission(id)).to_string(),
            "pipeline_name": id,
            "pipeline": document(),
            "benchmark": BENCHMARK,
            "bindings": [],
            "up_to": null,
        },
        "state": state,
        "history": history
            .iter()
            .map(|state| json!({ "state": state, "at": 1_700_000_000_000u64 }))
            .collect::<Vec<_>>(),
    })
}

/// A crash between filing the run and writing the job's end: the run is
/// stored under the announced id, which no other job could have filed
/// (`run_exists` refuses a second), so the job is done, not interrupted.
#[tokio::test]
async fn restart_finds_a_running_job_whose_run_is_filed_done() {
    let workspace = scratch("jobs-restart-run-filed");
    let jobs = workspace.join("jobs");
    std::fs::create_dir_all(&jobs).unwrap();
    let running = stored_job(
        "was-filing",
        1,
        json!({
            "kind": "running",
            "done": 1,
            "total": 1,
            "started_at": 1_700_000_000_500u64,
            "median_latency_nanos": 100,
        }),
        &["queued", "running"],
    );
    std::fs::write(
        jobs.join("was-filing.json"),
        serde_json::to_vec(&running).unwrap(),
    )
    .unwrap();
    let run_id = announced(&submission("was-filing"));
    let mut run = fixture_run();
    run.id = run_id;
    FileSystemRunStore::new(workspace.join("runs"))
        .save(&run)
        .unwrap();

    let app = server(&workspace, Arc::new(ScriptedLauncher::default()));

    let job = json(send(app, get("/api/v1/jobs/was-filing")).await).await;
    assert_eq!(kind(&job), "done", "{job}");
    assert_eq!(job["state"]["run_id"], run_id.to_string());
    let file = job_file(&workspace, "was-filing");
    assert_eq!(file["state"]["kind"], "done");
    let history: Vec<&str> = file["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|transition| transition["state"].as_str().unwrap())
        .collect();
    assert_eq!(history, ["queued", "running", "done"]);
}

/// A job whose `running` cannot be written is never executed by this
/// process: failed, with the failure written when the disk allows it, and
/// otherwise held in memory and reported — and then a restart finds it
/// queued, and runs it.
#[cfg(unix)]
#[tokio::test]
async fn a_job_whose_running_cannot_be_written_is_not_executed_and_a_restart_resumes_it() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = scratch("jobs-running-unwritable");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100],
        gate_after: Some(1),
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));
    let first = accepted(&app, PIPELINE).await;
    job_until(&app, &first, |job| kind(job) == "running").await;
    let second = accepted(&app, OTHER_PIPELINE).await;

    /// `jobs/` read-only while it lives, writable again when it drops —
    /// a failing assertion included, so the scratch directory stays usable.
    struct ReadOnly(PathBuf);
    impl Drop for ReadOnly {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let jobs = workspace.join("jobs");
    std::fs::set_permissions(&jobs, std::fs::Permissions::from_mode(0o555)).unwrap();
    let read_only = ReadOnly(jobs.clone());
    // A user the permissions do not bind — root — can still write there, and
    // nothing here can make the write fail: the test has nothing to check.
    let probe = jobs.join("probe");
    if std::fs::write(&probe, b"").is_ok() {
        let _ = std::fs::remove_file(&probe);
        launcher.gate.add_permits(1);
        eprintln!("skipped: a read-only directory is writable for this user");
        return;
    }
    launcher.gate.add_permits(1);
    let failed = job_until(&app, &second, finished).await;
    drop(read_only);

    assert_eq!(kind(&failed), "failed", "{failed}");
    assert!(failed["state"]["error"]
        .as_str()
        .unwrap()
        .contains("could not be started"));
    assert_eq!(*launcher.executed.lock().unwrap(), [PIPELINE]);
    let listing = json(send(app.clone(), get("/api/v1/jobs")).await).await;
    let reported = listing["faults"].as_array().unwrap().iter().any(|fault| {
        fault["path"]
            .as_str()
            .unwrap()
            .ends_with(&format!("{second}.json"))
            && fault["reason"]
                .as_str()
                .unwrap()
                .contains("a restart finds it queued")
    });
    assert!(reported, "{listing}");
    assert_eq!(job_file(&workspace, &second)["state"]["kind"], "queued");

    let rerun = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100],
        ..Script::default()
    }));
    let restarted = server(&workspace, Arc::clone(&rerun));
    let job = job_until(&restarted, &second, finished).await;
    assert_eq!(kind(&job), "done", "{job}");
    assert_eq!(*rerun.executed.lock().unwrap(), [OTHER_PIPELINE]);
}

#[tokio::test]
async fn restart_marks_the_running_job_interrupted_and_resumes_the_queue() {
    let workspace = scratch("jobs-restart");
    let jobs = workspace.join("jobs");
    std::fs::create_dir_all(&jobs).unwrap();
    let running = stored_job(
        "was-running",
        1,
        json!({
            "kind": "running",
            "done": 3,
            "total": 10,
            "started_at": 1_700_000_000_500u64,
            "median_latency_nanos": 100,
        }),
        &["queued", "running"],
    );
    let queued = stored_job("was-queued", 2, json!({ "kind": "queued" }), &["queued"]);
    for job in [&running, &queued] {
        std::fs::write(
            jobs.join(format!("{}.json", job["id"].as_str().unwrap())),
            serde_json::to_vec(job).unwrap(),
        )
        .unwrap();
    }
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100],
        gate_after: Some(1),
        ..Script::default()
    }));

    let app = server(&workspace, Arc::clone(&launcher));

    let listing = json(send(app.clone(), get("/api/v1/jobs")).await).await;
    let jobs: Vec<(&str, &str)> = listing["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|job| (job["id"].as_str().unwrap(), kind(job)))
        .collect();
    assert_eq!(jobs[0], ("was-running", "failed"));
    assert_eq!(jobs[1].0, "was-queued");
    assert!(matches!(jobs[1].1, "queued" | "running"), "{listing}");
    assert_eq!(listing["jobs"][0]["state"]["error"], "interrupted");
    // A crash leaves no partial traces: they are written when a run stops.
    assert_eq!(listing["jobs"][0]["state"]["partial_traces"], 0);
    let file = job_file(&workspace, "was-running");
    assert_eq!(file["state"]["kind"], "failed");
    assert_eq!(file["state"]["error"], "interrupted");

    job_until(&app, "was-queued", |job| kind(job) == "running").await;
    launcher.gate.add_permits(1);
    job_until(&app, "was-queued", finished).await;
    assert_eq!(*launcher.executed.lock().unwrap(), ["was-queued"]);
}

#[tokio::test]
async fn a_job_file_that_does_not_read_is_reported_never_repaired() {
    let workspace = scratch("jobs-unreadable-file");
    let jobs = workspace.join("jobs");
    std::fs::create_dir_all(&jobs).unwrap();
    std::fs::write(jobs.join("torn.json"), b"{ \"id\": ").unwrap();

    let app = server(&workspace, Arc::new(ScriptedLauncher::default()));

    let listing = json(send(app.clone(), get("/api/v1/jobs")).await).await;
    assert_eq!(listing["jobs"], json!([]));
    let faults = listing["faults"].as_array().unwrap();
    assert_eq!(faults.len(), 1, "{listing}");
    assert!(faults[0]["path"].as_str().unwrap().ends_with("torn.json"));
    assert_eq!(
        std::fs::read(jobs.join("torn.json")).unwrap(),
        b"{ \"id\": "
    );
}

#[tokio::test]
async fn the_event_stream_resumes_from_the_last_event_id() {
    let workspace = scratch("jobs-event-stream-resumes");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100, 200],
        gate_after: Some(1),
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));

    // A first connection sees the job until its first tick, then drops.
    let watcher = {
        let app = app.clone();
        tokio::spawn(async move {
            events(&app, None, |events| {
                events
                    .iter()
                    .any(|event| event.name == "running" && event.data["state"]["done"] == 1)
            })
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    let id = accepted(&app, PIPELINE).await;
    let first = watcher.await.unwrap();
    let last = first.last().unwrap().id.clone();

    // What happens while no one is connected.
    launcher.gate.add_permits(1);
    job_until(&app, &id, finished).await;

    let resumed = events(&app, Some(&last), |events| {
        events.iter().any(|event| event.name == "done")
    })
    .await;
    assert_eq!(
        names_of(&resumed, &id),
        ["running", "done"],
        "the tick and the end it missed, and nothing twice: {resumed:#?}"
    );
    assert_eq!(resumed[0].data["state"]["done"], 2);
    let seen: Vec<&str> = first.iter().map(|event| event.id.as_str()).collect();
    assert!(resumed
        .iter()
        .all(|event| !seen.contains(&event.id.as_str())));
}

#[tokio::test]
async fn a_stream_opened_without_a_last_event_id_starts_from_the_whole_queue() {
    let workspace = scratch("jobs-event-stream-resync");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100],
        gate_after: Some(1),
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));
    let id = accepted(&app, PIPELINE).await;
    job_until(&app, &id, |job| kind(job) == "running").await;

    for last in [None, Some("from-another-server-7")] {
        let received = events(&app, last, |events| !events.is_empty()).await;
        assert_eq!(received[0].name, "resync", "{received:#?}");
        assert_eq!(received[0].data["jobs"][0]["id"], id);
    }
    launcher.gate.add_permits(1);
}

#[test]
fn every_transition_is_on_disk_before_the_next_one_starts() {
    let workspace = scratch("jobs-transitions-on-disk");
    let mut scripted = ScriptedLauncher::new(Script {
        queries: vec![100, 200],
        hang_after: Some(1),
        ..Script::default()
    });
    scripted.workspace = Some(workspace.clone());
    let launcher = Arc::new(scripted);

    // The service on a runtime of its own, shut down while the worker is
    // inside `execute`: between the `running` transition and the next.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let id = runtime.block_on(async {
        let app = server(&workspace, Arc::clone(&launcher));
        let id = accepted(&app, PIPELINE).await;
        while !launcher.hung.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        id
    });
    runtime.shutdown_background();

    // `execute` was called after `running` was written, never before.
    let seen = launcher.seen_on_disk.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:#?}");
    assert_eq!(seen[0]["state"]["kind"], "running");
    assert_eq!(seen[0]["state"]["done"], 0);

    let file = job_file(&workspace, &id);
    assert_eq!(file["state"]["kind"], "running", "{file}");
    let history: Vec<&str> = file["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|transition| transition["state"].as_str().unwrap())
        .collect();
    assert_eq!(history, ["queued", "running"]);

    // Whoever starts the service next finds it interrupted.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let app = server(&workspace, Arc::new(ScriptedLauncher::default()));
        let job = json(send(app, get(&format!("/api/v1/jobs/{id}"))).await).await;
        assert_eq!(kind(&job), "failed");
        assert_eq!(job["state"]["error"], "interrupted");
    });
}

/// A failed run job, its traces for queries 1 and 2 kept, and the server
/// over it: the reranker fails on query 3.
async fn failed_at_the_third_query(name: &str) -> (PathBuf, Server, String) {
    let workspace = scratch(name);
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100, 200, 300],
        fail_at: Some(3),
        ..Script::default()
    }));
    let app = server(&workspace, launcher);
    let id = accepted(&app, PIPELINE).await;
    let job = job_until(&app, &id, finished).await;
    assert_eq!(kind(&job), "failed", "{job}");
    (workspace, app, id)
}

/// The job view's count and Replay over the job read one record: a failed
/// job's partial traces, listed with the graph its snapshot lowers to, and
/// each read in the node shape `GET /runs/{id}/trace/{query}` serves.
#[tokio::test]
async fn a_failed_job_s_partial_traces_are_listed_and_read_query_by_query() {
    let (_workspace, app, id) = failed_at_the_third_query("jobs-partial-served").await;

    let response = send(app.clone(), get(&format!("/api/v1/jobs/{id}/queries"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let listed = json(response).await;
    assert_eq!(listed["job"]["id"], id.as_str());
    assert_eq!(listed["job"]["state"]["kind"], "failed");
    assert_eq!(listed["job"]["state"]["at_node"], "rerank");
    assert_eq!(listed["job"]["state"]["partial_traces"], 2);
    let nodes: Vec<&str> = listed["graph"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap())
        .collect();
    assert_eq!(nodes, ["answer", "context", "fused", "leg_a", "leg_b"]);
    // Nothing recorded the dataset these traces ran on, so nothing is scored
    // and no text is read (ADR-C36 § 4).
    assert_eq!(
        listed["queries"],
        json!([
            { "id": "q-1", "text": null, "scores": {}, "duration_nanos": 100 },
            { "id": "q-2", "text": null, "scores": {}, "duration_nanos": 200 },
        ])
    );

    let response = send(app.clone(), get(&format!("/api/v1/jobs/{id}/trace/q-2"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json(response).await,
        json!({
            "job": id,
            "query": "q-2",
            "nodes": [{
                "node": "answer",
                "inputs": [],
                "output": null,
                "duration_nanos": 200,
                "error": null,
                "metrics": null,
                "gold_ranks": null,
            }],
        })
    );
}

/// A job that has no partial traces says why with a stable code: one still
/// queued or running has not written them yet; a done one filed its run; a
/// crash left none; and a query the traces do not hold is not found.
#[tokio::test]
async fn a_job_without_partial_traces_answers_a_stable_code() {
    async fn problem(app: &Server, path: &str) -> (StatusCode, Value) {
        let response = send(app.clone(), get(path)).await;
        (response.status(), json(response).await)
    }

    let (_workspace, app, failed) = failed_at_the_third_query("jobs-partial-codes").await;
    let (status, body) = problem(&app, &format!("/api/v1/jobs/{failed}/trace/q-3")).await;
    assert_eq!(
        (status, &body["code"]),
        (StatusCode::NOT_FOUND, &json!("query_not_found"))
    );
    assert_eq!(
        body["hint"],
        format!("Pick a query from GET /jobs/{failed}/queries.")
    );
    let (status, body) = problem(&app, "/api/v1/jobs/no-such-job/queries").await;
    assert_eq!(
        (status, &body["code"]),
        (StatusCode::NOT_FOUND, &json!("job_not_found"))
    );

    let workspace = scratch("jobs-partial-codes-running");
    let launcher = Arc::new(ScriptedLauncher::new(Script {
        queries: vec![100, 200],
        gate_after: Some(1),
        ..Script::default()
    }));
    let app = server(&workspace, Arc::clone(&launcher));
    let running = accepted(&app, PIPELINE).await;
    job_until(&app, &running, |job| job["state"]["done"] == 1).await;
    for path in [
        format!("/api/v1/jobs/{running}/queries"),
        format!("/api/v1/jobs/{running}/trace/q-1"),
    ] {
        let (status, body) = problem(&app, &path).await;
        assert_eq!(
            (status, &body["code"]),
            (StatusCode::CONFLICT, &json!("job_not_ended")),
            "{path}"
        );
    }
    launcher.gate.add_permits(1);
    let done = job_until(&app, &running, finished).await;
    assert_eq!(kind(&done), "done", "{done}");
    let (status, body) = problem(&app, &format!("/api/v1/jobs/{running}/queries")).await;
    assert_eq!(
        (status, &body["code"]),
        (StatusCode::NOT_FOUND, &json!("no_partial_traces"))
    );

    let workspace = scratch("jobs-partial-codes-crashed");
    std::fs::create_dir_all(workspace.join("jobs")).unwrap();
    let crashed = stored_job(
        "was-running",
        1,
        json!({ "kind": "running", "done": 1, "total": 2, "started_at": null, "median_latency_nanos": null }),
        &["queued", "running"],
    );
    std::fs::write(
        workspace.join("jobs/was-running.json"),
        serde_json::to_vec(&crashed).unwrap(),
    )
    .unwrap();
    let app = server(&workspace, Arc::new(ScriptedLauncher::default()));
    let (status, body) = problem(&app, "/api/v1/jobs/was-running/trace/q-1").await;
    assert_eq!(
        (status, &body["code"]),
        (StatusCode::NOT_FOUND, &json!("no_partial_traces"))
    );
}

/// A job file written before the count was recorded: the count is read from
/// the traces it kept, and a job that kept none says none.
#[tokio::test]
async fn a_job_file_without_the_count_reads_it_from_its_partial_traces() {
    let workspace = scratch("jobs-partial-count-recovered");
    let jobs = workspace.join("jobs");
    std::fs::create_dir_all(jobs.join("kept/partial")).unwrap();
    let ended = |id: &str, position| {
        stored_job(
            id,
            position,
            json!({ "kind": "failed", "error": "the reranker failed", "at_node": "rerank", "finished_at": null }),
            &["queued", "running", "failed"],
        )
    };
    for job in [ended("kept", 1), ended("none", 2)] {
        std::fs::write(
            jobs.join(format!("{}.json", job["id"].as_str().unwrap())),
            serde_json::to_vec(&job).unwrap(),
        )
        .unwrap();
    }
    let traces: BTreeMap<QueryId, TraceDocument> = [
        (QueryId::new("q-1"), trace(100)),
        (QueryId::new("q-2"), trace(200)),
    ]
    .into();
    std::fs::write(
        jobs.join("kept/partial/traces.json"),
        serde_json::to_vec(&traces).unwrap(),
    )
    .unwrap();

    let app = server(&workspace, Arc::new(ScriptedLauncher::default()));

    let kept = json(send(app.clone(), get("/api/v1/jobs/kept")).await).await;
    assert_eq!(kept["state"]["partial_traces"], 2, "{kept}");
    let none = json(send(app.clone(), get("/api/v1/jobs/none")).await).await;
    assert_eq!(none["state"]["partial_traces"], 0, "{none}");
    let listed = json(send(app.clone(), get("/api/v1/jobs/kept/queries")).await).await;
    assert_eq!(
        listed["queries"].as_array().map(Vec::len),
        Some(2),
        "{listed}"
    );
}
