//! The `bench` subcommand: evaluate a pipeline against a benchmark.
//!
//! This is the composition root doing its one job
//! (`docs/code-architecture.md` §4.3). It is the only place that knows both
//! the engine and the concrete components, so it is the only place that can
//! put them together: it loads the configuration, loads the benchmark,
//! prepares the corpus, constructs the components **from that corpus**, and
//! hands the harness a context and the same prepared index (ADR-C26).
//!
//! Thin by rule, all the same: every step below is a call into the crate that
//! owns it. What is genuinely this module's own is the *order* — ADR-C32 § 4's
//! six steps, with the identity of every component this build knows how to
//! construct read before the benchmark is loaded — and one thing only a composition root can do: [`prepare`] embeds
//! the corpus before any component exists, because a `ComponentCtor` is
//! synchronous and the two calls that fill a vector store are not.
//!
//! # The launch record
//!
//! After the evaluation, `bench` stamps the run's launch record (ADR-C39 § 3)
//! from its two paths alone — `launched_as` — and the choices it makes are
//! this module's, recorded here:
//!
//! - **A name, only under the workspace convention.** The configuration is
//!   `<W>/pipelines/<name>.yaml` and the store is `<W>/runs`, for one `W`
//!   once the directories they sit in are canonicalized.
//! - **Parents only, on both sides.** The config's parent is canonicalized,
//!   never the file, and the store's parent, never the store, whose last
//!   component as given must be literally `runs`. So a link to a directory
//!   on either path, or a `..`, is followed. A config file that is itself a
//!   link is named by where it sits, not by its target: a link inside
//!   `pipelines/` records its own stem, as the workspace lists it, and a link
//!   from outside `pipelines/` records nothing. A `<W>/runs` that is a link,
//!   to a bigger disk say, is still `<W>`'s store, as `ragondin ui --store`
//!   reads it (`ui/location.rs`).
//! - **A fresh workspace still records it.** The store's parent is what is
//!   canonicalized, so a `<W>/runs` that does not exist yet matches: the
//!   first run into a new workspace is named.
//! - **Only `.yaml`.** A workspace pipeline is `pipelines/<name>.yaml`, so a
//!   `.yml` records no name. The stem is taken as the file has it; the
//!   workspace's own rules on what a pipeline name may be are not applied
//!   here, since a recorded name is a fact about a launch, not a claim the
//!   workspace lists it.
//! - **A name or no record.** Outside the convention, the run is saved with
//!   no record and no `provenance.json` is written — never an empty `{}`, and
//!   never a path, whatever the convention's answer. `bench` never sets
//!   `prefix_of`: it runs whole pipelines.
//!
//! The record is outside the run's identity (INV-8), and the summary `bench`
//! prints does not show it, so neither the run id nor the output changes.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{bail, Context, Result};
use ragondin_benchmarks::{BeirAdapter, Benchmark, BenchmarkAdapter, SquadAdapter};
use ragondin_config::{ConfigSource, LocalFile};
use ragondin_contracts::EmbeddedChunk;
use ragondin_engine::EngineContext;
use ragondin_experiments::{
    ConfigDocument, FileSystemRunStore, Run, RunProvenance, RunTimes, UnixMillis,
};
use ragondin_harness::{evaluate, CorpusIndex, Evaluation};
use ragondin_pipeline::LogicalPipeline;

use crate::binding::Bindings;
use crate::wiring;

/// A benchmark format `--benchmark` names, by the text before its `/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    /// `beir/<dir>`: a BEIR directory, qrels only — any `answers.jsonl`
    /// beside it is ignored, so an M2 run reads exactly as it always has.
    Beir,
    /// `beir-qa/<dir>`: the same directory with its `answers.jsonl`, which is
    /// then required (ADR-C30 § 2).
    BeirQa,
    /// `squad/<dir>`: the SQuAD v1.1 dev file in that directory (ADR-C30 § 2).
    Squad,
}

impl Format {
    /// Every format, in the order a refusal lists them.
    const ALL: [Self; 3] = [Self::Beir, Self::BeirQa, Self::Squad];

    /// The selector prefix that names the format.
    fn name(self) -> &'static str {
        match self {
            Self::Beir => "beir",
            Self::BeirQa => "beir-qa",
            Self::Squad => "squad",
        }
    }

    /// Loads the dataset at `root` with the adapter this format names.
    fn load(self, root: &Path) -> Result<Benchmark> {
        let loaded = match self {
            Self::Beir => BeirAdapter::new(root).load(),
            Self::BeirQa => BeirAdapter::new(root).with_reference_answers().load(),
            Self::Squad => SquadAdapter::new(root).load(),
        };
        loaded.with_context(|| format!("loading the benchmark at {}", root.display()))
    }
}

