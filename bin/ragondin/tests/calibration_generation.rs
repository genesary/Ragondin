//! The generation chain calibrated on real data with a real model: `ragondin
//! bench` through the real retrieval components, the real context builder and
//! the reference `Remote` generator service, in front of a local inference
//! server running a pinned open-weights model.
//!
//! `tests/calibration.rs` earned M2's numbers by reproducing a published
//! retrieval figure and freezing that run query by query. This file does the
//! same for the chain that ends in an answer, with the difference ADR-C30 § 4
//! states: the retrieval side is held to a recorded figure, and the generation
//! side, which no seed makes deterministic (ADR-15), is recorded and held to a
//! tolerance on a rerun instead. Three tests, because they need three
//! different sets of material and cost three very different amounts, and a
//! calibration that misses is diagnosed one leg at a time:
//!
//! - **SciFact through a generation pipeline.** `dense → context_builder →
//!   stub generator` over BEIR SciFact, which carries qrels and no reference
//!   answer. The run scores retrieval only, reading the ranking through the
//!   generator's context port and the builder's chunks port (ADR-C30 § 3), so
//!   it must land on the dense-only figures `tests/calibration.rs` records —
//!   the same node, read by the new path. 83 s recorded; no model, no service.
//! - **SQuAD v1.1 dev, the retrieval leg**, over all 10 570 questions: hybrid
//!   retrieval with reranking, the concatenating builder, and the stub
//!   generator in place of the model (a benchmark carrying reference answers
//!   refuses a pipeline producing none, ADR-C30 § 5). Its retrieval figures
//!   are asserted against the recorded ones. 7 524 s recorded:
//!   the cross-encoder reranks up to forty passages for each question.
//! - **SQuAD v1.1 dev, the generation leg**, over the first
//!   [`GENERATION_QUESTIONS`] questions in file order — the documented subset
//!   ADR-C30 § 4 allows, because ten thousand generations through a real model
//!   are a budget the retrieval leg does not have. The same retrieval and
//!   builder, then the generator named `answerer`, bound with `--remote` to a
//!   `ragondin-generator-service` this test spawns on an ephemeral port, which
//!   relays each call to the inference server. Temperature zero and a fixed
//!   seed are the configuration's. Its retrieval figures are held to
//!   [`RECORDED_TOLERANCE`], its exact match and F1 means to
//!   [`RERUN_TOLERANCE`] of the recorded run, and the number of answers whose
//!   text moved since the recorded run is printed, never bounded. 3 032 s on
//!   the recording machine.
//!
//! # Ignored by default, run by `just calibrate-generation`
//!
//! Nothing it needs enters the tree or is fetched by it: the rule ADR-C27
//! applies to models applies to the dataset and the LLM as well. Four
//! environment variables name the material, and a test that needs one fails,
//! naming it, when it is unset:
//!
//! - `RAGONDIN_CALIBRATION_DATASETS` — `scifact/` in the BEIR layout, as
//!   `tests/calibration.rs` reads it, and `squad/dev-v1.1.json`, the official
//!   SQuAD v1.1 dev file, under that name.
//! - `RAGONDIN_CALIBRATION_MODELS` — `all-MiniLM-L6-v2/` and
//!   `ms-marco-MiniLM-L6-v2/`, exported as `tests/calibration.rs` records.
//! - `RAGONDIN_CALIBRATION_INFERENCE_URL` — the root of an OpenAI-compatible
//!   inference server, without `/v1`, serving the model under the alias the
//!   configuration's `served_model` names. Started by hand, as recorded.
//! - `RAGONDIN_GENERATOR_SERVICE_BIN` — the `ragondin-generator-service`
//!   binary, built with `--features service`. The recipe builds it and passes
//!   its path: Cargo gives a test the path of its own package's binaries only.
//!
//! The reference — the dataset and model digests, the inference server and the
//! command line it was started with, the numbers the recorded run produced and
//! what its rerun moved — is in `bin/ragondin/ARCHITECTURE.md` § Calibrating
//! the generation chain; the constants below are the part of that record the
//! tests hold themselves to.
//!
//! # What is frozen, and where
//!
//! The generation leg's run is frozen query by query — the ranking the
//! metrics read, the answer as text, the qrels and reference answers, and the
//! values the SQuAD script and `pytrec_eval` score them — by
//! `eval/ragondin-metrics/tests/squad_generation_calibration_fixture.rs`,
//! which runs in ordinary CI with no dataset, model, network or Python. The
//! generation test here reads that fixture's answers to count how many of this
//! run's answers differ from the recorded ones. The retrieval leg over the
//! whole set is asserted by its aggregates only (ADR-C30 § 4).

