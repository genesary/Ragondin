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
//!
//! Every test is a `#[tokio::test]` because [`ConfigSource::load`] is async —
//! `tokio` is a dev-dependency of this crate and not a dependency, which is
//! the split that leaves the runtime selected at the binary level.

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
/// back its path. No `tempfile` dependency for this: each call names its own
/// file after the test that makes it, so two tests cannot collide.
fn scratch(name: &str, text: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("local_file");
    fs::create_dir_all(&dir).expect("the test scratch directory must be creatable");
    let path = dir.join(format!("{name}.yaml"));
    fs::write(&path, text).expect("the fixture must be writable");
    path
}

#[tokio::test]
async fn a_valid_configuration_file_loads_as_a_logical_pipeline() {
    let pipeline = LocalFile::new(fixture())
        .load()
        .await
        .expect("the checked-in fixture must load");

    // The graph's signature (ADR-C18), then the nodes in canonical order —
    // `validate` sorts them by id, which is not the order the file lists them
    // in. That the two differ is what makes this assertion say something.
    assert_eq!(pipeline.inputs(), &[NodeId::new("question")]);
    let ids: Vec<&str> = pipeline.nodes().iter().map(|n| n.id().as_str()).collect();
    assert_eq!(ids, vec!["dense", "fuse", "sparse"]);
}

#[tokio::test]
async fn formatting_is_not_configuration() {
    // §8.3: the YAML run locally *is* the custom resource. Two spellings of
    // one configuration must reach one `LogicalPipeline` — the canonical value
    // INV-8's content hash is taken over, so this is the property the load
    // path exists to deliver to `ragondin validate` (#30) and to run
    // identity (#29), which folds the pipeline hash into a run's id.
    //
    // The perturbations are chosen to reach past the YAML lexer. Stripping
    // comments alone would only prove that a parser discards comments, which
    // is `serde_yaml`'s property and not this crate's. Node order is the one
    // thing `validate` actually normalizes, and flow versus block style is the
    // one that changes the text without changing the tree — so the two
    // together exercise both halves of "formatted differently".
    //
    // Stated as equality of the canonical value rather than of its digest:
    // `LogicalPipeline::content_hash` belongs to another branch, and a test
    // here that reached for it would make this crate's work wait on that one.
    let original = LocalFile::new(fixture())
        .load()
        .await
        .expect("the fixture loads");

    let restyled = "\
pipeline:
    nodes:
        -   id: fuse
            component: fusion
            impl: rrf
            inputs:
                - dense
                - sparse
            params:
                k: 60.0
        -   id: sparse
            component: retriever
            impl: bm25
            inputs:
                - question
            params:
                top_k: 50
        -   id: dense
            component: retriever
            impl: qdrant_dense
            inputs:
                - question
            params:
                top_k: 50
    inputs:
        - question
";

    let source = fs::read_to_string(fixture()).expect("the fixture must be readable");
    // Both orders read out of the two texts and compared. An earlier version
    // of this guard inspected only `restyled`, so reordering the fixture to
    // match it made the perturbation evaporate with nothing firing.
    let ids_in = |text: &str| -> Vec<String> {
        text.lines()
            .filter_map(|line| line.split("id:").nth(1))
            .map(|id| id.trim().to_string())
            .collect()
    };
    let (source_ids, restyled_ids) = (ids_in(&source), ids_in(restyled));
    assert_eq!(
        source_ids.len(),
        3,
        "the fixture must still declare three nodes, got {source_ids:?}"
    );
    assert_ne!(
        source_ids, restyled_ids,
        "the copy must list its nodes in a different order from the fixture"
    );
    let (mut a, mut b) = (source_ids.clone(), restyled_ids.clone());
    a.sort();
    b.sort();
    assert_eq!(
        a, b,
        "and must list the same nodes, or it is a different graph"
    );
    assert!(
        source.contains("params: { top_k: 50 }"),
        "the fixture must still use flow style, or the restyling proves nothing"
    );

    let reformatted = LocalFile::new(scratch("reformatted", restyled))
        .load()
        .await
        .expect("reformatting must not break the configuration");
    assert_eq!(
        original, reformatted,
        "node order, YAML style and comments are formatting, not configuration"
    );
}

#[tokio::test]
async fn a_path_that_does_not_exist_is_a_typed_error_naming_it() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-configuration.yaml");
    let error = LocalFile::new(&path)
        .load()
        .await
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

