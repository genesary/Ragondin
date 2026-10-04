//! The `bench` subcommand: evaluate a pipeline against a benchmark.
//!
//! This is the composition root doing its one job
//! (`docs/code-architecture.md` §4.3). It is the only place that knows both
//! the engine and the concrete components, so it is the only place that can
//! put them together: it loads the configuration, loads the benchmark,
//! prepares the corpus, constructs the components **from that corpus**, and
//! hands the harness a context and the same prepared index (ADR-C26).
//!
//! Thin by rule, all the same. The steps — ADR-C32 § 4's six, in their order —
//! are [`crate::execution`]'s, shared with `ragondin ui`'s launcher, and the
//! run's times are stamped there; `bench` is that preparation, then that
//! execution, then the save and the summary. What is this module's own is
//! what it does around them: the launch record it stamps from its two paths,
//! and what it does with a run the store already holds.
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
//!
//! # A run already stored
//!
//! One run, one record, and the first launch's record wins (ADR-C39 § 8).
//! After the evaluation, `bench` asks the store for the run id the harness
//! computed. When the store holds it, nothing is saved, the stored run is left
//! byte for byte as it was, and instead of the summary `bench` prints the id
//! and then the two facts in this order:
//!
//! ```text
//! run <id>
//!   already stored, launched as <name>; this execution was not kept
//! ```
//!
//! `<name>` is the stored record's name. When the stored run has no record,
//! or a record with no name, the line is
//! `already stored; this execution was not kept`. It exits `0`: nothing
//! failed, and a script that re-runs a benchmark keeps working. A run the
//! store holds but cannot read is an error that names the run, and nothing is
//! saved over it; before this rule it exited `0` with the usual summary. When the store does not hold the id, `bench` saves the run
//! and prints exactly what it always has.
//!
//! The whole evaluation still runs before the store is asked: refusing
//! before the corpus is embedded, as the UI's `409 run_exists` does, is a
//! change of `bench`'s behaviour this module does not make yet.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use ragondin_experiments::{FileSystemRunStore, Run, RunProvenance, RunStoreError};

use crate::binding::Bindings;
use crate::execution::{self, Pipeline, Refusal};

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
    // Refused on their text alone: a malformed binding is found before the
    // configuration is even read (ADR-C32 § 2).
    let bindings = Bindings::parse(request.remote)?;

    // Steps 1–3 and the index build, then 4–6: the path the launcher runs,
    // which stamps the run's times and its bindings. How it was launched
    // (ADR-C39 § 3) is `bench`'s to say: the workspace name, or no record at
    // all — never an empty one, and never a path.
    let prepared = execution::prepare(execution::Request {
        pipeline: Pipeline::File(request.config),
        benchmark: request.benchmark,
        datasets: request.datasets,
        bindings,
    })
    .await
    .map_err(Refusal::into_error)?;
    let run = prepared
        .execute(
            launched_as(request.config, request.store).map(RunProvenance::named),
            |_| {},
            &AtomicBool::new(false),
        )
        .await?;

    // One run, one record (ADR-C39 § 8): a run already stored is kept as it
    // is, its launch record included, and this execution is dropped and said
    // to be — `save` is not called, and the run is never reported as filed.
    // Looked up through the store's own read, so "stored" means what the
    // store says, and the record is read as the store reads it.
    let store = FileSystemRunStore::new(request.store);
    match store.load(&run.id) {
        Ok(stored) => {
            print!("{}", render_already_stored(&stored));
            return Ok(());
        }
        Err(RunStoreError::NotFound { .. }) => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "run {} is already in the store and does not read; this execution was not kept",
                    run.id
                )
            })
        }
    }
    store.save(&run)?;
    print!("{}", render(&run));

    Ok(())
}

/// What `bench` prints for a run the store already holds: its id, then that
/// it was kept and this execution was not — naming the workspace pipeline it
/// was first launched as when its record has a name, and naming none when it
/// has no record or a record without one.
fn render_already_stored(stored: &Run) -> String {
    match stored.provenance.as_ref().and_then(RunProvenance::name) {
        Some(name) => format!(
            "run {}\n  already stored, launched as {name}; this execution was not kept\n",
            stored.id
        ),
        None => format!(
            "run {}\n  already stored; this execution was not kept\n",
            stored.id
        ),
    }
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

    use ragondin_experiments::{ConfigDocument, RunId, RunInputs, RunTimes, UnixMillis};
    use ragondin_pipeline::PipelineHash;

    use super::*;

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