#![cfg(all(
    feature = "bm25",
    feature = "onnx",
    feature = "stub",
    feature = "remote"
))]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Process, Output, Stdio};
use std::time::Instant;

use assert_cmd::Command;
use ragondin_experiments::{FileSystemRunStore, Run, RunId};

/// Where `scifact/` and `squad/` sit.
const DATASETS_VAR: &str = "RAGONDIN_CALIBRATION_DATASETS";
/// Where the two exported models sit.
const MODELS_VAR: &str = "RAGONDIN_CALIBRATION_MODELS";
/// The inference server's root, without `/v1`.
const INFERENCE_URL_VAR: &str = "RAGONDIN_CALIBRATION_INFERENCE_URL";
/// The reference generator service's binary.
const SERVICE_BIN_VAR: &str = "RAGONDIN_GENERATOR_SERVICE_BIN";

const SCIFACT: &str = "beir/scifact";
/// `squad/` under the datasets root, holding `dev-v1.1.json`.
const SQUAD: &str = "squad/squad";
/// The generation subset's directory under this test's own datasets root.
const SQUAD_SUBSET: &str = "squad-first-1000";
const SQUAD_FILE: &str = "dev-v1.1.json";

const SCIFACT_DENSE_STUB: &str = "generation/scifact-dense-stub.yaml";
const SQUAD_RETRIEVAL: &str = "generation/squad-retrieval.yaml";
const SQUAD_GENERATION: &str = "generation/squad-generation.yaml";

/// The `impl:` name the generation configuration gives its generator, bound
/// to the spawned service on the command line (ADR-C32 § 2).
const GENERATOR_NAME: &str = "answerer";

const NDCG: &str = "ndcg@10";
const EXACT_MATCH: &str = "exact_match";
const TOKEN_F1: &str = "token_f1";

/// ADR-C30 § 4's subset: the first N dev questions in file order, N ≥ 1 000.
const GENERATION_QUESTIONS: usize = 1000;

/// SciFact's `dataset_version`, and the embedder and reranker identities, as
/// `tests/calibration.rs` records them: the same material, read by the same
/// adapter and components. Repeated rather than shared, because that file is
/// the M2 calibration and this one does not reach into it.
const RECORDED_SCIFACT_DATASET: &str =
    "9a07f80c0d4f1e9e74912d033a8d1fbd52c54b758dafcaa85c19abacfdee5f29";
const RECORDED_EMBEDDER: &str = "9348202758f11c56c329d947ae359fea54be1a3d905bfcac4a3521a1eafc0414\
     +da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0";
const RECORDED_RERANKER: &str = "8b0fe5bc3c5ddc752524552d8e081baa7726e389b1d23396e56ad31d69b88d52\
     +d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66";

/// What `tests/calibration.rs` records for SciFact dense-only — the figures the
/// generation pipeline over the same retrieval node must reproduce — and the
/// published nDCG@10 they reproduce, within ADR-10's half a point.
const RECORDED_SCIFACT_DENSE: [(&str, f64); 3] = [
    ("mrr", 0.6047248677248677),
    (NDCG, 0.6450816521455768),
    ("recall@10", 0.7833333333333333),
];
const PUBLISHED_SCIFACT_NDCG: f64 = 0.64508;
const PUBLISHED_TOLERANCE: f64 = 0.005;

