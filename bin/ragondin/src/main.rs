//! # ragondin
//!
//! The platform binary and the **composition root** — the single place that
//! assembles the engine and the concrete components (P2: one binary, one config
//! file, it runs). One binary, five subcommands:
//!
//! ```text
//! ragondin bench <config> --benchmark beir/scifact --datasets <dir> --store <dir> \
//!                [--remote <family>/<name>=<uri>]...
//! ragondin compare <run-a> <run-b> --store <path>   # compare two runs
//! ragondin serve <config>                           # serve the pipeline
//! ragondin ui [--workspace <dir> | --store <dir>] [--port <port>] [--bind <loopback>]  # the UI
//! ragondin validate <config>                        # validate a configuration
//! ```
//!
//! All five are **declared**; four are implemented, one of them behind a
//! feature. `validate` loads a
//! configuration through `ragondin-config`, stops at the `LogicalPipeline`, and
//! prints its content hash — the config→logical→hash path end to end, with no
//! registry and no execution. `compare` reads two runs already recorded in a
//! run store (`ragondin-experiments`) and prints their diff — metric by
//! metric, then the configuration parameters they differ in — with no
//! re-execution and no new metric, a packaging-only handler over that
//! crate's comparison (ADR-C15). `bench` evaluates a configuration against a
//! benchmark and records the run. `serve` parses its arguments and then
//! reports that this build does not implement it. `ui`, behind the `ui`
//! feature, serves the front end and its JSON API on a loopback address
//! (ADR-C36 § 1); a build without the feature refuses it, naming the feature.
//!
//! Beside the subcommands, `ragondin --notices` prints the licence notices of
//! the Rust crates the binary links, which [`notices`] embeds.
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
mod binding;
mod compare;
mod notices;
#[cfg(feature = "ui")]
mod ui;
mod validate;
mod wiring;

/// The fake `Remote` services `tests/bench.rs` binds, shared with the unit
/// tests of [`wiring`] rather than written twice.
#[cfg(all(test, feature = "remote"))]
#[path = "../tests/support/remote.rs"]
mod remote_fakes;

