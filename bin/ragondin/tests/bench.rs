//! The `bench` subcommand, exercised as a process.
//!
//! `bench` is the composition root, and a composition root is only exercised
//! by the thing it composes: the components it registers are compiled into the
//! binary, so a test that called a function in this crate would be registering
//! its own. Every test here spawns the built binary over one of the miniature
//! fixtures beside it — BEIR (`beir-mini/`), BEIR with reference answers
//! (`qa-mini/`), SQuAD (`squad-mini/`) — and reads back what the run store holds
//! afterwards.
//!
//! # What runs when
//!
//! The tests are split by the feature that carries the components they need
//! (ADR-C14): the lean build registers no retriever and no generator, so `just
//! test` compiles those tests away and `just test-features` is where they run.
//! The four refusals at the top — an unknown benchmark format, an extension
//! node, a generator no build registers, a node missing a required key — need
//! no component this build lacks,
//! since each is refused before anything runs, so they run in both. So does
//! the refusal of a malformed `--remote` argument.
//!
//! The tests that bind a component with `--remote` run under the `remote`
//! feature, against fake services this process starts (`support/remote.rs`):
//! `tonic` servers hosting in-test components, and no inference server.

use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;

#[cfg(feature = "remote")]
#[path = "support/remote.rs"]
mod remote;

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

/// A generator no build registers is refused before the benchmark loads, in
/// planning's words for an unknown `impl:`, naming the family and the name —
/// in every build, lean included: a
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