/// How far a retrieval metric may sit from the recorded one: `tests/
/// calibration.rs`'s tolerance, for the reason it gives — ONNX Runtime's
/// summation order moves the last bits of an `f64` on another machine. On the
/// recording machine the agreement is exact.
const RECORDED_TOLERANCE: f64 = 1e-4;

/// ADR-C30 § 4's rerun tolerance on the generation subset's mean exact match
/// and mean token F1: 0.01 absolute, ten flipped answers in a thousand.
const RERUN_TOLERANCE: f64 = 0.01;

/// The SQuAD dev file's `dataset_version`, and the generation subset's.
const RECORDED_SQUAD_DATASET: &str =
    "e4e3b7605b66545c91fdfb2ac8b4ddb177df1b1b34e7f8e557f63f0dcf074010";
const RECORDED_SUBSET_DATASET: &str =
    "0ad9e48a5cc958c047533f5fa714b0c4c64ec87e347e478493e6c152e870b865";

/// The identity the service reads for the served model from the inference
/// server's `/v1/models` (ADR-C33 § 4). `llama-server` reports the alias and
/// nothing the identity keeps beside it, so this pins the name and not the
/// weights: the record pins those by the digests of the GGUF files.
const RECORDED_GENERATOR: &str = r#"{"id":"qwen2.5-7b-instruct"}"#;

/// The retrieval leg over every dev question. Its exact match and F1 are the
/// stub's and are not held to anything.
const RECORDED_SQUAD_RETRIEVAL: [(&str, f64); 3] = [
    ("mrr", 0.9180896442462196),
    (NDCG, 0.935018599326266),
    ("recall@10", 0.9859981078524125),
];

/// The generation leg over the subset: its run id, its retrieval figures, and
/// its exact match and F1.
const RECORDED_GENERATION_RUN: &str =
    "9e64e3de18d2be0dc14b9c2c4672fad042c55c735d777f5695fee821c865250e";
const RECORDED_GENERATION_RETRIEVAL: [(&str, f64); 3] = [
    ("mrr", 0.8773896825396829),
    (NDCG, 0.9050833556297418),
    ("recall@10", 0.988),
];
const RECORDED_GENERATION_ANSWERS: [(&str, f64); 2] =
    [(EXACT_MATCH, 0.568), (TOKEN_F1, 0.7146161833448988)];

/// The frozen answers of the recorded run, as the CI fixture holds them.
fn frozen_answers_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../eval/ragondin-metrics/tests/fixtures/squad_generation_calibration.answers.tsv")
}

/// The directory an environment variable names, or a failure naming it and
/// what goes in it. Failing rather than passing quietly: this test only runs
/// when asked for, and an ask that cannot be honoured is an error, not a skip.
fn required(var: &str, holds: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| {
        panic!(
            "{var} is not set: it names {holds}. How that material is obtained is \
             recorded in `bin/ragondin/ARCHITECTURE.md` § Calibrating the generation chain."
        )
    })
}

fn datasets() -> PathBuf {
    PathBuf::from(required(
        DATASETS_VAR,
        "the directory holding `scifact/` in the BEIR layout and `squad/dev-v1.1.json`",
    ))
}

fn models() -> PathBuf {
    PathBuf::from(required(
        MODELS_VAR,
        "the directory holding `all-MiniLM-L6-v2/` and `ms-marco-MiniLM-L6-v2/`, each with \
         `model.onnx` and `tokenizer.json`",
    ))
}

fn configs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/calibration")
}

fn path(buf: &Path) -> &str {
    buf.to_str().expect("UTF-8 path")
}

/// A directory of this test's own under the target directory.
fn scratch(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("calibration-generation")
        .join(name)
}

