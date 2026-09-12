//! # ragondin
//!
//! The platform binary and the **composition root** — the single place that
//! assembles the engine and the concrete components (P2: one binary, one config
//! file, it runs). One binary, four subcommands:
//!
//! ```text
//! ragondin bench <config> --benchmark beir/scifact  # evaluate a pipeline
//! ragondin compare <run-a> <run-b>                  # compare two runs
//! ragondin serve <config>                           # serve the pipeline
//! ragondin validate <config>                        # validate a configuration
//! ```
//!
//! All four are **declared**; one is implemented. `validate` loads a
//! configuration through `ragondin-config`, stops at the `LogicalPipeline`, and
//! prints its content hash — the config→logical→hash path end to end, with no
//! registry and no execution. `bench`, `compare` and `serve` parse their
//! arguments and then report that this build does not implement them.
//!
//! Component wiring is therefore still absent from `main`. There is more here
//! than `main`, though: `tests/vertical_slice.rs` assembles the composition root
//! for real — a configuration file read from disk, stub components registered on
//! an `EngineContext` through the ordinary API, a plan, and the trace the
//! executor returns. It is where the wiring this binary will do is exercised
//! first.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod validate;

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
    Bench {
        /// The pipeline configuration to evaluate.
        config: PathBuf,
        /// The benchmark to evaluate it against, e.g. `beir/scifact`.
        #[arg(long)]
        benchmark: String,
    },
    /// Compare two runs.
    Compare {
        /// The first run's identity.
        run_a: String,
        /// The second run's identity.
        run_b: String,
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
        Command::Bench { .. } => anyhow::bail!("`bench` is not implemented in this build"),
        Command::Compare { .. } => anyhow::bail!("`compare` is not implemented in this build"),
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
