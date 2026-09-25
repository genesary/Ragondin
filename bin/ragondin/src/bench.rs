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
//! six steps, with every component's identity read before the benchmark is
//! loaded — and one thing only a composition root can do: [`prepare`] embeds
//! the corpus before any component exists, because a `ComponentCtor` is
//! synchronous and the two calls that fill a vector store are not.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ragondin_benchmarks::{BeirAdapter, Benchmark, BenchmarkAdapter, SquadAdapter};
use ragondin_config::{ConfigSource, LocalFile};
use ragondin_contracts::EmbeddedChunk;
use ragondin_engine::EngineContext;
use ragondin_experiments::{ConfigDocument, FileSystemRunStore, Run};
use ragondin_harness::{evaluate, CorpusIndex, Evaluation};
use ragondin_pipeline::LogicalPipeline;

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
}

/// Evaluates the configuration against the benchmark, records the run, and
/// prints what it scored.
///
/// # Errors
///
/// Anything on the way: an unreadable or invalid configuration, a
/// configuration v0 does not run, a selector naming no dataset this build
/// reads, a dataset that does not load, an `impl:` this build did not register,
/// a query that fails, or a store that cannot be written.
pub async fn run(request: &Request<'_>) -> Result<()> {
    // Read for the record and loaded for the run, from the same file. The run
    // store keeps the configuration **verbatim** — re-serializing the loaded
    // pipeline would file a second spelling of it beside the digest of the
    // first — so the text is what is kept and the pipeline is what is run.
    let text = std::fs::read_to_string(request.config)
        .with_context(|| format!("reading {}", request.config.display()))?;
    let (format, root) = benchmark_root(request.benchmark, request.datasets)?;

    // 1. The configuration: what v0 does not run, and the keys of every node
    //    whose keys this composition root owns (ADR-C32 § 1).
    let pipeline = LocalFile::new(request.config).load().await?;
    wiring::refuse_unsupported(&pipeline)?;
    wiring::check_nodes(&pipeline)?;
    // 2. Every component's identity, read from the component, before the
    //    benchmark is loaded and the corpus embedded: a model file that is
    //    missing, or a model a generator does not serve, is found now and not
    //    after the expensive step. A refusal here ends the run.
    let model_hashes = wiring::model_hashes(&pipeline).await?;

    // 3. The benchmark.
    let benchmark = format.load(&root)?;

    // One index, built here and used twice: the components below are
    // constructed from its chunks, and the harness records its version as the
    // `index_version` of the run. Building a second one anywhere would make
    // that recorded version name a set nothing searched (ADR-C26).
    let index = CorpusIndex::build(benchmark.corpus());
    // 4. The corpus, embedded.
    let embedded = prepare(&pipeline, &index).await?;

    // 5. The components, constructed from that corpus.
    let mut ctx = EngineContext::new();
    wiring::register(&mut ctx, index.chunks(), embedded.as_deref());

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

    FileSystemRunStore::new(request.store).save(&run)?;
    print!("{}", render(&run));

    Ok(())
}

/// Embeds the corpus, if the pipeline retrieves densely over it.
///
/// The whole of the asynchrony a component constructor cannot do: `embed` is
/// `async` and `ComponentCtor` is not, so the vectors are made here, before
/// any component exists, and the store reaches its constructor already holding
/// them. `None` means the pipeline names no dense node — there is then nothing
/// to embed, and nothing downstream to search.
#[cfg(feature = "onnx")]
async fn prepare(
    pipeline: &LogicalPipeline,
    index: &CorpusIndex,
) -> Result<Option<Vec<EmbeddedChunk>>> {
    use ragondin_contracts::{EmbedParams, EmbedRole, Embedder};

    let Some(spec) = wiring::embedder_spec(pipeline)? else {
        return Ok(None);
    };

    let embedder = wiring::onnx_embedder(&spec).context("constructing the corpus embedder")?;
    let texts: Vec<String> = index
        .chunks()
        .iter()
        .map(|chunk| chunk.text.clone())
        .collect();

    // `Passage`, and that is the half of ADR-C17 only the indexer supplies: a
    // corpus embedded under the query prefix is silently the wrong index.
    let vectors = embedder
        .embed(&texts, &EmbedParams::new(EmbedRole::Passage))
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
/// The configuration is checked all the same. A pipeline whose `dense` nodes
/// disagree about the embedder is malformed whether or not this build could
/// have run it, and a diagnosis that depends on which features were compiled in
/// is one the person reading it cannot reproduce. What this build cannot do is
/// *run* the node, and that is reported at planning, where the unknown `impl:`
/// is named against the node that carries it.
#[cfg(not(feature = "onnx"))]
async fn prepare(
    pipeline: &LogicalPipeline,
    index: &CorpusIndex,
) -> Result<Option<Vec<EmbeddedChunk>>> {
    let _ = wiring::embedder_spec(pipeline)?;
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
    fn the_summary_carries_what_the_run_was_taken_over() {
        // The identity tuple is what makes the number reproducible (P4), so a
        // summary that printed the score alone would be the one thing a reader
        // cannot check later.
        let summary = render(&a_run(&[("ndcg@10", 0.5)]));

        assert!(summary.contains("beir-mini@1"), "{summary}");
        assert!(summary.contains("corpus@1"), "{summary}");
    }
}