/// A node missing a required key is refused before the benchmark loads, in
/// every build: the key check runs before anything is read, so a benchmark
/// that does not exist is never reached.
#[test]
fn a_node_without_its_required_top_k_is_refused_before_the_benchmark_loads() {
    let store = store("missing-top-k");

    let output = ragondin(&[
        "bench",
        &fixture("missing-top-k.yaml"),
        "--benchmark",
        "beir/no-such-benchmark",
        "--datasets",
        path(&fixtures()),
        "--store",
        path(&store),
    ]);

    assert!(!output.status.success(), "{}", stdout(&output));
    let error = stderr(&output);
    assert!(error.contains("node `search`"), "{error}");
    assert!(error.contains("`top_k` is required"), "{error}");
    assert!(!error.contains("no-such-benchmark"), "{error}");
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

/// The `run <id>` line `bench` prints first.
#[cfg(any(feature = "bm25", feature = "remote"))]
fn reported_run_id(summary: &str) -> ragondin_experiments::RunId {
    let first = summary.lines().next().expect("a summary has a first line");
    let id = first
        .strip_prefix("run ")
        .unwrap_or_else(|| panic!("the summary opens with the run id: {summary}"));
    id.parse().expect("the printed id is a run id")
}

/// A malformed binding is refused on its text, in every build, before the
/// configuration is read (ADR-C32 § 2): the configuration named here does not
/// exist, and it is not what the refusal is about.
#[test]
fn a_malformed_remote_argument_is_refused_naming_it_before_anything_is_read() {
    let store = store("malformed-remote");

    let output = ragondin(&[
        "bench",
        &fixture("no-such-configuration.yaml"),
        "--benchmark",
        "beir/beir-mini",
        "--datasets",
        path(&fixtures()),
        "--store",
        path(&store),
        "--remote",
        "embedder/bge=https://embedder.internal",
    ]);

    assert!(!output.status.success(), "{}", stdout(&output));
    let error = stderr(&output);
    assert!(
        error.contains("--remote embedder/bge=https://embedder.internal"),
        "{error}"
    );
    assert!(!error.contains("no-such-configuration"), "{error}");
    assert!(!store.exists(), "a refused binding records no run");
}

/// A build without the `remote` feature parses a binding and then refuses it,
/// naming the feature, so a lean build never runs a configuration whose
/// bound component it would silently lack (ADR-C32 § 2).
#[cfg(not(feature = "remote"))]
#[test]
fn a_build_without_the_remote_feature_refuses_a_binding_naming_the_feature() {
    let store = store("lean-remote");

    let output = ragondin(&[
        "bench",
        &fixture("remote-dense-rerank.yaml"),
        "--benchmark",
        "beir/beir-mini",
        "--datasets",
        path(&fixtures()),
        "--store",
        path(&store),
        "--remote",
        "embedder/bge=http://127.0.0.1:50051",
    ]);

    assert!(!output.status.success(), "{}", stdout(&output));
    assert!(
        stderr(&output).contains("`remote` feature"),
        "{}",
        stderr(&output)
    );
    assert!(!store.exists(), "a refused binding records no run");
}

#[cfg(feature = "remote")]
mod with_remote_components {
    use ragondin_experiments::{FileSystemRunStore, Run};

    use super::remote::{self, FakeEmbedder, Service};
    use super::*;

    /// The embedder and reranker services the fixture's two bound names are
    /// bound to, and the `--remote` arguments that bind them.
    struct Services {
        embedder: FakeEmbedder,
        _services: [Service; 2],
        arguments: Vec<String>,
    }

    fn services() -> Services {
        let embedder = FakeEmbedder::default();
        let embedder_service = remote::serve_embedder(embedder.clone());
        let reranker_service = remote::serve_reranker();
        let arguments = vec![
            "--remote".to_owned(),
            format!("embedder/bge={}", embedder_service.uri),
            "--remote".to_owned(),
            format!("reranker/bge-reranker={}", reranker_service.uri),
        ];
        Services {
            embedder,
            _services: [embedder_service, reranker_service],
            arguments,
        }
    }

    fn bench(store: &Path, remote: &[String]) -> Output {
        let config = fixture("remote-dense-rerank.yaml");
        let datasets = fixtures();
        let mut arguments: Vec<&str> = vec![
            "bench",
            &config,
            "--benchmark",
            "beir/beir-mini",
            "--datasets",
            path(&datasets),
            "--store",
            path(store),
        ];
        arguments.extend(remote.iter().map(String::as_str));
        ragondin(&arguments)
    }

    fn saved(store: &Path, output: &Output) -> Run {
        assert!(output.status.success(), "{}", stderr(output));
        FileSystemRunStore::new(store)
            .load(&reported_run_id(&stdout(output)))
            .expect("the run bench reported is the run bench saved")
    }

    #[test]
    fn bench_over_a_bound_embedder_and_reranker_records_the_identities_their_services_report() {
        let store = store("remote-dense-rerank");
        let services = services();

        let run = saved(&store, &bench(&store, &services.arguments));

        // The fakes answer only for the served model each node names, so an
        // identity recorded at all was read with that name (ADR-C32 § 4).
        assert_eq!(
            run.inputs.model_hashes.get("embedder").map(String::as_str),
            Some(remote::EMBEDDER_IDENTITY)
        );
        assert_eq!(
            run.inputs.model_hashes.get("reranker").map(String::as_str),
            Some(remote::RERANKER_IDENTITY)
        );
        assert!(
            run.metrics.get("ndcg@10").is_some(),
            "the run was scored: {:?}",
            run.metrics
        );
        // The bindings are on the run, as written on the command line, in
        // the order given (ADR-C32 § 2).
        let recorded: Vec<String> = run
            .bindings
            .iter()
            .map(|binding| format!("{}/{}={}", binding.family, binding.name, binding.uri))
            .collect();
        assert_eq!(
            recorded,
            [services.arguments[1].clone(), services.arguments[3].clone()]
        );
    }

    #[test]
    fn the_corpus_and_the_queries_go_through_the_bound_embedder_with_the_node_s_prefixes() {
        let store = store("remote-prefixes");
        let services = services();

        saved(&store, &bench(&store, &services.arguments));

        // The adapter applies the prefixes, and the text on the wire is final
        // (ADR-C32 § 4): the service sees the passage prefix on the corpus,
        // which the execution embedded, and the query prefix on every query the
        // dense retriever embedded. Every call named the served model, or the
        // fake would have refused it and the run failed.
        let texts = services.embedder.texts();
        let passages = texts
            .iter()
            .filter(|text| text.starts_with("passage: "))
            .count();
        let queries = texts
            .iter()
            .filter(|text| text.starts_with("query: "))
            .count();
        assert!(passages > 0, "the corpus was embedded remotely: {texts:?}");
        assert!(queries > 0, "the queries were embedded remotely: {texts:?}");
        assert_eq!(passages + queries, texts.len(), "{texts:?}");
    }

    #[test]
    fn where_a_service_listens_is_not_part_of_the_run_s_identity() {
        // Two benches of one configuration against two pairs of services on
        // two pairs of ports: one experiment, so one `run_id` (ADR-C32 § 2).
        let first_store = store("remote-identity-first");
        let second_store = store("remote-identity-second");
        let first = services();
        let second = services();
        assert_ne!(first.arguments, second.arguments);

        let first_run = saved(&first_store, &bench(&first_store, &first.arguments));
        let second_run = saved(&second_store, &bench(&second_store, &second.arguments));

        assert_eq!(first_run.id, second_run.id);
        // Each run still records where it was answered from: provenance,
        // outside identity.
        assert_ne!(first_run.bindings, second_run.bindings);
    }

    #[test]
    fn a_stored_run_is_refused_before_execution() {
        // The bound embedder is the observer: it sees every text the corpus
        // embedding and the queries send it. A second bench of a run the
        // store holds sends it nothing — the refusal comes after the identity
        // read and before step 4 (ADR-C39 § 8).
        let store = store("remote-refused-before-execution");
        let services = services();
        let first = saved(&store, &bench(&store, &services.arguments));
        let run_dir = store.join(first.id.to_string());
        let embedded = services.embedder.texts().len();
        assert!(embedded > 0, "the first run embedded through the observer");
        let stored = std::fs::read_dir(&run_dir)
            .expect("the run directory reads")
            .map(|entry| {
                let path = entry.expect("an entry reads").path();
                let bytes = std::fs::read(&path).expect("a run file reads");
                (path, bytes)
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let second = bench(&store, &services.arguments);

        assert!(!second.status.success(), "{}", stdout(&second));
        assert_eq!(
            stderr(&second),
            format!(
                "error: run {}: already stored; this execution was not kept\n",
                first.id
            )
        );
        assert_eq!(stdout(&second), "");
        assert_eq!(
            services.embedder.texts().len(),
            embedded,
            "nothing was embedded and no query ran"
        );
        for (path, bytes) in stored {
            assert_eq!(
                std::fs::read(&path).expect("a run file reads"),
                bytes,
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn bench_over_a_bound_generator_records_its_identity_and_binding_and_scores_its_answers() {
        let store = store("remote-generation");
        let embedder = remote::serve_embedder(FakeEmbedder::default());
        let builder = remote::serve_context_builder();
        let generator = remote::serve_generator();
        let bindings = [
            format!("embedder/bge={}", embedder.uri),
            format!("context_builder/lines={}", builder.uri),
            format!("generator/vllm={}", generator.uri),
        ];
        let config = fixture("remote-generation.yaml");
        let datasets = fixtures();
        let mut arguments: Vec<&str> = vec![
            "bench",
            &config,
            "--benchmark",
            "beir-qa/qa-mini",
            "--datasets",
            path(&datasets),
            "--store",
            path(&store),
        ];
        for binding in &bindings {
            arguments.extend(["--remote", binding.as_str()]);
        }

        let run = saved(&store, &ragondin(&arguments));

        // The generator's identity was read with its node's `served_model`:
        // the fake refuses every other name (ADR-C31 § 4).
        assert_eq!(
            run.inputs.model_hashes.get("generator").map(String::as_str),
            Some(remote::GENERATOR_IDENTITY)
        );
        assert_eq!(
            run.inputs
                .model_hashes
                .get("context_builder")
                .map(String::as_str),
            Some(remote::CONTEXT_BUILDER_IDENTITY)
        );
        let recorded: Vec<String> = run
            .bindings
            .iter()
            .map(|binding| format!("{}/{}={}", binding.family, binding.name, binding.uri))
            .collect();
        assert_eq!(recorded, bindings);
        // Answers came back over the wire and were scored against the
        // references, which is what a benchmark carrying answers requires.
        for metric in ["exact_match", "token_f1"] {
            assert!(run.metrics.get(metric).is_some(), "{:?}", run.metrics);
        }
    }

    #[test]
    fn a_binding_no_node_uses_is_refused_before_the_benchmark_is_loaded() {
        let store = store("remote-unused");
        let services = services();
        let mut arguments = services.arguments.clone();
        arguments.extend([
            "--remote".to_owned(),
            "generator/vllm=http://127.0.0.1:1".to_owned(),
        ]);

        let output = bench(&store, &arguments);

        assert!(!output.status.success(), "{}", stdout(&output));
        let error = stderr(&output);
        assert!(error.contains("generator/vllm"), "{error}");
        assert!(error.contains("no node"), "{error}");
        assert!(!store.exists(), "a refused binding records no run");
    }

    #[test]
    fn an_unreachable_service_ends_the_run_at_the_identity_read_naming_the_node() {
        let store = store("remote-unreachable");
        let services = services();
        let arguments = vec![
            services.arguments[0].clone(),
            services.arguments[1].clone(),
            "--remote".to_owned(),
            format!("reranker/bge-reranker={}", remote::unreachable_uri()),
        ];

        let output = bench(&store, &arguments);

        assert!(!output.status.success(), "{}", stdout(&output));
        let error = stderr(&output);
        // The node's context reads exactly as it always has, whatever it
        // carries for `ragondin ui`'s launcher.
        assert!(
            error.starts_with("error: node `reranked`\n  caused by: "),
            "{error}"
        );
        assert!(error.contains("unavailable"), "{error}");
        assert!(!store.exists(), "a refused run records nothing");
    }
}

#[cfg(feature = "bm25")]
mod with_components {
    use ragondin_experiments::FileSystemRunStore;

    use super::*;

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

    /// What `bench` printed over the M2 and M3 fixtures before its steps were
    /// split into the preparation and the execution `ragondin ui`'s launcher
    /// shares with it — recorded from the binary built before the split, and
    /// pinned literally, run id included: the split moved code, not output.
    #[test]
    fn bench_prints_and_files_what_it_did_before_the_split() {
        let cases = [
            ("lexical-pipeline.yaml", "beir/beir-mini", LEXICAL_GOLDEN),
            #[cfg(feature = "stub")]
            (
                "stub-generation-bench.yaml",
                "squad/squad-mini",
                GENERATION_GOLDEN,
            ),
        ];
        for (config, benchmark, golden) in cases {
            let store = store(&format!("golden-{config}"));

            let output = ragondin(&[
                "bench",
                &fixture(config),
                "--benchmark",
                benchmark,
                "--datasets",
                path(&fixtures()),
                "--store",
                path(&store),
            ]);

            assert!(output.status.success(), "{}", stderr(&output));
            assert_eq!(stdout(&output), golden, "{config}");
            let id = reported_run_id(golden);
            FileSystemRunStore::new(&store)
                .load(&id)
                .expect("the run is filed under the id it always had");
        }
    }

    const LEXICAL_GOLDEN: &str = concat!(
        "run 4f66078d4e4173c98c7769ff0114bd36fa88ba634fa9184df2018f377c8f78b8\n",
        "  pipeline      26569fa152e509446c2b9bc6c4734d6a092e887e026183c0bcee861548517c8f\n",
        "  dataset       6910c589cedaa5c024928381be132a210b39d7d8eaec02b442a4da8de9e6e51c\n",
        "  index         4317ad07139072337d02d2d6a21554f318807ded3aec079eeb3c7016aa441168\n",
        "mrr: 1.0000\n",
        "ndcg@10: 1.0000\n",
        "recall@10: 1.0000\n",
    );

    #[cfg(feature = "stub")]
    const GENERATION_GOLDEN: &str = concat!(
        "run b6d06eefc434f7f9716b62bd95c451ec5ea6e745d2598668e54dd8b8b8c199c5\n",
        "  pipeline      4ae8cfcc6a0d048b839b83f53975e72cc0d1b95c18c4cf728517c4d3e4736de1\n",
        "  dataset       cd5838b7d2755f91362604f5036b19723c76065f589bbda12ebe3fa564d5682c\n",
        "  index         d1424378b3520bfb31b64e73ba4480e326842c56d028c884c55d2ba7b87aa54f\n",
        "  model[context_builder]  sha256:2483887ed2fe9664ba2a2618f196184b7500f0cf0f0018be46ea1ce99a505112\n",
        "  model[generator]  ragondin-stub/generator:first-line-of-context:v1\n",
        "exact_match: 1.0000\n",
        "mrr: 1.0000\n",
        "ndcg@10: 1.0000\n",
        "recall@10: 1.0000\n",
        "token_f1: 1.0000\n",
    );

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

    /// `bench` over a generation pipeline and a miniature benchmark with
    /// reference answers: the run carries the generation metrics beside the
    /// retrieval ones, and the identities of the builder and the generator.
    ///
    /// The generator is the stub, which needs no model and no service — the
    /// only generator a build can carry in-process, behind the `stub` feature.
    #[cfg(feature = "stub")]
    #[test]
    fn bench_scores_a_generation_pipeline_by_exact_match_and_token_f1() {
        let store = store("generation");

        let output = ragondin(&[
            "bench",
            &fixture("stub-generation-bench.yaml"),
            "--benchmark",
            "beir-qa/qa-mini",
            "--datasets",
            path(&fixtures()),
            "--store",
            path(&store),
        ]);

        assert!(output.status.success(), "{}", stderr(&output));
        let summary = stdout(&output);
        let run = FileSystemRunStore::new(&store)
            .load(&reported_run_id(&summary))
            .expect("the run bench reported is the run bench saved");

        // BM25 ranks each question's passage first, so the stub answers with
        // its first line — the reference — on both queries. A pipeline whose
        // answer never reached the harness would score zero and still exit
        // zero; the value is what rules that out.
        for metric in ["exact_match", "token_f1"] {
            let value = run
                .metrics
                .get(metric)
                .unwrap_or_else(|| panic!("a run over reference answers scores {metric}"));
            assert_eq!(value, 1.0, "{metric}: {summary}");
            assert!(summary.contains(&format!("{metric}: 1.0000")), "{summary}");
        }
        assert!(
            run.metrics.get("ndcg@10").is_some(),
            "the ranking behind the answer is still scored: {summary}"
        );

        // The identities were read before the run, by family (ADR-C31 § 4).
        let recorded = &run.inputs.model_hashes;
        assert_eq!(
            recorded.get("generator").map(String::as_str),
            Some(ragondin_stub::StubGenerator::IDENTITY)
        );
        assert!(recorded.contains_key("context_builder"), "{recorded:?}");
    }

    /// `bench` stamps the run's times itself: `started` before anything is
    /// read, `finished` once the evaluation returns and before the run is
    /// saved — so both fall between this test's own clock readings, taken
    /// before the process is spawned and after it exits.
    #[cfg(feature = "stub")]
    #[test]
    fn bench_stamps_started_before_preparation_and_finished_before_save() {
        use ragondin_experiments::UnixMillis;
        use std::time::SystemTime;

        let store = store("times");
        let now = || UnixMillis::from_system_time(SystemTime::now()).expect("after the epoch");

        let before = now();
        let output = ragondin(&[
            "bench",
            &fixture("stub-generation-bench.yaml"),
            "--benchmark",
            "beir-qa/qa-mini",
            "--datasets",
            path(&fixtures()),
            "--store",
            path(&store),
        ]);
        let after = now();

        assert!(output.status.success(), "{}", stderr(&output));
        let run = FileSystemRunStore::new(&store)
            .load(&reported_run_id(&stdout(&output)))
            .expect("the run bench reported is the run bench saved");
        let times = run.times.expect("bench records when the run ran");
        assert!(
            before <= times.started()
                && times.started() <= times.finished()
                && times.finished() <= after,
            "{before:?} <= {times:?} <= {after:?}"
        );
    }

    /// The same pipeline over a miniature SQuAD v1.1 dev file under
    /// `squad/`: the question's paragraph opens with its answer on a line of
    /// its own, so the stub answers it exactly when BM25 ranks it first. The
    /// fixture is hand-written and quotes no SQuAD text.
    #[cfg(feature = "stub")]
    #[test]
    fn bench_reads_a_squad_benchmark_and_scores_its_answers() {
        let store = store("squad");

        let output = ragondin(&[
            "bench",
            &fixture("stub-generation-bench.yaml"),
            "--benchmark",
            "squad/squad-mini",
            "--datasets",
            path(&fixtures()),
            "--store",
            path(&store),
        ]);

        assert!(output.status.success(), "{}", stderr(&output));
        let run = FileSystemRunStore::new(&store)
            .load(&reported_run_id(&stdout(&output)))
            .expect("the run bench reported is the run bench saved");
        assert_eq!(
            run.metrics.get("exact_match"),
            Some(1.0),
            "{:?}",
            run.metrics
        );
        assert_eq!(run.metrics.get("token_f1"), Some(1.0), "{:?}", run.metrics);
        assert!(run.metrics.get("ndcg@10").is_some(), "{:?}", run.metrics);
    }

    /// A workspace of this test's own, `<W>/`, emptied first: the lexical
    /// fixture copied to each of `documents` under it, and nothing else — no
    /// `runs/`, so the first run into it creates the store.
    fn workspace(test_name: &str, documents: &[&str]) -> PathBuf {
        let root = store(&format!("workspace-{test_name}"));
        for document in documents {
            let path = root.join(document);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a writable tmpdir");
            std::fs::copy(fixtures().join("lexical-pipeline.yaml"), &path)
                .expect("the fixture copies");
        }
        root
    }

    /// Runs `bench` over the lexical fixture at `config` into `store`, and
    /// returns the run directory it saved.
    fn bench_into(config: &Path, store: &Path) -> PathBuf {
        let output = ragondin(&[
            "bench",
            path(config),
            "--benchmark",
            "beir/beir-mini",
            "--datasets",
            path(&fixtures()),
            "--store",
            path(store),
        ]);
        assert!(output.status.success(), "{}", stderr(&output));
        store.join(reported_run_id(&stdout(&output)).to_string())
    }

    fn launch_record(run_dir: &Path) -> Option<serde_json::Value> {
        let file = run_dir.join("provenance.json");
        file.exists().then(|| {
            serde_json::from_str(&std::fs::read_to_string(file).expect("the record reads"))
                .expect("the record is JSON")
        })
    }

    #[test]
    fn bench_records_the_name_when_config_and_store_share_a_workspace() {
        // `<W>/runs` does not exist yet: the first run into a fresh workspace
        // still records its name.
        let w = workspace("share", &["pipelines/hybrid.yaml"]);
        assert!(!w.join("runs").exists());

        let run_dir = bench_into(&w.join("pipelines/hybrid.yaml"), &w.join("runs"));

        // The name and nothing else: no `prefix_of`, and never a path.
        assert_eq!(
            launch_record(&run_dir),
            Some(serde_json::json!({"name": "hybrid"}))
        );
        let id = run_dir
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.parse().ok())
            .expect("a run directory is named by its id");
        let run = FileSystemRunStore::new(w.join("runs"))
            .load(&id)
            .expect("the run reads back");
        assert_eq!(
            run.provenance.as_ref().and_then(|record| record.name()),
            Some("hybrid")
        );
    }

    #[test]
    fn bench_records_no_name_outside_the_workspace_convention() {
        // Each case runs into a store of its own: one run id per store, and
        // the first record wins, so a case reusing a store would read the
        // record an earlier case left.
        let cases = [
            // The store is the `runs/` of another directory than the workspace.
            ("elsewhere", "pipelines/hybrid.yaml", "elsewhere/runs"),
            // The configuration is not under `pipelines/`.
            ("drafts", "drafts/hybrid.yaml", "runs"),
            // Only `.yaml` is a workspace pipeline.
            ("yml", "pipelines/hybrid.yml", "runs"),
            // The store's last component is not literally `runs`.
            ("store", "pipelines/hybrid.yaml", "store"),
        ];
        for (case, config, runs) in cases {
            let w = workspace(&format!("outside-{case}"), &[config]);
            let run_dir = bench_into(&w.join(config), &w.join(runs));
            assert_eq!(launch_record(&run_dir), None, "{case}: no record");
        }
    }

    #[cfg(unix)]
    #[test]
    fn bench_canonicalizes_before_matching_the_convention() {
        let named = Some(serde_json::json!({"name": "hybrid"}));

        // Through `..`, into a store that already exists. A workspace per
        // case: one run id per store, and the first record wins, so a case
        // sharing a store with another would read that one's record.
        let w = workspace("canonical-dots", &["pipelines/hybrid.yaml"]);
        std::fs::create_dir_all(w.join("runs")).expect("an existing store");
        let run_dir = bench_into(
            &w.join("pipelines/../pipelines/hybrid.yaml"),
            &w.join("runs"),
        );
        assert_eq!(launch_record(&run_dir), named, "a config through `..`");

        // The config through a link to the workspace.
        let w = workspace("canonical-config-link", &["pipelines/hybrid.yaml"]);
        let link = store("workspace-canonical-config-link-to");
        std::os::unix::fs::symlink(&w, &link).expect("a link is creatable");
        let run_dir = bench_into(&link.join("pipelines/hybrid.yaml"), &w.join("runs"));
        assert_eq!(launch_record(&run_dir), named, "a config through a link");

        // The store through a link to the workspace, before it exists.
        let w = workspace("canonical-store-link", &["pipelines/hybrid.yaml"]);
        let link = store("workspace-canonical-store-link-to");
        std::os::unix::fs::symlink(&w, &link).expect("a link is creatable");
        let run_dir = bench_into(&w.join("pipelines/hybrid.yaml"), &link.join("runs"));
        assert_eq!(launch_record(&run_dir), named, "a store through a link");
    }

    /// `bench` over the lexical fixture at `config` into `store`, as a process.
    fn bench_output(config: &Path, store: &Path) -> Output {
        ragondin(&[
            "bench",
            path(config),
            "--benchmark",
            "beir/beir-mini",
            "--datasets",
            path(&fixtures()),
            "--store",
            path(store),
        ])
    }

    /// Every file of a run directory, by name, with its bytes.
    fn files_of(run_dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(run_dir)
            .expect("the run directory reads")
            .map(|entry| {
                let entry = entry.expect("an entry reads");
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    std::fs::read(entry.path()).expect("a run file reads"),
                )
            })
            .collect()
    }

    #[test]
    fn a_first_bench_prints_as_before() {
        // The golden of a first run: the summary `bench` has always printed,
        // line for line, over the run it filed.
        let w = workspace("first-golden", &["pipelines/hybrid.yaml"]);
        let output = bench_output(&w.join("pipelines/hybrid.yaml"), &w.join("runs"));

        assert!(output.status.success(), "{}", stderr(&output));
        let id = reported_run_id(&stdout(&output));
        let run = FileSystemRunStore::new(w.join("runs"))
            .load(&id)
            .expect("the run bench reported is the run bench saved");
        let mut golden = format!(
            "run {}\n  pipeline      {}\n  dataset       {}\n  index         {}\n",
            run.id, run.inputs.pipeline, run.inputs.dataset_version, run.inputs.index_version
        );
        for (name, value) in run.metrics.iter() {
            golden.push_str(&format!("{name}: {value:.4}\n"));
        }
        assert_eq!(stdout(&output), golden);
    }

    #[test]
    fn a_second_bench_is_refused_naming_the_stored_run_and_keeps_it() {
        // The same pipeline under a second name: one run id, and the first
        // launch's record is the one kept (ADR-C39 § 8). The second launch is
        // refused, as the UI's `409 run_exists` refuses it.
        let w = workspace(
            "second-reported",
            &["pipelines/hybrid.yaml", "pipelines/fork.yaml"],
        );
        let first = bench_output(&w.join("pipelines/hybrid.yaml"), &w.join("runs"));
        assert!(first.status.success(), "{}", stderr(&first));
        let id = reported_run_id(&stdout(&first));
        let run_dir = w.join("runs").join(id.to_string());
        let stored = files_of(&run_dir);

        let second = bench_output(&w.join("pipelines/fork.yaml"), &w.join("runs"));

        assert!(!second.status.success(), "{}", stdout(&second));
        assert_eq!(
            stderr(&second),
            format!(
                "error: run {id}: already stored, launched as hybrid; this execution was not kept\n"
            )
        );
        assert_eq!(stdout(&second), "", "no summary of a run not filed");
        assert_eq!(files_of(&run_dir), stored, "the stored run is untouched");
    }

    #[test]
    fn a_second_bench_over_a_run_without_a_record_is_refused_without_a_name() {
        // Outside the workspace convention: the first run is filed with no
        // record, and no `provenance.json`.
        let w = workspace("second-unnamed", &["drafts/hybrid.yaml"]);
        let first = bench_output(&w.join("drafts/hybrid.yaml"), &w.join("runs"));
        assert!(first.status.success(), "{}", stderr(&first));
        let id = reported_run_id(&stdout(&first));
        let run_dir = w.join("runs").join(id.to_string());
        assert_eq!(launch_record(&run_dir), None);
        let stored = files_of(&run_dir);

        let second = bench_output(&w.join("drafts/hybrid.yaml"), &w.join("runs"));

        assert!(!second.status.success(), "{}", stdout(&second));
        assert_eq!(
            stderr(&second),
            format!("error: run {id}: already stored; this execution was not kept\n")
        );
        assert_eq!(stdout(&second), "", "no summary of a run not filed");
        assert_eq!(files_of(&run_dir), stored, "the stored run is untouched");
    }

    #[test]
    fn a_second_bench_over_a_stored_run_that_does_not_read_fails_naming_it() {
        // A record that no longer parses: the run is in the store, so this
        // execution is not kept, and what is wrong with it is reported rather
        // than passed over as a run filed.
        let w = workspace("second-unreadable", &["pipelines/hybrid.yaml"]);
        let first = bench_output(&w.join("pipelines/hybrid.yaml"), &w.join("runs"));
        assert!(first.status.success(), "{}", stderr(&first));
        let id = reported_run_id(&stdout(&first));
        let run_dir = w.join("runs").join(id.to_string());
        std::fs::write(run_dir.join("times.json"), "{}").expect("the record is writable");
        let stored = files_of(&run_dir);

        let second = bench_output(&w.join("pipelines/hybrid.yaml"), &w.join("runs"));

        assert!(!second.status.success(), "{}", stdout(&second));
        let error = stderr(&second);
        assert!(error.contains(&id.to_string()), "{error}");
        assert!(error.contains("this execution was not kept"), "{error}");
        assert_eq!(stdout(&second), "", "no summary of a run not filed");
        assert_eq!(files_of(&run_dir), stored, "the stored run is untouched");
    }

    /// The same fixture under `beir/`, which ignores `answers.jsonl`: the
    /// benchmark then carries no reference answers, and the run is scored by
    /// retrieval alone — `beir/` keeps its M2 meaning exactly (ADR-C30 § 2).
    #[test]
    fn the_beir_selector_ignores_the_reference_answers_beside_a_dataset() {
        let store = store("qa-mini-as-beir");

        let output = ragondin(&[
            "bench",
            &fixture("lexical-pipeline.yaml"),
            "--benchmark",
            "beir/qa-mini",
            "--datasets",
            path(&fixtures()),
            "--store",
            path(&store),
        ]);

        assert!(output.status.success(), "{}", stderr(&output));
        let run = FileSystemRunStore::new(&store)
            .load(&reported_run_id(&stdout(&output)))
            .expect("the run bench reported is the run bench saved");
        assert!(run.metrics.get("ndcg@10").is_some(), "{:?}", run.metrics);
        assert!(
            run.metrics.get("exact_match").is_none(),
            "{:?}",
            run.metrics
        );
    }
}
