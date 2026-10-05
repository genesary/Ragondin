//! The fixture workspace and the demo workspace: directories built from the
//! repository's test fixtures through the binary's own API. The fixture is
//! what the Rust tests open and the UI's dev server and end-to-end tests
//! serve — one truth on both sides (the front-end design, § 9); the demo is
//! what `just demo` serves.
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
//! [`generate_demo`] writes the demo workspace `just demo` serves with the
//! same steps — the models copied, benchmarks imported, documents written
//! and launched through the API — over the example pipelines in
//! `tests/fixtures/demo/`; its documentation says what it holds. Both
//! generators empty their output directory first, and both refuse one that
//! holds files and not their manifest, `fixture.json` or `demo.json`.
//!
//! Neither workspace opens as written from anywhere: the workspace is the
//! `workspace/` below the output directory, and its documents' model paths
//! are relative to the directory the server runs in. `ragondin ui
//! --workspace <out>` opens an empty workspace in `<out>`, and `ragondin ui
//! --workspace <out>/workspace` started elsewhere refuses the dense
//! pipelines, their models not found; the recipes start it inside the
//! workspace, as [`serve`] does.
//!
//! Shared by `tests/fixture_workspace.rs`, `tests/demo_workspace.rs` and
//! `tests/ui_parity.rs`, beside `support/ui.rs`, whose `Server` it starts.

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
    layout_of(&document(name))
}

