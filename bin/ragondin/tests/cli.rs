//! The command line, exercised as a process.
//!
//! `validate`'s contract is not a function's return value: it is an **exit
//! status** and what lands on **stdout** and **stderr**. A caller pipes the
//! hash into a script, reads the failure report, and branches on `$?`; none of
//! that is observable from inside the crate, so every test here spawns the
//! built binary through `assert_cmd`.

use std::path::PathBuf;
use std::process::Output;

use assert_cmd::Command;
use ragondin_config::{ConfigSource, LocalFile};

/// A checked-in fixture, by file name.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Runs the binary and returns the whole result — status, stdout and stderr —
/// rather than asserting on one of them, because most tests here read two.
fn ragondin(args: &[&str]) -> Output {
    Command::cargo_bin("ragondin")
        .expect("the binary under test is built by `cargo test`")
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

#[tokio::test]
async fn validate_prints_the_content_hash_of_a_valid_configuration_and_exits_zero() {
    // The expected hash is computed the way any other consumer would compute
    // it, so this pins that the binary prints *the* content hash of the file
    // it was given and not merely something hex-shaped.
    let expected = LocalFile::new(fixture("stub-pipeline.yaml"))
        .load()
        .await
        .expect("the checked-in fixture is a valid configuration")
        .content_hash()
        .to_string();

    let output = ragondin(&[
        "validate",
        fixture("stub-pipeline.yaml").to_str().expect("UTF-8 path"),
    ]);

    assert!(
        output.status.success(),
        "a valid configuration exits 0, got {:?}: {}",
        output.status.code(),
        stderr(&output)
    );
    assert!(
        stdout(&output).contains(&expected),
        "stdout must carry the content hash `{expected}`, got: {}",
        stdout(&output)
    );
}

#[test]
fn validate_reports_an_incompatible_wiring_with_the_edge_and_both_kinds() {
    let output = ragondin(&[
        "validate",
        fixture("incompatible-wiring.yaml")
            .to_str()
            .expect("UTF-8 path"),
    ]);

    assert!(
        !output.status.success(),
        "an incompatible wiring exits non-zero"
    );

    // The three things the report owes a reader, per ADR-C16: which edge, what
    // that port expects, and what actually arrives there. A reader who has
    // only "invalid pipeline" has to re-derive all three from the file.
    let report = stderr(&output);
    for fragment in [
        "`legs` feeds `ranked` at port 0",
        "expected: query",
        "found: chunks",
    ] {
        assert!(
            report.contains(fragment),
            "the report must carry `{fragment}`, got:\n{report}"
        );
    }
}

#[test]
fn validate_refuses_a_malformed_configuration_readably_rather_than_panicking() {
    let path = fixture("malformed.yaml");
    let output = ragondin(&["validate", path.to_str().expect("UTF-8 path")]);

    assert!(
        !output.status.success(),
        "a malformed configuration exits non-zero"
    );

    let report = stderr(&output);
    assert!(
        !report.contains("panicked"),
        "a bad file is a diagnosis, never a crash, got:\n{report}"
    );
    assert!(
        report.contains("malformed.yaml"),
        "the report must name the file it is about, got:\n{report}"
    );
    // The deserializer's own message, kept under the diagnosis: it carries the
    // line and column, which nothing written in this crate could reconstruct.
    assert!(
        report.contains("caused by:"),
        "the report must keep the parser's own message, got:\n{report}"
    );
}

#[test]
fn validate_refuses_a_file_that_is_not_there_readably_rather_than_panicking() {
    let output = ragondin(&["validate", "/definitely/not/here.yaml"]);

    assert!(!output.status.success(), "a missing file exits non-zero");

    let report = stderr(&output);
    assert!(!report.contains("panicked"), "got:\n{report}");
    assert!(
        report.contains("/definitely/not/here.yaml"),
        "the report must name the file it is about, got:\n{report}"
    );
}

#[test]
fn every_subcommand_of_the_documented_surface_is_declared() {
    // `docs/code-architecture.md` §4.2 lists four, and ADR-C15 makes one binary
    // carrying all four the entire user-facing surface. A subcommand missing
    // from the help is missing from that surface.
    let output = ragondin(&["--help"]);
    let help = stdout(&output);

    assert!(
        output.status.success(),
        "`--help` exits 0: {}",
        stderr(&output)
    );
    for subcommand in ["bench", "compare", "serve", "validate"] {
        assert!(
            help.contains(subcommand),
            "`{subcommand}` must appear in the help, got:\n{help}"
        );
    }
}

#[test]
fn the_help_for_validate_says_extension_nodes_are_not_kind_checked() {
    // The command checks the kind of every edge except one, and help that did
    // not say so would promise coverage the validation pass does not give
    // (ADR-C16: an `extension` node's ports are unknown to the core).
    let output = ragondin(&["validate", "--help"]);
    let help = stdout(&output);

    assert!(output.status.success(), "`validate --help` exits 0");
    assert!(
        help.contains("extension") && help.contains("not kind-checked"),
        "the help must state the exception, got:\n{help}"
    );
}

#[test]
fn serve_reports_that_it_is_not_available_in_this_version() {
    // Declared so the surface is whole (ADR-C15), and refused because the
    // serving driver is not built yet. Silence or a zero exit would read as a
    // server that started.
    let output = ragondin(&["serve", "anything.yaml"]);

    assert!(!output.status.success(), "a refusal exits non-zero");
    assert!(
        stderr(&output).contains("not available in v0"),
        "got:\n{}",
        stderr(&output)
    );
}

#[test]
fn a_subcommand_that_is_not_declared_is_refused_by_the_parser() {
    let output = ragondin(&["evaluate", "anything.yaml"]);

    assert!(!output.status.success());
    assert!(!stderr(&output).contains("panicked"));
}
