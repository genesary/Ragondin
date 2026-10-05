//! "Run up to a node": `POST /runs` with `up_to` cuts the workspace document
//! at that node on the wire schema, validates the cut, and queues it as an
//! ordinary run whose identity is the cut's own hash — the parent recorded
//! beside it as provenance, never in its identity (ADR-C39). And the prefix
//! relation `GET /runs` serves: the run's record, and the structural test
//! over every current document.

mod support;

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::http::StatusCode;
use ragondin_api::{
    ApiError, BenchmarkEntry, BenchmarkState, Cancellation, Capabilities, GroundTruth, Launcher,
    LauncherError, PinnedBenchmark, ProgressSink, Registry, RunDataset, RunObserver, Server,
    ServiceBinding, ServiceIdentity, Submission,
};
use ragondin_benchmarks::{Benchmark, Qrels};
use ragondin_config::{parse_document, ConfigSource, LocalFile};
use ragondin_experiments::{PrefixOf, Run, RunId, RunProvenance};
use ragondin_pipeline::LogicalPipeline;
use serde_json::{json, Value};
use support::datasets::scratch;
use support::runs::run_over;
use support::{fakes, get, json, router_over, send, write_request, FakeRunStore, HeldPipelines};

const PARENT: &str = "hybrid-rerank-gen";

const HYBRID_RERANK_GEN: &str = "\
# The parent: two retrieval legs, a fusion, a reranker, a third leg fused
# with the reranked list, a context builder and a generator.
pipeline:
  inputs: [question]
  nodes:
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
      params: { top_k: 100 }
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
      params: { top_k: 50.0 }
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [bm25, dense]
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, rrf]
    - id: extra
      component: retriever
      impl: bm25
      inputs: [question]
    - id: both
      component: fusion
      impl: rrf
      inputs: [rerank, extra]
    - id: concat
      component: context_builder
      impl: concat
      inputs: [question, both]
    - id: generate
      component: generator
      impl: answerer
      inputs: [question, concat]
";

/// [`HYBRID_RERANK_GEN`] cut at `rerank`, written by hand: `rerank`, what it
/// reads transitively, and the declared inputs. `extra` is read only by
/// `both`, which is cut, so it is dropped too.
const UP_TO_RERANK: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
      params: { top_k: 100 }
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
      params: { top_k: 50.0 }
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [bm25, dense]
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, rrf]
";

const QRELS_ONLY: &str = "beir/scifact";
const WITH_ANSWERS: &str = "squad/dev";

fn hash_of(document: &str) -> String {
    parse_document(document)
        .expect("a test document validates")
        .content_hash()
        .to_string()
}

/// A launcher that announces the content hash of the document it is handed
/// as the run id — the identity a prefix run's cut carries — and records
/// every submission.
#[derive(Default)]
struct RecordingLauncher {
    seen: Mutex<Vec<Submission>>,
}

#[async_trait]
impl Launcher for RecordingLauncher {
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
        self.seen.lock().unwrap().push(submission.clone());
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
        _: &Submission,
        _: Arc<dyn RunObserver>,
        _: Cancellation,
    ) -> Result<Run, LauncherError> {
        Err(LauncherError::Cancelled)
    }
}

/// A registry whose two benchmarks carry what their names say: qrels alone,
/// or qrels and reference answers.
struct CarryingRegistry;

#[async_trait]
impl Registry for CarryingRegistry {
    async fn benchmarks(&self) -> Result<Vec<BenchmarkEntry>, ApiError> {
        Ok(vec![
            self.verify(QRELS_ONLY).await?,
            self.verify(WITH_ANSWERS).await?,
        ])
    }

