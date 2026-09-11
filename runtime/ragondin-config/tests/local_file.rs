//! The `LocalFile` configuration source, over real files on disk.
//!
//! These are integration tests rather than unit tests because the subject is
//! I/O: what `LocalFile` must get right is a path that does not exist, a file
//! whose text does not parse, and a file that parses into a graph validation
//! refuses — and each of those is a different diagnosis for the person who
//! wrote the file.
//!
//! The valid fixture is checked in under `tests/fixtures/`. The invalid ones
//! are written to a temporary directory: a file the whole point of which is to
//! be broken does not belong in the tree, where a future reader would take it
//! for an example.

use std::fs;
use std::path::{Path, PathBuf};

use ragondin_config::{ConfigError, ConfigSource, LocalFile};
use ragondin_pipeline::NodeId;

/// The checked-in fixture: §5.1's hybrid retrieval pipeline, trimmed to the
/// node families this build has a variant for.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hybrid-retrieval.yaml")
}

/// Writes `text` to a uniquely named file under the target directory and hands
/// back its path. No `tempfile` dependency for four test files: the name
/// carries the test's own name, so two tests cannot collide.
fn scratch(name: &str, text: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("local_file");
    fs::create_dir_all(&dir).expect("the test scratch directory must be creatable");
    let path = dir.join(format!("{name}.yaml"));
    fs::write(&path, text).expect("the fixture must be writable");
    path
}

#[test]
fn a_valid_configuration_file_loads_as_a_logical_pipeline() {
    let pipeline = LocalFile::new(fixture())
        .load()
        .expect("the checked-in fixture must load");

    // The graph's signature (ADR-C18), then the nodes in canonical order —
    // `validate` sorts them by id, which is not the order the file lists them
    // in. That the two differ is what makes this assertion say something.
    assert_eq!(pipeline.inputs(), &[NodeId::new("question")]);
    let ids: Vec<&str> = pipeline.nodes().iter().map(|n| n.id().as_str()).collect();
    assert_eq!(ids, vec!["dense", "fuse", "sparse"]);
}

#[test]
fn formatting_is_not_configuration() {
    // §8.3: the YAML run locally *is* the custom resource. Two spellings of
    // one configuration must reach one `LogicalPipeline` — the canonical value
    // that INV-8's content hash is taken over, so this is the property that
    // load path exists to deliver to `ragondin validate` (#30) and to the run
    // cache (#29).
    //
    // Stated as equality of the canonical value rather than of its digest:
    // `LogicalPipeline::content_hash` belongs to another branch, and a test
    // here that reached for it would make this crate's work wait on that one.
    let original = LocalFile::new(fixture()).load().expect("the fixture loads");

    let text = fs::read_to_string(fixture()).expect("the fixture must be readable");
    let stripped: String = text
        .lines()
        .map(|line| match line.find('#') {
            // Crude, and sound for this fixture: no `#` of its own appears
            // inside a value here.
            Some(at) => line[..at].trim_end(),
            None => line.trim_end(),
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !stripped.contains('#'),
        "the comments must have been removed"
    );
    assert_ne!(
        stripped, text,
        "the copy must actually differ from the file"
    );

    let reformatted = LocalFile::new(scratch("reformatted", &stripped))
        .load()
        .expect("stripping comments must not break the configuration");
    assert_eq!(
        original, reformatted,
        "comments and blank lines are formatting, not configuration"
    );
}

#[test]
fn a_path_that_does_not_exist_is_a_typed_error_naming_it() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-configuration.yaml");
    let error = LocalFile::new(&path)
        .load()
        .expect_err("a missing file must not load");

    assert!(
        matches!(error, ConfigError::Unreadable { .. }),
        "expected Unreadable, got {error:?}"
    );
    assert!(
        error.to_string().contains("no-such-configuration.yaml"),
        "the message must name the path the user gave: {error}"
    );
}

#[test]
fn text_that_does_not_parse_is_a_parse_error_not_a_validation_error() {
    // The #8 half of the load path: this never reaches `validate`, so
    // reporting it as invalid would send the reader hunting for a graph fault
    // in a file that is not YAML.
    let path = scratch(
        "unparseable",
        "pipeline:\n  inputs: [question\n  nodes: - - -\n",
    );
    let error = LocalFile::new(&path)
        .load()
        .expect_err("malformed text must not load");

    assert!(
        matches!(error, ConfigError::Malformed { .. }),
        "expected Malformed, got {error:?}"
    );
    assert!(
        error.to_string().contains("unparseable.yaml"),
        "the message must name the file: {error}"
    );
}