/// A run store of this test's own, emptied first.
fn store(name: &str) -> PathBuf {
    let root = scratch(name);
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

fn reported_run_id(summary: &str) -> RunId {
    let first = summary.lines().next().expect("a summary has a first line");
    first
        .strip_prefix("run ")
        .unwrap_or_else(|| panic!("the summary opens with the run id: {summary}"))
        .parse()
        .expect("the printed id is a run id")
}

/// Evaluates one configuration over one benchmark into `store`, the binary
/// run from the models directory so the configurations' relative model paths
/// resolve, and returns the run as the store holds it.
fn bench(
    models: &Path,
    config: &str,
    benchmark: &str,
    datasets: &Path,
    store: &Path,
    extra: &[&str],
) -> Run {
    let started = Instant::now();
    let config_path = configs().join(config);
    let mut arguments = vec![
        "bench",
        path(&config_path),
        "--benchmark",
        benchmark,
        "--datasets",
        path(datasets),
        "--store",
        path(store),
    ];
    arguments.extend_from_slice(extra);
    let output = Command::cargo_bin("ragondin")
        .expect("the binary under test is built by `cargo test`")
        .current_dir(models)
        .args(&arguments)
        .output()
        .expect("the binary runs");
    assert!(
        output.status.success(),
        "`bench {config}` over {benchmark} failed: {}",
        stderr(&output)
    );
    println!(
        "{config} over {benchmark}: {:.0} s\n{}",
        started.elapsed().as_secs_f64(),
        stdout(&output)
    );
    FileSystemRunStore::new(store)
        .load(&reported_run_id(&stdout(&output)))
        .expect("the run bench reported is the run bench saved")
}

fn metric(run: &Run, name: &str) -> f64 {
    run.metrics
        .get(name)
        .unwrap_or_else(|| panic!("the run scores {name}: {:?}", run.metrics))
}

fn model<'a>(run: &'a Run, family: &str) -> Option<&'a str> {
    run.inputs.model_hashes.get(family).map(String::as_str)
}

fn assert_within(run: &Run, recorded: &[(&str, f64)], tolerance: f64, which: &str) {
    for (name, expected) in recorded {
        let got = metric(run, name);
        assert!(
            (got - expected).abs() <= tolerance,
            "{which} {name}: {got} against the recorded {expected}, beyond {tolerance}"
        );
    }
}

#[test]
#[ignore = "needs BEIR SciFact and the exported embedder on disk; run with `just calibrate-generation`"]
fn scifact_dense_only_through_a_generation_pipeline_scores_the_recorded_ranking() {
    let (datasets, models) = (datasets(), models());
    let run = bench(
        &models,
        SCIFACT_DENSE_STUB,
        SCIFACT,
        &datasets,
        &store("scifact"),
        &[],
    );

    assert_eq!(
        run.inputs.dataset_version, RECORDED_SCIFACT_DATASET,
        "not the recorded SciFact"
    );
    assert_eq!(
        model(&run, "embedder"),
        Some(RECORDED_EMBEDDER),
        "not the recorded embedder export"
    );
    // The qrels-only regime (ADR-8): no reference answer, so no answer metric,
    // whatever the pipeline ends in.
    for name in [EXACT_MATCH, TOKEN_F1] {
        assert!(
            run.metrics.get(name).is_none(),
            "SciFact carries no reference answer, and the run scored {name}"
        );
    }
    let ndcg = metric(&run, NDCG);
    assert!(
        (ndcg - PUBLISHED_SCIFACT_NDCG).abs() <= PUBLISHED_TOLERANCE,
        "{NDCG} {ndcg:.5} is {:.2} points from the published {PUBLISHED_SCIFACT_NDCG}",
        (ndcg - PUBLISHED_SCIFACT_NDCG).abs() * 100.0
    );
    assert_within(
        &run,
        &RECORDED_SCIFACT_DENSE,
        RECORDED_TOLERANCE,
        "scifact dense → stub",
    );
}

#[test]
#[ignore = "needs SQuAD v1.1 dev and two exported models on disk; run with `just calibrate-generation`"]
fn squad_dev_retrieval_over_every_question_scores_the_recorded_figures() {
    let (datasets, models) = (datasets(), models());
    let run = bench(
        &models,
        SQUAD_RETRIEVAL,
        SQUAD,
        &datasets,
        &store("squad-retrieval"),
        &[],
    );

    let figures: BTreeMap<_, _> = run.metrics.iter().collect();
    println!("retrieval leg: {figures:?}");
    assert_eq!(
        run.inputs.dataset_version, RECORDED_SQUAD_DATASET,
        "not the recorded SQuAD v1.1 dev file"
    );
    assert_eq!(model(&run, "embedder"), Some(RECORDED_EMBEDDER));
    assert_eq!(model(&run, "reranker"), Some(RECORDED_RERANKER));
    assert_within(
        &run,
        &RECORDED_SQUAD_RETRIEVAL,
        RECORDED_TOLERANCE,
        "squad retrieval leg",
    );
}