    async fn verify(&self, name: &str) -> Result<BenchmarkEntry, ApiError> {
        let ground_truth = match name {
            QRELS_ONLY => GroundTruth::Qrels,
            WITH_ANSWERS => GroundTruth::Both,
            _ => {
                return Err(ApiError::BenchmarkNotFound {
                    name: name.to_owned(),
                })
            }
        };
        Ok(BenchmarkEntry {
            name: name.to_owned(),
            format: name.split('/').next().unwrap().to_owned(),
            state: BenchmarkState::Ready {
                dataset_version: "digest".to_owned(),
            },
            ground_truth: Some(ground_truth),
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

fn server(test: &str, launcher: Arc<RecordingLauncher>) -> Server {
    let mut backends = fakes(FakeRunStore::default());
    backends.pipelines = Arc::new(HeldPipelines {
        files: vec![(PARENT.to_owned(), HYBRID_RERANK_GEN.to_owned())],
    });
    backends.registry = Arc::new(CarryingRegistry);
    backends.launcher = launcher;
    router_over(backends, &scratch(test))
}

async fn submit(app: &Server, up_to: &str, benchmark: &str) -> (StatusCode, Value) {
    let response = send(
        app.clone(),
        write_request(
            "POST",
            "/api/v1/runs",
            &json!({ "pipeline": PARENT, "benchmark": benchmark, "up_to": up_to }),
            &[],
        ),
    )
    .await;
    (response.status(), json(response).await)
}

#[tokio::test]
async fn up_to_a_node_submits_the_cut_document_announced_under_its_own_hash() {
    let launcher = Arc::new(RecordingLauncher::default());
    let app = server("prefix-cut", Arc::clone(&launcher));

    let (status, body) = submit(&app, "rerank", QRELS_ONLY).await;

    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let submission = launcher.seen.lock().unwrap()[0].clone();
    // The cut is the parent's nodes up to `rerank`, with the declared
    // inputs, and it validates: its hash is the hand-written cut's.
    assert_eq!(hash_of(&submission.pipeline), hash_of(UP_TO_RERANK));
    // As `ragondin validate` reads it from a file.
    let file = scratch("prefix-cut-file").join("cut.yaml");
    std::fs::write(&file, &submission.pipeline).unwrap();
    let loaded = LocalFile::new(&file).load().await.expect("the cut loads");
    assert_eq!(loaded.content_hash().to_string(), hash_of(UP_TO_RERANK));
    assert_eq!(body["run_id"], hash_of(UP_TO_RERANK));
    // The parent is the submission's name, and its hash at submission is
    // carried beside `up_to`.
    assert_eq!(submission.pipeline_name, PARENT);
    assert_eq!(submission.up_to.as_deref(), Some("rerank"));
    assert_eq!(
        submission.parent_pipeline_hash.map(|hash| hash.to_string()),
        Some(hash_of(HYBRID_RERANK_GEN))
    );

    // The job records the prefix as provenance: the node and the parent's hash.
    let job = json(
        send(
            app.clone(),
            get(&format!(
                "/api/v1/jobs/{}",
                body["job_id"].as_str().unwrap()
            )),
        )
        .await,
    )
    .await;
    assert_eq!(job["work"]["pipeline"], PARENT);
    assert_eq!(job["work"]["up_to"], "rerank");
    assert_eq!(
        job["work"]["parent_pipeline_hash"],
        hash_of(HYBRID_RERANK_GEN)
    );
}

#[tokio::test]
async fn a_whole_run_carries_no_parent() {
    let launcher = Arc::new(RecordingLauncher::default());
    let app = server("prefix-whole", Arc::clone(&launcher));

    let response = send(
        app.clone(),
        write_request(
            "POST",
            "/api/v1/runs",
            &json!({ "pipeline": PARENT, "benchmark": WITH_ANSWERS }),
            &[],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let submission = launcher.seen.lock().unwrap()[0].clone();
    assert_eq!(submission.pipeline, HYBRID_RERANK_GEN);
    assert_eq!(submission.parent_pipeline_hash, None);
}

#[tokio::test]
async fn each_cut_that_cannot_be_a_prefix_is_refused_naming_the_node() {
    for (up_to, code, words) in [
        ("nowhere", "prefix_node_not_found", "has no node `nowhere`"),
        (
            "generate",
            "prefix_is_whole_pipeline",
            "the prefix would be the whole pipeline",
        ),
        (
            "concat",
            "prefix_ends_in_context",
            "a context is scored by nothing",
        ),
        // A declared input is not a node.
        (
            "question",
            "prefix_node_not_found",
            "has no node `question`",
        ),
    ] {
        let launcher = Arc::new(RecordingLauncher::default());
        let app = server(&format!("prefix-refused-{up_to}"), Arc::clone(&launcher));

        let (status, body) = submit(&app, up_to, QRELS_ONLY).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{up_to}: {body}");
        assert_eq!(body["code"], code, "{up_to}");
        assert_eq!(body["location"]["node"], up_to, "{up_to}");
        assert!(
            body["detail"].as_str().unwrap().contains(words),
            "{up_to}: {body}"
        );
        assert!(
            launcher.seen.lock().unwrap().is_empty(),
            "nothing submitted"
        );
    }
}

#[tokio::test]
async fn a_prefix_ending_before_the_generator_is_refused_on_a_benchmark_with_reference_answers() {
    let launcher = Arc::new(RecordingLauncher::default());
    let app = server("prefix-answers", Arc::clone(&launcher));

    let (status, body) = submit(&app, "rerank", WITH_ANSWERS).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "prefix_not_scorable");
    assert_eq!(body["location"]["node"], "rerank");
    let detail = body["detail"].as_str().unwrap();
    // The harness's reason: the benchmark asked for answers to be scored.
    assert!(
        detail.contains(
            "the benchmark carries reference answers, and the pipeline's output is of kind `chunks`, not an answer"
        ),
        "{detail}"
    );
    assert!(detail.contains(WITH_ANSWERS), "{detail}");
    assert!(launcher.seen.lock().unwrap().is_empty());

    // The same cut on a benchmark of qrels alone is accepted.
    let (status, body) = submit(&app, "rerank", QRELS_ONLY).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
}

// ---- The prefix relation on `GET /runs` ------------------------------------

fn benchmark() -> Benchmark {
    Benchmark::new(Vec::new(), Vec::new(), Qrels::new())
}

fn listing(test: &str, documents: &[(&str, &str)], runs: Vec<Run>) -> Server {
    let mut backends = fakes(FakeRunStore::holding(runs));
    backends.pipelines = Arc::new(HeldPipelines {
        files: documents
            .iter()
            .map(|(name, document)| ((*name).to_owned(), (*document).to_owned()))
            .collect(),
    });
    router_over(backends, &scratch(test))
}

async fn listed(app: &Server) -> Vec<Value> {
    json(send(app.clone(), get("/api/v1/runs")).await).await["runs"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn a_run_submitted_up_to_a_node_is_a_prefix_by_its_record_and_by_structure() {
    let mut run = run_over(1, UP_TO_RERANK, &benchmark(), Vec::new(), &[]);
    let parent = parse_document(HYBRID_RERANK_GEN).unwrap().content_hash();
    run.provenance = Some(RunProvenance::prefix(
        PARENT,
        PrefixOf::new("rerank", parent),
    ));
    let app = listing(
        "prefix-listing-recorded",
        &[(PARENT, HYBRID_RERANK_GEN)],
        vec![run],
    );

    let runs = listed(&app).await;

    assert_eq!(
        runs[0]["launched_as"]["prefix_of"],
        json!({ "up_to": "rerank", "parent_pipeline_hash": parent.to_string() })
    );
    assert_eq!(
        runs[0]["prefix_of_documents"],
        json!([{ "pipeline": PARENT, "up_to": "rerank" }])
    );
}

#[tokio::test]
async fn a_run_launched_from_the_command_line_is_a_prefix_by_structure_alone() {
    let run = run_over(1, UP_TO_RERANK, &benchmark(), Vec::new(), &[]);
    let whole = run_over(2, HYBRID_RERANK_GEN, &benchmark(), Vec::new(), &[]);
    let app = listing(
        "prefix-listing-structural",
        &[(PARENT, HYBRID_RERANK_GEN)],
        vec![run, whole],
    );

    let runs = listed(&app).await;
    let by_id = |byte: u8| {
        runs.iter()
            .find(|run| run["id"] == RunId::from_digest([byte; 32]).to_string())
            .unwrap()
    };

    assert_eq!(by_id(1)["launched_as"], Value::Null);
    assert_eq!(
        by_id(1)["prefix_of_documents"],
        json!([{ "pipeline": PARENT, "up_to": "rerank" }])
    );
    // The whole pipeline is not its own prefix: that is the hash match.
    assert_eq!(by_id(2)["prefix_of_documents"], json!([]));
    assert_eq!(by_id(2)["pipeline_names"], json!([PARENT]));
}

/// One canonical hash is one canonical form, so the listing lowers a
/// pipeline once for all its runs: a second run of the same hash is read by
/// the first's lowering, whatever its own stored text says.
#[tokio::test]
async fn the_listing_lowers_each_pipeline_once_whatever_its_runs_number() {
    let first = run_over(1, UP_TO_RERANK, &benchmark(), Vec::new(), &[]);
    let mut second = run_over(2, UP_TO_RERANK, &benchmark(), Vec::new(), &[]);
    second.config = ragondin_experiments::ConfigDocument::new("not: [a pipeline");
    let app = listing(
        "prefix-listing-once",
        &[(PARENT, HYBRID_RERANK_GEN)],
        vec![first, second],
    );

    let runs = listed(&app).await;

    for run in &runs {
        assert_eq!(
            run["prefix_of_documents"],
            json!([{ "pipeline": PARENT, "up_to": "rerank" }]),
            "{}",
            run["id"]
        );
    }
}

#[tokio::test]
async fn a_changed_parameter_makes_no_prefix_and_one_document_under_two_names_makes_two() {
    let changed = UP_TO_RERANK.replace("top_k: 100", "top_k: 99");
    let run = run_over(1, &changed, &benchmark(), Vec::new(), &[]);
    let cut = run_over(2, UP_TO_RERANK, &benchmark(), Vec::new(), &[]);
    let app = listing(
        "prefix-listing-negative",
        &[(PARENT, HYBRID_RERANK_GEN), ("fork", HYBRID_RERANK_GEN)],
        vec![run, cut],
    );

    let runs = listed(&app).await;
    let by_id = |byte: u8| {
        runs.iter()
            .find(|run| run["id"] == RunId::from_digest([byte; 32]).to_string())
            .unwrap()
    };

    assert_eq!(by_id(1)["prefix_of_documents"], json!([]));
    assert_eq!(
        by_id(2)["prefix_of_documents"],
        json!([
            { "pipeline": "fork", "up_to": "rerank" },
            { "pipeline": PARENT, "up_to": "rerank" },
        ])
    );
}
