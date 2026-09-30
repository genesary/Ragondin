//! `ragondin ui`: the JSON API and the UI's embedded assets on a loopback
//! listener (ADR-C36 § 1).
//!
//! The handler is the composition root's, and thin: it checks the address,
//! binds the listener, constructs the backends `ragondin-api` consumes, hands
//! them in with the embedded assets, and serves what comes back. The router,
//! its endpoints and its four security layers are `ragondin-api`'s, applied
//! last over the API and the assets alike; the `Server` it returns is served
//! as it is, never wrapped in a router of this crate's, which would answer
//! outside those layers.
//!
//! - [`address`] — the loopback rule, and its refusal.
//! - [`assets`] — the embedded UI, or the notice page, at `/`.
//! - [`launcher`] — `Launcher`: capabilities, the identity probe.
//! - [`stopgap`] — the empty backends the file backends replace.

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use ragondin_api::{Backends, ServerConfig};
use ragondin_experiments::FileSystemRunStore;

mod address;
mod assets;
mod launcher;
mod stopgap;

/// The directory under the workspace the run store is rooted at: where
/// `bench --store <workspace>/runs` writes, so the UI reads what the command
/// line records (the design document § 6).
const RUNS: &str = "runs";

/// One `ragondin ui`'s arguments.
pub struct Request<'a> {
    /// The workspace directory.
    pub workspace: &'a Path,
    /// `--port`, when given.
    pub port: Option<u16>,
    /// `--bind`, when given.
    pub bind: Option<&'a str>,
}

/// Runs the server until the process is stopped.
///
/// The address is refused before anything opens; the workspace must be a
/// directory. Once bound, the URL is printed on stdout — the port the system
/// chose, under `--port 0` — and the served authority handed to the `Host`
/// check is exactly the one printed.
pub async fn run(request: &Request<'_>) -> Result<()> {
    let address = address::listen_address(request.bind, request.port)?;
    if !request.workspace.is_dir() {
        bail!(
            "the workspace `{}` is not a directory",
            request.workspace.display()
        );
    }

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("cannot listen on {address}"))?;
    let served = listener
        .local_addr()
        .context("the listener has no local address")?
        .to_string();

    let backends = Backends {
        runs: Arc::new(FileSystemRunStore::new(request.workspace.join(RUNS))),
        pipelines: Arc::new(stopgap::NoPipelines),
        registry: Arc::new(stopgap::NoBenchmarks),
        settings: Arc::new(stopgap::NoSettings),
        launcher: Arc::new(launcher::BinaryLauncher),
    };
    let server = ragondin_api::router(
        backends,
        ServerConfig {
            served: served.clone(),
            build: build_identity(),
            workspace: request.workspace.to_path_buf(),
        },
        assets::router(),
    );

    println!(
        "ragondin ui: serving {} at http://{served}/",
        request.workspace.display()
    );
    axum::serve(listener, server.into_make_service())
        .await
        .context("the server stopped")
}

/// The build's identity: the crate version, then the commit `build.rs` read,
/// e.g. `0.0.0+3f9a1c2b7d4e`, or `0.0.0+unknown` outside a git checkout. The
/// UI compares it with the one it loaded under and reloads when they differ
/// (ADR-C36 § 1); a commit changes with any committed change to the API or to
/// the UI it embeds, which a version alone would not.
fn build_identity() -> String {
    format!(
        "{}+{}",
        env!("CARGO_PKG_VERSION"),
        env!("RAGONDIN_BUILD_COMMIT")
    )
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_help_states_the_default_port_this_module_listens_on() {
        let mut command = crate::Cli::command();
        let help = command
            .find_subcommand_mut("ui")
            .expect("`ui` is declared")
            .render_help()
            .to_string();

        assert!(
            help.contains(&format!("[default: {}]", address::DEFAULT_PORT)),
            "{help}"
        );
    }

    #[test]
    fn the_build_identity_is_the_version_and_the_commit() {
        let identity = build_identity();

        let (version, commit) = identity.split_once('+').expect("`<version>+<commit>`");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        assert!(
            commit == "unknown"
                || (commit.len() == 12 && commit.bytes().all(|byte| byte.is_ascii_hexdigit())),
            "{commit}"
        );
    }
}
