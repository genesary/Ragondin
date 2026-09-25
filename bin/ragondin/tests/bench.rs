//! The `bench` subcommand, exercised as a process.
//!
//! `bench` is the composition root, and a composition root is only exercised
//! by the thing it composes: the components it registers are compiled into the
//! binary, so a test that called a function in this crate would be registering
//! its own. Every test here spawns the built binary over the miniature BEIR
//! fixture beside it, and reads back what the run store holds afterwards.
//!
//! # What runs when
//!
//! The tests are split by the feature that carries the components they need
//! (ADR-C14): the lean build registers no retriever, so `just test` compiles
//! those tests away and `just test-features` is where they run. The two
//! refusals below need no component at all — a configuration is refused before
//! anything is constructed — so they run in both.

use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;

fn ragondin(args: &[&str]) -> Output {
    Command::cargo_bin("ragondin")
        .expect("the binary under test is built by `cargo test`")
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture(name: &str) -> String {
    fixtures()
        .join(name)
        .to_str()
        .expect("UTF-8 path")
        .to_owned()
}

/// A run store of this test's own, emptied first so a run left by a previous,
/// killed run of this test cannot decide this one.
fn store(test_name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("bench")
        .join(test_name);
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn path(buf: &Path) -> &str {
    buf.to_str().expect("UTF-8 path")
}

#[test]
fn a_selector_naming_a_format_this_build_does_not_read_is_refused() {
    let store = store("unknown-format");

    let output = ragondin(&[
        "bench",
        &fixture("lexical-pipeline.yaml"),
        "--benchmark",
        "trec/robust04",
        "--datasets",
        path(&fixtures()),
        "--store",
        path(&store),
    ]);

    assert!(!output.status.success(), "{}", stdout(&output));
    let error = stderr(&output);
    assert!(error.contains("trec"), "{error}");
    assert!(error.contains("beir"), "{error}");
}

#[test]
fn a_configuration_holding_an_extension_node_is_refused_before_anything_runs() {
    let store = store("extension");

    let output = ragondin(&[
        "bench",
        &fixture("extension-pipeline.yaml"),
        "--benchmark",
        "beir/beir-mini",
        "--datasets",
        path(&fixtures()),
        "--store",
        path(&store),
    ]);

    assert!(!output.status.success(), "{}", stdout(&output));
    let error = stderr(&output);
    assert!(error.contains("not supported in v0"), "{error}");
    assert!(
        error.contains("judged"),
        "the refusal names the node: {error}"
    );
    assert!(!store.exists(), "a refused configuration records no run");
}

/// A generator no build registers is refused by planning's unknown `impl:`,
/// naming the family and the name — in every build, lean included: a
/// `Remote` generator is named by an ordinary `impl:` name, and until
/// something binds it, it is a name this composition root does not know.
#[test]
fn a_generator_this_build_does_not_register_is_refused_naming_family_and_name() {
    let store = store("unregistered-generator");

    let output = ragondin(&[
        "bench",
        &fixture("unregistered-generator.yaml"),
        "--benchmark",
        "beir/beir-mini",
        "--datasets",
        path(&fixtures()),
        "--store",
        path(&store),
    ]);

    assert!(!output.status.success(), "{}", stdout(&output));
    let error = stderr(&output);
    assert!(
        error.contains("no generator implementation is registered under `vllm`"),
        "{error}"
    );
    assert!(!store.exists(), "a refused configuration records no run");
}

/// What a build with no retriever does with a configuration that names one.
///
/// The claim `Cargo.toml` makes for the lean build: it still reads, validates
/// and plans a configuration naming a component it does not carry, and refuses
/// it by name rather than by mystery.
#[cfg(not(feature = "bm25"))]
#[test]
fn a_build_without_the_component_names_the_impl_it_cannot_resolve() {
    let store = store("lean");

    let output = ragondin(&[
        "bench",
        &fixture("lexical-pipeline.yaml"),
        "--benchmark",
        "beir/beir-mini",
        "--datasets",
        path(&fixtures()),
        "--store",
        path(&store),
    ]);

    assert!(!output.status.success(), "{}", stdout(&output));
    let error = stderr(&output);
    assert!(error.contains("bm25"), "{error}");
}

#[cfg(feature = "bm25")]
mod with_components {
    use ragondin_experiments::{FileSystemRunStore, RunId};

    use super::*;

    /// The `run <id>` line `bench` prints first.
    fn reported_run_id(summary: &str) -> RunId {
        let first = summary.lines().next().expect("a summary has a first line");
        let id = first
            .strip_prefix("run ")
            .unwrap_or_else(|| panic!("the summary opens with the run id: {summary}"));
        id.parse().expect("the printed id is a run id")
    }

    #[test]
    fn bench_scores_the_pipeline_prints_the_run_and_records_it() {
        let store = store("lexical");

        let output = ragondin(&[
            "bench",
            &fixture("lexical-pipeline.yaml"),
            "--benchmark",
            "beir/beir-mini",
            "--datasets",
            path(&fixtures()),
            "--store",
            path(&store),
        ]);

        assert!(output.status.success(), "{}", stderr(&output));
        let summary = stdout(&output);
        assert!(summary.contains("ndcg@10:"), "{summary}");

        // The run is in the store, under the identity that was printed, with
        // the metrics that were printed: the summary is a view of the record
        // rather than a second, unrelated report.
        let id = reported_run_id(&summary);
        let run = FileSystemRunStore::new(&store)
            .load(&id)
            .expect("the run bench reported is the run bench saved");
        let ndcg = run
            .metrics
            .get("ndcg@10")
            .expect("a retrieval run scores nDCG at the cutoff");
        assert!(
            summary.contains(&format!("ndcg@10: {ndcg:.4}")),
            "{summary}"
        );

        // BM25 over this corpus finds the judged documents — a pipeline that
        // retrieved nothing would score zero and still exit zero, which is the
        // one way this test could pass while saying nothing.
        assert!(ndcg > 0.0, "the lexical leg retrieved nothing: {summary}");
        assert_eq!(
            run.inputs.model_hashes.len(),
            0,
            "bm25 reads no model, so the run records no model hash"
        );
    }

    /// The hybrid configuration: a lexical leg and a dense one, fused.
    ///
    /// The model is the repository's one committed tiny embedder, which lives
    /// where it is generated — `components/ragondin-embedder-onnx/tests/
    /// fixtures`, from `generate.py` beside it. It is read from there rather
    /// than copied here: a second copy would be an opaque binary with no
    /// generator next to it, which is what that crate's fixtures exist not to
    /// be. It embeds nothing meaningful, and nothing here reads a number the
    /// dense leg alone produces — what this test exercises is that the dense
    /// leg is constructed from the corpus this run prepared, and runs.
    #[cfg(feature = "onnx")]
    fn hybrid_config() -> String {
        let embedder = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../components/ragondin-embedder-onnx/tests/fixtures");
        let config = format!(
            "pipeline:\n  \
             inputs: [question]\n  \
             nodes:\n    \
             - id: lexical\n      component: retriever\n      impl: bm25\n      \
             inputs: [question]\n      params: {{ top_k: 10 }}\n    \
             - id: vectors\n      component: retriever\n      impl: dense\n      \
             inputs: [question]\n      params: {{ top_k: 10, embedder: onnx, model: {model}, \
             tokenizer: {tokenizer} }}\n    \
             - id: fused\n      component: fusion\n      impl: rrf\n      \
             inputs: [lexical, vectors]\n      params: {{ k: 60 }}\n",
            model = embedder.join("tiny-embedder.onnx").display(),
            tokenizer = embedder.join("tokenizer.json").display(),
        );

        // Written rather than committed: the paths are absolute, because a
        // relative one in a configuration file would resolve against whatever
        // directory the process was started in.
        let written = Path::new(env!("CARGO_TARGET_TMPDIR")).join("bench/hybrid-pipeline.yaml");
        std::fs::create_dir_all(written.parent().expect("a parent")).expect("a writable tmpdir");
        std::fs::write(&written, config).expect("a writable tmpdir");
        written.to_str().expect("UTF-8 path").to_owned()
    }

    #[cfg(feature = "onnx")]
    #[test]
    fn bench_runs_the_hybrid_pipeline_end_to_end() {
        let store = store("hybrid");

        let output = ragondin(&[
            "bench",
            &hybrid_config(),
            "--benchmark",
            "beir/beir-mini",
            "--datasets",
            path(&fixtures()),
            "--store",
            path(&store),
        ]);

        assert!(output.status.success(), "{}", stderr(&output));
        let summary = stdout(&output);
        assert!(summary.contains("ndcg@10:"), "{summary}");

        let run = FileSystemRunStore::new(&store)
            .load(&reported_run_id(&summary))
            .expect("the run bench reported is the run bench saved");
        assert_eq!(
            run.traces.len(),
            3,
            "one trace per query of the benchmark, from the executor's return value"
        );
        // A floor, not a discriminating number: BM25 alone already finds this
        // corpus whole, so the fixture cannot separate the dense leg's
        // contribution from the lexical one's. What the floor rules out is a
        // fusion that ranked nothing.
        let ndcg = run
            .metrics
            .get("ndcg@10")
            .expect("a retrieval run scores nDCG at the cutoff");
        assert!(
            ndcg > 0.0,
            "the fused pipeline retrieved nothing: {summary}"
        );
        // The corpus was embedded by a model, so the run records which one —
        // without it two runs over two models would content-address alike.
        let digest = run
            .inputs
            .model_hashes
            .get("embedder")
            .expect("a dense run records its embedder");
        assert!(summary.contains(digest), "{summary}");
    }
}
