//! The M4 journey on real data, through the API: **`ragondin ui` with no
//! argument on an empty workspace — obtain SciFact, compose a hybrid
//! pipeline, launch it, compare it with dense-only, replay one query node by
//! node in both** (the front-end design, § 2).
//!
//! The same journey runs on every change against the fixture benchmark, in a
//! browser, over the real binary (`ui/e2e/`, `just test-ui-e2e`). This one is
//! its counterpart over the dataset the criterion names, and the one place
//! the download path is exercised: the benchmark is obtained as the Setup
//! screen obtains it, by `POST /benchmarks/{name}/download` from the
//! manifest's pinned URLs, verified against its digests.
//!
//! # Ignored by default, run by `just journey-scifact`
//!
//! It fetches SciFact from the network, which no other test may do, and it
//! needs the two exported models the calibrations use, which never enter the
//! tree (ADR-C27); reranking 300 queries over a fused list of a hundred with a
//! real cross-encoder costs some twenty-five minutes of CPU. So it is
//! `#[ignore]`, and the models are named as `tests/calibration.rs` names
//! them:
//!
//! - `RAGONDIN_CALIBRATION_MODELS` — a directory holding `all-MiniLM-L6-v2/`
//!   and `ms-marco-MiniLM-L6-v2/`, each with a `model.onnx` and its
//!   `tokenizer.json`, exported as `bin/ragondin/ARCHITECTURE.md`
//!   § Calibration against a published leaderboard records.
//!
//! The server runs from that directory, so the composed documents' model
//! paths are the calibration's own, relative ones — the pipelines composed
//! here hash, under `ragondin validate`, to the calibration's
//! `fixtures/calibration/` files, which the test asserts: the run it launches
//! is the calibration's run.
//!
//! # What runs when
//!
//! Compiled under `ui`, `bm25` and `onnx` together, like the other journeys;
//! ignored even there.

#![cfg(all(feature = "ui", feature = "bm25", feature = "onnx"))]

#[path = "support/ui.rs"]
mod ui;
#[path = "support/workspace.rs"]
mod workspace;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ui::Server;

/// Where the two exported models sit, as the calibrations name it.
const MODELS_VAR: &str = "RAGONDIN_CALIBRATION_MODELS";
/// The manifest entry the journey downloads.
const SCIFACT: &str = "beir/scifact";
const NDCG: &str = "ndcg@10";
/// How long one job over real data may take: the reranked run is some
/// twenty-five minutes on a laptop's CPU, in a debug build.
const REAL_DATA_LIMIT: Duration = Duration::from_secs(2 * 60 * 60);

fn models() -> PathBuf {
    std::env::var_os(MODELS_VAR)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "{MODELS_VAR} is not set: it names the directory holding `all-MiniLM-L6-v2/` and \
                 `ms-marco-MiniLM-L6-v2/`, each with `model.onnx` and `tokenizer.json`. How they \
                 are obtained is recorded in \
                 `bin/ragondin/ARCHITECTURE.md` § Calibration against a published leaderboard."
            )
        })
}

fn string(value: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "string", "value": value })
}

fn int(value: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "int", "value": value })
}

/// The dense leg, as the editor holds it: the calibration's encoder.
fn dense_leg() -> serde_json::Value {
    serde_json::json!({
        "id": "vectors", "component": "retriever", "impl": "dense", "inputs": ["question"],
        "params": {
            "top_k": int("50"),
            "embedder": string("onnx"),
            "model": string("all-MiniLM-L6-v2/model.onnx"),
            "tokenizer": string("all-MiniLM-L6-v2/tokenizer.json"),
            "max_sequence_length": int("256"),
        }
    })
}

fn dense_only() -> serde_json::Value {
    let mut leg = dense_leg();
    // The baseline keeps the ten the metrics are cut at, as the calibration's.
    leg["params"]["top_k"] = int("10");
    serde_json::json!({ "pipeline": { "inputs": ["question"], "nodes": [leg] } })
}

/// The hybrid pipeline composed on the canvas: BM25 and the dense leg, fused
/// by RRF, reranked by the cross-encoder.
fn hybrid() -> serde_json::Value {
    serde_json::json!({
        "pipeline": {
            "inputs": ["question"],
            "nodes": [
                {
                    "id": "lexical", "component": "retriever", "impl": "bm25",
                    "inputs": ["question"], "params": { "top_k": int("50") }
                },
                dense_leg(),
                {
                    "id": "fused", "component": "fusion", "impl": "rrf",
                    "inputs": ["lexical", "vectors"], "params": { "k": int("60") }
                },
                {
                    "id": "reranked", "component": "reranker", "impl": "cross_encoder",
                    "inputs": ["question", "fused"],
                    "params": {
                        "top_k": int("10"),
                        "model": string("ms-marco-MiniLM-L6-v2/model.onnx"),
                        "tokenizer": string("ms-marco-MiniLM-L6-v2/tokenizer.json"),
                        "max_sequence_length": int("512"),
                    }
                }
            ]
        }
    })
}

