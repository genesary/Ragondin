//! # ragondin
//!
//! The platform binary and the **composition root** — the single place that
//! assembles the engine and the concrete components (P2: one binary, one config
//! file, it runs). One binary, four subcommands:
//!
//! ```text
//! ragondin bench <config> --benchmark beir/scifact --datasets <dir> --store <dir>
//! ragondin compare <run-a> <run-b> --store <path>   # compare two runs
//! ragondin serve <config>                           # serve the pipeline
//! ragondin validate <config>                        # validate a configuration
//! ```
//!
//! All four are **declared**; three are implemented. `validate` loads a
//! configuration through `ragondin-config`, stops at the `LogicalPipeline`, and
//! prints its content hash — the config→logical→hash path end to end, with no
//! registry and no execution. `compare` reads two runs already recorded in a
//! run store (`ragondin-experiments`) and prints their diff — metric by
//! metric, then the configuration parameters they differ in — with no
//! re-execution and no new metric, a packaging-only handler over that
//! crate's comparison (ADR-C15). `bench` evaluates a configuration against a
//! benchmark and records the run. `serve` parses its arguments and then
//! reports that this build does not implement it.
//!
//! **`bench` is where the composition root does its job** (§4.3): it is the one
//! subcommand that registers concrete components on an `EngineContext`, which
//! is why this crate — and no crate under it — depends on them (INV-5). The
//! registration itself lives in [`wiring`], the corpus preparation and the
//! order of the steps in [`mod@bench`].
//!
//! `tests/vertical_slice.rs` assembles the same root over `ragondin-stub`
//! instead: components that fabricate their answers, so that what the test
//! asserts on is the wiring — a plan, and the trace the executor returns —
//! rather than a retrieval result.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod bench;
mod compare;
mod validate;
mod wiring;

/// The command line, as `clap` parses it.
#[derive(Debug, Parser)]
#[command(
    name = "ragondin",
    version,
    about = "Evaluate, compare, serve and validate RAG pipelines.",
    long_about = "One binary, one configuration file, it runs.\n\n\
                  A pipeline is described by a single YAML file. `validate` \
                  checks one without running it; the other subcommands take \
                  the same file into an evaluation or a serving deployment."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// The four subcommands of `docs/code-architecture.md` §4.2.
///
/// They are one binary's doors onto crates that stay distinct behind them: the
/// serving driver and the evaluation harness are separate crates over the same
/// engine, and bringing them under one command name is packaging rather than
/// architecture (ADR-C15).
#[derive(Debug, Subcommand)]
enum Command {
    /// Evaluate a pipeline against a benchmark.
    #[command(long_about = "Evaluate a pipeline against a benchmark.\n\n\
        The configuration is loaded, the dataset is read from disk, the corpus \
        is prepared once, and the components this build carries are \
        constructed from it. Every query of the benchmark is executed and the \
        judged ones are scored; the run is written to the store and its \
        identity and metrics are printed.\n\n\
        v0 evaluates retrieval only: a configuration holding an extension node \
        is refused. Neither `--datasets` nor `--store` has a default, because \
        no location for either is settled yet.")]
    Bench {
        /// The pipeline configuration to evaluate.
        config: PathBuf,
        /// The benchmark to evaluate it against, e.g. `beir/scifact`.
        #[arg(long)]
        benchmark: String,
        /// Root directory the named dataset sits under.
        #[arg(long)]
        datasets: PathBuf,
        /// Root directory of the run store the run is written to.
        #[arg(long)]
        store: PathBuf,
    },
    /// Compare two runs.
    #[command(long_about = "Compare two runs already recorded in a run store.\n\n\
        Both are read by their `run_id` and never re-executed: the diff is \
        metric by metric, over whatever either run recorded, and names which \
        side scored higher on each one; then it names the configuration \
        parameters and `impl:` names the two runs differ in, node by node, \
        with both values. No default run store location is \
        settled yet, so `--store` names it explicitly.")]
    Compare {
        /// The first run's identity.
        run_a: String,
        /// The second run's identity.
        run_b: String,
        /// Root directory of the run store both runs are read from.
        #[arg(long)]
        store: PathBuf,
    },
    /// Serve a pipeline over the network.
    Serve {
        /// The pipeline configuration to serve.
        config: PathBuf,
    },
    /// Check a configuration and print its content hash, without running it.
    #[command(long_about = "Check a configuration and print its content hash, \
        without running it.\n\n\
        The file is parsed into the wire schema, validated, and canonicalized; \
        the hash is over that canonical logical form, so two files that differ \
        only in formatting print the same hash. Nothing is executed and no \
        component is resolved, so a configuration naming an implementation this \
        build does not have still validates.\n\n\
        The checks include the kind of every edge: a node fed a value of the \
        wrong kind is reported with the edge, the kind expected and the kind \
        found. `extension` nodes are the exception — their ports are not known \
        to the core, so an edge at one is not kind-checked. An edge arriving \
        where the consuming node declares no port at all is still refused, \
        whatever produced it.")]
    Validate {
        /// The pipeline configuration to check.
        config: PathBuf,
    },
}

/// Dispatches one parsed command. Thin by rule: a subcommand handler composes
/// and plumbs, and the work belongs to the crate behind it.
async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Validate { config } => validate::run(&config).await,
        Command::Compare {
            run_a,
            run_b,
            store,
        } => compare::run(&store, &run_a, &run_b),
        Command::Bench {
            config,
            benchmark,
            datasets,
            store,
        } => {
            bench::run(&bench::Request {
                config: &config,
                benchmark: &benchmark,
                datasets: &datasets,
                store: &store,
            })
            .await
        }
        Command::Serve { .. } => anyhow::bail!("serving is not available in v0"),
    }
}

/// Prints an error and everything under it, one cause per line.
///
/// `anyhow`'s own `Debug` rendering would do nearly this, and this exists to
/// keep the shape of a failure report the binary's decision rather than a
/// dependency's: `validate` builds a multi-line report for an incompatible
/// wiring, and it must not arrive under a heading that says `Error:`.
fn report(error: &anyhow::Error) {
    eprintln!("error: {error}");
    for cause in error.chain().skip(1) {
        eprintln!("  caused by: {cause}");
    }
}

/// Returns an exit status rather than a `Result`, so that no failure of this
/// binary is rendered by `anyhow`'s `Debug` and every one goes through
/// [`report`].
#[tokio::main]
async fn main() -> ExitCode {
    match dispatch(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&error);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_line_definition_is_internally_consistent() {
        // clap's own assertions: duplicate argument ids, a long flag declared
        // twice, an `arg` that no subcommand can reach. They run only when
        // asked, and a binary that never asks discovers them in the field.
        Cli::command().debug_assert();
    }
}
