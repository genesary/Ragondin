//! The fixture workspace: one directory, built from the exit-criterion
//! fixtures, that the Rust tests open and the UI's dev server and end-to-end
//! tests serve — one truth on both sides (the front-end design, § 9).
//!
//! [`generate`] writes, under an output directory:
//!
//! - `workspace/` — a workspace `ragondin ui` opens: the exit criterion's two
//!   pipeline documents, `dense-only` and `hybrid-rerank`, and the lexical
//!   leg alone, `bm25-only`, each with a layout; the toy models under
//!   `models/`, where the documents' relative paths resolve once the server
//!   runs from the workspace; the fixture benchmark imported as
//!   `beir/exit-criterion`; four runs (below); and an empty job queue, the
//!   launches' records removed once their runs are filed.
//! - `corpus/exit-criterion/` — the fixture corpus as BEIR, outside the
//!   workspace: what the first-run journey imports into an empty one.
//! - `fixture.json` — what an end-to-end test needs to know without reading
//!   the store: the paths, the benchmark and each run's id.
//!
//! The four runs, two of them the cases decision #390 leaves to be shown
//! rather than guessed:
//!
//! - **`dense-only` and `hybrid-rerank`**, launched through `POST /runs` as
//!   the UI launches them, each with its launch record.
//! - **A run without a record**: `bm25-only`, run by `ragondin bench` from a
//!   configuration outside the workspace's `pipelines/`, so no
//!   `provenance.json` is written — the shape of every run stored before the
//!   record existed. Its content is a current document's, so `GET /runs`
//!   finds it by hash. A third document rather than one of the two: a run id
//!   is a function of the content and the benchmark, so a third run on the
//!   one benchmark needs a third content.
//! - **A run whose recorded name's content has since changed**: launched as
//!   `hybrid-rerank` while that document's reranker kept five passages
//!   ([`CHANGED_TOP_K`]), before the document was written back with the
//!   fixture's ten.
//!
//! The fixture benchmark is registered through `POST /benchmarks/import`, the
//! one mechanism that registers a corpus on the server's disk: the binary's
//! manifest is compiled in and names only downloads. The fixture dataset holds
//! `answers.jsonl`, which an import reads as `beir-qa` — reference answers a
//! ranking-only pipeline is refused on (ADR-C30 § 5) — so the corpus is
//! copied without it, as `exit_criterion.rs` reads it through `beir/`.
//!
//! Shared by `tests/fixture_workspace.rs` and `tests/ui_parity.rs`, beside
//! `support/ui.rs`, whose `Server` it starts.

#![allow(dead_code)] // each includer uses the part it needs

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::ui::{self as http, Server};

/// The benchmark selector the fixture benchmark is imported under.
pub const BENCHMARK: &str = "beir/exit-criterion";
/// The import's name: its directory under `datasets/`.
pub const BENCHMARK_NAME: &str = "exit-criterion";
/// The `top_k` the changed-content run's reranker kept; the fixture's
/// `hybrid-rerank.yaml` keeps ten.
pub const CHANGED_TOP_K: u32 = 5;
/// The three workspace documents, by name.
pub const DOCUMENTS: [&str; 3] = ["dense-only", "hybrid-rerank", "bm25-only"];

/// The exit criterion's fixtures: the configurations, the dataset, the models.
pub fn exit_criterion() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exit-criterion")
}

/// Every fixture under `tests/fixtures`.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The run ids [`generate`] recorded.
#[derive(Debug, Clone)]
pub struct Runs {
    /// `dense-only`, launched through the API.
    pub dense_only: String,
    /// `hybrid-rerank`, launched through the API.
    pub hybrid_rerank: String,
    /// `bm25-only`, run by `bench` with no launch record.
    pub unrecorded: String,
    /// Launched as `hybrid-rerank` with [`CHANGED_TOP_K`].
    pub changed: String,
}

/// What [`generate`] wrote.
#[derive(Debug, Clone)]
pub struct FixtureWorkspace {
    /// The workspace directory.
    pub workspace: PathBuf,
    /// The fixture corpus as BEIR, outside the workspace.
    pub corpus: PathBuf,
    /// Each run's id.
    pub runs: Runs,
}

/// The source text of a document [`generate`] writes, by name.
pub fn document(name: &str) -> String {
    let file = match name {
        "dense-only" => "dense-only.yaml",
        "hybrid-rerank" => "hybrid-rerank.yaml",
        "bm25-only" => "ablations/bm25-only.yaml",
        other => panic!("no fixture document is named {other}"),
    };
    std::fs::read_to_string(exit_criterion().join(file)).expect("the fixture document reads")
}

