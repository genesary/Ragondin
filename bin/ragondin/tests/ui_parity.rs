//! The M4 exit criterion's first half, as a test: **everything the command
//! line does, the UI does without a file or a terminal, and the converse** —
//! a pipeline composed in the front end and exported hashes identically under
//! `ragondin validate` (the front-end design, § 2; ADR-016's Consequences).
//!
//! Everything here runs through the built binary: `ragondin ui` on a
//! workspace of the test's own, on loopback and an ephemeral port, asked over
//! a real connection (`support/ui.rs`); and `ragondin validate` and `ragondin
//! bench`, spawned beside it over the same documents and benchmarks.
//!
//! - **Hash parity, both directions.** A document written through
//!   `PUT /pipelines/{name}` as the editor writes it — the typed document
//!   (ADR-C40 § 5) — and exported is the file the workspace stores, byte for
//!   byte, and `ragondin validate` on it prints the hash
//!   `POST /pipelines/validate` showed. And the converse: a file the command
//!   line validates keeps its hash once the API reads it, and once the editor
//!   holds it and the server renders it again, its comments gone. No text is
//!   normalised to make two hashes agree (INV-8): each is computed by the
//!   binary over the bytes it was given.
//! - **Validate-error parity.** `incompatible-wiring.yaml` is refused by both
//!   with the same edge and the same two kinds.
//! - **Run parity over the three formats.** `POST /runs` over `beir-mini/`,
//!   `qa-mini/` and `squad-mini/`, imported into the workspace, files a run
//!   with the id `ragondin bench` gives the same document on the same
//!   benchmark, and the same identity, metrics, configuration and bindings,
//!   file for file (P1: one engine, one path). With a fake `Remote` generator
//!   bound in the workspace, the run records the binding `bench --remote`
//!   records.
//!
//! # What runs when
//!
//! The runs need BM25 and, for the two answer benchmarks, a generator in
//! process — the stub — and the server needs `ui`: compiled under `ui`,
//! `bm25` and `stub` together, and the `Remote` case under `remote` too, so
//! `just test-features` (and CI's `--all-features` run) is where it runs.

#![cfg(all(feature = "ui", feature = "bm25", feature = "stub"))]

#[cfg(feature = "remote")]
#[path = "support/remote.rs"]
mod remote;
#[path = "support/ui.rs"]
mod ui;
#[path = "support/workspace.rs"]
mod workspace;

use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use ui::Server;

/// A directory of this test's own under `CARGO_TARGET_TMPDIR`, emptied first.
fn scratch(test_name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("ui-parity")
        .join(test_name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("the directory is created");
    root
}

/// `ragondin ui` over an empty workspace under `root`.
fn empty_workspace(root: &Path) -> (PathBuf, Server) {
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).expect("the workspace is created");
    let server = workspace::serve(&workspace);
    (workspace, server)
}

fn ragondin(args: &[&str]) -> Output {
    Command::cargo_bin("ragondin")
        .expect("the binary under test is built by `cargo test`")
        .args(args)
        .output()
        .expect("the binary runs")
}

/// The hash `ragondin validate` prints for the file at `path`.
fn cli_hash(path: &Path) -> String {
    let output = ragondin(&["validate", path.to_str().expect("UTF-8 path")]);
    let printed = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        output.status.success(),
        "validate {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    printed
        .lines()
        .find_map(|line| line.strip_prefix("content hash: "))
        .unwrap_or_else(|| panic!("validate prints the hash: {printed}"))
        .to_owned()
}

fn string(value: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "string", "value": value })
}

fn int(value: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "int", "value": value })
}

/// A hybrid pipeline as the editor holds it: two legs, fused, reranked —
/// every parameter tagged with its kind, nodes in the order they were placed
/// on the canvas.
fn composed() -> serde_json::Value {
    serde_json::json!({
        "pipeline": {
            "inputs": ["query"],
            "nodes": [
                {
                    "id": "lexical", "component": "retriever", "impl": "bm25",
                    "inputs": ["query"], "params": { "top_k": int("20") }
                },
                {
                    "id": "vectors", "component": "retriever", "impl": "dense",
                    "inputs": ["query"],
                    "params": {
                        "top_k": int("20"),
                        "embedder": string("onnx"),
                        "model": string("models/embedder.onnx"),
                        "tokenizer": string("models/tokenizer.json"),
                    }
                },
                {
                    "id": "fused", "component": "fusion", "impl": "rrf",
                    "inputs": ["lexical", "vectors"], "params": { "k": int("60") }
                },
                {
                    "id": "reranked", "component": "reranker", "impl": "cross_encoder",
                    "inputs": ["query", "fused"],
                    "params": {
                        "top_k": int("10"),
                        "model": string("models/cross-encoder.onnx"),
                        "tokenizer": string("models/tokenizer.json"),
                    }
                }
            ]
        }
    })
}

