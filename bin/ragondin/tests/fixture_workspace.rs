//! The fixture workspace (`support/workspace.rs`), checked as the UI will
//! read it, and written out for the UI's dev server and end-to-end tests.
//!
//! The first test generates the workspace and opens it in `ragondin ui`: the
//! listing holds exactly its four runs, each carrying the two facts ADR-C39
//! § 4 keeps apart — what its launch record says, and which current documents
//! hold its content — as the generator meant them. The second writes it for
//! the UI: ignored, because it writes outside the build directory, and run by
//! `just fixture-workspace` (and so by `just test-ui-e2e`) with the output
//! directory in `RAGONDIN_FIXTURE_WORKSPACE`.
//!
//! # What runs when
//!
//! The runs need BM25, the ONNX embedder and the cross-encoder, and the
//! listing needs the server: compiled under `ui`, `bm25` and `onnx` together,
//! so `just test-features` (and CI's `--all-features` run) is where it runs.

#![cfg(all(feature = "ui", feature = "bm25", feature = "onnx"))]

#[path = "support/ui.rs"]
mod ui;
#[path = "support/workspace.rs"]
mod workspace;

use std::path::PathBuf;

use ragondin_experiments::{FileSystemRunStore, RunId};

/// Where the workspace is written, named by `just fixture-workspace`.
const EXPORT_VAR: &str = "RAGONDIN_FIXTURE_WORKSPACE";

fn out(test_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("fixture-workspace")
        .join(test_name)
}

#[test]
fn the_generated_workspace_opens_in_ui_and_lists_exactly_its_runs() {
    let generated = workspace::generate(&out("lists"));
    let runs = &generated.runs;

    let server = workspace::serve(&generated.workspace);
    let listing = workspace::read(&server, "/api/v1/runs");

    let listed = listing["runs"].as_array().expect("a list of runs");
    let mut ids: Vec<&str> = listed
        .iter()
        .map(|run| run["id"].as_str().expect("an id"))
        .collect();
    ids.sort_unstable();
    let mut expected = vec![
        runs.dense_only.as_str(),
        runs.hybrid_rerank.as_str(),
        runs.unrecorded.as_str(),
        runs.changed.as_str(),
    ];
    expected.sort_unstable();
    assert_eq!(ids, expected);
    assert_eq!(listing["unreadable"], serde_json::json!([]));
    let jobs = workspace::read(&server, "/api/v1/jobs");
    assert_eq!(
        jobs["jobs"],
        serde_json::json!([]),
        "the queue starts empty"
    );
    let run = |id: &str| {
        listed
            .iter()
            .find(|run| run["id"] == id)
            .expect("the run is listed")
            .clone()
    };

    // The two launched as they are named, each its own document's content.
    for (id, name) in [
        (&runs.dense_only, "dense-only"),
        (&runs.hybrid_rerank, "hybrid-rerank"),
    ] {
        let run = run(id);
        assert_eq!(run["launched_as"]["name"], name, "{run}");
        assert_eq!(run["launched_as"]["held"], "exactly", "{run}");
        assert_eq!(run["pipeline_names"], serde_json::json!([name]), "{run}");
        assert_eq!(
            run["benchmark_names"],
            serde_json::json!([workspace::BENCHMARK])
        );
    }

    // No record: found by its content alone.
    let unrecorded = run(&runs.unrecorded);
    assert_eq!(unrecorded["launched_as"], serde_json::Value::Null);
    assert_eq!(
        unrecorded["pipeline_names"],
        serde_json::json!(["bm25-only"])
    );
    let store = FileSystemRunStore::new(generated.workspace.join("runs"));
    let id: RunId = runs.unrecorded.parse().expect("a run id");
    assert_eq!(store.load(&id).expect("the run loads").provenance, None);
    assert!(
        !generated
            .workspace
            .join("runs")
            .join(&runs.unrecorded)
            .join("provenance.json")
            .exists(),
        "no record is written, not an empty one"
    );

    // Recorded as `hybrid-rerank`, whose content has changed since.
    let changed = run(&runs.changed);
    assert_eq!(changed["launched_as"]["name"], "hybrid-rerank");
    assert_eq!(changed["launched_as"]["held"], "exactly");
    assert_eq!(changed["pipeline_names"], serde_json::json!([]));
    let detail = workspace::read(&server, &format!("/api/v1/runs/{}", runs.changed));
    assert!(
        detail["configuration"]
            .as_str()
            .expect("the configuration")
            .contains(&format!("top_k: {}", workspace::CHANGED_TOP_K)),
        "{detail}"
    );

    // The benchmark is registered, and the documents have their layouts.
    let benchmarks = workspace::read(&server, "/api/v1/benchmarks");
    let entry = benchmarks["benchmarks"]
        .as_array()
        .expect("a list of benchmarks")
        .iter()
        .find(|entry| entry["name"] == workspace::BENCHMARK)
        .unwrap_or_else(|| panic!("the fixture benchmark is listed: {benchmarks}"))
        .clone();
    assert_eq!(entry["state"]["kind"], "local", "{entry}");
    assert_eq!(entry["ground_truth"], "qrels", "{entry}");
    for name in workspace::DOCUMENTS {
        let layout = workspace::read(&server, &format!("/api/v1/pipelines/{name}/layout"));
        assert_eq!(layout["layout"]["version"], 1, "{name}: {layout}");
    }

    // The manifest the end-to-end tests read names the same runs.
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out("lists").join("fixture.json")).expect("the manifest reads"),
    )
    .expect("the manifest is JSON");
    assert_eq!(manifest["runs"]["changed"], runs.changed.as_str());
    assert_eq!(manifest["benchmark"], workspace::BENCHMARK);
}

/// The generator empties its directory first, so a directory it did not
/// write — no `fixture.json` — is refused, untouched: a mistyped
/// `just fixture-workspace <dir>` must not delete someone's files.
#[test]
fn a_non_empty_directory_the_generator_did_not_write_is_refused_untouched() {
    let dir = out("foreign");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "mine").unwrap();

    let refused = std::panic::catch_unwind(|| workspace::generate(&dir));

    let message = refused.expect_err("the generator refuses");
    let message = message
        .downcast_ref::<String>()
        .map(String::as_str)
        .unwrap_or_default();
    assert!(message.contains("fixture.json"), "{message}");
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "mine"
    );
}

/// What `just fixture-workspace` runs: the same generator, into the
/// directory `RAGONDIN_FIXTURE_WORKSPACE` names.
#[test]
#[ignore = "writes the fixture workspace outside the build directory; run with `just fixture-workspace <dir>`"]
fn write_the_fixture_workspace() {
    let out = std::env::var_os(EXPORT_VAR)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{EXPORT_VAR} names the directory to write"));
    let generated = workspace::generate(&out);
    // The workspace is a directory below `out`, and its documents' model
    // paths are relative: served from anywhere else, it opens empty or its
    // dense pipelines cannot find their models.
    let at = generated.workspace.display();
    println!("fixture workspace: {at}");
    println!(
        "serve it from inside itself, with a binary built with ui,bm25,onnx:\n  \
         cd {at} && ragondin ui --workspace {at}"
    );
}
