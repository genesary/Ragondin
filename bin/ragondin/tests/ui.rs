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
        assert_eq!(runs.body, r#"{"runs":[],"unreadable":[]}"#);
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
                .unwrap_or_else(|| panic!("`{family}` is listed: {capabilities}"))["local"]
                .as_array()
                .expect("a list of names")
                .iter()
                .map(|name| name.as_str().expect("a name").to_owned())
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

    /// The release assertion (ADR-C36 § 5): the job that ships a binary sets
    /// `RAGONDIN_REQUIRE_UI_ASSETS`, and then this fails if what the binary
    /// serves at `/` is the notice page rather than a real build. Without the
    /// variable it checks nothing, since a Rust-only build embeds the notice
    /// by design.
    #[test]
    fn under_the_release_assertion_the_binary_serves_real_assets() {
        let server = Server::start(&workspace("release_assertion"), &[]);
        let page = http::get(server.authority(), "/");

        http::assert_shipped(std::env::var_os(http::REQUIRE_ASSETS).is_some(), &page.body)
            .unwrap_or_else(|why| panic!("{why}"));
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
}
