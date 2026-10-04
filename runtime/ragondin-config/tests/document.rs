//! A pipeline document loaded from text: [`parse_document`], the one
//! definition of the load every caller runs — `LocalFile` over a file's
//! contents, the API over a request's body, the experiment plane over a
//! stored run's configuration — and [`incompatible_wiring`], the one
//! rendering of an edge of the wrong kind.

use std::collections::BTreeMap;

use ragondin_config::{
    incompatible_wiring, parse_document, read_document, render_document, DocumentError, RenderError,
};
use ragondin_pipeline::{
    NodeId, RawGraph, RawNode, RawParamValue, RawPipeline, SchemaVersion, ValidationError,
    ValueKind,
};

const VALID: &str = "pipeline:\n  inputs: [question]\n  nodes:\n    - id: lexical\n      component: retriever\n      impl: bm25\n      inputs: [question]\n      params: { top_k: 10 }\n";

#[test]
fn a_valid_document_lowers_to_its_logical_pipeline() {
    let pipeline = parse_document(VALID).expect("valid");

    assert_eq!(pipeline.inputs(), &[NodeId::new("question")]);
    assert_eq!(
        parse_document(&format!("# a comment\n{VALID}"))
            .unwrap()
            .content_hash(),
        pipeline.content_hash()
    );
}

#[test]
fn a_version_this_build_cannot_read_is_its_own_diagnosis() {
    let error = parse_document("version: 99\npipeline:\n  inputs: [q]\n  nodes: []\n").unwrap_err();

    let DocumentError::UnsupportedSchemaVersion(source) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(source.found(), 99);
    assert_eq!(
        error.to_string(),
        "the configuration is written in a schema version this build cannot read"
    );
}

#[test]
fn text_outside_the_wire_schema_is_malformed_with_the_deserializer_s_location() {
    let error = parse_document("pipeline:\n  inputs: [q\n  nodes: []\n").unwrap_err();

    let DocumentError::Malformed(source) = &error else {
        panic!("{error:?}");
    };
    assert!(source.to_string().contains("line 3 column 8"), "{source}");
    assert_eq!(error.to_string(), "could not parse configuration");
}

#[test]
fn a_graph_the_pass_refuses_is_invalid_with_the_pass_s_verdict_intact() {
    let error = parse_document(&VALID.replace(
        "inputs: [question]\n      params",
        "inputs: [nowhere]\n      params",
    ))
    .unwrap_err();

    let DocumentError::Invalid(source) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        source,
        &ValidationError::DanglingInput {
            node: NodeId::new("lexical"),
            missing: NodeId::new("nowhere"),
        }
    );
    assert_eq!(error.to_string(), "configuration is not a valid pipeline");
}

fn mismatch(port: usize, expected: Option<ValueKind>) -> ValidationError {
    ValidationError::KindMismatch {
        consumer: NodeId::new("ranked"),
        port,
        producer: NodeId::new("legs"),
        expected,
        found: ValueKind::Chunks,
    }
}

#[test]
fn the_wiring_report_names_its_subject_the_edge_the_kind_expected_and_the_kind_found() {
    assert_eq!(
        incompatible_wiring("`pipeline.yaml`", &mismatch(0, Some(ValueKind::Query))).as_deref(),
        Some(
            "`pipeline.yaml` wires two nodes incompatibly\n  \
             edge: `legs` feeds `ranked` at port 0\n  \
             expected: query\n  \
             found: chunks"
        )
    );
}

#[test]
fn a_port_that_does_not_exist_is_reported_as_an_edge_that_should_not_exist() {
    // `expected: None` is a consumer whose variant declares no port at that
    // position, so the fault is the edge rather than the kind on it.
    assert_eq!(
        incompatible_wiring("the configuration", &mismatch(2, None)).as_deref(),
        Some(
            "the configuration wires two nodes incompatibly\n  \
             edge: `legs` feeds `ranked` at port 2\n  \
             expected: nothing — `ranked` declares no port at position 2\n  \
             found: chunks"
        )
    );
}

#[test]
fn every_other_verdict_has_no_wiring_report() {
    assert_eq!(
        incompatible_wiring(
            "the configuration",
            &ValidationError::InputArity { declared: 0 }
        ),
        None
    );
}

// The first half of the load, alone: text into the wire schema, whether or
// not the graph it describes validates.

#[test]
fn reading_stops_at_the_wire_schema_so_an_invalid_graph_still_reads() {
    let dangling = VALID.replace(
        "inputs: [question]\n      params",
        "inputs: [nowhere]\n      params",
    );

    let raw = read_document(&dangling).expect("it reads into the wire schema");

    assert_eq!(raw.pipeline.nodes[0].inputs, vec!["nowhere".to_owned()]);
    assert!(matches!(
        parse_document(&dangling),
        Err(DocumentError::Invalid(_))
    ));
}

