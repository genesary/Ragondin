//! `ragondin ui`, exercised as a process.
//!
//! The subcommand's contract is a listener and what it answers, an exit
//! status and a message when it refuses — nothing a function inside the crate
//! could show. So each test here spawns the built binary: a refusal is read
//! from its exit status and stderr, and a running server is asked over a real
//! loopback connection, with a request written by hand (`support/ui.rs`)
//! rather than a client dependency.
//!
//! A build without the `ui` feature is the lean build: it declares the
//! subcommand and refuses it, which is the one test here that runs there.

#[cfg(feature = "ui")]
#[path = "support/ui.rs"]
mod ui;

use std::process::Output;

use assert_cmd::Command;

fn ragondin(args: &[&str]) -> Output {
    Command::cargo_bin("ragondin")
        .expect("the binary under test is built by `cargo test`")
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

#[cfg(not(feature = "ui"))]
#[test]
fn a_build_without_the_ui_feature_refuses_the_subcommand_naming_the_feature() {
    let output = ragondin(&["ui", "--workspace", env!("CARGO_TARGET_TMPDIR")]);

    assert!(!output.status.success(), "the lean build refuses `ui`");
    let report = stderr(&output);
    assert!(
        report.contains("this build does not carry the UI; rebuild with `--features ui`"),
        "{report}"
    );
    assert!(!report.contains("panicked"), "{report}");
}

#[cfg(feature = "ui")]
mod with_the_feature {
    use std::path::PathBuf;

    use super::ui::{self as http, Server};
    use super::*;

    /// A workspace of this test's own, under `CARGO_TARGET_TMPDIR`, emptied
    /// first so what a killed earlier run left cannot decide this one.
    fn workspace(test_name: &str) -> PathBuf {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("ui")
            .join(test_name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the workspace directory is created");
        root
    }

    #[test]
    fn a_bind_address_other_than_loopback_is_refused_before_any_listener_opens() {
        let workspace = workspace("non_loopback");
        // Port 1 is reserved and no test binds it: a refusal that came from a
        // failed bind rather than from the rule would name another reason.
        for address in [
            "0.0.0.0",
            "::",
            "192.168.1.10",
            "10.0.0.1",
            "localhost",
            "127.0.0.2",
        ] {
            let output = ragondin(&[
                "ui",
                "--workspace",
                workspace.to_str().expect("UTF-8 path"),
                "--port",
                "1",
                "--bind",
                address,
            ]);

            assert!(!output.status.success(), "`--bind {address}` is refused");
            let report = stderr(&output);
            assert!(report.contains(&format!("`--bind {address}`")), "{report}");
            assert!(report.contains("no authentication"), "{report}");
            assert!(report.contains("ssh -L 1:127.0.0.1:1"), "{report}");
            assert!(!report.contains("panicked"), "{report}");
        }
    }

    #[test]
    fn both_loopback_forms_are_accepted_and_the_bound_address_is_printed() {
        let workspace = workspace("loopback");
        for (bind, host) in [("127.0.0.1", "127.0.0.1"), ("::1", "[::1]")] {
            let server = Server::start(&workspace, &["--bind", bind]);

            assert!(
                server.url().starts_with(&format!("http://{host}:")),
                "{}",
                server.url()
            );
            let response = http::get(server.authority(), "/api/v1/runs");
            assert_eq!(response.status, 200, "{response:?}");
        }
    }

    #[test]
    fn a_workspace_that_is_not_a_directory_is_refused() {
        let workspace = workspace("not_a_directory").join("absent");

        let output = ragondin(&[
            "ui",
            "--workspace",
            workspace.to_str().expect("UTF-8 path"),
            "--port",
            "0",
        ]);

        assert!(!output.status.success());
        let report = stderr(&output);
        assert!(report.contains("is not a directory"), "{report}");
    }

    #[test]
    fn the_server_answers_the_ui_at_the_root_and_the_api_under_api_v1() {
        let server = Server::start(&workspace("serves"), &[]);

        let page = http::get(server.authority(), "/");
        assert_eq!(page.status, 200, "{page:?}");
        assert_eq!(
            page.header("content-type"),
            Some("text/html; charset=utf-8")
        );
        // The build script embeds the notice page when `ui/dist/` was absent
        // at compile time, and the real build otherwise: the test reads which
        // one this build took from the variable the build script sets.
        let notice = http::is_notice(&page.body);
        assert_eq!(
            notice,
            env!("RAGONDIN_UI_ASSETS_KIND") == "notice",
            "{}",
            page.body
        );

        // A client-side route answers the same page: the UI routes itself.
        let route = http::get(server.authority(), "/runs/some-run");
        assert_eq!(route.status, 200, "{route:?}");
        assert_eq!(route.body, page.body);

        let runs = http::get(server.authority(), "/api/v1/runs");
        assert_eq!(runs.status, 200, "{runs:?}");
        assert_eq!(runs.header("content-type"), Some("application/json"));
        assert_eq!(runs.body, r#"{"runs":[],"unreadable":[],"shapes":{}}"#);
    }

    /// The envelope `ragondin-api` applies reaches the page this binary
    /// embeds: the content security policy and the build identity on `/`, and
    /// a foreign `Host` refused there as on the API.
    #[test]
    fn the_embedded_page_is_served_inside_the_envelope() {
        let server = Server::start(&workspace("envelope"), &[]);

        let page = http::get(server.authority(), "/");
        assert_eq!(
            page.header("content-security-policy"),
            Some("default-src 'self'; frame-ancestors 'none'")
        );
        assert!(page.header("x-ragondin-build").is_some(), "{page:?}");

        let refused = http::get_as(server.authority(), "evil.example", "/");
        assert_eq!(refused.status, 421, "{refused:?}");
    }

    #[test]
    fn every_response_carries_the_build_identity_the_workspace_reports() {
        let server = Server::start(&workspace("build_identity"), &[]);

        let workspace = http::get(server.authority(), "/api/v1/workspace");
        assert_eq!(workspace.status, 200, "{workspace:?}");
        let body: serde_json::Value =
            serde_json::from_str(&workspace.body).expect("the workspace is JSON");
        let build = body["build"].as_str().expect("a build identity");
        assert!(
            build.starts_with(&format!("{}+", env!("CARGO_PKG_VERSION"))),
            "{build}"
        );
        assert_eq!(workspace.header("x-ragondin-build"), Some(build));
        assert_eq!(
            http::get(server.authority(), "/").header("x-ragondin-build"),
            Some(build)
        );
    }

    #[test]
    fn the_workspace_reports_the_capabilities_of_this_build() {
        let server = Server::start(&workspace("capabilities"), &[]);

        let workspace = http::get(server.authority(), "/api/v1/workspace");
        let body: serde_json::Value =
            serde_json::from_str(&workspace.body).expect("the workspace is JSON");
        let capabilities = &body["capabilities"];

        assert_eq!(capabilities["remote"], cfg!(feature = "remote"));
        let local = |family: &str| -> Vec<String> {
            capabilities["families"]
                .as_array()
                .expect("a list of families")
                .iter()
                .find(|entry| entry["family"] == family)
                .unwrap_or_else(|| panic!("`{family}` is listed: {capabilities}"))["parameters"]
                .as_array()
                .expect("a list of carried names")
                .iter()
                .map(|entry| entry["name"].as_str().expect("a name").to_owned())
                .collect()
        };
        // In every build: rank and string arithmetic, never gated.
        assert_eq!(local("fusion"), ["rrf"]);
        assert_eq!(local("context_builder"), ["concat"]);
        assert_eq!(
            local("retriever").contains(&"bm25".to_owned()),
            cfg!(feature = "bm25")
        );
        assert_eq!(
            local("retriever").contains(&"dense".to_owned()),
            cfg!(any(feature = "onnx", feature = "remote"))
        );
        assert_eq!(
            local("reranker").contains(&"cross_encoder".to_owned()),
            cfg!(feature = "onnx")
        );
        assert_eq!(
            local("embedder").contains(&"onnx".to_owned()),
            cfg!(feature = "onnx")
        );
        assert_eq!(
            local("generator").contains(&"stub_generator".to_owned()),
            cfg!(feature = "stub")
        );
    }

    /// Every directory a workspace holds, and its settings file.
    const LAYOUT: [&str; 7] = [
        "workspace.toml",
        "pipelines",
        "layouts",
        "runs",
        "jobs",
        "cache",
        "datasets",
    ];

    #[test]
    fn ui_with_no_argument_creates_the_home_workspace() {
        let root = workspace("no_argument_home");
        let (home, cwd) = (root.join("home"), root.join("empty"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();

        let server = Server::start_with(&[], Some(&cwd), Some(&home));

        let created = home.join(".ragondin");
        for entry in LAYOUT {
            assert!(
                created.join(entry).exists(),
                "{entry} under {}",
                created.display()
            );
        }
        assert!(
            server.banner().contains(created.to_str().unwrap()),
            "the resolved root is printed: {}",
            server.banner()
        );
        // Nothing was created where the command ran.
        assert_eq!(std::fs::read_dir(&cwd).unwrap().count(), 0);
        let runs = http::get(server.authority(), "/api/v1/runs");
        assert_eq!(runs.status, 200, "{runs:?}");
    }

    #[test]
    fn ui_with_no_argument_opens_a_local_runs_directory() {
        let root = workspace("no_argument_local");
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let local = root.join("project");
        std::fs::create_dir_all(local.join("runs")).unwrap();

        let server = Server::start_with(&[], Some(&local), Some(&home));

        for entry in LAYOUT {
            assert!(local.join(entry).exists(), "{entry}");
        }
        assert!(
            !home.join(".ragondin").exists(),
            "the home workspace is not created"
        );
        let body: serde_json::Value =
            serde_json::from_str(&http::get(server.authority(), "/api/v1/workspace").body).unwrap();
        let reported = std::path::PathBuf::from(body["path"].as_str().unwrap());
        assert_eq!(
            reported.canonicalize().unwrap(),
            local.canonicalize().unwrap()
        );
    }

    #[test]
    fn a_store_named_runs_has_its_parent_as_the_workspace() {
        let root = workspace("store_named_runs");

        let server = Server::start_with(
            &["--store", root.join("runs").to_str().unwrap()],
            None,
            None,
        );

        for entry in LAYOUT {
            assert!(root.join(entry).exists(), "{entry}");
        }
        let body: serde_json::Value =
            serde_json::from_str(&http::get(server.authority(), "/api/v1/workspace").body).unwrap();
        assert_eq!(body["path"], root.to_str().unwrap());
    }

    /// `bench --store <ws>/runs` writes a run, and `ui --store <ws>/runs`
    /// lists it: one argument, one store. The run needs a retriever and a
    /// generator in process, so this runs where `stub` and `bm25` are built.
    #[cfg(all(feature = "stub", feature = "bm25"))]
    #[test]
    fn ui_and_bench_resolve_the_same_store_from_the_same_argument() {
        let root = workspace("same_store");
        let store = root.join("runs");
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let bench = ragondin(&[
            "bench",
            fixtures
                .join("stub-generation-bench.yaml")
                .to_str()
                .unwrap(),
            "--benchmark",
            "beir-qa/qa-mini",
            "--datasets",
            fixtures.to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ]);
        assert!(bench.status.success(), "{}", stderr(&bench));
        let printed = String::from_utf8(bench.stdout).unwrap();
        let run_id = printed
            .split_whitespace()
            .find(|word| word.len() == 64 && word.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .unwrap_or_else(|| panic!("bench prints the run id: {printed}"))
            .to_owned();

        let server = Server::start_with(&["--store", store.to_str().unwrap()], None, None);

        let runs: serde_json::Value =
            serde_json::from_str(&http::get(server.authority(), "/api/v1/runs").body).unwrap();
        let ids: Vec<&str> = runs["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|run| run["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, [run_id.as_str()]);
    }

    #[test]
    fn a_malformed_workspace_toml_is_refused_naming_the_file_and_the_line() {
        let root = workspace("malformed_settings");
        let settings = root.join("workspace.toml");
        std::fs::write(&settings, "datasets = \"/data\"\n[jobs]\n").unwrap();

        let output = ragondin(&["ui", "--workspace", root.to_str().unwrap(), "--port", "0"]);

        assert!(!output.status.success());
        let report = stderr(&output);
        assert!(
            report.contains(&format!("{}:2", settings.display())),
            "{report}"
        );
        // Touched nothing: no directory created, the file as it was.
        let entries: Vec<_> = std::fs::read_dir(&root).unwrap().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            "datasets = \"/data\"\n[jobs]\n"
        );
    }

    /// What `ragondin ui --workspace <root>` printed on stderr by the time it
    /// was serving: started, its banner read off stdout, then stopped.
    fn stderr_until_serving(root: &std::path::Path) -> String {
        use std::io::{BufRead, BufReader, Read};
        let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("ragondin"))
            .args(["ui", "--port", "0", "--workspace", root.to_str().unwrap()])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the binary starts");
        let mut banner = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut banner)
            .expect("the server prints its address");
        assert!(banner.contains("http://"), "{banner}");
        let _ = child.kill();
        let _ = child.wait();
        let mut printed = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut printed)
            .unwrap();
        printed
    }

    #[test]
    fn a_new_workspace_is_announced_and_an_existing_one_is_not() {
        let root = workspace("announced");

        let first = stderr_until_serving(&root);
        assert!(
            first.contains(&format!("created a new workspace at {}", root.display())),
            "{first}"
        );

        let second = stderr_until_serving(&root);
        assert!(!second.contains("created a new workspace"), "{second}");
    }

    #[test]
    fn a_folder_holding_files_and_no_workspace_toml_is_refused_naming_its_workspace_subfolder() {
        // The trap of #492: the fixture's output directory named instead of
        // the workspace below it.
        let out = workspace("parent_of_a_workspace");
        std::fs::write(out.join("fixture.json"), "{}").unwrap();
        let inner = out.join("workspace");
        std::fs::create_dir_all(&inner).unwrap();
        drop(Server::start(&inner, &[]));

        let output = ragondin(&["ui", "--workspace", out.to_str().unwrap(), "--port", "0"]);

        assert!(!output.status.success());
        let report = stderr(&output);
        assert!(report.contains(out.to_str().unwrap()), "{report}");
        assert!(
            report.contains(&format!("did you mean {}?", inner.display())),
            "{report}"
        );
        assert!(!out.join("workspace.toml").exists(), "nothing created");
        assert!(!report.contains("panicked"), "{report}");
    }

    #[test]
    fn a_staging_directory_an_interrupted_download_left_is_swept_at_startup() {
        let root = workspace("sweep");
        let staging = root.join("datasets").join(".scifact.download-4242-1");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("corpus.jsonl"), "{}\n").unwrap();

        let _server = Server::start(&root, &[]);

        assert!(!staging.exists(), "the leftover is removed before serving");
        assert!(root.join("datasets").is_dir());
    }

    /// Every configuration under `tests/fixtures`, recursively.
    fn fixture_configurations() -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if path
                    .extension()
                    .is_some_and(|extension| extension == "yaml")
                {
                    found.push(path);
                }
            }
        }
        found.sort();
        assert!(found.len() > 10, "the fixtures are found: {found:?}");
        found
    }

    /// One document per refusal the load can make that no fixture under
    /// `tests/fixtures` makes: the version, the syntax, and every verdict of
    /// the validation pass. Written under `root`, beside the fixtures they
    /// complete.
    fn branch_documents(root: &std::path::Path) -> Vec<PathBuf> {
        let directory = root.join("branches");
        std::fs::create_dir_all(&directory).unwrap();
        let node = "    - id: legs\n      component: retriever\n      impl: bm25\n";
        [
            ("unsupported-version", "version: 99\npipeline:\n  inputs: [q]\n  nodes: []\n".to_owned()),
            ("syntax", "pipeline:\n  inputs: [q\n  nodes: []\n".to_owned()),
            ("wrong-type", "pipeline: [\n".to_owned()),
            ("missing-key", "pipeline:\n  inputs: [q]\n".to_owned()),
            ("no-input", format!("pipeline:\n  inputs: []\n  nodes:\n{node}")),
            ("two-inputs", format!("pipeline:\n  inputs: [a, b]\n  nodes:\n{node}      inputs: [a]\n")),
            ("duplicate-id", format!("pipeline:\n  inputs: [q]\n  nodes:\n{node}      inputs: [q]\n{node}      inputs: [q]\n")),
            ("dangling", format!("pipeline:\n  inputs: [q]\n  nodes:\n{node}      inputs: [nowhere]\n")),
            ("collides", format!("pipeline:\n  inputs: [legs]\n  nodes:\n{node}      inputs: [legs]\n")),
            ("cycle", format!("pipeline:\n  inputs: [q]\n  nodes:\n{node}      inputs: [legs]\n")),
            ("non-finite", format!("pipeline:\n  inputs: [q]\n  nodes:\n{node}      inputs: [q]\n      params: {{ top_k: .inf }}\n")),
        ]
        .into_iter()
        .map(|(name, text)| {
            let path = directory.join(format!("{name}.yaml"));
            std::fs::write(&path, text).unwrap();
            path
        })
        .collect()
    }

    /// The `detail` `POST /pipelines/validate` answers for a document, as
    /// derived from what `ragondin validate` prints on stderr for the same
    /// bytes in a file at `path`: the CLI's words, with the file the request
    /// does not have replaced, byte for byte.
    ///
    /// Every detail opens with the problem's own heading, `the pipeline does
    /// not validate: `. After it, the incompatible-wiring report is the CLI's
    /// own, with "the configuration" where the CLI names the file; every other
    /// refusal is the heading `ConfigError` renders, without the path and
    /// joined to its one cause by a colon.
    fn detail_from_the_cli_report(report: &str, path: &std::path::Path) -> String {
        let quoted = format!("`{}`", path.display());
        let report = report
            .strip_suffix('\n')
            .and_then(|report| report.strip_prefix("error: "))
            .unwrap_or_else(|| panic!("not one error report: {report:?}"));
        if let Some(rest) = report.strip_prefix(&format!("{quoted} wires two nodes incompatibly")) {
            return format!(
                "the pipeline does not validate: the configuration wires two nodes incompatibly{rest}"
            );
        }
        let (heading, cause) = report
            .split_once("\n  caused by: ")
            .unwrap_or_else(|| panic!("no cause: {report:?}"));
        assert!(!cause.contains("\n  caused by: "), "one cause: {report:?}");
        let heading = heading.replace(&format!(" {quoted}"), "");
        let prefix = match heading.as_str() {
            "could not parse configuration" => "could not parse configuration",
            "configuration is not a valid pipeline" => "configuration is not a valid pipeline",
            "configuration is written in a schema version this build cannot read" => {
                "the configuration is written in a schema version this build cannot read"
            }
            other => panic!("a heading the load does not render: {other:?}"),
        };
        format!("the pipeline does not validate: {prefix}: {cause}")
    }

    #[test]
    fn validate_answers_what_the_cli_prints_byte_for_byte_on_every_fixture_and_branch() {
        let root = workspace("validate_parity");
        let server = Server::start(&root, &[]);
        let mut refused = 0;

        let mut documents = fixture_configurations();
        documents.extend(branch_documents(&root));
        for path in documents {
            let text = std::fs::read_to_string(&path).unwrap();
            let cli = ragondin(&["validate", path.to_str().unwrap()]);
            let body = serde_json::json!({ "document": text }).to_string();
            let api = http::send_json(
                server.authority(),
                "POST",
                "/api/v1/pipelines/validate",
                &body,
            );
            let answer: serde_json::Value = serde_json::from_str(&api.body).unwrap();

            if cli.status.success() {
                let printed = String::from_utf8(cli.stdout).unwrap();
                let hash = printed
                    .lines()
                    .find_map(|line| line.strip_prefix("content hash: "))
                    .unwrap_or_else(|| panic!("{}: {printed}", path.display()));
                assert_eq!(api.status, 200, "{}: {}", path.display(), api.body);
                assert_eq!(answer["hash"], hash, "{}", path.display());
            } else {
                refused += 1;
                assert_eq!(api.status, 422, "{}: {}", path.display(), api.body);
                assert_eq!(answer["code"], "pipeline_invalid", "{}", path.display());
                let report = String::from_utf8(cli.stderr).unwrap();
                assert_eq!(
                    answer["detail"].as_str().unwrap(),
                    detail_from_the_cli_report(&report, &path),
                    "{}",
                    path.display()
                );
            }
        }
        // The fixtures' four refusals and the eleven branch documents.
        assert_eq!(refused, 15);
    }

    /// A binding hand-edited into `workspace.toml` that `--remote` would
    /// refuse — startup does not check bindings — blocks no save of a
    /// document that does not use it.
    #[test]
    fn a_bad_binding_no_document_node_uses_blocks_no_save() {
        let root = workspace("bad_binding");
        std::fs::write(
            root.join("workspace.toml"),
            "[services]\n\"store/q\" = \"ftp://h\"\n",
        )
        .unwrap();
        let server = Server::start(&root, &[]);
        let document = "pipeline:\n  inputs: [question]\n  nodes:\n    - id: lexical\n      \
                        component: retriever\n      impl: bm25\n      inputs: [question]\n      \
                        params: { top_k: 10 }\n";

        let answer = http::send_json_with(
            server.authority(),
            "PUT",
            "/api/v1/pipelines/lexical",
            &serde_json::json!({ "document": document }).to_string(),
            &[("If-None-Match", "*")],
        );

        assert_eq!(answer.status, 200, "{answer:?}");
        assert!(root.join("pipelines/lexical.yaml").is_file());
    }

    /// The composition root's key refusals, on `PUT` only: a key the
    /// component does not read is refused in `bench`'s words, a URL-valued
    /// key it reads is stored.
    #[test]
    fn a_write_is_refused_as_bench_refuses_its_keys_and_a_url_valued_read_key_is_stored() {
        let root = workspace("put_keys");
        let server = Server::start(&root, &[]);
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let accepted = std::fs::read_to_string(fixtures.join("url-valued-parameter.yaml")).unwrap();
        let refused = accepted.replace(
            "        query_prefix:",
            "        endpoint: \"http://10.0.0.5:50051\"\n        query_prefix:",
        );
        let put = |name: &str, text: &str| {
            http::send_json_with(
                server.authority(),
                "PUT",
                &format!("/api/v1/pipelines/{name}"),
                &serde_json::json!({ "document": text }).to_string(),
                &[("If-None-Match", "*")],
            )
        };

        let stored = put("url-valued", &accepted);
        assert_eq!(stored.status, 200, "{stored:?}");
        assert_eq!(
            std::fs::read_to_string(root.join("pipelines/url-valued.yaml")).unwrap(),
            accepted
        );

        let answer = put("stray-key", &refused);
        assert_eq!(answer.status, 422, "{answer:?}");
        let problem: serde_json::Value = serde_json::from_str(&answer.body).unwrap();
        assert_eq!(problem["code"], "pipeline_invalid");
        assert_eq!(problem["location"]["node"], "vectors");
        assert!(
            problem["detail"]
                .as_str()
                .unwrap()
                .contains("node `vectors`: `endpoint` is not a key"),
            "{problem}"
        );
        assert!(!root.join("pipelines/stray-key.yaml").exists());
    }

    #[test]
    fn validate_locates_a_mis_kinded_edge_in_the_cli_words() {
        let server = Server::start(&workspace("validate_words"), &[]);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/incompatible-wiring.yaml");
        let text = std::fs::read_to_string(&path).unwrap();

        let cli = stderr(&ragondin(&["validate", path.to_str().unwrap()]));
        let body = serde_json::json!({ "document": text }).to_string();
        let api = http::send_json(
            server.authority(),
            "POST",
            "/api/v1/pipelines/validate",
            &body,
        );

        let answer: serde_json::Value = serde_json::from_str(&api.body).unwrap();
        let detail = answer["detail"].as_str().unwrap();
        // The report's lines after its first, which names the file: the
        // edge, the kind expected, the kind found — the same in both.
        let report: Vec<&str> = cli
            .lines()
            .map(str::trim)
            .filter(|line| {
                ["edge:", "expected:", "found:"]
                    .iter()
                    .any(|key| line.starts_with(key))
            })
            .collect();
        assert_eq!(report.len(), 3, "{cli}");
        for line in report {
            assert!(detail.contains(line), "{line:?} in {detail}");
        }
        assert_eq!(answer["location"]["edge"]["from"], "legs");
        assert_eq!(answer["location"]["edge"]["to"], "ranked");
    }

    /// The release assertion (ADR-C36 § 5): the job that ships a binary sets
    /// `RAGONDIN_REQUIRE_UI_ASSETS`, and then this fails if what the binary
    /// serves at `/` is the notice page rather than a real build, or if it
    /// does not serve the third-party notices of the bundle it embeds.
    /// Without the variable it checks nothing, since a Rust-only build embeds
    /// the notice page by design.
    #[test]
    fn under_the_release_assertion_the_binary_serves_real_assets() {
        let server = Server::start(&workspace("release_assertion"), &[]);
        let required = std::env::var_os(http::REQUIRE_ASSETS).is_some();
        let page = http::get(server.authority(), "/");
        let notices = http::get(server.authority(), http::NOTICES_PATH);

        http::assert_shipped(required, &page.body).unwrap_or_else(|why| panic!("{why}"));
        http::assert_notices_shipped(required, &notices).unwrap_or_else(|why| panic!("{why}"));
    }

    /// A real build serves the third-party notices `npm run build` writes
    /// into `ui/dist/`, as plain text; the notice page, which bundles no
    /// third-party code, has none to serve.
    #[test]
    fn a_built_ui_serves_its_third_party_notices() {
        let server = Server::start(&workspace("notices"), &[]);
        let notices = http::get(server.authority(), http::NOTICES_PATH);

        if env!("RAGONDIN_UI_ASSETS_KIND") == "built" {
            // `just check` rebuilds `ui/dist/` first, but a bare `cargo test`
            // over a `dist/` built before the notices existed lands here.
            assert_eq!(
                notices.status, 200,
                "ui/dist predates the third-party notices; run `npm run build` in ui/ \
                 and rebuild the binary: {notices:?}"
            );
            assert_eq!(
                notices.header("content-type"),
                Some("text/plain; charset=utf-8")
            );
            assert!(
                notices.body.starts_with(http::NOTICES_HEADING),
                "{notices:?}"
            );
        } else {
            assert_eq!(notices.status, 404, "{notices:?}");
        }
    }

    /// The Rust crates' notices are the binary's own, not the bundle's: every
    /// `ui` build serves them, whether it embedded `ui/dist/` or the notice
    /// page, as the text `ragondin --notices` prints.
    #[test]
    fn every_ui_build_serves_the_rust_crates_notices() {
        let server = Server::start(&workspace("rust_notices"), &[]);
        let notices = http::get(server.authority(), "/third-party-notices-rust.txt");
        let committed = include_str!("../third-party-notices-rust.txt");

        assert_eq!(notices.status, 200, "{notices:?}");
        assert_eq!(
            notices.header("content-type"),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(notices.body, committed);
    }

    #[test]
    fn the_release_assertion_fails_on_the_notice_page_and_passes_on_a_real_build() {
        let notice = include_str!(concat!(env!("OUT_DIR"), "/ui-notice/index.html"));
        let real = "<!doctype html><html><head><title>ragondin</title></head>\
                    <body><div id=\"root\"></div></body></html>";

        assert!(http::assert_shipped(true, notice).is_err());
        assert!(http::assert_shipped(true, real).is_ok());
        assert!(http::assert_shipped(false, notice).is_ok());
    }

    #[test]
    fn the_release_assertion_fails_without_the_third_party_notices() {
        let served = http::Response::new(
            200,
            "text/plain; charset=utf-8",
            &format!("{}\n\nreact 19.3.0\n", http::NOTICES_HEADING),
        );
        let absent = http::Response::new(404, "application/problem+json", "{}");
        let wrong = http::Response::new(200, "text/plain; charset=utf-8", "something else");

        assert!(http::assert_notices_shipped(true, &served).is_ok());
        assert!(http::assert_notices_shipped(true, &absent).is_err());
        assert!(http::assert_notices_shipped(true, &wrong).is_err());
        assert!(http::assert_notices_shipped(false, &absent).is_ok());
    }
}
