//! The demo workspace (`support/workspace.rs`'s `generate_demo`), checked as
//! the UI will read it, and written out for `just demo`.
//!
//! The first test generates the demo and opens it in `ragondin ui` from the
//! workspace, as `just demo` starts it: its benchmarks are imported and
//! verified, each of its example pipelines is a document that runs, and the
//! listing holds every run the generator filed — a prefix run and a fork
//! among them — so that Runs, Compare, the Pipeline matrix and Replay all
//! have something to show. The second is the generator's refusal of a
//! directory it did not write. The third writes the demo where
//! `RAGONDIN_DEMO_WORKSPACE` says: ignored, because it writes outside the
//! build directory, and run by `just demo`.
//!
//! # What runs when
//!
//! The demo's pipelines need BM25, the ONNX embedder and cross-encoder, and
//! the stub generator, and the listing needs the server: compiled under `ui`,
//! `bm25`, `onnx` and `stub` together, so `just test-features` (and CI's
//! `--all-features` run) is where it runs.

#![cfg(all(feature = "ui", feature = "bm25", feature = "onnx", feature = "stub"))]

#[path = "support/ui.rs"]
mod ui;
#[path = "support/workspace.rs"]
mod workspace;

use std::path::PathBuf;

/// Where the demo is written, named by `just demo`.
const EXPORT_VAR: &str = "RAGONDIN_DEMO_WORKSPACE";

fn out(test_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("demo-workspace")
        .join(test_name)
}

#[test]
fn the_demo_opens_with_its_benchmarks_pipelines_and_runs() {
    let demo = workspace::generate_demo(&out("opens"));

    let server = workspace::serve(&demo.workspace);

    // Two retrieval sets and two QA sets, imported and verified.
    let benchmarks = workspace::read(&server, "/api/v1/benchmarks");
    for (selector, ground_truth) in workspace::DEMO_BENCHMARKS {
        let entry = benchmarks["benchmarks"]
            .as_array()
            .expect("a list of benchmarks")
            .iter()
            .find(|entry| entry["name"] == selector)
            .unwrap_or_else(|| panic!("{selector} is listed: {benchmarks}"))
            .clone();
        assert_eq!(entry["state"]["kind"], "local", "{entry}");
        assert_eq!(entry["ground_truth"], ground_truth, "{entry}");
    }

    // Each example pipeline is stored, valid, laid out, and has a run.
    let pipelines = workspace::read(&server, "/api/v1/pipelines");
    let listed = pipelines["pipelines"].as_array().expect("a list");
    let runs = workspace::read(&server, "/api/v1/runs");
    let filed = runs["runs"].as_array().expect("a list of runs");
    assert_eq!(runs["unreadable"], serde_json::json!([]));
    for name in workspace::DEMO_DOCUMENTS {
        let entry = listed
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap_or_else(|| panic!("{name} is stored: {pipelines}"));
        assert_eq!(entry["error"], serde_json::Value::Null, "{entry}");
        let layout = workspace::read(&server, &format!("/api/v1/pipelines/{name}/layout"));
        assert_eq!(layout["layout"]["version"], 1, "{name}: {layout}");
        assert!(
            filed.iter().any(|run| run["pipeline_names"]
                .as_array()
                .unwrap()
                .contains(&name.into())
                && run["launched_as"]["name"] == name),
            "{name} has a run launched under its name: {runs}"
        );
    }

    // Every run the generator filed is listed, and only those.
    let mut ids: Vec<&str> = filed
        .iter()
        .map(|run| run["id"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    let mut expected: Vec<&str> = demo.runs.iter().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(ids, expected);

    // A second retrieval set, on which `lexical` has run and `hybrid` has
    // not: a cell of the matrix left for a launch from the page.
    let on_mini: Vec<&serde_json::Value> = filed
        .iter()
        .filter(|run| run["benchmark_names"] == serde_json::json!(["beir/beir-mini"]))
        .map(|run| &run["launched_as"]["name"])
        .collect();
    assert_eq!(on_mini, [&serde_json::json!("lexical")], "{runs}");

    // A prefix run of `hybrid`, cut at its fusion.
    assert!(
        filed.iter().any(|run| run["prefix_of_documents"]
            == serde_json::json!([{ "pipeline": "hybrid", "up_to": "fused" }])),
        "a prefix run of hybrid up to fused: {runs}"
    );
    // A fork: a document written from a run's configuration, edited, run.
    let fork = filed
        .iter()
        .find(|run| run["launched_as"]["name"] == workspace::DEMO_FORK)
        .unwrap_or_else(|| panic!("the fork has a run: {runs}"));
    assert_eq!(
        fork["pipeline_names"],
        serde_json::json!([workspace::DEMO_FORK])
    );

    // Replay: every run serves a trace for its first query.
    for id in &demo.runs {
        let queries = workspace::read(&server, &format!("/api/v1/runs/{id}/queries"));
        let first = queries.to_string();
        let query = queries["queries"][0]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("run {id} lists its queries: {first}"));
        workspace::read(&server, &format!("/api/v1/runs/{id}/trace/{query}"));
    }

    // The queue starts empty, so nothing on screen is still running.
    let jobs = workspace::read(&server, "/api/v1/jobs");
    assert_eq!(jobs["jobs"], serde_json::json!([]));
}

/// Regenerating empties the directory first, so one the generator did not
/// write — no `demo.json` — is refused, untouched: `just demo <dir>` naming
/// the wrong directory must not delete someone's files.
#[test]
fn a_non_empty_directory_the_demo_generator_did_not_write_is_refused_untouched() {
    let dir = out("foreign");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "mine").unwrap();

    let refused = std::panic::catch_unwind(|| workspace::generate_demo(&dir));

    let message = refused.expect_err("the generator refuses");
    let message = message
        .downcast_ref::<String>()
        .map(String::as_str)
        .unwrap_or_default();
    assert!(message.contains("demo.json"), "{message}");
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "mine"
    );
}

/// What `just demo` runs: the same generator, into the directory
/// `RAGONDIN_DEMO_WORKSPACE` names.
#[test]
#[ignore = "writes the demo workspace outside the build directory; run with `just demo`"]
fn write_the_demo_workspace() {
    let out = std::env::var_os(EXPORT_VAR)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{EXPORT_VAR} names the directory to write"));
    let demo = workspace::generate_demo(&out);
    println!("demo workspace: {}", demo.workspace.display());
}