#[test]
fn reading_refuses_what_the_load_refuses_before_the_pass() {
    assert!(matches!(
        read_document("version: 99\npipeline:\n  inputs: [q]\n  nodes: []\n"),
        Err(DocumentError::UnsupportedSchemaVersion(_))
    ));
    assert!(matches!(
        read_document("pipeline:\n  inputs: [q\n  nodes: []\n"),
        Err(DocumentError::Malformed(_))
    ));
}

#[test]
fn the_load_is_reading_then_the_pass() {
    let raw = read_document(VALID).unwrap();

    assert_eq!(
        ragondin_pipeline::validate(raw).unwrap().content_hash(),
        parse_document(VALID).unwrap().content_hash()
    );
}

// The renderer: the wire schema written back to the configuration format.

fn node(id: &str, params: Vec<(&str, RawParamValue)>) -> RawNode {
    RawNode {
        id: id.to_owned(),
        component: "retriever".to_owned(),
        implementation: "bm25".to_owned(),
        inputs: vec!["question".to_owned()],
        params: params
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn document(nodes: Vec<RawNode>) -> RawPipeline {
    RawPipeline {
        version: SchemaVersion::CURRENT,
        pipeline: RawGraph {
            inputs: vec!["question".to_owned()],
            nodes,
        },
    }
}

/// Text another reader of YAML — a 1.1 reader above all — takes for a
/// boolean, a null, a number, a date or a structure, and that the wire
/// schema holds as a string.
const AMBIGUOUS: &[&str] = &[
    "yes",
    "no",
    "on",
    "off",
    "y",
    "n",
    "Y",
    "N",
    "YES",
    "No",
    "On",
    "OFF",
    "true",
    "False",
    "null",
    "Null",
    "~",
    "",
    "60",
    "60.0",
    "-1",
    "0x1F",
    "0o17",
    "017",
    "1e3",
    "1_000",
    ".5",
    ".inf",
    "-.inf",
    ".nan",
    "2024-01-01",
    "2001-12-14t21:59:43.10-05:00",
    "12:30:00",
    "a: b",
    "- a",
    "#x",
    "x #y",
    "[a]",
    "{a: 1}",
    "*ref",
    "&anchor",
    "!tag",
    "|",
    ">",
    "'",
    "\"",
    " leading",
    "trailing ",
    "two\nlines",
    "tab\there",
    "%directive",
    "@at",
    "`tick",
    "=",
    "<<",
    "?",
    "é",
    "1,2",
];

fn tricky() -> RawPipeline {
    let mut params: Vec<(String, RawParamValue)> = AMBIGUOUS
        .iter()
        .enumerate()
        .map(|(i, text)| {
            (
                format!("s{i:02}"),
                RawParamValue::String((*text).to_owned()),
            )
        })
        .collect();
    for (key, value) in [
        ("f_integral", RawParamValue::Float(60.0)),
        ("f_negative_zero", RawParamValue::Float(-0.0)),
        ("f_large", RawParamValue::Float(1e20)),
        ("f_huge", RawParamValue::Float(f64::MAX)),
        ("f_small", RawParamValue::Float(1e-7)),
        ("f_tiny", RawParamValue::Float(f64::MIN_POSITIVE)),
        ("f_tenth", RawParamValue::Float(0.1)),
        ("i_min", RawParamValue::Int(i64::MIN)),
        ("i_max", RawParamValue::Int(i64::MAX)),
        ("i_zero", RawParamValue::Int(0)),
        ("b_true", RawParamValue::Bool(true)),
        ("b_false", RawParamValue::Bool(false)),
        ("l_empty", RawParamValue::List(vec![])),
        (
            "l_mixed",
            RawParamValue::List(vec![
                RawParamValue::Int(1),
                RawParamValue::Float(1.0),
                RawParamValue::String("1".to_owned()),
                RawParamValue::Bool(true),
                RawParamValue::String("yes".to_owned()),
                RawParamValue::List(vec![RawParamValue::String("on".to_owned())]),
            ]),
        ),
    ] {
        params.push((key.to_owned(), value));
    }
    // Keys and ids are strings too, and as ambiguous.
    for key in ["yes", "123", "null", "2024-01-01", "1.5"] {
        params.push((key.to_owned(), RawParamValue::Int(1)));
    }
    let params = params
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let mut first = node("on", params);
    first.component = "no".to_owned();
    first.implementation = "2024-01-01".to_owned();
    document(vec![first, node("1.0", vec![])])
}

#[test]
fn the_rendering_reads_back_to_the_same_wire_document() {
    let raw = tricky();

    let text = render_document(&raw).expect("it renders");

    assert_eq!(
        read_document(&text).expect("the rendering reads"),
        raw,
        "{text}"
    );
}

#[test]
fn the_rendering_is_a_fixed_point() {
    let text = render_document(&tricky()).unwrap();

    assert_eq!(
        render_document(&read_document(&text).unwrap()).unwrap(),
        text
    );
}

#[test]
fn a_float_is_rendered_with_its_fractional_part() {
    let text = render_document(&document(vec![node(
        "lexical",
        vec![
            ("k", RawParamValue::Float(60.0)),
            ("n", RawParamValue::Int(60)),
        ],
    )]))
    .unwrap();

    assert!(text.contains("\"k\": 60.0"), "{text}");
    assert!(text.contains("\"n\": 60\n"), "{text}");
}

#[test]
fn a_string_another_yaml_reader_would_retype_is_quoted() {
    let text = render_document(&tricky()).unwrap();

    for (i, ambiguous) in AMBIGUOUS.iter().enumerate() {
        let key = format!("\"s{i:02}\": ");
        let line = text
            .lines()
            .find(|line| line.trim_start().starts_with(&key))
            .unwrap_or_else(|| panic!("{key} is rendered on one line: {text}"));
        let value = line.trim_start().trim_start_matches(&key);
        assert!(
            value.starts_with('"'),
            "`{ambiguous}` is rendered plain: {line}"
        );
    }
    // As a key, an id, an implementation or a list item, too.
    for quoted in [
        "\"id\": \"on\"",
        "\"component\": \"no\"",
        "\"impl\": \"2024-01-01\"",
        "\"id\": \"1.0\"",
        "\"yes\": 1",
        "\"null\": 1",
        "\"123\": 1",
        "\"2024-01-01\": 1",
        "\"1.5\": 1",
        "  \"yes\",\n",
        "  \"on\"\n",
    ] {
        assert!(
            text.contains(quoted),
            "`{quoted}` is not in the rendering: {text}"
        );
    }
}

/// Every character a stored document may hold in a string reads back: the
/// ones YAML folds or refuses bare — U+007F to U+009F, U+2028, U+2029,
/// U+FFFE, U+FFFF — are written as `\uXXXX`, which JSON and YAML both read
/// as the character.
#[test]
fn every_character_a_string_may_hold_reads_back() {
    let all: Vec<char> = (0..=0x10FFFFu32).filter_map(char::from_u32).collect();
    // 128 characters at most 512 bytes: a key stays under YAML's limit on one.
    for chunk in all.chunks(128) {
        let text: String = chunk.iter().collect();
        // As a node's id, a parameter's key, a value and a list's item.
        let raw = document(vec![node(
            &text,
            vec![
                (&text, RawParamValue::String(text.clone())),
                (
                    "l",
                    RawParamValue::List(vec![RawParamValue::String(text.clone())]),
                ),
            ],
        )]);
        let rendered = render_document(&raw)
            .unwrap_or_else(|_| panic!("refused from U+{:04X}", chunk[0] as u32));
        assert_eq!(
            read_document(&rendered).unwrap(),
            raw,
            "from U+{:04X}",
            chunk[0] as u32
        );
    }
}

#[test]
fn a_character_yaml_folds_or_refuses_is_written_as_its_escape() {
    for (c, escape) in [
        ('\u{7f}', "\\u007f"),
        ('\u{85}', "\\u0085"),
        ('\u{9f}', "\\u009f"),
        ('\u{2028}', "\\u2028"),
        ('\u{2029}', "\\u2029"),
        ('\u{fffe}', "\\ufffe"),
        ('\u{ffff}', "\\uffff"),
    ] {
        let raw = document(vec![node(
            "lexical",
            vec![("k", RawParamValue::String(format!("a{c}b")))],
        )]);
        let rendered = render_document(&raw).unwrap();
        assert!(
            rendered.contains(&format!("\"a{escape}b\"")),
            "{c:?}: {rendered}"
        );
    }
}

/// YAML reads a key only when it is short enough once rendered, its quotes
/// and escapes included: about 1022 plain characters, about half as many `é`
/// or newlines, which render as two bytes or as `\n`. A parameter name longer
/// than that would not read back, so it is refused.
#[test]
fn a_parameter_name_too_long_for_a_yaml_key_is_refused() {
    let renders = |key: String| {
        render_document(&document(vec![node(
            "lexical",
            vec![(&key, RawParamValue::Bool(true))],
        )]))
    };
    assert!(renders("k".repeat(1000)).is_ok());
    assert!(renders("é".repeat(500)).is_ok());
    assert!(renders("\n".repeat(500)).is_ok());
    for key in ["k".repeat(1100), "é".repeat(600), "\n".repeat(600)] {
        assert_eq!(
            renders(key.clone()),
            Err(RenderError),
            "{} characters",
            key.chars().count()
        );
    }
}

#[test]
fn a_document_whose_rendering_would_read_back_otherwise_is_refused() {
    for value in [
        RawParamValue::Float(f64::NAN),
        RawParamValue::Float(f64::INFINITY),
        RawParamValue::Float(f64::NEG_INFINITY),
    ] {
        let raw = document(vec![node("lexical", vec![("k", value.clone())])]);

        assert_eq!(render_document(&raw), Err(RenderError), "{value:?}");
    }
}

#[test]
fn the_rendering_loads_to_the_hash_of_the_document_it_renders() {
    let raw = read_document(VALID).unwrap();

    assert_eq!(
        parse_document(&render_document(&raw).unwrap())
            .unwrap()
            .content_hash(),
        parse_document(VALID).unwrap().content_hash()
    );
}