#[test]
#[ignore = "needs SQuAD v1.1 dev, two exported models, the reference service and an inference \
            server; run with `just calibrate-generation`"]
fn squad_dev_generation_over_the_first_thousand_questions_reruns_within_tolerance() {
    let (datasets, models) = (datasets(), models());
    let inference = required(
        INFERENCE_URL_VAR,
        "the root of the OpenAI-compatible inference server serving the model, without `/v1`",
    );
    let binary = required(
        SERVICE_BIN_VAR,
        "the `ragondin-generator-service` binary, built with `--features service`",
    );

    let subset_root = scratch("datasets");
    let kept = write_subset(
        &datasets.join("squad").join(SQUAD_FILE),
        &subset_root.join(SQUAD_SUBSET).join(SQUAD_FILE),
        GENERATION_QUESTIONS,
    );
    assert_eq!(
        kept, GENERATION_QUESTIONS,
        "the dev file is shorter than the subset"
    );

    let service = GeneratorService::spawn(&binary, &inference);
    let binding = format!("generator/{GENERATOR_NAME}={}", service.uri);
    let run = bench(
        &models,
        SQUAD_GENERATION,
        &format!("squad/{SQUAD_SUBSET}"),
        &subset_root,
        &store("squad-generation"),
        &["--remote", &binding],
    );
    drop(service);

    let changed = changed_answers(&run);
    let figures: BTreeMap<_, _> = run.metrics.iter().collect();
    println!("generation leg: run {}: {figures:?}", run.id);
    for (name, recorded) in RECORDED_GENERATION_ANSWERS {
        println!(
            "{name}: {} against the recorded {recorded}, a move of {:+}",
            metric(&run, name),
            metric(&run, name) - recorded
        );
    }
    println!("answers whose text changed since the recorded run: {changed:?}");

    assert_eq!(
        run.inputs.dataset_version, RECORDED_SUBSET_DATASET,
        "not the recorded subset of the recorded dev file"
    );
    assert_eq!(model(&run, "embedder"), Some(RECORDED_EMBEDDER));
    assert_eq!(model(&run, "reranker"), Some(RECORDED_RERANKER));
    assert_eq!(
        model(&run, "generator"),
        Some(RECORDED_GENERATOR),
        "the inference server serves another model under the alias"
    );
    // Every input the run's identity digests is the recorded one, so this is
    // a rerun of the run the CI fixture froze, and not another experiment.
    assert_eq!(run.id.to_string(), RECORDED_GENERATION_RUN, "another run");
    assert_within(
        &run,
        &RECORDED_GENERATION_RETRIEVAL,
        RECORDED_TOLERANCE,
        "squad generation leg",
    );
    assert_within(
        &run,
        &RECORDED_GENERATION_ANSWERS,
        RERUN_TOLERANCE,
        "squad generation leg",
    );
    assert!(
        changed.is_some(),
        "no frozen answers at {} to compare this run with",
        frozen_answers_path().display()
    );
}