#[tokio::test]
async fn text_that_does_not_parse_is_a_parse_error_not_a_validation_error() {
    // The wire-schema half of the load path: this never reaches `validate`, so
    // reporting it as invalid would send the reader hunting for a graph fault
    // in a file that is not YAML.
    let path = scratch(
        "unparseable",
        "pipeline:\n  inputs: [question\n  nodes: - - -\n",
    );
    let error = LocalFile::new(&path)
        .load()
        .await
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

#[tokio::test]
async fn a_parameter_shape_outside_the_grammar_is_a_parse_error() {
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
        .await
        .expect_err("a nested parameter must not load");

    assert!(
        matches!(error, ConfigError::Malformed { .. }),
        "expected Malformed, got {error:?}"
    );
}

#[tokio::test]
async fn a_graph_the_pass_refuses_is_a_validation_error_not_a_parse_error() {
    // The `validate` half: well-formed YAML, well-formed wire schema, and a graph
    // `validate` refuses — here a node consuming an id that names neither a
    // node nor a declared input.
    let path = scratch(
        "dangling",
        "pipeline:\n  inputs: [question]\n  nodes:\n    - id: fuse\n      component: fusion\n      impl: rrf\n      inputs: [absent]\n",
    );
    let error = LocalFile::new(&path)
        .load()
        .await
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

#[tokio::test]
async fn an_unsupported_schema_version_is_reported_as_its_own_diagnosis() {
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
        .await
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

#[tokio::test]
async fn an_unreadable_peek_falls_through_to_the_full_parse() {
    // The peek walks the whole document, so a syntax error anywhere makes its
    // version verdict untrustworthy: `ragondin-pipeline` answers `Unreadable`
    // and prescribes falling through to the full parse. `LocalFile` does.
    //
    // Asserting the *variant* alone would pin almost nothing — reporting the
    // peek's own error would also produce `Malformed`, so the test would pass
    // with the fall-through deleted. The message is what separates them, and
    // what it says here is worth knowing rather than guessing: `SchemaVersion`
    // deserializes before the malformed region is reached, so the full parse
    // fails on the *version*, not on the syntax, and `SchemaVersion` refuses
    // through `Error::custom`, which erases the type. The fall-through
    // therefore yields `Malformed` carrying a version complaint.
    //
    // That mismatch between variant and message is a real wart, and it is
    // pinned rather than hidden so that a change to it is deliberate. It is
    // not this crate's to fix: the type that would let `load` tell the two
    // apart is erased upstream, by design (`raw.rs` says so).
    let path = scratch(
        "future-and-broken",
        "version: 7\npipeline:\n  inputs: [question\n  nodes: - - -\n",
    );
    let error = LocalFile::new(&path)
        .load()
        .await
        .expect_err("malformed text must not load");

    let ConfigError::Malformed { source, .. } = &error else {
        panic!("a syntax error outranks the version verdict, got {error:?}");
    };
    assert!(
        source.to_string().contains("schema version 7"),
        "this must be the full parse's verdict, not the peek's: {source}"
    );
}

#[tokio::test]
async fn a_syntax_error_alone_is_reported_where_it_occurs() {
    // The case `ragondin-pipeline`'s prescription is actually about: no
    // version key, so nothing outranks the syntax fault and the reader gets a
    // located message. Pinned because it is the outcome the fall-through
    // exists to produce, and the test above pins the exception to it.
    let path = scratch(
        "broken-only",
        "pipeline:\n  inputs: [question\n  nodes: - - -\n",
    );
    let error = LocalFile::new(&path)
        .load()
        .await
        .expect_err("malformed text must not load");

    let ConfigError::Malformed { source, .. } = &error else {
        panic!("expected Malformed, got {error:?}");
    };
    let location = source
        .location()
        .expect("a syntax fault carries a location");
    assert_eq!(
        (location.line(), location.column()),
        (3, 8),
        "the message must point at the fault, not at the top of the file: {source}"
    );
}

#[tokio::test]
async fn a_config_source_is_usable_behind_a_trait_object() {
    // §8.2: "the data plane does not know who configures it." The binary
    // (#30/#31) holds whichever source it was given, so the trait has to be
    // dyn-compatible — a signature that was not would make the abstraction
    // decorative.
    let source: Box<dyn ConfigSource> = Box::new(LocalFile::new(fixture()));
    let pipeline = source
        .load()
        .await
        .expect("the fixture must load through dyn");
    assert_eq!(pipeline.nodes().len(), 3);
}

#[tokio::test]
async fn every_error_variant_keeps_its_cause_reachable() {
    // `anyhow` in the binary (#30) prints an error chain; a wrapper that
    // swallowed its cause would make the file's actual fault invisible there —
    // and `Invalid` is the one that matters most, since `ValidationError`
    // carries the offending node id. One instance of each variant, so a
    // variant that stopped carrying a cause at all fails this. Note it does
    // not pin the `#[source]` attributes themselves: `thiserror` also infers
    // the source from a field named `source`, so deleting one attribute
    // changes nothing here — which is a reason the chain is hard to break by
    // accident, not a gap in the test.
    use std::error::Error;

    let missing = Path::new(env!("CARGO_TARGET_TMPDIR")).join("chain-absent.yaml");
    let cases = [
        LocalFile::new(&missing).load().await.unwrap_err(),
        LocalFile::new(scratch("chain-malformed", "pipeline:\n  inputs: [q\n  nodes: - - -\n"))
            .load()
            .await
            .unwrap_err(),
        LocalFile::new(scratch(
            "chain-version",
            "version: 7\npipeline:\n  inputs: [question]\n  nodes: []\n",
        ))
        .load()
        .await
        .unwrap_err(),
        LocalFile::new(scratch(
            "chain-invalid",
            "pipeline:\n  inputs: [question]\n  nodes:\n    - id: fuse\n      component: fusion\n      impl: rrf\n      inputs: [absent]\n",
        ))
        .load()
        .await
        .unwrap_err(),
    ];

    assert!(
        matches!(cases[0], ConfigError::Unreadable { .. })
            && matches!(cases[1], ConfigError::Malformed { .. })
            && matches!(cases[2], ConfigError::UnsupportedSchemaVersion { .. })
            && matches!(cases[3], ConfigError::Invalid { .. }),
        "the four fixtures must cover the four variants, got {cases:?}"
    );
    for error in &cases {
        assert!(
            error.source().is_some(),
            "the cause must stay reachable through the chain: {error}"
        );
    }
}
