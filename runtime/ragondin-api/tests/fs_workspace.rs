//! The workspace on disk: its layout, `workspace.toml`, and the
//! `WorkspaceSettings` backend over it (`src/fs/workspace.rs`,
//! `src/fs/settings.rs`).

use std::fs;
use std::path::{Path, PathBuf};

use ragondin_api::fs::{FsSettings, Workspace, WorkspaceError};
use ragondin_api::{ServiceBinding, Settings, WorkspaceSettings};

/// A directory of this test's own, emptied first.
fn scratch(test_name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("fs_workspace")
        .join(test_name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("the scratch directory is creatable");
    path
}

/// Every entry under `root`, relative, sorted: what a test compares to say
/// "nothing changed".
fn tree(root: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("readable") {
            let path = entry.expect("an entry").path();
            let relative = path
                .strip_prefix(root)
                .expect("under the root")
                .display()
                .to_string();
            if path.is_dir() {
                entries.push((relative, None));
                pending.push(path);
            } else {
                entries.push((relative, Some(fs::read(&path).expect("readable"))));
            }
        }
    }
    entries.sort();
    entries
}

#[test]
fn opening_a_root_without_the_workspace_directories_creates_them() {
    let root = scratch("creates");

    let workspace = Workspace::open(&root).expect("an empty directory is a workspace to be");

    for directory in ["pipelines", "layouts", "runs", "jobs", "cache", "datasets"] {
        assert!(root.join(directory).is_dir(), "{directory} is created");
    }
    assert!(root.join("workspace.toml").is_file());
    assert_eq!(workspace.root(), root);
    assert_eq!(workspace.settings_file(), root.join("workspace.toml"));
    assert_eq!(workspace.pipelines(), root.join("pipelines"));
    assert_eq!(workspace.layouts(), root.join("layouts"));
    assert_eq!(workspace.runs(), root.join("runs"));
    assert_eq!(workspace.jobs(), root.join("jobs"));
    assert_eq!(workspace.cache(), root.join("cache"));
    assert_eq!(workspace.default_datasets(), root.join("datasets"));

    // Opening it again changes nothing.
    let before = tree(&root);
    Workspace::open(&root).expect("an existing workspace opens");
    assert_eq!(tree(&root), before);
}

#[test]
fn a_workspace_may_keep_its_runs_in_a_store_named_elsewhere() {
    let root = scratch("store_elsewhere");
    let store = root.join("my-store");

    let workspace = Workspace::open_with_store(&root, &store).expect("opens");

    assert_eq!(workspace.runs(), store);
    assert!(store.is_dir());
}

#[test]
fn a_malformed_workspace_toml_is_reported_with_its_file_and_line_and_never_repaired() {
    let root = scratch("malformed");
    let text =
        "# my settings\ndatasets = \"/data\"\n\n[services]\n\"generator/qwen\" = http://host\n";
    fs::write(root.join("workspace.toml"), text).expect("written");
    let before = tree(&root);

    let error = Workspace::open(&root).expect_err("a malformed file is refused");

    match &error {
        WorkspaceError::Malformed { path, line, .. } => {
            assert_eq!(path, &root.join("workspace.toml"));
            assert_eq!(*line, 5);
        }
        other => panic!("{other:?}"),
    }
    let message = error.to_string();
    assert!(message.contains("workspace.toml:5"), "{message}");
    // Nothing created, nothing rewritten.
    assert_eq!(tree(&root), before);
}

