//! `ragondin ui`: the JSON API and the UI's embedded assets on a loopback
//! listener (ADR-C36 § 1).
//!
//! The handler is the composition root's, and thin: it checks the address,
//! resolves and opens the workspace, constructs the backends `ragondin-api`
//! consumes, binds the listener, hands the backends in with the embedded
//! assets as data, and hands what comes back to `ragondin_api::serve`. The
//! router, its endpoints, the routes that serve the assets and the four
//! security layers are all `ragondin-api`'s, applied last over the API and the
//! assets alike; this crate names no HTTP stack at all, so it cannot add a
//! route outside them.
//!
//! - [`address`] — the loopback rule, and its refusal.
//! - [`assets`] — the embedded UI, or the notice page, as `ragondin-api`'s
//!   asset table.
//! - [`launcher`] — `Launcher`: capabilities, a binding's and a document's
//!   check, the identity probe, and a run's identity and execution over
//!   `bench`'s path.
//! - [`location`] — where the workspace is, from `--workspace`, `--store` or
//!   nothing.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use ragondin_api::fs::{FsPipelines, FsRegistry, FsSettings};
use ragondin_api::{Backends, ServerConfig, WorkspaceSettings};
use ragondin_experiments::FileSystemRunStore;

mod address;
mod assets;
// `build.rs` runs it; the crate compiles it only to test what that runs.
#[cfg(test)]
mod build_identity;
mod launcher;
mod location;

/// One `ragondin ui`'s arguments.
pub struct Request<'a> {
    /// `--workspace`, when given.
    pub workspace: Option<&'a Path>,
    /// `--store`, when given.
    pub store: Option<&'a Path>,
    /// `--port`, when given.
    pub port: Option<u16>,
    /// `--bind`, when given.
    pub bind: Option<&'a str>,
}

/// Runs the server until the process is stopped.
///
/// The address is refused before anything opens. Then the workspace is
/// resolved ([`location::resolve`]) and opened — `workspace.toml` read
/// first, a malformed one refused before anything is created — and the
/// staging directories an interrupted download or import left under the
/// datasets directory are swept, before the listener opens and so before
/// any download can start. Once bound, one line is printed on stdout: the
/// workspace, its store, and the URL — the port the system chose, under
/// `--port 0` — whose authority is exactly the one the `Host` check accepts.
pub async fn run(request: &Request<'_>) -> Result<()> {
    let address = address::listen_address(request.bind, request.port)?;
    let location = location::resolve(
        request.workspace,
        request.store,
        &std::env::current_dir().context("the current directory cannot be read")?,
        std::env::var_os("HOME").as_deref().map(Path::new),
    )?;
    let workspace = location.open()?;
    // On stderr: stdout's first line is the banner a reader takes the
    // address from.
    if workspace.created() {
        eprintln!(
            "ragondin ui: created a new workspace at {}",
            workspace.root().display()
        );
    }

    let settings = FsSettings::new(&workspace);
    let datasets = settings
        .read()
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .datasets;
    // The launcher reads a submission's benchmark where the registry keeps
    // it, so a benchmark downloaded from the UI is the one a run evaluates.
    let launcher = launcher::BinaryLauncher::new(&datasets);
    let registry = FsRegistry::new(datasets, ragondin_benchmarks::manifest::manifest());
    registry
        .sweep_staging()
        .map_err(|error| anyhow::anyhow!("{error}"))?;

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("cannot listen on {address}"))?;
    let served = listener
        .local_addr()
        .context("the listener has no local address")?
        .to_string();

    let backends = Backends {
        runs: Arc::new(FileSystemRunStore::new(workspace.runs())),
        pipelines: Arc::new(FsPipelines::new(&workspace)),
        registry: Arc::new(registry),
        settings: Arc::new(settings),
        launcher: Arc::new(launcher),
    };
    let server = ragondin_api::router(
        backends,
        ServerConfig {
            served: served.clone(),
            build: build_identity(),
            workspace: workspace.root().to_path_buf(),
        },
        Arc::new(assets::Embedded),
    );

    println!(
        "ragondin ui: serving the workspace {} ({}; runs in {}) at http://{served}/",
        workspace.root().display(),
        location.reason(),
        workspace.runs().display(),
    );
    ragondin_api::serve(listener, server)
        .await
        .context("the server stopped")
}

/// The build's identity: the crate version, then the commit `build.rs` read,
/// with `-dirty` when a tracked file was modified or a change staged as it
/// ran, by the rule in `build-identity.rule` the UI's build reads too — e.g.
/// `0.0.0+3f9a1c2b7d4e` or `0.0.0+3f9a1c2b7d4e-dirty` — or `0.0.0+unknown`
/// outside a git checkout. The UI compares it with the one it loaded under
/// and reloads when they differ (ADR-C36 § 1); a commit changes with any
/// committed change to the API or to the UI it embeds, which a version alone
/// would not, and `-dirty` says when it cannot be trusted to.
fn build_identity() -> String {
    identity(
        env!("CARGO_PKG_VERSION"),
        env!("RAGONDIN_BUILD_COMMIT"),
        env!("RAGONDIN_BUILD_DIRTY") == "true",
    )
}

/// `<version>+<commit>`, with `-dirty` appended when `dirty`.
fn identity(version: &str, commit: &str, dirty: bool) -> String {
    match dirty {
        true => format!("{version}+{commit}-dirty"),
        false => format!("{version}+{commit}"),
    }
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
                || (commit.len() == 12 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()))
                || commit
                    .strip_suffix("-dirty")
                    .is_some_and(|sha| sha.len() == 12),
            "{commit}"
        );
    }

    #[test]
    fn a_tree_with_uncommitted_changes_is_marked_dirty() {
        assert_eq!(
            identity("1.2.3", "3f9a1c2b7d4e", false),
            "1.2.3+3f9a1c2b7d4e"
        );
        assert_eq!(
            identity("1.2.3", "3f9a1c2b7d4e", true),
            "1.2.3+3f9a1c2b7d4e-dirty"
        );
        // Outside a checkout there is no tree to call dirty.
        assert_eq!(identity("1.2.3", "unknown", false), "1.2.3+unknown");
    }
}
