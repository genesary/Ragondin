//! The one path from a pipeline and a benchmark to a [`Run`], shared by
//! `bench` and by `ragondin ui`'s launcher (ADR-C36 § 1): a **preparation**
//! that goes as far as the run's identity is known, and an **execution** that
//! takes it from there. There is no second copy of these steps anywhere in the
//! binary (P1, ADR-4): `bench` is [`prepare`] → [`Prepared::identity`],
//! refused when the store holds it → [`Prepared::execute`] → save → print, and
//! the launcher is [`prepare`] → [`Prepared::identity`] at submission, and
//! [`prepare`] → [`Prepared::execute`] again at execution.
//!
//! What is genuinely this module's own is the *order* — ADR-C32 § 4's six
//! steps, with the identity of every component this build knows how to
//! construct read before the benchmark is loaded — and one thing only a
//! composition root can do: the execution embeds the corpus before any
//! component exists, because a `ComponentCtor` is synchronous and the two
//! calls that fill a vector store are not.
//!
//! # The two identities
//!
//! [`prepare`] stops after step 3 and the index build, before the expensive
//! embedding, holding everything the harness hashes. [`Prepared::identity`] is
//! the harness's `run_identity` over the [`Evaluation`] that
//! [`Prepared::execute`] then evaluates — one function builds it for both — so
//! the id announced at submission and the id the finished run carries share
//! one construction, and differ only when an input changed in between: a
//! `Remote` service that swapped its model, a dataset rewritten. Within one
//! [`Prepared`] nothing it hashes changes, so the id `bench` checks against
//! the store before executing is the id its finished run carries. The binary
//! assembles no run id of its own; only the harness does.
//!
//! # The stamps
//!
//! The clock is read at the head of [`prepare`], so the identity read, the
//! benchmark load, the index build and the embedding all count toward the
//! run's time, and again once `evaluate` returns `Ok`, before the run is
//! handed back — storing it is not part of running it. Both readings go
//! through `RunTimes::from_readings`; a clock before the epoch leaves the
//! times unknown rather than made up. The launch record is the caller's —
//! `bench`'s workspace name, or the launcher's submitted one — and is stamped
//! here, beside the times and the bindings. All three are outside identity.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use anyhow::{bail, Context, Result};
use ragondin_benchmarks::{BeirAdapter, Benchmark, BenchmarkAdapter, SquadAdapter};
use ragondin_config::{ConfigSource, LocalFile};
use ragondin_contracts::EmbeddedChunk;
use ragondin_engine::EngineContext;
use ragondin_experiments::{ConfigDocument, Run, RunProvenance, RunTimes, UnixMillis};
use ragondin_harness::{evaluate_observed, CorpusIndex, Evaluation, QueryProgress};
use ragondin_pipeline::LogicalPipeline;

use crate::binding::Bindings;
use crate::wiring::{self, NotInBuild};

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

/// Where the pipeline to run comes from.
pub enum Pipeline<'a> {
    /// A configuration file: `bench`'s argument.
    File(&'a Path),
    /// A configuration document: a submission's snapshot.
    #[cfg(feature = "ui")]
    Document(&'a str),
}

/// What a run is asked to be: a pipeline, a benchmark selector, the
/// directory the selector's dataset sits under, and the bindings, already
/// through their refusals on their text.
pub struct Request<'a> {
    /// The pipeline.
    pub pipeline: Pipeline<'a>,
    /// The benchmark selector, `<format>/<name>`.
    pub benchmark: &'a str,
    /// The directory the named dataset sits under.
    pub datasets: &'a Path,
    /// The `Remote` bindings.
    pub bindings: Bindings,
}

/// Why [`prepare`] refused, by the step that refused: the launcher answers
/// each differently, and `bench` reports them all alike
/// ([`into_error`](Self::into_error)).
#[derive(Debug)]
pub enum Refusal {
    /// The configuration: unreadable, invalid, a node v0 does not run, a key
    /// no component reads, a binding no node uses.
    Pipeline(anyhow::Error),
    /// An `impl:` name this build constructs nothing under.
    NotInBuild(NotInBuild),
    /// A component's identity could not be read: a model file missing, a
    /// service that does not answer, a model a service does not serve. The
    /// node is on the error as a [`wiring::AtNode`] context.
    Identity(anyhow::Error),
    /// The benchmark: a selector naming no dataset this build reads, or a
    /// dataset that does not load.
    Benchmark(anyhow::Error),
}

impl Refusal {
    /// The refusal as `bench` reports it, in the words of the step.
    pub fn into_error(self) -> anyhow::Error {
        match self {
            Self::Pipeline(error) | Self::Identity(error) | Self::Benchmark(error) => error,
            Self::NotInBuild(refusal) => refusal.into(),
        }
    }
}

/// A run prepared up to the point its identity is known: steps 1–3 of
/// ADR-C32 § 4 and the index build, everything the harness hashes, and the
/// bound channels — nothing embedded, nothing constructed.
pub struct Prepared {
    started: Option<UnixMillis>,
    config: ConfigDocument,
    pipeline: LogicalPipeline,
    bound: wiring::Bound,
    model_hashes: BTreeMap<String, String>,
    benchmark: Benchmark,
    index: CorpusIndex,
}