/// The rank cutoff of the metrics — the `10` of nDCG@10.
///
/// Constant rather than a flag: it is the cutoff every BEIR leaderboard
/// reports, so a figure taken at another one is not comparable with the
/// published numbers this milestone exists to reproduce. A flag can be added
/// the day a benchmark reports at another cutoff; until then it would only
/// offer a way to produce an incomparable number.
const CUTOFF: usize = 10;

/// What `bench` was asked to evaluate.
pub struct Request<'a> {
    /// The pipeline configuration to evaluate.
    pub config: &'a Path,
    /// The benchmark selector, `<format>/<name>`.
    pub benchmark: &'a str,
    /// The directory the named dataset sits under.
    pub datasets: &'a Path,
    /// The run store the resulting run is written to.
    pub store: &'a Path,
    /// The `--remote <family>/<name>=<uri>` arguments, as given.
    pub remote: &'a [String],
}

/// Evaluates the configuration against the benchmark, records the run, and
/// prints what it scored.
///
/// # Errors
///
/// Anything on the way: a selector naming no dataset this build reads, a
/// `--remote` argument refused, an unreadable or invalid configuration, a configuration v0 does not run, a node
/// key the composition root refuses, a binding no node uses, a component whose
/// identity cannot be read, a dataset that does not load, an `impl:` this build did not register,
/// a query that fails, or a store that cannot be written.
pub async fn run(request: &Request<'_>) -> Result<()> {
    // The first statement, literally: the identity read, the benchmark load,
    // the index build and the embedding all count toward the run's time, so
    // `finished − started` is its wall time, preparation included. A clock
    // before the epoch gives no reading, and the run is then saved with its
    // times unknown rather than with a made-up one. This reading belongs at
    // the head of whatever shared execution path is extracted from here for
    // the UI's launcher (#353), so a run launched from the UI is timed by
    // this one definition and never a second.
    let started = UnixMillis::from_system_time(SystemTime::now());

    // Refused on their text alone: a malformed binding is found before the
    // configuration is even read (ADR-C32 § 2).
    let bindings = Bindings::parse(request.remote)?;

    // Read for the record and loaded for the run, from the same file. The run
    // store keeps the configuration **verbatim** — re-serializing the loaded
    // pipeline would file a second spelling of it beside the digest of the
    // first — so the text is what is kept and the pipeline is what is run.
    let text = std::fs::read_to_string(request.config)
        .with_context(|| format!("reading {}", request.config.display()))?;
    let (format, root) = benchmark_root(request.benchmark, request.datasets)?;

    // 1. The configuration: what v0 does not run, the keys of every node
    //    whose keys this composition root owns (ADR-C32 § 1), and a binding
    //    no node uses (§ 2).
    let pipeline = LocalFile::new(request.config).load().await?;
    wiring::refuse_unsupported(&pipeline)?;
    wiring::check_nodes(&pipeline, &bindings)?;
    bindings.refuse_unused(&pipeline)?;
    // One lazily connecting channel per binding; nothing connects yet.
    let bound = wiring::Bound::new(bindings)?;
    // 2. Every component's identity, read from the component, before the
    //    benchmark is loaded and the corpus embedded: a model file that is
    //    missing, a service that does not answer, or a model a service does not
    //    serve, is found now and not after the expensive step. A refusal here
    //    ends the run.
    let model_hashes = wiring::model_hashes(&pipeline, &bound).await?;

    // 3. The benchmark.
    let benchmark = format.load(&root)?;

    // One index, built here and used twice: the components below are
    // constructed from its chunks, and the harness records its version as the
    // `index_version` of the run. Building a second one anywhere would make
    // that recorded version name a set nothing searched (ADR-C26).
    let index = CorpusIndex::build(benchmark.corpus());
    // 4. The corpus, embedded.
    let embedded = prepare(&pipeline, &index, &bound).await?;

    // 5. The components, constructed from that corpus, and one registration
    //    per binding.
    let mut ctx = EngineContext::new();
    wiring::register(&mut ctx, index.chunks(), embedded.as_deref(), &bound);

    // 6. Evaluate, and save.

    let run = evaluate(
        &Evaluation {
            pipeline: &pipeline,
            config: &ConfigDocument::new(text),
            benchmark: &benchmark,
            index: &index,
            cutoff: CUTOFF,
            model_hashes,
        },
        &ctx,
    )
    .await?;
    // Read once `evaluate` has returned `Ok`, and before the save: storing
    // the run is not part of running it.
    let finished = UnixMillis::from_system_time(SystemTime::now());
    // Recorded on the run, never in its identity: the harness named the run
    // before it saw them, and where a service listened — or when the run
    // happened — is not an input of the experiment (ADR-C32 § 2, INV-8).
    // And how it was launched (ADR-C39 § 3): the workspace name, or no
    // record at all — never an empty one, and never a path.
    let run = Run {
        bindings: bound.bindings().record(),
        times: RunTimes::from_readings(started, finished),
        provenance: launched_as(request.config, request.store).map(RunProvenance::named),
        ..run
    };

    FileSystemRunStore::new(request.store).save(&run)?;
    print!("{}", render(&run));

    Ok(())
}

