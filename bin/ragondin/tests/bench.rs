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
//! The three refusals at the top — an unknown benchmark format, an extension
//! node, a generator no build registers — need no component this build lacks,
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
        // which `prepare` embedded, and the query prefix on every query the
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
        assert!(error.contains("reranked"), "{error}");
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
