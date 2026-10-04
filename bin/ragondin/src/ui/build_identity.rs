//! The git half of the build identity: the commit and whether the tree
//! differs from it, by the commands `build-identity.rule` names — the file
//! `ui/scripts/build-identity.mjs` reads for the UI's bundle, so both builds
//! apply one rule. `build.rs` compiles this file as its own module, and the
//! crate compiles it only for its tests, which pin what the build script runs.

use std::path::Path;
use std::process::Command;

/// The rule, embedded when this file is compiled: `build.rs` is recompiled,
/// and so rerun, when it changes.
const RULE: &str = include_str!("../../build-identity.rule");

/// The rule's commands, each as its arguments: the commit's, then the dirty
/// check's.
fn rule() -> (Vec<&'static str>, Vec<&'static str>) {
    let mut commands = RULE
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split_whitespace().collect::<Vec<_>>());
    let (Some(commit), Some(dirty), None) = (commands.next(), commands.next(), commands.next())
    else {
        panic!("build-identity.rule holds two commands: the commit's, then the dirty check's");
    };
    (commit, dirty)
}

/// The commit and whether the tree differs from it, or `None` outside a git
/// checkout.
pub fn state(dir: &Path) -> Option<(String, bool)> {
    let (commit, dirty) = rule();
    let sha = git(dir, &commit)?;
    Some((sha, git(dir, &dirty).is_some()))
}

/// One git command's trimmed stdout, or `None` when git is absent or fails,
/// or prints nothing.
pub fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    /// git in `dir`, isolated from the machine's configuration.
    fn run(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}: {output:?}");
        String::from_utf8(output.stdout)
            .expect("UTF-8")
            .trim()
            .to_owned()
    }

    /// A checkout of this test's own with one committed file, and its commit.
    fn checkout(test_name: &str) -> (PathBuf, String) {
        let root = std::env::temp_dir().join(format!(
            "ragondin-build-identity-{}-{test_name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp is writable");
        fs::write(root.join("main.rs"), "fn main() {}\n").expect("temp is writable");
        run(&root, &["init", "-q"]);
        run(&root, &["add", "."]);
        run(&root, &["commit", "-q", "-m", "fixture"]);
        let head = run(&root, &["rev-parse", "--short=12", "HEAD"]);
        (root, head)
    }

    #[test]
    fn a_checkout_with_nothing_changed_is_clean() {
        let (root, head) = checkout("clean");
        assert_eq!(state(&root), Some((head, false)));
    }

    #[test]
    fn an_untracked_file_no_build_reads_leaves_the_tree_clean() {
        let (root, head) = checkout("untracked");
        fs::write(root.join(".DS_Store"), "Finder").expect("temp is writable");
        assert_eq!(state(&root), Some((head, false)));
    }

    #[test]
    fn a_modified_tracked_file_makes_the_tree_dirty() {
        let (root, head) = checkout("modified");
        fs::write(root.join("main.rs"), "fn main() { todo!() }\n").expect("temp is writable");
        assert_eq!(state(&root), Some((head, true)));
    }

    #[test]
    fn a_staged_new_file_makes_the_tree_dirty() {
        let (root, head) = checkout("staged");
        fs::write(root.join("new.rs"), "").expect("temp is writable");
        run(&root, &["add", "new.rs"]);
        assert_eq!(state(&root), Some((head, true)));
    }

    #[test]
    fn outside_a_checkout_there_is_no_commit() {
        let root = std::env::temp_dir().join(format!(
            "ragondin-build-identity-{}-none",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp is writable");
        assert_eq!(state(&root), None);
    }
}