/// The command line, as `clap` parses it.
#[derive(Debug, Parser)]
#[command(
    name = "ragondin",
    version,
    about = "Evaluate, compare, serve and validate RAG pipelines, and browse them in a UI.",
    long_about = "One binary, one configuration file, it runs.\n\n\
                  A pipeline is described by a single YAML file. `validate` \
                  checks one without running it; the other subcommands take \
                  the same file into an evaluation or a serving deployment.\n\n\
                  ragondin is licensed under Apache-2.0. `--notices` prints \
                  the licence notices of the Rust crates it links."
)]
#[command(arg_required_else_help = true, args_conflicts_with_subcommands = true)]
struct Cli {
    /// Print the licence notices of the Rust crates this binary links, and
    /// exit.
    #[arg(long, exclusive = true)]
    notices: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

/// The five subcommands of `docs/code-architecture.md` §4.2.
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
        `--benchmark` names a format and a dataset: `beir/<dir>` (qrels only), \
        `beir-qa/<dir>` (the same directory with its `answers.jsonl`), or \
        `squad/<dir>` (the SQuAD v1.1 dev file). A benchmark carrying reference \
        answers is scored by exact match and token F1 as well, and a pipeline \
        that produces no answer is refused over it: a retrieval-only \
        configuration runs under `beir/`, not `beir-qa/` or `squad/`.\n\n\
        `--remote <family>/<name>=<uri>`, repeatable, binds an implementation \
        name to the service that answers under it: a node names a `Remote` \
        component by an ordinary `impl:` (or, for a `dense` node's embedder, \
        `embedder:`) value, and the address stays out of the configuration and \
        out of the run's identity. `<family>` is `retriever`, `fusion`, \
        `reranker`, `context_builder`, `generator` or `embedder`; `<uri>` is \
        `http://<host>` or `http://<host>:<port>`. A binding no node uses, one \
        bound twice, or one naming a component this binary carries in-process is \
        refused. The run records its bindings, outside its identity. A build \
        without the `remote` feature refuses every binding.\n\n\
        v0 runs retrieval and generation: a configuration holding an extension \
        node is refused. Neither `--datasets` nor `--store` has a default, because \
        no location for either is settled yet.")]
    Bench {
        /// The pipeline configuration to evaluate.
        config: PathBuf,
        /// The benchmark to evaluate it against: `beir/<dir>`, `beir-qa/<dir>`
        /// or `squad/<dir>`, e.g. `beir/scifact`.
        #[arg(long)]
        benchmark: String,
        /// Root directory the named dataset sits under.
        #[arg(long)]
        datasets: PathBuf,
        /// Root directory of the run store the run is written to.
        #[arg(long)]
        store: PathBuf,
        /// Binds a name to a service: `<family>/<name>=<uri>`, repeatable.
        #[arg(long, value_name = "FAMILY/NAME=URI")]
        remote: Vec<String>,
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
    /// Serve the UI and its API on a loopback address.
    #[command(
        long_about = "Serve the UI and its JSON API on a loopback address.\n\n\
        The UI's pages are served at `/` and the API under `/api/v1/`, over a \
        workspace: `workspace.toml` (the datasets directory and the services), \
        `pipelines/`, `layouts/`, `runs/`, `jobs/`, `cache/` and `datasets/`, \
        each created when missing. `--workspace <dir>` names it, its runs in \
        `<dir>/runs`. `--store <dir>` names the run store instead, as `bench \
        --store` does: the workspace is the store's parent when the store is \
        called `runs`, and the store's own directory otherwise, so `bench \
        --store <ws>/runs` writes where `ui --store <ws>/runs` reads. With \
        neither, `./runs` is opened when it is a directory, and otherwise \
        `~/.ragondin` is, created on first use. A malformed `workspace.toml` is \
        refused, naming its line, and never repaired. The workspace, its store \
        and the address are printed on start. Nothing opens a browser.\n\n\
        The server listens on loopback only: `--bind` accepts `127.0.0.1` (the \
        default) or `::1`, and refuses any other address, because the server has \
        no authentication yet. To use it from another machine, run it there and \
        forward its port over SSH (`ssh -L <port>:127.0.0.1:<port> <host>`), \
        then open the printed address on this one.\n\n\
        `--port 0` asks the system for a free port. A build without the `ui` \
        feature refuses this subcommand.\n\n\
        ragondin is licensed under Apache-2.0. The UI it serves bundles \
        third-party software and fonts under their own licences, whose notices \
        it serves at `/third-party-notices.txt`; the notices of the Rust crates \
        the binary links are served at `/third-party-notices-rust.txt`."
    )]
    Ui {
        /// The workspace directory, its runs in `<dir>/runs`.
        #[arg(long, conflicts_with = "store")]
        workspace: Option<PathBuf>,
        /// The run store, as `bench --store` names it; the workspace is its
        /// parent when it is called `runs`.
        #[arg(long)]
        store: Option<PathBuf>,
        /// The port to listen on; 0 for any free port. [default: 7341]
        #[arg(long)]
        port: Option<u16>,
        /// The loopback address to listen on: `127.0.0.1` or `::1`.
        /// [default: 127.0.0.1]
        #[arg(long)]
        bind: Option<String>,
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
    // The parser lets the subcommand be absent only for `--notices`, which
    // takes no other argument; a bare `ragondin` is help and exit 2 there.
    let command = match (cli.notices, cli.command) {
        (true, _) => {
            print!("{}", notices::RUST_CRATES);
            return Ok(());
        }
        (false, Some(command)) => command,
        (false, None) => unreachable!(
            "clap's arg_required_else_help answers a bare `ragondin` before dispatch, \
             and `--notices` is the only argument accepted without a subcommand"
        ),
    };
    match command {
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
            remote,
        } => {
            bench::run(&bench::Request {
                config: &config,
                benchmark: &benchmark,
                datasets: &datasets,
                store: &store,
                remote: &remote,
            })
            .await
        }
        Command::Serve { .. } => anyhow::bail!("serving is not available in v0"),
        #[cfg(feature = "ui")]
        Command::Ui {
            workspace,
            store,
            port,
            bind,
        } => {
            ui::run(&ui::Request {
                workspace: workspace.as_deref(),
                store: store.as_deref(),
                port,
                bind: bind.as_deref(),
            })
            .await
        }
        #[cfg(not(feature = "ui"))]
        Command::Ui { .. } => {
            anyhow::bail!("this build does not carry the UI; rebuild with `--features ui`")
        }
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