#[test]
fn a_pipeline_composed_in_the_editor_and_exported_hashes_identically_under_validate() {
    let root = scratch("composed");
    let (workspace, server) = empty_workspace(&root);
    let typed = composed();

    // What the editor shows while composing: the hash and the rendering of
    // exactly the bytes a write would store.
    let shown = workspace::call(
        &server,
        "POST",
        "/api/v1/pipelines/validate",
        &serde_json::json!({ "typed": typed }),
        200,
    );
    let hash = shown["hash"].as_str().expect("a hash");
    let rendering = shown["rendering"].as_str().expect("a rendering");

    // The editor's write.
    let written = ui::send_json_with(
        server.authority(),
        "PUT",
        "/api/v1/pipelines/composed",
        &serde_json::json!({ "typed": typed }).to_string(),
        &[("If-None-Match", "*")],
    );
    assert_eq!(written.status, 200, "{written:?}");
    let written: serde_json::Value = serde_json::from_str(&written.body).unwrap();
    assert_eq!(written["hash"], hash);

    // The export is the stored file, byte for byte, and what was shown.
    let stored = std::fs::read_to_string(workspace.join("pipelines/composed.yaml"))
        .expect("the document is stored as a file");
    let detail = workspace::read(&server, "/api/v1/pipelines/composed");
    let exported = detail["document"].as_str().expect("the document");
    assert_eq!(exported, stored);
    assert_eq!(exported, rendering);
    assert_eq!(detail["canonical"], true);
    assert_eq!(detail["hash"], hash);

    // The exported file, outside any workspace, under the command line.
    let file = root.join("exported.yaml");
    std::fs::write(&file, exported).expect("the export is written");
    assert_eq!(cli_hash(&file), hash);
}

#[test]
fn a_file_the_command_line_validates_keeps_its_hash_through_the_api_and_the_editor() {
    let root = scratch("converse");
    let (_workspace, server) = empty_workspace(&root);

    for (name, file) in [
        ("hybrid-rerank", "exit-criterion/hybrid-rerank.yaml"),
        ("dense-only", "exit-criterion/dense-only.yaml"),
        ("generation", "stub-generation-bench.yaml"),
    ] {
        let path = workspace::fixtures().join(file);
        let text = std::fs::read_to_string(&path).expect("the fixture reads");
        let hash = cli_hash(&path);

        let imported = workspace::put_text(&server, name, &text);
        assert_eq!(imported["hash"], hash.as_str(), "{file}");

        // The editor opens it as a typed document, and the server renders
        // that again: the comments and the flow mappings go, the hash stays.
        let detail = workspace::read(&server, &format!("/api/v1/pipelines/{name}"));
        assert_eq!(detail["document"], text.as_str(), "{file}: stored verbatim");
        assert_eq!(detail["canonical"], false, "{file}: written by hand");
        let rendered = workspace::call(
            &server,
            "POST",
            "/api/v1/pipelines/validate",
            &serde_json::json!({ "typed": detail["typed"] }),
            200,
        );
        assert_eq!(rendered["hash"], hash.as_str(), "{file}");
        let rendering = rendered["rendering"].as_str().expect("a rendering");
        assert_ne!(rendering, text, "{file}: the rendering is the server's own");

        let again = root.join(format!("{name}.rendered.yaml"));
        std::fs::write(&again, rendering).expect("the rendering is written");
        assert_eq!(cli_hash(&again), hash, "{file}");
    }
}

#[test]
fn incompatible_wiring_is_located_on_the_same_edge_and_kinds_by_both() {
    let root = scratch("wiring");
    let (_workspace, server) = empty_workspace(&root);
    let path = workspace::fixtures().join("incompatible-wiring.yaml");
    let text = std::fs::read_to_string(&path).expect("the fixture reads");

    let cli = ragondin(&["validate", path.to_str().unwrap()]);
    assert!(!cli.status.success(), "the command line refuses it");
    let report = String::from_utf8(cli.stderr).expect("stderr is UTF-8");
    let api = ui::send_json(
        server.authority(),
        "POST",
        "/api/v1/pipelines/validate",
        &serde_json::json!({ "document": text }).to_string(),
    );
    assert_eq!(api.status, 422, "{api:?}");
    let problem: serde_json::Value = serde_json::from_str(&api.body).unwrap();
    assert_eq!(problem["code"], "pipeline_invalid");

    // The edge and the kinds, as the command line prints them...
    let field = |key: &str| -> String {
        report
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or_else(|| panic!("`{key}` in {report}"))
            .trim()
            .to_owned()
    };
    let (edge, expected, found) = (field("edge:"), field("expected:"), field("found:"));
    // ...are the API's location, and its detail says them in the same words.
    let location = &problem["location"]["edge"];
    assert_eq!(
        edge,
        format!(
            "`{}` feeds `{}` at port {}",
            location["from"].as_str().unwrap(),
            location["to"].as_str().unwrap(),
            location["port"]
        ),
        "{report}\n{problem}"
    );
    let detail = problem["detail"].as_str().expect("a detail");
    for line in [
        format!("edge: {edge}"),
        format!("expected: {expected}"),
        format!("found: {found}"),
    ] {
        assert!(detail.contains(&line), "{line:?} in {detail}");
    }
}