/// A layout for the nodes of the document `text`: one column per node, in
/// document order.
fn layout_of(text: &str) -> serde_json::Value {
    let ids: Vec<String> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- id: "))
        .map(str::to_owned)
        .collect();
    assert!(!ids.is_empty(), "the document names its nodes");
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

/// Empties `out` for a generator whose manifest is `marker`, and creates
/// `out/workspace`, which it returns. Only a directory that generator wrote
/// before is emptied, or none: one that holds anything and no `marker` is
/// someone's, and refused untouched.
fn prepare(out: &Path, marker: &str) -> PathBuf {
    let foreign = std::fs::read_dir(out).is_ok_and(|mut entries| entries.next().is_some())
        && !out.join(marker).is_file();
    assert!(
        !foreign,
        "{} holds files and no {marker}: not a workspace this generator wrote, so it is \
         left as it is; name an empty or absent directory",
        out.display()
    );
    let _ = std::fs::remove_dir_all(out);
    let workspace = out.join("workspace");
    std::fs::create_dir_all(&workspace).expect("the workspace directory is created");
    workspace
}

/// Copies the exit criterion's toy models into `workspace/models`, where a
/// document's relative `models/…` resolves once the server runs from the
/// workspace.
fn copy_models(workspace: &Path) {
    copy_dir(&exit_criterion().join("models"), &workspace.join("models"));
    // The models' generator is how they were made, not something a run reads.
    std::fs::remove_file(workspace.join("models/generate.py")).expect("the script is there");
}

/// Writes the fixture corpus as BEIR under `out/corpus/`, without
/// `answers.jsonl` (see the module's documentation), and returns its path.
fn write_retrieval_corpus(out: &Path) -> PathBuf {
    let fixture = exit_criterion();
    let corpus = out.join("corpus").join(BENCHMARK_NAME);
    std::fs::create_dir_all(&corpus).expect("the corpus directory is created");
    for file in ["corpus.jsonl", "queries.jsonl"] {
        std::fs::copy(fixture.join("dataset").join(file), corpus.join(file))
            .expect("a corpus file copies");
    }
    copy_dir(&fixture.join("dataset/qrels"), &corpus.join("qrels"));
    corpus
}

/// Removes the queue's record of the launches: the runs are in the store,
/// and a workspace opened on them starts with an empty queue, so a journey's
/// job is the only one on screen.
fn empty_queue(workspace: &Path) {
    for entry in std::fs::read_dir(workspace.join("jobs")).expect("the queue's directory reads") {
        std::fs::remove_file(entry.expect("an entry").path()).expect("a job record is removed");
    }
}

/// Writes `manifest` as `out/name`, pretty-printed.
fn write_manifest(out: &Path, name: &str, manifest: &serde_json::Value) {
    std::fs::write(
        out.join(name),
        serde_json::to_string_pretty(manifest).expect("the manifest serializes") + "\n",
    )
    .expect("the manifest is written");
}

/// Builds the fixture workspace under `out`, emptied first, and returns what
/// it wrote. See the module's documentation for what that is.
pub fn generate(out: &Path) -> FixtureWorkspace {
    let workspace = prepare(out, "fixture.json");
    let fixture = exit_criterion();
    copy_models(&workspace);
    let corpus = write_retrieval_corpus(out);

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
    empty_queue(&workspace);

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
    write_manifest(out, "fixture.json", &manifest);
    FixtureWorkspace {
        workspace,
        corpus,
        runs,
    }
}

/// The demo's example pipelines, by name: each is
/// `tests/fixtures/demo/<name>.yaml`.
pub const DEMO_DOCUMENTS: [&str; 3] = ["lexical", "hybrid", "rag"];
/// The pipeline the demo forks from `hybrid`'s run.
pub const DEMO_FORK: &str = "hybrid-fork";
/// The demo's benchmarks, by selector, with the ground truth each holds.
pub const DEMO_BENCHMARKS: [(&str, &str); 4] = [
    (BENCHMARK, "qrels"),
    ("beir/beir-mini", "qrels"),
    ("beir-qa/exit-criterion-qa", "both"),
    ("squad/squad-mini", "both"),
];

/// What [`generate_demo`] wrote.
#[derive(Debug, Clone)]
pub struct DemoWorkspace {
    /// The workspace directory.
    pub workspace: PathBuf,
    /// Every run it filed, in the order it filed them.
    pub runs: Vec<String>,
}

/// The source text of a demo pipeline, by name.
pub fn demo_document(name: &str) -> String {
    std::fs::read_to_string(fixtures().join("demo").join(format!("{name}.yaml")))
        .expect("the demo document reads")
}

/// Builds the demo workspace under `out` — what `just demo` serves — emptied
/// first when this wrote it, and returns what it wrote. Its steps are the
/// fixture workspace's, through the server's own API as the UI takes them:
///
/// - the toy models under `workspace/models/`;
/// - four benchmarks imported: two retrieval sets, the fixture corpus as
///   `beir/exit-criterion` and `beir-mini/` as `beir/beir-mini` (qrels), and
///   two QA sets, the fixture corpus with its reference answers as
///   `beir-qa/exit-criterion-qa` and `squad-mini/` as `squad/squad-mini`;
/// - the example pipelines of [`DEMO_DOCUMENTS`], each with a layout;
/// - the runs: `lexical` and `hybrid` on `beir/exit-criterion`; `hybrid` up
///   to its fusion there, a prefix run; a fork of `hybrid`'s run with RRF's
///   `k` lowered to 10, written as [`DEMO_FORK`] and launched there;
///   `lexical` on `beir/beir-mini`, where `hybrid` is left for a launch from
///   the page; and `rag` on both QA sets;
/// - an empty job queue, and `demo.json`, the marker a regeneration checks
///   before it empties the directory.
pub fn generate_demo(out: &Path) -> DemoWorkspace {
    let workspace = prepare(out, "demo.json");
    copy_models(&workspace);
    let corpus = write_retrieval_corpus(out);
    let fixtures = fixtures();

    let server = serve(&workspace);
    let imports = [
        (BENCHMARK_NAME, corpus),
        ("beir-mini", fixtures.join("beir-mini")),
        ("exit-criterion-qa", exit_criterion().join("dataset")),
        ("squad-mini", fixtures.join("squad-mini/dev-v1.1.json")),
    ];
    for ((name, source), (selector, _)) in imports.iter().zip(DEMO_BENCHMARKS) {
        assert_eq!(import(&server, name, source), selector);
    }
    let [retrieval, mini, qa, squad] = DEMO_BENCHMARKS.map(|(selector, _)| selector);

    for name in DEMO_DOCUMENTS {
        let text = demo_document(name);
        put_text(&server, name, &text);
        call(
            &server,
            "PUT",
            &format!("/api/v1/pipelines/{name}/layout"),
            &layout_of(&text),
            200,
        );
    }
    let lexical = launch(&server, "lexical", retrieval);
    let hybrid = launch(&server, "hybrid", retrieval);
    let prefix = launch_prefix(&server, "hybrid", retrieval, "fused");
    fork(&server, &hybrid, DEMO_FORK, ("k: 60", "k: 10"));
    let forked = launch(&server, DEMO_FORK, retrieval);
    let lexical_mini = launch(&server, "lexical", mini);
    let rag_qa = launch(&server, "rag", qa);
    let rag_squad = launch(&server, "rag", squad);
    drop(server);
    empty_queue(&workspace);

    let runs = vec![
        lexical,
        hybrid,
        prefix,
        forked,
        lexical_mini,
        rag_qa,
        rag_squad,
    ];
    let manifest = serde_json::json!({
        "workspace": workspace,
        "benchmarks": DEMO_BENCHMARKS.map(|(selector, _)| selector),
        "pipelines": DEMO_DOCUMENTS,
        "runs": runs,
    });
    write_manifest(out, "demo.json", &manifest);
    DemoWorkspace { workspace, runs }
}

/// A prefix run: `pipeline` on `benchmark` cut after the node `up_to`, as
/// "Run up to here" submits it. Returns the run id it was filed under.
pub fn launch_prefix(server: &Server, pipeline: &str, benchmark: &str, up_to: &str) -> String {
    let accepted = call(
        server,
        "POST",
        "/api/v1/runs",
        &serde_json::json!({ "pipeline": pipeline, "benchmark": benchmark, "up_to": up_to }),
        202,
    );
    let job = accepted["job_id"].as_str().expect("a job id").to_owned();
    let state = wait_for(server, &job, FIXTURE_JOB_LIMIT);
    assert_eq!(state["kind"], "done", "{pipeline} up to {up_to}: {state}");
    state["run_id"].as_str().expect("a filed run id").to_owned()
}

/// What "Fork this run" does, then an edit: the run's configuration written
/// as the new document `name` with `edit.0` replaced by `edit.1`, and the
/// layout recorded at the run's launch copied beside it.
pub fn fork(server: &Server, run: &str, name: &str, edit: (&str, &str)) {
    let detail = read(server, &format!("/api/v1/runs/{run}"));
    let configuration = detail["configuration"]
        .as_str()
        .expect("the run's configuration");
    let edited = configuration.replacen(edit.0, edit.1, 1);
    assert_ne!(edited, configuration, "the edit applies: {configuration}");
    put_text(server, name, &edited);
    let at_launch = read(server, &format!("/api/v1/runs/{run}/layout"));
    call(
        server,
        "PUT",
        &format!("/api/v1/pipelines/{name}/layout"),
        &at_launch["layout"],
        200,
    );
}
