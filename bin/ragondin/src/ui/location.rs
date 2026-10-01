//! Where `ragondin ui`'s workspace is, and why: from `--workspace`, from
//! `--store`, or from nothing (the design document § 3 and § 6).
//!
//! - `--workspace <dir>` names the workspace; its store is `<dir>/runs`.
//! - `--store <dir>` names the store, as `bench --store` does, so the two
//!   commands given one argument use one store. The workspace is the store's
//!   parent when the store is called `runs` — `--store <ws>/runs` opens `<ws>`
//!   — and the store's own directory otherwise.
//! - Nothing: `./runs` when it is a directory, the workspace then being the
//!   current directory; otherwise `~/.ragondin`, created with its `runs/`, so
//!   the first launch is a screen and not an error.
//!
//! A workspace named by an argument must already be a directory: a typo is
//! refused, never created. Only the home workspace is created whole.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use ragondin_api::fs::Workspace;

/// The store's name inside a workspace.
const RUNS: &str = "runs";
/// The home workspace's directory under `$HOME`.
const HOME_WORKSPACE: &str = ".ragondin";

/// A resolved workspace, not yet opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    root: PathBuf,
    store: PathBuf,
    reason: Reason,
}

/// Which rule chose the workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reason {
    Workspace,
    StoreNamedRuns,
    StoreElsewhere,
    LocalRuns,
    Home,
}

/// Resolves the workspace from the arguments, the current directory and
/// `$HOME`. A relative argument is read against `cwd`.
pub fn resolve(
    workspace: Option<&Path>,
    store: Option<&Path>,
    cwd: &Path,
    home: Option<&Path>,
) -> Result<Location> {
    let location = match (workspace, store) {
        (Some(_), Some(_)) => {
            bail!("`--workspace` and `--store` name the same thing twice; give one")
        }
        (Some(workspace), None) => {
            let root = cwd.join(workspace);
            Location {
                store: root.join(RUNS),
                root,
                reason: Reason::Workspace,
            }
        }
        (None, Some(store)) => {
            let store = cwd.join(store);
            match (store.file_name(), store.parent()) {
                (Some(name), Some(parent)) if name == RUNS => Location {
                    root: parent.to_path_buf(),
                    store,
                    reason: Reason::StoreNamedRuns,
                },
                _ => Location {
                    root: store.clone(),
                    store,
                    reason: Reason::StoreElsewhere,
                },
            }
        }
        (None, None) if cwd.join(RUNS).is_dir() => Location {
            root: cwd.to_path_buf(),
            store: cwd.join(RUNS),
            reason: Reason::LocalRuns,
        },
        (None, None) => {
            let Some(home) = home else {
                bail!(
                    "no `./runs` here and `HOME` is not set, so there is no home workspace; name \
                     one with `--workspace <dir>`"
                );
            };
            let root = home.join(HOME_WORKSPACE);
            Location {
                store: root.join(RUNS),
                root,
                reason: Reason::Home,
            }
        }
    };
    if location.reason != Reason::Home && !location.root.is_dir() {
        bail!(
            "the workspace `{}` is not a directory",
            location.root.display()
        );
    }
    Ok(location)
}

impl Location {
    /// Opens the workspace: the home one is created first, then
    /// [`Workspace::open_with_store`] reads `workspace.toml` and creates what
    /// is missing — or refuses a malformed file, naming it and its line,
    /// having created nothing.
    pub fn open(&self) -> Result<Workspace> {
        if self.reason == Reason::Home {
            std::fs::create_dir_all(&self.root).map_err(|error| {
                anyhow::anyhow!("cannot create {}: {error}", self.root.display())
            })?;
        }
        Ok(Workspace::open_with_store(&self.root, &self.store)?)
    }

    /// Why this workspace, in the words the startup line prints.
    pub fn reason(&self) -> &'static str {
        match self.reason {
            Reason::Workspace => "named by --workspace",
            Reason::StoreNamedRuns => "the parent of the store --store names",
            Reason::StoreElsewhere => "the store --store names",
            Reason::LocalRuns => "./runs is a directory here",
            Reason::Home => "no ./runs here: the home workspace",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test_name: &str) -> PathBuf {
        let path = std::env::temp_dir()
            .join(format!("ragondin-location-{}", std::process::id()))
            .join(test_name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("creatable");
        path
    }

    #[test]
    fn a_store_named_runs_opens_its_parent_and_any_other_opens_itself() {
        let root = scratch("store");
        std::fs::create_dir_all(root.join("elsewhere")).unwrap();

        let named = resolve(None, Some(&root.join("runs")), &root, None).unwrap();
        assert_eq!(
            (named.root.as_path(), named.store.as_path()),
            (root.as_path(), root.join("runs").as_path())
        );

        let other = resolve(None, Some(Path::new("elsewhere")), &root, None).unwrap();
        assert_eq!(other.root, root.join("elsewhere"));
        assert_eq!(other.store, root.join("elsewhere"));
    }

    #[test]
    fn nothing_given_opens_a_local_runs_directory_or_the_home_workspace() {
        let root = scratch("nothing");
        let home = root.join("home");

        let away = resolve(None, None, &root, Some(&home)).unwrap();
        assert_eq!(away.root, home.join(".ragondin"));
        assert_eq!(away.store, home.join(".ragondin/runs"));

        std::fs::create_dir_all(root.join("runs")).unwrap();
        let here = resolve(None, None, &root, Some(&home)).unwrap();
        assert_eq!(here.root, root);
        assert_eq!(here.store, root.join("runs"));
    }

    #[test]
    fn a_named_workspace_that_is_not_a_directory_is_refused_and_both_arguments_are_one_too_many() {
        let root = scratch("refusals");

        let absent = resolve(Some(Path::new("absent")), None, &root, None).unwrap_err();
        assert!(
            absent.to_string().contains("is not a directory"),
            "{absent}"
        );
        let both = resolve(Some(&root), Some(&root), &root, None).unwrap_err();
        assert!(both.to_string().contains("give one"), "{both}");
        let homeless = resolve(None, None, &root, None).unwrap_err();
        assert!(homeless.to_string().contains("HOME"), "{homeless}");
    }
}