/// Writes the dev file with only its first `questions` questions, in file
/// order, and returns how many it kept. Every article and paragraph is kept —
/// a paragraph whose questions are dropped keeps an empty `qas` — so the
/// corpus, and with it every question's ranking, is the full file's.
fn write_subset(source: &Path, target: &Path, questions: usize) -> usize {
    let bytes =
        std::fs::read(source).unwrap_or_else(|e| panic!("cannot read {}: {e}", source.display()));
    let mut file: serde_json::Value = serde_json::from_slice(&bytes).expect("the dev file is JSON");
    let mut left = questions;
    let articles = file["data"].as_array_mut().expect("`data` is an array");
    for article in articles {
        let paragraphs = article["paragraphs"]
            .as_array_mut()
            .expect("`paragraphs` is an array");
        for paragraph in paragraphs {
            let qas = paragraph["qas"].as_array_mut().expect("`qas` is an array");
            let keep = left.min(qas.len());
            qas.truncate(keep);
            left -= keep;
        }
    }
    std::fs::create_dir_all(target.parent().expect("a file has a parent"))
        .expect("create the subset directory");
    std::fs::write(target, serde_json::to_vec(&file).expect("JSON encodes"))
        .expect("write the subset");
    questions - left
}

/// How many of this run's answers differ, as text, from the frozen fixture's —
/// `None` when there is no fixture to compare with. The traces name each
/// query's answer in the output entry of the terminal node.
fn changed_answers(run: &Run) -> Option<usize> {
    let frozen = std::fs::read_to_string(frozen_answers_path()).ok()?;
    let frozen: BTreeMap<&str, String> = frozen
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut fields = line.split('\t');
            let query = fields.next().expect("a query id");
            let answer = unescape(fields.next().expect("an answer"));
            (query, answer)
        })
        .collect();

    let traces = scratch("squad-generation")
        .join(run.id.to_string())
        .join("traces.json");
    let traces: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&traces).unwrap_or_else(|e| panic!("{}: {e}", traces.display())),
    )
    .expect("traces.json is JSON");
    let traces = traces.as_object().expect("traces are keyed by query");
    assert_eq!(
        traces.len(),
        frozen.len(),
        "the run and the fixture answer different question sets"
    );

    let mut changed = 0;
    for (query, trace) in traces {
        let terminal = trace["nodes"]
            .as_array()
            .and_then(|nodes| nodes.last())
            .expect("a trace names its nodes");
        let answer = terminal["output"]["answer"]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{query}: the terminal node produced no answer"));
        let recorded = frozen
            .get(query.as_str())
            .unwrap_or_else(|| panic!("{query} is not in the fixture"));
        if answer != recorded {
            changed += 1;
        }
    }
    Some(changed)
}

/// Decodes a fixture field as `regenerate_squad_generation_calibration.py`
/// encodes it: `\\` is a backslash, `\u{hex}` a character, and a field that is
/// exactly `\e` the empty string.
fn unescape(field: &str) -> String {
    if field == "\\e" {
        return String::new();
    }
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('u') => {
                assert_eq!(chars.next(), Some('{'), "malformed escape in {field:?}");
                let hex: String = chars.by_ref().take_while(|&c| c != '}').collect();
                let code = u32::from_str_radix(&hex, 16).expect("escape is hexadecimal");
                out.push(char::from_u32(code).expect("escape is a scalar value"));
            }
            other => panic!("unknown escape \\{other:?} in {field:?}"),
        }
    }
    out
}

/// The reference generator service, spawned on an ephemeral loopback port and
/// killed when dropped.
struct GeneratorService {
    uri: String,
    child: Child,
}

impl GeneratorService {
    /// Spawns `binary` relaying to `inference`, and reads the one line it
    /// writes once bound, `listening on <address>`, to learn the port.
    fn spawn(binary: &str, inference: &str) -> Self {
        let mut child = Process::new(binary)
            .args(["--base-url", inference, "--listen", "127.0.0.1:0"])
            // The inference server is on loopback: a proxy the environment
            // names must not stand between the service and it.
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("ALL_PROXY")
            .env_remove("http_proxy")
            .env_remove("https_proxy")
            .env_remove("all_proxy")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("cannot spawn {binary}: {e}"));
        let mut line = String::new();
        BufReader::new(child.stdout.take().expect("stdout is piped"))
            .read_line(&mut line)
            .expect("read the service's stdout");
        let address = line
            .trim_end()
            .strip_prefix("listening on ")
            .unwrap_or_else(|| panic!("the service did not report its address: {line:?}"));
        Self {
            uri: format!("http://{address}"),
            child,
        }
    }
}

impl Drop for GeneratorService {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