#[test]
fn a_parameter_shape_outside_the_grammar_is_a_parse_error() {
    // ADR-C22 keeps the parameter grammar flat, and a nested map is refused at
    // the *raw* level — so it is a parse fault, not a validation one. Pinned
    // because it is the one place the two halves are easy to confuse: the file
    // is well-formed YAML and the graph is well-formed, and it still fails
    // before `validate` ever runs.
    let path = scratch(
        "nested-param",
        "pipeline:\n  inputs: [question]\n  nodes:\n    - id: dense\n      component: retriever\n      impl: qdrant_dense\n      inputs: [question]\n      params: { filters: { lang: fr } }\n",
    );
    let error = LocalFile::new(&path)
        .load()
        .expect_err("a nested parameter must not load");

    assert!(
        matches!(error, ConfigError::Malformed { .. }),
        "expected Malformed, got {error:?}"
    );
}

#[test]
fn a_graph_the_pass_refuses_is_a_validation_error_not_a_parse_error() {
    // The #9 half: well-formed YAML, well-formed wire schema, and a graph
    // `validate` refuses — here a node consuming an id that names neither a
    // node nor a declared input.
    let path = scratch(
        "dangling",
        "pipeline:\n  inputs: [question]\n  nodes:\n    - id: fuse\n      component: fusion\n      impl: rrf\n      inputs: [absent]\n",
    );
    let error = LocalFile::new(&path)
        .load()
        .expect_err("a dangling input must not load");

    let ConfigError::Invalid { source, .. } = &error else {
        panic!("expected Invalid, got {error:?}");
    };
    assert!(
        source.to_string().contains("absent"),
        "the validation error must survive intact, naming the offending id: {source}"
    );
    assert!(
        error.to_string().contains("dangling.yaml"),
        "and the wrapper must name the file: {error}"
    );
}

#[test]
fn an_unreadable_schema_version_is_reported_as_its_own_diagnosis() {
    // `ragondin-pipeline` refuses an unsupported version through
    // `serde::de::Error::custom`, which erases the type — so a plain parse
    // would report "this build is too old" as a syntax error. `LocalFile`
    // peeks first precisely so the one fault a user cannot fix by editing the
    // file is not disguised as one they can.
    let path = scratch(
        "from-the-future",
        "version: 7\npipeline:\n  inputs: [question]\n  nodes: []\n",
    );
    let error = LocalFile::new(&path)
        .load()
        .expect_err("a future schema version must not load");

    let ConfigError::UnsupportedSchemaVersion { source, .. } = &error else {
        panic!("expected UnsupportedSchemaVersion, got {error:?}");
    };
    assert_eq!(source.found(), 7);
    assert!(
        error.to_string().contains("from-the-future.yaml"),
        "the message must name the file: {error}"
    );
}

#[test]
fn an_unparseable_file_that_also_states_a_version_reports_the_parse_fault() {
    // The peek walks the whole document, so a syntax error anywhere makes the
    // version verdict untrustworthy. `ragondin-pipeline` states what a caller
    // should then do — fall through to the full parse, which fails too and
    // with the better-located message — and this pins that `LocalFile` does,
    // rather than reporting the peek's own vaguer complaint.
    let path = scratch(
        "future-and-broken",
        "version: 7\npipeline:\n  inputs: [question\n  nodes: - - -\n",
    );
    let error = LocalFile::new(&path)
        .load()
        .expect_err("malformed text must not load");

    assert!(
        matches!(error, ConfigError::Malformed { .. }),
        "a syntax error outranks the version verdict, got {error:?}"
    );
}

#[test]
fn a_config_source_is_usable_behind_a_trait_object() {
    // §8.2: "the data plane does not know who configures it." The binary
    // (#30/#31) holds whichever source it was given, so the trait has to be
    // dyn-compatible — a signature that was not would make the abstraction
    // decorative.
    let source: Box<dyn ConfigSource> = Box::new(LocalFile::new(fixture()));
    let pipeline = source.load().expect("the fixture must load through dyn");
    assert_eq!(pipeline.nodes().len(), 3);
}

#[test]
fn the_error_type_exposes_the_underlying_cause_as_a_source() {
    // `anyhow` in the binary (#30) prints an error chain; a wrapper that
    // swallowed its cause would make the file's actual fault invisible there.
    use std::error::Error;

    let path = scratch(
        "sourced",
        "pipeline:\n  inputs: [question\n  nodes: - - -\n",
    );
    let error = LocalFile::new(&path).load().unwrap_err();
    assert!(
        error.source().is_some(),
        "the parse fault must remain reachable as a source: {error}"
    );
}
