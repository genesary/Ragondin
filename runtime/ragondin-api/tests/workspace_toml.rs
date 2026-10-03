//! `workspace.toml` edited in place (ADR-C38): what each per-key operation of
//! `FsSettings` writes, byte for byte, on a file a person has commented — and
//! when it writes nothing at all.

use std::fs;
use std::path::{Path, PathBuf};

use ragondin_api::fs::{FsSettings, Workspace, WorkspaceError};
use ragondin_api::{ApiError, ServiceBinding, Settings, WorkspaceSettings};

/// A directory of this test's own, emptied first.
fn scratch(test_name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("workspace_toml")
        .join(test_name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("the scratch directory is creatable");
    path
}

/// A workspace whose `workspace.toml` is `text`, and its settings backend.
fn workspace_holding(test_name: &str, text: &str) -> (PathBuf, FsSettings) {
    let root = scratch(test_name);
    fs::write(root.join("workspace.toml"), text).expect("written");
    let workspace = Workspace::open(&root).expect("the file is within the schema");
    (root, FsSettings::new(&workspace))
}

/// A fresh workspace: `workspace.toml` as `Workspace::open` creates it.
fn fresh_workspace(test_name: &str) -> (PathBuf, FsSettings) {
    let root = scratch(test_name);
    let workspace = Workspace::open(&root).expect("opens");
    (root, FsSettings::new(&workspace))
}

fn text(root: &Path) -> String {
    fs::read_to_string(root.join("workspace.toml")).expect("readable")
}

fn bytes(root: &Path) -> Vec<u8> {
    fs::read(root.join("workspace.toml")).expect("readable")
}

fn binding(family: &str, name: &str, uri: &str) -> ServiceBinding {
    ServiceBinding {
        family: family.to_owned(),
        name: name.to_owned(),
        uri: uri.to_owned(),
    }
}

/// The commented fixture: a header, a comment above, beside and below each
/// entry, blank lines, and a commented-out binding at the end.
const COMMENTED: &str = "\
# My workspace.
#
# Shared with the team.

datasets = \"benchmarks\"   # the team's copy

# Bindings below.
[services] # one per line
# The big model.
\"generator/qwen\" = \"http://gpu:8080\" # on the GPU box

# The embedder.
\"embedder/bge\" = 'http://cpu:9090'
# commented out: \"reranker/old\" = \"http://old\"
";

/// `empty()`, as `main` wrote it before ADR-C38: committed, so that an edit to
/// the header is seen.
const EMPTY: &str = include_str!("fixtures/empty-workspace.toml");

// --- The header a fresh workspace starts with ---------------------------------

#[test]
fn a_fresh_workspace_toml_is_the_committed_empty_file() {
    let (root, _) = fresh_workspace("fresh");

    assert_eq!(text(&root), EMPTY);
}

// --- Comments ------------------------------------------------------------------

#[tokio::test]
async fn adding_a_binding_appends_it_and_keeps_every_comment() {
    let (root, settings) = workspace_holding("add", COMMENTED);

    settings
        .bind(binding("reranker", "new", "http://new"))
        .await
        .expect("binds");

    assert_eq!(
        text(&root),
        format!("{COMMENTED}\"reranker/new\" = \"http://new\"\n")
    );
}

#[tokio::test]
async fn replacing_an_address_keeps_its_trailing_comment_and_every_other_comment() {
    let (root, settings) = workspace_holding("replace", COMMENTED);

    settings
        .bind(binding("generator", "qwen", "http://other:1"))
        .await
        .expect("binds");

    assert_eq!(
        text(&root),
        COMMENTED.replace("\"http://gpu:8080\"", "\"http://other:1\"")
    );
}

#[tokio::test]
async fn removing_a_binding_keeps_the_comments_above_and_beside_it_on_what_follows() {
    let (root, settings) = workspace_holding("remove", COMMENTED);

    settings
        .unbind("generator", "qwen")
        .await
        .expect("unbinds")
        .expect("it was bound");

    assert_eq!(
        text(&root),
        COMMENTED.replace(
            "\"generator/qwen\" = \"http://gpu:8080\" # on the GPU box\n",
            "# on the GPU box\n"
        )
    );
}

// --- The header ---------------------------------------------------------------

#[tokio::test]
async fn the_header_survives_removing_datasets() {
    let (root, settings) = workspace_holding("header_datasets", COMMENTED);

    settings.set_datasets(None).await.expect("clears");

    assert_eq!(
        text(&root),
        COMMENTED.replace(
            "datasets = \"benchmarks\"   # the team's copy\n",
            "# the team's copy\n"
        )
    );
}

#[tokio::test]
async fn the_first_put_on_a_fresh_workspace_keeps_the_header_above_services() {
    let (root, settings) = fresh_workspace("header_fresh");

    settings
        .bind(binding("generator", "qwen", "http://127.0.0.1:8080"))
        .await
        .expect("binds");

    assert_eq!(
        text(&root),
        format!("{EMPTY}\n[services]\n\"generator/qwen\" = \"http://127.0.0.1:8080\"\n")
    );
}

#[tokio::test]
async fn setting_datasets_on_a_fresh_workspace_keeps_the_header_above_it() {
    let (root, settings) = fresh_workspace("header_fresh_datasets");

    settings
        .set_datasets(Some(PathBuf::from("/data/benchmarks")))
        .await
        .expect("sets");

    assert_eq!(
        text(&root),
        format!("{EMPTY}\ndatasets = \"/data/benchmarks\"\n")
    );
}

#[tokio::test]
async fn setting_datasets_before_an_existing_services_table_keeps_the_header_on_top() {
    let (root, settings) = fresh_workspace("header_datasets_after_services");
    settings
        .bind(binding("generator", "qwen", "http://h"))
        .await
        .expect("binds");

    settings
        .set_datasets(Some(PathBuf::from("/data")))
        .await
        .expect("sets");

    assert_eq!(
        text(&root),
        format!("{EMPTY}\ndatasets = \"/data\"\n\n[services]\n\"generator/qwen\" = \"http://h\"\n")
    );
}

// --- Order --------------------------------------------------------------------

#[tokio::test]
async fn key_order_and_blank_lines_are_kept() {
    let original = "\
[services]
\"b/second\" = \"http://2\"


\"a/first\" = \"http://1\"

\"c/third\" = \"http://3\"
";
    let (root, settings) = workspace_holding("order", original);

    settings
        .bind(binding("a", "first", "http://one"))
        .await
        .expect("binds");
    settings
        .bind(binding("a", "fourth", "http://4"))
        .await
        .expect("binds");

    assert_eq!(
        text(&root),
        format!(
            "{}\"a/fourth\" = \"http://4\"\n",
            original.replace("http://1", "http://one")
        )
    );
    let names: Vec<String> = settings
        .read()
        .await
        .expect("reads")
        .services
        .into_iter()
        .map(|binding| binding.name)
        .collect();
    assert_eq!(names, ["second", "first", "third", "fourth"]);
}

// --- No-op --------------------------------------------------------------------

#[tokio::test]
async fn binding_a_name_to_the_address_it_has_writes_nothing_even_on_a_crlf_file() {
    let original = "# mine\r\n[services]\r\n\"generator/qwen\" = 'http://h' # here\r\n";
    let (root, settings) = workspace_holding("noop_bind_crlf", original);

    let after = settings
        .bind(binding("generator", "qwen", "http://h"))
        .await
        .expect("binds");

    assert_eq!(bytes(&root), original.as_bytes());
    assert_eq!(after.services, [binding("generator", "qwen", "http://h")]);
}

#[tokio::test]
async fn setting_datasets_to_the_path_it_resolves_to_writes_nothing_even_with_a_byte_order_mark() {
    let original = "\u{feff}datasets = 'benchmarks' # spelled relative\n";
    let (root, settings) = workspace_holding("noop_datasets_bom", original);

    // The same directory, spelled absolute and with a trailing separator.
    settings
        .set_datasets(Some(root.join("benchmarks/")))
        .await
        .expect("sets");
    // And spelled relative, as the file does.
    settings
        .set_datasets(Some(PathBuf::from("benchmarks")))
        .await
        .expect("sets");

    assert_eq!(bytes(&root), original.as_bytes());
}

#[tokio::test]
async fn unbinding_a_name_that_is_not_bound_writes_nothing() {
    let original = "# mine\r\n[services]\r\n\"generator/qwen\" = \"http://h\"\r\n";
    let (root, settings) = workspace_holding("noop_unbind", original);

    let after = settings.unbind("generator", "other").await.expect("reads");

    assert_eq!(after, None);
    assert_eq!(bytes(&root), original.as_bytes());
}

#[tokio::test]
async fn a_changing_write_turns_crlf_into_lf_and_drops_a_byte_order_mark() {
    let original = "\u{feff}# mine\r\n[services]\r\n\"generator/qwen\" = \"http://h\"\r\n";
    let (root, settings) = workspace_holding("crlf_changed", original);

    settings
        .bind(binding("generator", "qwen", "http://other"))
        .await
        .expect("binds");

    assert_eq!(
        text(&root),
        "# mine\n[services]\n\"generator/qwen\" = \"http://other\"\n"
    );
}

// --- `datasets` untouched by a service write ----------------------------------

#[tokio::test]
async fn a_service_write_leaves_the_datasets_line_byte_identical_whatever_its_spelling() {
    for (index, line) in [
        "datasets = \"benchmarks\"",
        "datasets='/data/absolute'   # absolute",
        "datasets = 'C:\\data\\literal'",
        "datasets = \"\"\"/data/multi\"\"\"",
    ]
    .into_iter()
    .enumerate()
    {
        let original = format!("{line}\n\n[services]\n\"generator/qwen\" = \"http://h\"\n");
        let (root, settings) = workspace_holding(&format!("datasets_untouched_{index}"), &original);

        settings
            .bind(binding("embedder", "bge", "http://e"))
            .await
            .expect("binds");
        settings
            .unbind("generator", "qwen")
            .await
            .expect("unbinds")
            .expect("it was bound");

        let written = text(&root);
        assert_eq!(written.lines().next(), Some(line), "{written}");
    }
}

// --- Refusals name their line ---------------------------------------------------

/// Opens a workspace over `text` and returns the refusal, after checking that
/// it is reported as `file:line: message` and that `FsSettings::read` reports
/// the same line.
fn refused(test_name: &str, text: &str) -> (usize, String) {
    let root = scratch(test_name);
    let file = root.join("workspace.toml");
    fs::write(&file, text).expect("written");
    let error = Workspace::open(&root).expect_err("refused");
    let WorkspaceError::Malformed { path, line, reason } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(path, &file);
    assert_eq!(
        error.to_string(),
        format!("{}:{line}: {reason}", file.display())
    );
    (*line, reason.clone())
}

macro_rules! refusal {
    ($name:ident, $text:expr, $line:expr, $words:expr) => {
        #[test]
        fn $name() {
            let (line, reason) = refused(stringify!($name), $text);
            assert_eq!(line, $line, "{reason}");
            assert!(reason.contains($words), "{reason}");
        }
    };
}

refusal!(
    another_key_is_refused,
    "datasets = \"d\"\n\nunknown = \"x\"\n",
    3,
    "`unknown`"
);
refusal!(
    another_table_is_refused,
    "# a\n[services]\n\n[jobs]\n",
    4,
    "`[jobs]`"
);
refusal!(
    a_sub_table_is_refused,
    "[services]\n\"a/b\" = \"x\"\n[services.extra]\n",
    3,
    "sub-table"
);
refusal!(
    an_array_of_tables_is_refused,
    "\n[[services]]\n",
    2,
    "array of tables"
);
refusal!(
    an_inline_table_is_refused,
    "[services]\n\"generator/qwen\" = { uri = \"http://h\" }\n",
    2,
    "inline table"
);
refusal!(
    an_inline_services_table_is_refused,
    "services = { \"generator/qwen\" = \"http://h\" }\n",
    1,
    "inline table"
);
refusal!(
    a_dotted_key_is_refused,
    "[services]\n\"a/b\" = \"x\"\n\"generator/qwen\".uri = \"http://h\"\n",
    3,
    "dotted key"
);
refusal!(
    a_dotted_services_key_at_the_top_is_refused,
    "services.\"generator/qwen\" = \"http://h\"\n",
    1,
    "dotted key"
);
refusal!(
    a_value_that_is_not_a_string_is_refused,
    "# a\ndatasets = 3\n",
    2,
    "not a string"
);
refusal!(
    a_service_address_that_is_not_a_string_is_refused,
    "[services]\n\"generator/qwen\" = [\"http://h\"]\n",
    2,
    "not a string"
);
refusal!(
    a_service_key_without_a_slash_is_refused,
    "[services]\n\n\"qwen\" = \"http://h\"\n",
    3,
    "`qwen`"
);
refusal!(
    a_service_key_with_an_empty_family_is_refused,
    "[services]\n\"/qwen\" = \"http://h\"\n",
    2,
    "empty family"
);
refusal!(
    a_service_key_with_an_empty_name_is_refused,
    "[services]\n\"generator/\" = \"http://h\"\n",
    2,
    "empty name"
);
refusal!(
    a_duplicate_key_is_refused,
    "[services]\n\"generator/qwen\" = \"a\"\n'generator/qwen' = \"b\"\n",
    3,
    "duplicate key"
);
refusal!(
    a_parse_error_is_refused,
    "# mine\ndatasets = \"/data\"\n\n[services]\n\"generator/qwen\" = http://host\n",
    5,
    ""
);

#[tokio::test]
async fn a_read_reports_a_refusal_as_file_line_message() {
    let (root, settings) = fresh_workspace("read_refusal");
    let file = root.join("workspace.toml");
    // Edited by hand after the workspace opened.
    fs::write(&file, "[services]\nqwen = \"http://h\"\n").expect("written");

    let error = settings.read().await.expect_err("refused");

    let ApiError::BackendFailed { detail } = error else {
        panic!("{error:?}");
    };
    assert!(
        detail.starts_with(&format!("{}:2: ", file.display())),
        "{detail}"
    );
}

#[tokio::test]
async fn an_operation_on_a_refused_file_writes_nothing() {
    let (root, settings) = fresh_workspace("operation_refusal");
    let original = "[services]\nqwen = \"http://h\"\n";
    fs::write(root.join("workspace.toml"), original).expect("written");

    settings
        .bind(binding("generator", "qwen", "http://h"))
        .await
        .expect_err("refused");

    assert_eq!(text(&root), original);
}

// --- By meaning -----------------------------------------------------------------

#[tokio::test]
async fn every_string_form_is_read_alike() {
    for (index, (datasets, uri)) in [
        ("\"/data/x\"", "\"http://h\""),
        ("'/data/x'", "'http://h'"),
        ("\"\"\"/data/x\"\"\"", "\"\"\"http://h\"\"\""),
        ("'''/data/x'''", "'''http://h'''"),
        ("\"\"\"\n/data/x\"\"\"", "'''\nhttp://h'''"),
        ("\"/data/\\u0078\"", "\"http://\\x68\""),
    ]
    .into_iter()
    .enumerate()
    {
        let text = format!("datasets = {datasets}\n[services]\n'generator/qwen' = {uri}\n");
        let (_, settings) = workspace_holding(&format!("forms_{index}"), &text);

        assert_eq!(
            settings.read().await.expect("reads"),
            Settings {
                datasets: PathBuf::from("/data/x"),
                services: vec![binding("generator", "qwen", "http://h")],
            },
            "{text}"
        );
    }
}

#[tokio::test]
async fn what_toml_edit_writes_for_a_value_needing_another_form_is_read_back() {
    let (root, settings) = fresh_workspace("own_output");
    let quoted = "http://h/\"quoted\"";
    let multiline = "http://h/first\nsecond";

    settings
        .bind(binding("generator", "quoted", quoted))
        .await
        .expect("binds");
    settings
        .bind(binding("generator", "lines", multiline))
        .await
        .expect("binds");

    let written = text(&root);
    assert!(written.contains("'http://h/\"quoted\"'"), "{written}");
    assert!(written.contains("\"\"\""), "{written}");
    let reopened = FsSettings::new(&Workspace::open(&root).expect("its own output opens"));
    assert_eq!(
        reopened.read().await.expect("reads").services,
        [
            binding("generator", "quoted", quoted),
            binding("generator", "lines", multiline),
        ]
    );
}

// --- Round-trip ------------------------------------------------------------------

/// Strings that need escaping, or another form, in TOML.
const AWKWARD: [&str; 14] = [
    "",
    "plain",
    "\"double\"",
    "'single'",
    "back\\slash",
    "trailing\\",
    "new\nline",
    "carriage\r\nreturn",
    "tab\there",
    "control\u{1}\u{7f}",
    "triple \"\"\" double",
    "triple ''' single",
    "both ' and \" and \\ and \n",
    "unicode é ✓",
];

#[tokio::test]
async fn what_is_written_is_read_back_for_any_value() {
    let (root, settings) = fresh_workspace("round_trip");
    let mut expected = Vec::new();

    for (index, value) in AWKWARD.into_iter().enumerate() {
        let name = format!("n{index}{value}");
        let datasets = PathBuf::from(format!("/data/{value}"));
        settings
            .bind(binding("generator", &name, value))
            .await
            .expect("binds");
        settings
            .set_datasets(Some(datasets.clone()))
            .await
            .expect("sets");
        expected.push(binding("generator", &name, value));

        let read = FsSettings::new(&Workspace::open(&root).expect("reopens"))
            .read()
            .await
            .expect("reads");
        assert_eq!(read.datasets, datasets, "{value:?}");
        assert_eq!(read.services, expected, "{value:?}");
    }
}

// --- The race ----------------------------------------------------------------------

#[tokio::test]
async fn a_binding_added_by_hand_between_two_api_writes_survives_the_second() {
    let (root, settings) = fresh_workspace("race");
    settings
        .bind(binding("generator", "qwen", "http://q"))
        .await
        .expect("binds");

    // A person edits the file while the server runs.
    let by_hand = format!(
        "{}# added by hand\n\"embedder/bge\" = \"http://b\"\n",
        text(&root)
    );
    fs::write(root.join("workspace.toml"), &by_hand).expect("written");

    settings
        .bind(binding("reranker", "mini", "http://m"))
        .await
        .expect("binds");

    assert_eq!(
        text(&root),
        format!("{by_hand}\"reranker/mini\" = \"http://m\"\n")
    );
    assert_eq!(
        settings.read().await.expect("reads").services,
        [
            binding("generator", "qwen", "http://q"),
            binding("embedder", "bge", "http://b"),
            binding("reranker", "mini", "http://m"),
        ]
    );
}

// --- The choices ADR-C38 leaves to the implementation --------------------------------

#[tokio::test]
async fn the_first_key_of_services_carries_its_comments_onto_what_follows_as_datasets_does() {
    let original = "\
datasets = \"d\"

[services]
# About the first binding.
\"generator/qwen\" = \"http://q\" # beside
\"embedder/bge\" = \"http://b\"
";
    let (root, settings) = workspace_holding("first_of_services", original);

    settings
        .unbind("generator", "qwen")
        .await
        .expect("unbinds")
        .expect("it was bound");

    assert_eq!(
        text(&root),
        "\
datasets = \"d\"

[services]
# About the first binding.
# beside
\"embedder/bge\" = \"http://b\"
"
    );
}

#[tokio::test]
async fn a_removed_key_that_nothing_follows_leaves_its_comments_at_the_end() {
    let original = "\
[services]
\"generator/qwen\" = \"http://q\"

# About the last one.
\"embedder/bge\" = \"http://b\" # beside
";
    let (root, settings) = workspace_holding("nothing_follows", original);

    settings
        .unbind("embedder", "bge")
        .await
        .expect("unbinds")
        .expect("it was bound");

    assert_eq!(
        text(&root),
        "\
[services]
\"generator/qwen\" = \"http://q\"

# About the last one.
# beside
"
    );
}

#[tokio::test]
async fn the_last_unbind_keeps_an_empty_services_table_and_its_comments() {
    let original = "\
# Header.

# The bindings.
[services] # on this machine
\"generator/qwen\" = \"http://q\"
";
    let (root, settings) = workspace_holding("empty_services", original);

    let after = settings
        .unbind("generator", "qwen")
        .await
        .expect("unbinds")
        .expect("it was bound");

    assert!(after.services.is_empty());
    assert_eq!(
        text(&root),
        "\
# Header.

# The bindings.
[services] # on this machine
"
    );
}

#[tokio::test]
async fn clearing_datasets_that_states_the_default_writes_nothing() {
    let original = "datasets = \"datasets\" # the default, stated\n";
    let (root, settings) = workspace_holding("clear_default", original);

    let after = settings.set_datasets(None).await.expect("clears");

    assert_eq!(after.datasets, root.join("datasets"));
    assert_eq!(text(&root), original);
}

#[tokio::test]
async fn clearing_datasets_removes_the_key_and_the_default_applies() {
    let (root, settings) = workspace_holding("clear", "datasets = \"/data\"\n");

    let after = settings.set_datasets(None).await.expect("clears");

    assert_eq!(after.datasets, root.join("datasets"));
    assert_eq!(text(&root), "");
}

#[tokio::test]
async fn a_service_key_splits_at_its_first_slash() {
    let (root, settings) = workspace_holding("split", "[services]\n\"a//b\" = \"http://h\"\n");

    assert_eq!(
        settings.read().await.expect("reads").services,
        [binding("a", "/b", "http://h")]
    );

    settings
        .bind(binding("g", "/n/m", "http://n"))
        .await
        .expect("binds");

    assert!(text(&root).contains("\"g//n/m\" = \"http://n\""));
    assert_eq!(
        settings.read().await.expect("reads").services,
        [
            binding("a", "/b", "http://h"),
            binding("g", "/n/m", "http://n")
        ]
    );
}

#[tokio::test]
async fn a_binding_the_file_could_not_read_back_is_refused_and_not_written() {
    let (root, settings) = fresh_workspace("unwritable_binding");
    let before = text(&root);

    for (family, name) in [("a/b", "c"), ("", "c"), ("a", "")] {
        let error = settings
            .bind(binding(family, name, "http://h"))
            .await
            .expect_err("refused");
        assert!(
            matches!(error, ApiError::BindingRefused { .. }),
            "{family}/{name}: {error:?}"
        );
    }
    assert_eq!(text(&root), before);
}

#[tokio::test]
async fn setting_datasets_to_the_default_states_it() {
    let (root, settings) = workspace_holding("set_default", "datasets = \"/data\" # mine\n");

    let after = settings
        .set_datasets(Some(root.join("datasets")))
        .await
        .expect("sets");

    assert_eq!(after.datasets, root.join("datasets"));
    assert_eq!(text(&root), "datasets = \"datasets\" # mine\n");
}

#[tokio::test]
async fn datasets_paths_are_compared_lexically_after_joining_to_the_root() {
    let (root, settings) = workspace_holding("lexical", "datasets = 'benchmarks'\n");

    // `.` and a trailing separator are not a change: nothing is written, so
    // the literal string is not respelled.
    settings
        .set_datasets(Some(root.join("./benchmarks/")))
        .await
        .expect("sets");
    assert_eq!(text(&root), "datasets = 'benchmarks'\n");

    // `..` is not resolved, and nothing is canonicalised: this is a change,
    // written relative since it is under the root.
    let after = settings
        .set_datasets(Some(root.join("other/../benchmarks")))
        .await
        .expect("sets");
    assert_eq!(after.datasets, root.join("other/../benchmarks"));
    assert_eq!(text(&root), "datasets = \"other/../benchmarks\"\n");

    // A directory outside the root is written absolute.
    settings
        .set_datasets(Some(PathBuf::from("/elsewhere")))
        .await
        .expect("sets");
    assert_eq!(text(&root), "datasets = \"/elsewhere\"\n");
}