/// The hash `ragondin validate` prints for a calibration configuration.
fn calibration_hash(file: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/calibration")
        .join(file);
    let output = assert_cmd::Command::cargo_bin("ragondin")
        .expect("the binary is built")
        .args(["validate", path.to_str().expect("UTF-8 path")])
        .output()
        .expect("the binary runs");
    assert!(output.status.success(), "validate {file}");
    String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .lines()
        .find_map(|line| line.strip_prefix("content hash: "))
        .expect("validate prints the hash")
        .to_owned()
}

/// Composes `typed` as the editor does — validated, then written — and
/// returns the hash it was shown.
fn compose(server: &Server, name: &str, typed: &serde_json::Value) -> String {
    let shown = workspace::call(
        server,
        "POST",
        "/api/v1/pipelines/validate",
        &serde_json::json!({ "typed": typed }),
        200,
    );
    let written = ui::send_json_with(
        server.authority(),
        "PUT",
        &format!("/api/v1/pipelines/{name}"),
        &serde_json::json!({ "typed": typed }).to_string(),
        &[("If-None-Match", "*")],
    );
    assert_eq!(written.status, 200, "{written:?}");
    shown["hash"].as_str().expect("a hash").to_owned()
}

fn state_of(server: &Server, name: &str) -> serde_json::Value {
    let listing = workspace::read(server, "/api/v1/benchmarks");
    listing["benchmarks"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("{name} is listed: {listing}"))["state"]
        .clone()
}

#[test]
#[ignore = "needs network access to download BEIR SciFact, and the exported models on disk; run with `just journey-scifact`"]
fn first_run_on_scifact_compose_launch_compare_and_replay() {
    let models = models();
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("journey-scifact");
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("the home directory is created");

    // `ragondin ui` with no argument: the home workspace, created empty.
    let server = Server::start_with(&[], Some(&models), Some(&home));
    let started = Instant::now();
    assert_eq!(
        workspace::read(&server, "/api/v1/runs")["runs"],
        serde_json::json!([])
    );

    // Obtain SciFact, as Setup's first step does.
    assert_eq!(state_of(&server, SCIFACT)["kind"], "available");
    let accepted = workspace::call(
        &server,
        "POST",
        &format!(
            "/api/v1/benchmarks/{}/download",
            SCIFACT.replace('/', "%2F")
        ),
        &serde_json::json!({}),
        202,
    );
    let downloaded = workspace::wait_for(
        &server,
        accepted["job_id"].as_str().expect("a job"),
        REAL_DATA_LIMIT,
    );
    assert_eq!(downloaded["kind"], "done", "{downloaded}");
    assert_eq!(state_of(&server, SCIFACT)["kind"], "ready");

    // Compose the two pipelines on the canvas: the calibration's, by hash.
    assert_eq!(
        compose(&server, "dense-only", &dense_only()),
        calibration_hash("dense-only.yaml")
    );
    assert_eq!(
        compose(&server, "hybrid", &hybrid()),
        calibration_hash("hybrid-rerank.yaml")
    );

    // Launch both.
    let dense = workspace::launch_within(&server, "dense-only", SCIFACT, REAL_DATA_LIMIT);
    let hybrid = workspace::launch_within(&server, "hybrid", SCIFACT, REAL_DATA_LIMIT);

    // Compare, against dense-only.
    let comparison = workspace::call(
        &server,
        "POST",
        "/api/v1/compare",
        &serde_json::json!({ "run_ids": [dense, hybrid], "baseline": dense }),
        200,
    );
    let ndcg = comparison["metrics"]
        .as_array()
        .expect("the metric rows")
        .iter()
        .find(|row| row["name"] == NDCG)
        .unwrap_or_else(|| panic!("{NDCG} is compared: {comparison}"))
        .clone();
    assert_eq!(
        ndcg["best"],
        serde_json::json!([hybrid]),
        "hybrid+rerank scores the higher {NDCG}: {ndcg}"
    );

    // Replay one query node by node, in both.
    let queries = workspace::read(&server, &format!("/api/v1/runs/{hybrid}/queries"));
    let query = queries["queries"][0]["id"]
        .as_str()
        .expect("a query")
        .to_owned();
    for (run, nodes) in [
        (&dense, &["vectors"][..]),
        (&hybrid, &["lexical", "vectors", "fused", "reranked"][..]),
    ] {
        let trace = workspace::read(&server, &format!("/api/v1/runs/{run}/trace/{query}"));
        let traced: Vec<&str> = trace["nodes"]
            .as_array()
            .expect("the nodes")
            .iter()
            .map(|node| node["node"].as_str().expect("a node id"))
            .collect();
        for node in nodes {
            assert!(traced.contains(node), "{node} in {traced:?}");
        }
        assert_eq!(
            trace["passages"]["status"], "verified",
            "{}",
            trace["passages"]
        );
    }

    println!(
        "journey over {SCIFACT} in {:.0} s\n  dense-only {dense}: {NDCG} {}\n  hybrid {hybrid}: {NDCG} {}\n  replayed {query}",
        started.elapsed().as_secs_f64(),
        ndcg["values"][0],
        ndcg["values"][1]
    );
}