/// The workspace pipeline name `bench` records for a run of `config` stored
/// into `store`, under the workspace convention ADR-C39 § 3 gives it: `config`
/// is `<W>/pipelines/<name>.yaml` and `store` is `<W>/runs`, for one `W` once
/// the directories both sit in are canonicalized. `None` for anything else —
/// and a directory that cannot be canonicalized is something else.
///
/// Only the parents are canonicalized, never the last components, on both
/// sides: a link to a directory on either path is followed, while the config
/// file and the store directory are each named by where they sit, not by
/// what they link to. So a config file that is itself a link is named by its
/// own stem when it sits in `pipelines/`, and names nothing when it sits
/// elsewhere, whatever it points at; and a `<W>/runs` that is a link — to a
/// bigger disk, say — is still `<W>`'s store, as `ragondin ui --store` reads
/// it, whether or not it exists yet.
fn launched_as(config: &Path, store: &Path) -> Option<String> {
    let file = Path::new(config.file_name()?);
    if file.extension()? != "yaml" {
        return None;
    }
    let name = file.file_stem()?.to_str()?;

    let pipelines = canonical_parent(config)?;
    if pipelines.file_name()? != "pipelines" {
        return None;
    }

    if store.file_name()? != "runs" {
        return None;
    }
    let workspace = canonical_parent(store)?;

    (pipelines.parent()? == workspace).then(|| name.to_owned())
}

/// The directory `path` sits in, canonicalized: every link and `..` on the
/// way to it resolved, and `path`'s own last component left as it is.
fn canonical_parent(path: &Path) -> Option<PathBuf> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    std::fs::canonicalize(parent).ok()
}

/// Embeds the corpus, if the pipeline retrieves densely over it.
///
/// The whole of the asynchrony a component constructor cannot do: `embed` is
/// `async` and `ComponentCtor` is not, so the vectors are made here, before
/// any component exists, and the store reaches its constructor already holding
/// them. `None` means the pipeline names no dense node — there is then nothing
/// to embed, and nothing downstream to search.
///
/// Through whichever embedder the `dense` nodes name — the ONNX one, or a
/// bound one — and with the served model the dense retriever will ask for, so
/// the index and the queries come from one model (ADR-C32 § 4).
#[cfg(any(feature = "onnx", feature = "remote"))]
async fn prepare(
    pipeline: &LogicalPipeline,
    index: &CorpusIndex,
    bound: &wiring::Bound,
) -> Result<Option<Vec<EmbeddedChunk>>> {
    use ragondin_contracts::{EmbedParams, EmbedRole};

    let Some(spec) = wiring::embedder_spec(pipeline, bound.bindings())? else {
        return Ok(None);
    };

    let embedder = wiring::embedder(&spec, bound).context("constructing the corpus embedder")?;
    // `Passage`, and that is the half of ADR-C17 only the indexer supplies: a
    // corpus embedded under the query prefix is silently the wrong index.
    let mut params = EmbedParams::new(EmbedRole::Passage);
    if let Some(served_model) = spec.served_model() {
        params = params.with_served_model(served_model);
    }
    let texts: Vec<String> = index
        .chunks()
        .iter()
        .map(|chunk| chunk.text.clone())
        .collect();

    let vectors = embedder
        .embed(&texts, &params)
        .await
        .context("embedding the corpus")?;

    // The contract says one vector per text, in order. Checked rather than
    // trusted, because the zip below would otherwise drop the tail of the
    // corpus and leave a store that is quietly short.
    if vectors.len() != texts.len() {
        bail!(
            "the embedder returned {} vectors for {} chunks",
            vectors.len(),
            texts.len()
        );
    }

    Ok(Some(
        index
            .chunks()
            .iter()
            .cloned()
            .zip(vectors)
            .map(|(chunk, embedding)| EmbeddedChunk { chunk, embedding })
            .collect(),
    ))
}