#[test]
fn every_form_outside_the_settings_grammar_is_refused_naming_its_line() {
    for (text, line) in [
        ("datasets = 3\n", 1),
        ("[jobs]\n", 1),
        ("\nunknown = \"x\"\n", 2),
        ("datasets = \"a\"\ndatasets = \"b\"\n", 2),
        ("[services]\nqwen = \"http://host\"\n", 2),
        (
            "[services]\n\"generator/qwen\" = \"a\"\n\"generator/qwen\" = \"b\"\n",
            3,
        ),
        ("[services]\n[services]\n", 2),
        ("datasets = \"\"\"x\"\"\"\n", 1),
        ("datasets = \"unterminated\n", 1),
        ("datasets = \"a\" trailing\n", 1),
        // Not TOML, and so not read: a control character in a literal
        // string or in a comment, whitespace TOML does not count as such
        // (a no-break space here), and a `\u` escape that is not four hex
        // digits — `u32::from_str_radix` alone would take the `+`.
        ("datasets = 'a\u{1}b'\n", 1),
        ("# a comment \u{7f} with DEL\ndatasets = \"a\"\n", 1),
        ("datasets = \"a\" # \u{0}\n", 1),
        ("\u{a0}datasets = \"a\"\n", 1),
        ("datasets\u{a0}= \"a\"\n", 1),
        ("datasets = \"\\u+041\"\n", 1),
    ] {
        let root = scratch("grammar");
        fs::write(root.join("workspace.toml"), text).expect("written");

        match Workspace::open(&root) {
            Err(WorkspaceError::Malformed { line: found, .. }) => {
                assert_eq!(found, line, "{text:?}")
            }
            other => panic!("{text:?}: {other:?}"),
        }
    }
}

#[test]
fn a_byte_order_mark_is_refused_by_name() {
    let root = scratch("bom");
    fs::write(root.join("workspace.toml"), "\u{feff}datasets = \"a\"\n").expect("written");

    let error = Workspace::open(&root).expect_err("refused");

    assert!(error.to_string().contains("a byte-order mark"), "{error}");
}

#[tokio::test]
async fn settings_writes_are_atomic_and_a_service_round_trips_through_workspace_toml() {
    let root = scratch("round_trip");
    let workspace = Workspace::open(&root).expect("opens");
    let settings = FsSettings::new(&workspace);

    assert_eq!(
        settings.read().await.expect("reads"),
        Settings {
            datasets: root.join("datasets"),
            services: Vec::new(),
        }
    );

    let qwen = ServiceBinding {
        family: "generator".to_owned(),
        name: "qwen".to_owned(),
        uri: "http://[::1]:8080".to_owned(),
    };
    let written = Settings {
        datasets: PathBuf::from("/data/benchmarks"),
        services: vec![qwen.clone()],
    };
    settings.write(written.clone()).await.expect("writes");

    assert_eq!(settings.read().await.expect("reads"), written);
    // A second backend over the same file reads the same thing: it is on disk.
    assert_eq!(
        FsSettings::new(&workspace).read().await.expect("reads"),
        written
    );
    let text = fs::read_to_string(root.join("workspace.toml")).expect("readable");
    assert!(
        text.contains("\"generator/qwen\" = \"http://[::1]:8080\""),
        "{text}"
    );
    // Written beside and renamed over: nothing is left beside the file.
    let leftovers: Vec<String> = fs::read_dir(&root)
        .expect("readable")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.contains("workspace.toml") && name != "workspace.toml")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    // And the file reopens.
    Workspace::open(&root).expect("the written file is read back at startup");
}

#[tokio::test]
async fn a_relative_datasets_directory_is_read_against_the_root() {
    let root = scratch("relative_datasets");
    fs::write(
        root.join("workspace.toml"),
        "datasets = 'benchmarks' # mine\n",
    )
    .expect("written");
    let workspace = Workspace::open(&root).expect("opens");

    let read = FsSettings::new(&workspace).read().await.expect("reads");

    assert_eq!(read.datasets, root.join("benchmarks"));
}

#[tokio::test]
async fn escapes_survive_the_round_trip() {
    let root = scratch("escapes");
    let workspace = Workspace::open(&root).expect("opens");
    let settings = FsSettings::new(&workspace);
    let written = Settings {
        datasets: PathBuf::from("/data/a \"quoted\" \\ dir\tx"),
        services: Vec::new(),
    };

    settings.write(written.clone()).await.expect("writes");

    assert_eq!(settings.read().await.expect("reads"), written);
}