/// `hybrid-rerank.yaml` with its reranker keeping [`CHANGED_TOP_K`]
/// passages: the content the changed-content run was launched with.
pub fn changed_hybrid_rerank() -> String {
    let original = document("hybrid-rerank");
    let changed = original.replace(
        "        top_k: 10\n",
        &format!("        top_k: {CHANGED_TOP_K}\n"),
    );
    assert_ne!(changed, original, "the reranker's top_k is in the fixture");
    changed
}

/// Copies the files of the directory `from` into `to`, recursively.
pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the destination is created");
    for entry in std::fs::read_dir(from).expect("the source reads") {
        let path = entry.expect("an entry").path();
        let target = to.join(path.file_name().expect("a name"));
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            std::fs::copy(&path, &target).expect("a file copies");
        }
    }
}

/// `ragondin ui` over `workspace`, run from it: a configuration's relative
/// model path resolves against the process's directory, so the documents'
/// `models/…` resolve under the workspace.
pub fn serve(workspace: &Path) -> Server {
    Server::start_with(
        &["--workspace", workspace.to_str().expect("UTF-8 path")],
        Some(workspace),
        None,
    )
}

/// `method path` with `body`, answered with `expected`, as JSON.
pub fn call(
    server: &Server,
    method: &str,
    path: &str,
    body: &serde_json::Value,
    expected: u16,
) -> serde_json::Value {
    let response = http::send_json(server.authority(), method, path, &body.to_string());
    assert_eq!(
        response.status, expected,
        "{method} {path}: {}",
        response.body
    );
    serde_json::from_str(&response.body).expect("the answer is JSON")
}

/// `GET path`, answered `200`, as JSON.
pub fn read(server: &Server, path: &str) -> serde_json::Value {
    let response = http::get(server.authority(), path);
    assert_eq!(response.status, 200, "GET {path}: {}", response.body);
    serde_json::from_str(&response.body).expect("the answer is JSON")
}

/// Imports the corpus at `source` as the local benchmark `name`, through the
/// API, and returns its selector.
pub fn import(server: &Server, name: &str, source: &Path) -> String {
    let entry = call(
        server,
        "POST",
        "/api/v1/benchmarks/import",
        &serde_json::json!({ "name": name, "path": source }),
        200,
    );
    entry["name"]
        .as_str()
        .expect("the entry names its selector")
        .to_owned()
}

/// Writes `text` as the workspace document `name`, byte for byte, as an
/// import does; a document already there is replaced.
pub fn put_text(server: &Server, name: &str, text: &str) -> serde_json::Value {
    let path = format!("/api/v1/pipelines/{name}");
    let current = http::get(server.authority(), &path);
    let precondition = match current.status {
        404 => ("If-None-Match".to_owned(), "*".to_owned()),
        200 => (
            "If-Match".to_owned(),
            current
                .header("etag")
                .expect("a stored document has an etag")
                .to_owned(),
        ),
        status => panic!("GET {path}: {status} {}", current.body),
    };
    let response = http::send_json_with(
        server.authority(),
        "PUT",
        &path,
        &serde_json::json!({ "document": text }).to_string(),
        &[(precondition.0.as_str(), precondition.1.as_str())],
    );
    assert_eq!(response.status, 200, "PUT {path}: {}", response.body);
    serde_json::from_str(&response.body).expect("the answer is JSON")
}

/// How long a job over a fixture benchmark may take: seconds, in practice.
pub const FIXTURE_JOB_LIMIT: Duration = Duration::from_secs(600);

/// Submits a run of the workspace document `pipeline` on `benchmark`, waits
/// for its job to end, and returns the run id it was filed under.
pub fn launch(server: &Server, pipeline: &str, benchmark: &str) -> String {
    launch_within(server, pipeline, benchmark, FIXTURE_JOB_LIMIT)
}

/// [`launch`], waiting at most `limit` for the job to end.
pub fn launch_within(server: &Server, pipeline: &str, benchmark: &str, limit: Duration) -> String {
    let accepted = call(
        server,
        "POST",
        "/api/v1/runs",
        &serde_json::json!({ "pipeline": pipeline, "benchmark": benchmark }),
        202,
    );
    let job = accepted["job_id"].as_str().expect("a job id").to_owned();
    let state = wait_for(server, &job, limit);
    assert_eq!(state["kind"], "done", "{pipeline} on {benchmark}: {state}");
    state["run_id"].as_str().expect("a filed run id").to_owned()
}

/// Polls `GET /jobs/{id}` until the job has ended, at most `limit`, and
/// returns its state.
pub fn wait_for(server: &Server, job: &str, limit: Duration) -> serde_json::Value {
    let deadline = Instant::now() + limit;
    loop {
        let summary = read(server, &format!("/api/v1/jobs/{job}"));
        let state = summary["state"].clone();
        if !matches!(state["kind"].as_str(), Some("queued" | "running")) {
            return state;
        }
        assert!(Instant::now() < deadline, "job {job} never ended: {state}");
        std::thread::sleep(Duration::from_millis(if limit > FIXTURE_JOB_LIMIT {
            1000
        } else {
            50
        }));
    }
}