/// The lean build's half: nothing to embed with.
///
/// Nothing reaches it that needs embedding: [`wiring::check_nodes`] has already
/// refused a `dense` node naming the `onnx` embedder in a build without the
/// `onnx` feature, naming the feature — no planner will ever look an embedder
/// up to name what is missing — and a build without `remote` refuses every
/// binding. The configuration is read all the same, so the builds share one
/// path through it.
#[cfg(not(any(feature = "onnx", feature = "remote")))]
async fn prepare(
    pipeline: &LogicalPipeline,
    index: &CorpusIndex,
    bound: &wiring::Bound,
) -> Result<Option<Vec<EmbeddedChunk>>> {
    let _ = wiring::embedder_spec(pipeline, bound.bindings())?;
    let _ = index;
    Ok(None)
}

/// Resolves `--benchmark <format>/<name>` against the datasets root.
///
/// The selector names a format and a dataset; the root says where the datasets
/// live. Two arguments rather than a path, because the format is not
/// discoverable from the directory — a BEIR directory read with its reference
/// answers (`beir-qa/`) and the same directory read without them (`beir/`) are
/// told apart by what the user asked for, not by what is on disk. `--datasets`
/// has no default for the reason `compare --store` has none: no dataset
/// location is settled anywhere in `docs/` yet, and this crate does not invent
/// one.
///
/// Resolved before the configuration is loaded: a selector is refused on its
/// text alone, whatever the pipeline says.
fn benchmark_root(selector: &str, datasets: &Path) -> Result<(Format, PathBuf)> {
    let Some((prefix, name)) = selector.split_once('/') else {
        bail!("`{selector}` is not a benchmark: name one as `<format>/<dataset>`, e.g. `beir/scifact`");
    };

    let Some(format) = Format::ALL
        .into_iter()
        .find(|format| format.name() == prefix)
    else {
        let known: Vec<String> = Format::ALL
            .iter()
            .map(|format| format!("`{}`", format.name()))
            .collect();
        bail!(
            "`{prefix}` is not a benchmark format this build reads; it reads {}",
            known.join(", ")
        );
    };
    if name.is_empty() {
        bail!(
            "`{selector}` names no dataset: the form is `{}/<dataset>`",
            format.name()
        );
    }
    // The name is joined onto the root, so a separator in it would reach
    // outside the directory `--datasets` names. A dataset is one directory
    // under that root and nothing else.
    if name.contains('/')
        || name.contains(std::path::MAIN_SEPARATOR)
        || Path::new(name).parent() != Some(Path::new(""))
    {
        bail!("`{name}` is not a dataset name: it must name one directory under the datasets root");
    }

    Ok((format, datasets.join(name)))
}