/// Prepares `request` up to its identity, stamping when the run started.
///
/// # Errors
///
/// A [`Refusal`], by the step that refused.
pub async fn prepare(request: Request<'_>) -> Result<Prepared, Refusal> {
    // The head of the shared path: the identity read, the benchmark load, the
    // index build and the embedding all count toward the run's time, so
    // `finished − started` is its wall time, preparation included. A launcher
    // that prepares at submission and again at execution stamps the second:
    // the time a job spent queued is the job's, never the run's.
    let started = UnixMillis::from_system_time(SystemTime::now());

    // Read for the record and loaded for the run, from the same text. The run
    // store keeps the configuration **verbatim** — re-serializing the loaded
    // pipeline would file a second spelling of it beside the digest of the
    // first — so the text is what is kept and the pipeline is what is run.
    let text = match request.pipeline {
        Pipeline::File(path) => std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))
            .map_err(Refusal::Pipeline)?,
        #[cfg(feature = "ui")]
        Pipeline::Document(document) => document.to_owned(),
    };
    // Refused on its text alone, whatever the pipeline says.
    let (format, root) =
        benchmark_root(request.benchmark, request.datasets).map_err(Refusal::Benchmark)?;

    // 1. The configuration: what v0 does not run, the keys of every node
    //    whose keys this composition root owns (ADR-C32 § 1), a binding no
    //    node uses (§ 2), and an `impl:` this build constructs nothing under.
    let pipeline = match request.pipeline {
        Pipeline::File(path) => LocalFile::new(path)
            .load()
            .await
            .map_err(anyhow::Error::from),
        #[cfg(feature = "ui")]
        Pipeline::Document(_) => {
            ragondin_config::parse_document(&text).map_err(anyhow::Error::from)
        }
    }
    .map_err(Refusal::Pipeline)?;
    let bindings = request.bindings;
    wiring::refuse_unsupported(&pipeline).map_err(Refusal::Pipeline)?;
    wiring::check_nodes(&pipeline, &bindings).map_err(Refusal::Pipeline)?;
    bindings
        .refuse_unused(&pipeline)
        .map_err(Refusal::Pipeline)?;
    wiring::refuse_not_in_build(&pipeline, &bindings).map_err(Refusal::NotInBuild)?;
    // One lazily connecting channel per binding; nothing connects yet.
    let bound = wiring::Bound::new(bindings).map_err(Refusal::Pipeline)?;
    // 2. Every component's identity, read from the component, before the
    //    benchmark is loaded and the corpus embedded: a model file that is
    //    missing, a service that does not answer, or a model a service does not
    //    serve, is found now and not after the expensive step. A refusal here
    //    ends the run.
    let model_hashes = wiring::model_hashes(&pipeline, &bound)
        .await
        .map_err(Refusal::Identity)?;

    // 3. The benchmark.
    let benchmark = format.load(&root).map_err(Refusal::Benchmark)?;

    // One index, built here and used twice: the components are constructed
    // from its chunks, and the harness records its version as the
    // `index_version` of the run. Building a second one anywhere would make
    // that recorded version name a set nothing searched (ADR-C26).
    let index = CorpusIndex::build(benchmark.corpus());

    Ok(Prepared {
        started,
        config: ConfigDocument::new(text),
        pipeline,
        bound,
        model_hashes,
        benchmark,
        index,
    })
}

impl Prepared {
    /// The evaluation this run is: the one construction both
    /// [`identity`](Self::identity) and [`execute`](Self::execute) hand the
    /// harness.
    fn evaluation(&self) -> Evaluation<'_> {
        Evaluation {
            pipeline: &self.pipeline,
            config: &self.config,
            benchmark: &self.benchmark,
            index: &self.index,
            cutoff: CUTOFF,
            model_hashes: self.model_hashes.clone(),
        }
    }

    /// The id this run will carry, from the harness, without running it.
    pub fn identity(&self) -> ragondin_experiments::RunId {
        ragondin_harness::run_identity(&self.evaluation())
    }

    /// Runs the prepared run — steps 4–6 — reporting each query to
    /// `observer` and stopping between two queries once `cancel` is set, and
    /// returns it with its bindings, its times and `provenance` stamped.
    ///
    /// # Errors
    ///
    /// The corpus that cannot be embedded, and the harness's errors —
    /// `HarnessError::Cancelled` among them — as the source of the error
    /// returned.
    pub async fn execute<O>(
        self,
        provenance: Option<RunProvenance>,
        observer: O,
        cancel: &AtomicBool,
    ) -> Result<Run>
    where
        O: FnMut(QueryProgress<'_>) + Send,
    {
        // 4. The corpus, embedded.
        let embedded = embed(&self.pipeline, &self.index, &self.bound).await?;

        // 5. The components, constructed from that corpus, and one
        //    registration per binding.
        let mut ctx = EngineContext::new();
        wiring::register(
            &mut ctx,
            self.index.chunks(),
            embedded.as_deref(),
            &self.bound,
        );

        // 6. Evaluate.
        let run = evaluate_observed(&self.evaluation(), &ctx, observer, cancel).await?;
        // Read once `evaluate` has returned `Ok`, and before the run is handed
        // back: storing it is not part of running it.
        let finished = UnixMillis::from_system_time(SystemTime::now());
        // Recorded on the run, never in its identity: the harness named the
        // run before it saw them, and where a service listened, when the run
        // happened, or how it was launched (ADR-C39 § 3) is not an input of
        // the experiment (ADR-C32 § 2, INV-8).
        Ok(Run {
            bindings: self.bound.bindings().record(),
            times: RunTimes::from_readings(self.started, finished),
            provenance,
            ..run
        })
    }
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
async fn embed(
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
async fn embed(
    pipeline: &LogicalPipeline,
    index: &CorpusIndex,
    bound: &wiring::Bound,
) -> Result<Option<Vec<EmbeddedChunk>>> {
    let _ = wiring::embedder_spec(pipeline, bound.bindings())?;
    let _ = index;
    Ok(None)
}

/// Resolves a benchmark selector, `<format>/<name>`, against the datasets
/// root.
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

#[cfg(test)]
mod tests {
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
}