/// `ragondin bench` over `config` on `benchmark`, from `cwd`, into `store`,
/// with `extra` arguments; returns the run id it reports.
pub fn bench(
    cwd: &Path,
    config: &Path,
    benchmark: &str,
    datasets: &Path,
    store: &Path,
    extra: &[&str],
) -> String {
    let output = Command::new(assert_cmd::cargo::cargo_bin("ragondin"))
        .current_dir(cwd)
        .arg("bench")
        .arg(config)
        .args(["--benchmark", benchmark, "--datasets"])
        .arg(datasets)
        .arg("--store")
        .arg(store)
        .args(extra)
        .output()
        .expect("the binary runs");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        output.status.success(),
        "bench {}: {}",
        config.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("run "))
        .unwrap_or_else(|| panic!("bench prints the run id first: {stdout}"))
        .to_owned()
}

/// A layout for `name`'s nodes: one column per node, in document order.
fn layout(name: &str) -> serde_json::Value {
    let ids: Vec<String> = document(name)
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- id: "))
        .map(str::to_owned)
        .collect();
    assert!(!ids.is_empty(), "{name} names its nodes");
    let nodes: serde_json::Map<String, serde_json::Value> = ids
        .into_iter()
        .enumerate()
        .map(|(column, id)| {
            let x = 240.0 * f64::from(u32::try_from(column).expect("a few nodes"));
            (id, serde_json::json!({ "x": x, "y": 0.0 }))
        })
        .collect();
    serde_json::json!({ "version": 1, "nodes": nodes })
}

/// Builds the fixture workspace under `out`, emptied first, and returns what
/// it wrote. See the module's documentation for what that is.
pub fn generate(out: &Path) -> FixtureWorkspace {
    let _ = std::fs::remove_dir_all(out);
    let workspace = out.join("workspace");
    let corpus = out.join("corpus").join(BENCHMARK_NAME);
    std::fs::create_dir_all(&workspace).expect("the workspace directory is created");

    let fixture = exit_criterion();
    copy_dir(&fixture.join("models"), &workspace.join("models"));
    // The models' generator is how they were made, not something a run reads.
    std::fs::remove_file(workspace.join("models/generate.py")).expect("the script is there");
    for file in ["corpus.jsonl", "queries.jsonl"] {
        std::fs::create_dir_all(&corpus).expect("the corpus directory is created");
        std::fs::copy(fixture.join("dataset").join(file), corpus.join(file))
            .expect("a corpus file copies");
    }
    copy_dir(&fixture.join("dataset/qrels"), &corpus.join("qrels"));

    let server = serve(&workspace);
    let benchmark = import(&server, BENCHMARK_NAME, &corpus);
    assert_eq!(benchmark, BENCHMARK);

    // The run of earlier content first, under the name it still has.
    put_text(&server, "hybrid-rerank", &changed_hybrid_rerank());
    let changed = launch(&server, "hybrid-rerank", BENCHMARK);

    for name in DOCUMENTS {
        put_text(&server, name, &document(name));
        call(
            &server,
            "PUT",
            &format!("/api/v1/pipelines/{name}/layout"),
            &layout(name),
            200,
        );
    }
    let dense_only = launch(&server, "dense-only", BENCHMARK);
    let hybrid_rerank = launch(&server, "hybrid-rerank", BENCHMARK);

    // From outside `pipelines/`, so `bench` records no launch.
    let unrecorded = bench(
        &workspace,
        &fixture.join("ablations/bm25-only.yaml"),
        BENCHMARK,
        &workspace.join("datasets"),
        &workspace.join("runs"),
        &[],
    );
    drop(server);
    // The queue's record of the three launches: the runs are in the store, and
    // a workspace opened on them starts with an empty queue, so a journey's
    // job is the only one on screen.
    for entry in std::fs::read_dir(workspace.join("jobs")).expect("the queue's directory reads") {
        std::fs::remove_file(entry.expect("an entry").path()).expect("a job record is removed");
    }

    let runs = Runs {
        dense_only,
        hybrid_rerank,
        unrecorded,
        changed,
    };
    let manifest = serde_json::json!({
        "workspace": workspace,
        "corpus": corpus,
        "benchmark": BENCHMARK,
        "benchmark_name": BENCHMARK_NAME,
        "changed_top_k": CHANGED_TOP_K,
        "runs": {
            "dense_only": runs.dense_only,
            "hybrid_rerank": runs.hybrid_rerank,
            "unrecorded": runs.unrecorded,
            "changed": runs.changed,
        },
    });
    std::fs::write(
        out.join("fixture.json"),
        serde_json::to_string_pretty(&manifest).expect("the manifest serializes") + "\n",
    )
    .expect("the manifest is written");
    FixtureWorkspace {
        workspace,
        corpus,
        runs,
    }
}