/// One benchmark of each format `bench` reads: the fixture to import, the
/// name to import it as, its selector, and a document that runs on it.
const FORMATS: [(&str, &str, &str, &str); 3] = [
    (
        "beir-mini",
        "beir-mini",
        "beir/beir-mini",
        "lexical-pipeline.yaml",
    ),
    (
        "qa-mini",
        "qa-mini",
        "beir-qa/qa-mini",
        "stub-generation-bench.yaml",
    ),
    (
        "squad-mini/dev-v1.1.json",
        "squad-mini",
        "squad/squad-mini",
        "stub-generation-bench.yaml",
    ),
];

/// The run files that carry what a run is and what it scored — everything
/// but its traces (whose node durations are measured, so never twice the
/// same), its times and its launch record, which say when and how it was
/// launched and are outside its identity.
const RUN_FILES: [&str; 4] = [
    "inputs.json",
    "metrics.json",
    "config.yaml",
    "bindings.json",
];

/// Asserts the run `id` holds the same bytes, file for file, in both stores.
fn assert_same_run(api_store: &Path, bench_store: &Path, id: &str, what: &str) {
    for file in RUN_FILES {
        let read = |store: &Path| {
            std::fs::read(store.join(id).join(file))
                .unwrap_or_else(|error| panic!("{what}: {id}/{file}: {error}"))
        };
        assert_eq!(
            read(api_store),
            read(bench_store),
            "{what}: {file} differs between the API's run and bench's"
        );
    }
}

#[test]
fn a_run_submitted_through_the_api_is_the_run_bench_records_on_each_format() {
    let root = scratch("formats");
    let (workspace, server) = empty_workspace(&root);
    let fixtures = workspace::fixtures();

    for (source, name, selector, config) in FORMATS {
        let imported = workspace::import(&server, name, &fixtures.join(source));
        assert_eq!(imported, selector);
        let document_name = format!("{name}-pipeline");
        let text = std::fs::read_to_string(fixtures.join(config)).expect("the config reads");
        workspace::put_text(&server, &document_name, &text);

        let through_api = workspace::launch(&server, &document_name, selector);
        let bench_store = root.join("bench").join(name);
        let through_bench = workspace::bench(
            &fixtures,
            &fixtures.join(config),
            selector,
            &fixtures,
            &bench_store,
            &[],
        );

        assert_eq!(through_api, through_bench, "{selector}: two run ids");
        assert_same_run(
            &workspace.join("runs"),
            &bench_store,
            &through_api,
            selector,
        );
        let metrics: serde_json::Value = serde_json::from_slice(
            &std::fs::read(bench_store.join(&through_bench).join("metrics.json")).unwrap(),
        )
        .unwrap();
        assert!(
            metrics.as_object().is_some_and(|m| !m.is_empty()),
            "{selector}: the run was scored: {metrics}"
        );
    }
}

#[cfg(feature = "remote")]
#[test]
fn a_workspace_binding_is_recorded_on_the_run_as_bench_records_its_remote_argument() {
    let root = scratch("remote");
    let (workspace, server) = empty_workspace(&root);
    let fixtures = workspace::fixtures();
    let generator = remote::serve_generator();
    let document = format!(
        "pipeline:\n  inputs: [question]\n  nodes:\n    - id: search\n      component: retriever\n      \
         impl: bm25\n      inputs: [question]\n      params: {{ top_k: 3 }}\n    - id: prompt\n      \
         component: context_builder\n      impl: concat\n      inputs: [question, search]\n      \
         params: {{ budget: 2000, separator: \"\\n\" }}\n    - id: answer\n      component: generator\n      \
         impl: vllm\n      inputs: [question, prompt]\n      params:\n        served_model: {}\n        \
         template: \"Answer from the context.\\n{{context}}\\nQuestion: {{query}}\"\n",
        remote::GENERATOR_MODEL
    );
    let config = root.join("remote-generator.yaml");
    std::fs::write(&config, &document).expect("the document is written");

    workspace::import(&server, "qa-mini", &fixtures.join("qa-mini"));
    workspace::call(
        &server,
        "PUT",
        "/api/v1/services/generator/vllm",
        &serde_json::json!({ "uri": generator.uri }),
        200,
    );
    workspace::put_text(&server, "remote-generator", &document);
    let through_api = workspace::launch(&server, "remote-generator", "beir-qa/qa-mini");

    let bench_store = root.join("bench");
    let binding = format!("generator/vllm={}", generator.uri);
    let through_bench = workspace::bench(
        &root,
        &config,
        "beir-qa/qa-mini",
        &fixtures,
        &bench_store,
        &["--remote", &binding],
    );

    assert_eq!(through_api, through_bench);
    assert_same_run(
        &workspace.join("runs"),
        &bench_store,
        &through_api,
        "the bound generator",
    );
    let detail = workspace::read(&server, &format!("/api/v1/runs/{through_api}"));
    assert_eq!(
        detail["bindings"],
        serde_json::json!([{ "family": "generator", "name": "vllm", "uri": generator.uri }])
    );
    assert_eq!(
        detail["inputs"]["model_hashes"]["generator"],
        remote::GENERATOR_IDENTITY,
        "{detail}"
    );
}