/// Renders what a finished run scored: its identity, then one line per metric.
///
/// The identity tuple is printed beside the numbers rather than left in the
/// store, because a score is only a claim about a pipeline once you can see
/// what it was taken over (P4). What this does *not* do is say which side of a
/// metric is better: `ragondin compare` takes that same position, for the
/// reason it records — this binary knows no metric's direction.
fn render(run: &Run) -> String {
    let mut summary = format!("run {}\n", run.id);
    summary.push_str(&format!("  pipeline      {}\n", run.inputs.pipeline));
    summary.push_str(&format!("  dataset       {}\n", run.inputs.dataset_version));
    summary.push_str(&format!("  index         {}\n", run.inputs.index_version));
    for (role, digest) in &run.inputs.model_hashes {
        summary.push_str(&format!("  model[{role}]  {digest}\n"));
    }
    for binding in &run.bindings {
        summary.push_str(&format!(
            "  bound[{}/{}]  {}\n",
            binding.family, binding.name, binding.uri
        ));
    }
    for (name, value) in run.metrics.iter() {
        summary.push_str(&format!("{name}: {value:.4}\n"));
    }
    summary
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ragondin_experiments::{ConfigDocument, RunId, RunInputs};
    use ragondin_pipeline::PipelineHash;

    use super::*;

    fn datasets() -> PathBuf {
        PathBuf::from("/datasets")
    }

    #[test]
    fn a_beir_selector_names_a_directory_under_the_datasets_root() {
        assert_eq!(
            benchmark_root("beir/scifact", &datasets()).expect("beir is supported"),
            (Format::Beir, PathBuf::from("/datasets/scifact"))
        );
    }

    #[test]
    fn a_beir_qa_selector_reads_the_same_directory_with_its_answers() {
        // ADR-C30 § 2: `beir-qa/` is a BEIR directory read with its
        // `answers.jsonl`; `beir/` over the same directory ignores the file.
        assert_eq!(
            benchmark_root("beir-qa/dataset", &datasets()).expect("beir-qa is supported"),
            (Format::BeirQa, PathBuf::from("/datasets/dataset"))
        );
    }

    #[test]
    fn a_squad_selector_names_the_directory_its_dev_file_sits_in() {
        assert_eq!(
            benchmark_root("squad/squad-v1.1", &datasets()).expect("squad is supported"),
            (Format::Squad, PathBuf::from("/datasets/squad-v1.1"))
        );
    }

    #[test]
    fn a_selector_with_no_format_says_what_the_form_is() {
        let error = benchmark_root("scifact", &datasets()).expect_err("the format is required");

        assert!(error.to_string().contains("beir/"), "{error}");
    }

    #[test]
    fn an_unsupported_format_names_the_formats_this_build_reads() {
        let error = benchmark_root("trec/robust04", &datasets()).expect_err("not a format here");

        assert!(error.to_string().contains("trec"), "{error}");
        for format in ["`beir`", "`beir-qa`", "`squad`"] {
            assert!(error.to_string().contains(format), "{error}");
        }
    }

    #[test]
    fn a_selector_naming_no_dataset_is_refused() {
        for selector in ["beir/", "squad/", "beir-qa/"] {
            let error =
                benchmark_root(selector, &datasets()).expect_err("a format alone is not a dataset");

            assert!(error.to_string().contains("dataset"), "{error}");
        }
    }

    #[test]
    fn a_dataset_name_is_one_directory_and_never_a_path() {
        // A name is joined onto the datasets root, so a separator in it would
        // reach wherever the caller's string pointed — which is not what
        // `--datasets` means.
        let error =
            benchmark_root("squad/../elsewhere", &datasets()).expect_err("a name is not a path");

        assert!(error.to_string().contains("../elsewhere"), "{error}");
    }

    fn a_run(metrics: &[(&str, f64)]) -> Run {
        Run {
            id: RunId::from_digest([0xab; 32]),
            inputs: RunInputs {
                pipeline: PipelineHash::from_digest([0xbe; 32]),
                dataset_version: "beir-mini@1".to_owned(),
                index_version: "corpus@1".to_owned(),
                model_hashes: BTreeMap::new(),
                engine_version: "0.0.0".to_owned(),
            },
            metrics: metrics.iter().copied().collect(),
            config: ConfigDocument::new("pipeline:\n  inputs: []\n  nodes: []\n"),
            traces: BTreeMap::new(),
            bindings: Vec::new(),
            times: None,
            provenance: None,
        }
    }

    #[test]
    fn the_summary_names_the_run_and_every_metric_it_scored() {
        let run = a_run(&[("ndcg@10", 0.64), ("recall@10", 0.75)]);

        let summary = render(&run);

        assert!(summary.contains(&run.id.to_string()), "{summary}");
        assert!(summary.contains("ndcg@10: 0.6400"), "{summary}");
        assert!(summary.contains("recall@10: 0.7500"), "{summary}");
    }

    #[test]
    fn the_summary_names_where_each_bound_component_answered() {
        let mut run = a_run(&[("ndcg@10", 0.5)]);
        run.bindings = vec![ragondin_experiments::RunBinding {
            family: "generator".to_owned(),
            name: "vllm".to_owned(),
            uri: "http://localhost:8000".to_owned(),
        }];

        let summary = render(&run);

        assert!(
            summary.contains("bound[generator/vllm]  http://localhost:8000"),
            "{summary}"
        );
    }

    #[test]
    fn the_summary_carries_what_the_run_was_taken_over() {
        // The identity tuple is what makes the number reproducible (P4), so a
        // summary that printed the score alone would be the one thing a reader
        // cannot check later.
        let summary = render(&a_run(&[("ndcg@10", 0.5)]));

        assert!(summary.contains("beir-mini@1"), "{summary}");
        assert!(summary.contains("corpus@1"), "{summary}");
    }

    #[test]
    fn the_summary_is_the_same_whether_or_not_the_times_are_known() {
        // The times are for the UI to display and order runs by; `bench`'s
        // printed output is byte for byte what it was before they existed.
        let without = a_run(&[("ndcg@10", 0.5), ("recall@10", 0.75)]);
        let with = Run {
            times: Some(RunTimes::new(
                UnixMillis::new(1_700_000_000_000),
                UnixMillis::new(1_700_000_004_250),
            )),
            ..without.clone()
        };

        assert_eq!(render(&with), render(&without));
    }

    #[test]
    fn the_summary_is_the_same_whether_or_not_a_launch_record_is_kept() {
        // The record is for the UI's grouping; `bench`'s printed output is
        // byte for byte what it was before it existed.
        let without = a_run(&[("ndcg@10", 0.5), ("recall@10", 0.75)]);
        let with = Run {
            provenance: Some(RunProvenance::named("hybrid")),
            ..without.clone()
        };

        assert_eq!(render(&with), render(&without));
    }

    /// A workspace of this test's own under the system's temporary
    /// directory, emptied first, holding the files named.
    fn workspace(test_name: &str, files: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ragondin-bench-launched-as-{}-{test_name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        for file in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a writable tmpdir");
            std::fs::write(&path, "pipeline: {}\n").expect("a writable tmpdir");
        }
        root
    }

    #[test]
    fn the_name_is_the_stem_of_a_yaml_file_under_the_store_s_workspace() {
        let w = workspace("named", &["pipelines/hybrid.yaml"]);

        // Fresh: `runs/` does not exist yet, and the name is still recorded.
        assert_eq!(
            launched_as(&w.join("pipelines/hybrid.yaml"), &w.join("runs")),
            Some("hybrid".to_owned())
        );
        std::fs::create_dir_all(w.join("runs")).expect("a store");
        assert_eq!(
            launched_as(&w.join("pipelines/hybrid.yaml"), &w.join("runs")),
            Some("hybrid".to_owned())
        );
    }

    #[test]
    fn no_name_outside_the_convention() {
        let w = workspace(
            "unnamed",
            &[
                "pipelines/hybrid.yml",
                "pipelines/hybrid.yaml",
                "drafts/hybrid.yaml",
                "pipelines/nested/hybrid.yaml",
                "elsewhere/pipelines/hybrid.yaml",
            ],
        );
        let none = [
            ("pipelines/hybrid.yml", "runs"),
            ("drafts/hybrid.yaml", "runs"),
            ("pipelines/nested/hybrid.yaml", "runs"),
            ("pipelines/hybrid.yaml", "store"),
            ("pipelines/hybrid.yaml", "elsewhere/runs"),
            ("elsewhere/pipelines/hybrid.yaml", "runs"),
            // A store whose parent does not exist.
            ("pipelines/hybrid.yaml", "absent/runs"),
        ];
        for (config, store) in none {
            assert_eq!(
                launched_as(&w.join(config), &w.join(store)),
                None,
                "{config} into {store}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_runs_directory_that_is_a_link_is_still_the_workspace_s_store() {
        // `<W>/runs` linked to a bigger disk: the workspace is read from the
        // path as given, as `ragondin ui --store` reads it, never from where
        // the link points.
        let w = workspace("runs-link", &["pipelines/hybrid.yaml"]);
        let disk = workspace("runs-link-disk", &[]);
        std::fs::create_dir_all(&disk).expect("a writable tmpdir");
        std::os::unix::fs::symlink(&disk, w.join("runs")).expect("a link is creatable");

        assert_eq!(
            launched_as(&w.join("pipelines/hybrid.yaml"), &w.join("runs")),
            Some("hybrid".to_owned())
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_config_file_that_is_a_link_is_named_by_where_it_sits() {
        let w = workspace(
            "config-link",
            &["pipelines/hybrid.yaml", "drafts/other.yaml"],
        );

        // A link inside `pipelines/` to a file elsewhere is that pipeline,
        // under its own stem — as the workspace lists it.
        std::os::unix::fs::symlink(w.join("drafts/other.yaml"), w.join("pipelines/alias.yaml"))
            .expect("a link is creatable");
        assert_eq!(
            launched_as(&w.join("pipelines/alias.yaml"), &w.join("runs")),
            Some("alias".to_owned())
        );

        // A link outside `pipelines/` to a workspace pipeline is not one.
        std::os::unix::fs::symlink(
            w.join("pipelines/hybrid.yaml"),
            w.join("drafts/hybrid.yaml"),
        )
        .expect("a link is creatable");
        assert_eq!(
            launched_as(&w.join("drafts/hybrid.yaml"), &w.join("runs")),
            None
        );
    }
}
