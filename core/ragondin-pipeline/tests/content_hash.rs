//! The golden tests for INV-8, over the **public** path a configuration
//! actually travels: YAML source → [`RawPipeline`] → [`validate`] →
//! [`LogicalPipeline::content_hash`].
//!
//! They live here rather than beside the encoder because that path is the
//! claim INV-8 makes. A unit test can pin the encoder's framing; only a test
//! that starts from two differently formatted documents can show that "two
//! semantically equivalent configurations formatted differently produce the
//! same hash" — the sentence the invariant is written in.
//!
//! `serde_yaml` is a dev-dependency, so the source text here is the same
//! format a user writes.

use ragondin_pipeline::{
    validate, LogicalNode, LogicalPipeline, ParamValue, PipelineHash, RawPipeline,
};

/// Parses a YAML configuration and canonicalizes it, panicking with the
/// document on either failure — a golden test that silently skipped its
/// subject would be worse than no test.
fn canonicalize(yaml: &str) -> LogicalPipeline {
    let raw: RawPipeline =
        serde_yaml::from_str(yaml).unwrap_or_else(|e| panic!("fixture did not parse: {e}\n{yaml}"));
    validate(raw).unwrap_or_else(|e| panic!("fixture did not validate: {e}\n{yaml}"))
}

fn hash_of(yaml: &str) -> PipelineHash {
    canonicalize(yaml).content_hash()
}

/// The reference document: two retrieval legs fused, wired off one declared
/// input. Every other document in this file is this one, perturbed.
const REFERENCE: &str = r#"
pipeline:
  inputs: [question]
  nodes:
    - id: dense
      component: retriever
      impl: qdrant_dense
      inputs: [question]
      params: { top_k: 50, metric: cosine }
    - id: sparse
      component: retriever
      impl: bm25
      inputs: [question]
      params: { top_k: 50 }
    - id: fuse
      component: fusion
      impl: rrf
      inputs: [dense, sparse]
      params: { k: 60.0 }
"#;

#[test]
fn node_order_is_a_formatting_artifact() {
    // The one normalization `validate` performs (it sorts the node list by
    // id). A document that lists `fuse` first is the same configuration.
    let reordered = r#"
pipeline:
  inputs: [question]
  nodes:
    - id: fuse
      component: fusion
      impl: rrf
      inputs: [dense, sparse]
      params: { k: 60.0 }
    - id: sparse
      component: retriever
      impl: bm25
      inputs: [question]
      params: { top_k: 50 }
    - id: dense
      component: retriever
      impl: qdrant_dense
      inputs: [question]
      params: { top_k: 50, metric: cosine }
"#;
    assert_eq!(
        hash_of(REFERENCE),
        hash_of(reordered),
        "reordering the node list must not change the hash (INV-8)"
    );
}

#[test]
fn param_key_order_is_a_formatting_artifact() {
    // `Params` is a `BTreeMap`, so this is canonical before the hasher sees
    // it. Pinned anyway: an encoder that iterated an insertion-ordered map
    // would break INV-8 and pass every other test here.
    let reordered = REFERENCE.replace(
        "params: { top_k: 50, metric: cosine }",
        "params: { metric: cosine, top_k: 50 }",
    );
    assert_ne!(reordered, REFERENCE, "the substitution must have applied");
    assert_eq!(
        hash_of(REFERENCE),
        hash_of(&reordered),
        "reordering param keys must not change the hash (INV-8)"
    );
}

#[test]
fn whitespace_and_yaml_style_are_formatting_artifacts() {
    // The same configuration in block style, re-indented, with comments and
    // blank lines. Nothing a parser keeps.
    let restyled = r#"
# The hybrid retrieval pipeline, written out the long way.
pipeline:

    inputs:
        - question          # one declared input, of kind Query

    nodes:

        -   id: dense
            component: retriever
            impl: qdrant_dense
            inputs:
                - question
            params:
                top_k: 50
                metric: cosine

        -   id: sparse
            component: retriever
            impl: bm25
            inputs:
                - question
            params:
                top_k: 50

        -   id: fuse
            component: fusion
            impl: rrf
            inputs:
                - dense
                - sparse
            params:
                k: 60.0
"#;
    assert_eq!(
        hash_of(REFERENCE),
        hash_of(restyled),
        "whitespace and YAML style must not change the hash (INV-8)"
    );
}

#[test]
fn a_param_that_changes_the_computation_changes_the_hash() {
    let retuned = REFERENCE.replace("top_k: 50, metric: cosine", "top_k: 8, metric: cosine");
    assert_ne!(retuned, REFERENCE, "the substitution must have applied");
    assert_ne!(
        hash_of(REFERENCE),
        hash_of(&retuned),
        "top_k: 50 -> 8 is a different computation and must hash differently"
    );
}

#[test]
fn an_impl_name_changes_the_hash() {
    // ADR-C2 § Amendments: the `impl:` value is part of the logical form, so
    // two backends are two configurations. The claim that they share a hash
    // was retracted there; this is the mechanical form of that retraction.
    let repointed = REFERENCE.replace("impl: qdrant_dense", "impl: pgvector_dense");
    assert_ne!(repointed, REFERENCE, "the substitution must have applied");
    assert_ne!(
        hash_of(REFERENCE),
        hash_of(&repointed),
        "swapping the backend must change the hash"
    );
}

#[test]
fn a_nodes_inputs_are_positional_and_never_reordered() {
    // ADR-C16 derives a node's consumed kinds positionally, so `inputs` is
    // order-significant: a fusion over [dense, sparse] and one over
    // [sparse, dense] are two configurations. This is the sign AGENTS.md
    // names for an INV-8 violation — a canonicalization step that sorts or
    // deduplicates `inputs` would make this test fail, and nothing else
    // here would notice.
    let swapped = REFERENCE.replace("inputs: [dense, sparse]", "inputs: [sparse, dense]");
    assert_ne!(swapped, REFERENCE, "the substitution must have applied");
    assert_ne!(
        hash_of(REFERENCE),
        hash_of(&swapped),
        "a node's inputs are positional: reordering them is a different pipeline"
    );
}

#[test]
fn a_node_id_changes_the_hash() {
    // `fuse` deliberately, because nothing references it: renaming a node any
    // other node consumes would have to rewrite that consumer's `inputs` too,
    // and the test would then fail for either of two reasons. This one fails
    // for exactly one.
    let renamed = REFERENCE.replace("id: fuse", "id: combine");
    assert_ne!(renamed, REFERENCE, "the substitution must have applied");
    assert_ne!(
        hash_of(REFERENCE),
        hash_of(&renamed),
        "node ids are part of the logical form"
    );
}

#[test]
fn an_int_and_a_float_of_equal_value_hash_differently() {
    // `node.rs` states the rule: `k: 60` and `k: 60.0` are different
    // configurations. An encoder that widened `Int` to `f64` would erase it.
    let as_int = REFERENCE.replace("k: 60.0", "k: 60");
    assert_ne!(as_int, REFERENCE, "the substitution must have applied");
    assert_ne!(
        hash_of(REFERENCE),
        hash_of(&as_int),
        "Int and Float are distinct in the grammar and must hash distinctly"
    );
}

#[test]
fn negative_zero_is_one_value_through_the_public_path() {
    // `-0.0` and `0.0` are one configuration, and must be one digest. Two
    // layers make that true and this test cannot tell them apart: `validate`
    // folds `-0.0` during lowering, and `content_hash` folds it again before
    // hashing. Deliberate duplication, because lowering is not on every path
    // to a `LogicalPipeline` — see
    // `the_two_zeroes_agree_whichever_door_the_pipeline_came_through`, which
    // is the test that isolates the encoder's half.
    let positive = REFERENCE.replace("k: 60.0", "bias: 0.0");
    let negative = REFERENCE.replace("k: 60.0", "bias: -0.0");
    assert_ne!(positive, negative, "the substitutions must differ");
    assert_eq!(
        hash_of(&positive),
        hash_of(&negative),
        "-0.0 and 0.0 are one value here and must hash identically"
    );
}

#[test]
fn the_digest_of_the_reference_pipeline_is_pinned() {
    // The tripwire. This literal changes only when the canonical form or its
    // encoding deliberately changes — and every hash ever written to a run
    // store is invalidated when it does, so the change must be deliberate.
    // A pinned literal is also the only way to state "stable across process
    // invocations" as a test: within one process, any deterministic function
    // of the value would pass.
    assert_eq!(
        hash_of(REFERENCE).to_string(),
        "f21ea146cdf1c6e1d9cbb83a4fc1bde1d43b98b0d28acf9aa2eb0288795b2e70",
        "the canonical encoding changed; update this literal only deliberately"
    );
}

#[test]
fn a_hash_displays_and_round_trips_as_lowercase_hex() {
    let hash = hash_of(REFERENCE);
    let rendered = hash.to_string();
    assert_eq!(rendered.len(), 64, "sha-256 renders as 64 hex digits");
    assert!(
        rendered
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "lowercase hex only, got {rendered}"
    );

    let json = serde_json::to_string(&hash).expect("a hash serializes");
    assert_eq!(json, format!("\"{rendered}\""), "serde writes the hex form");
    let back: PipelineHash = serde_json::from_str(&json).expect("a hash reads back");
    assert_eq!(back, hash, "the serde round trip is lossless");
}

#[test]
fn a_malformed_hex_string_is_a_typed_error_not_a_panic() {
    // A run store reads these back from untrusted-ish storage; a wrong
    // length or a non-hex digit must be a `Result`, and `Display`'s output
    // must be the only thing that reads back.
    for bad in [
        "\"\"",
        "\"1ec0\"",
        "\"zz c0e9ec5a5bdec0d31caf0bbbbc4b25a0e1e3ef0a5a7dcd8f906dbf4b1d3a2\"",
        "\"1EC0E9EC5A5BDEC0D31CAF0BBBBC4B25A0E1E3EF0A5A7DCD8F906DBF4B1D3A2F\"",
        "42",
    ] {
        assert!(
            serde_json::from_str::<PipelineHash>(bad).is_err(),
            "{bad} must not read back as a hash"
        );
    }
}

#[test]
fn the_two_zeroes_agree_whichever_door_the_pipeline_came_through() {
    // The second door. ADR-C23 routes `Deserialize` through the structural
    // checks and explicitly *not* through lowering — its Consequences name
    // `NonFiniteParam` as unreachable from that path — so a `LogicalPipeline`
    // obtained this way never met the `-0.0` fold that `validate` performs.
    //
    // Before the encoder folded it too, these two compared equal and carried
    // two content hashes: INV-8's failure mode through a public path, with no
    // test helper and no `unsafe`. This is the test that fails if the fold in
    // `feed_param_value` is ever removed as redundant with lowering — which it
    // is not, because lowering is not on this path.
    let doc = |float: &str| {
        format!(
            r#"{{"inputs":["question"],"nodes":[{{"Retriever":{{"id":"dense","implementation":"qdrant_dense","inputs":["question"],"params":{{"bias":{{"Float":{float}}}}}}}}}]}}"#
        )
    };
    let positive: LogicalPipeline =
        serde_json::from_str(&doc("0.0")).expect("the fixture must deserialize");
    let negative: LogicalPipeline =
        serde_json::from_str(&doc("-0.0")).expect("the fixture must deserialize");

    // The premise: these really are the two bit patterns, and this crate
    // really does call them one value. Without this the test could pass on a
    // deserializer that had already erased the sign.
    assert_eq!(
        positive, negative,
        "node.rs's float contract makes these one value"
    );
    assert_ne!(
        serde_json::to_string(&positive).unwrap(),
        serde_json::to_string(&negative).unwrap(),
        "and the serialized forms really do differ, or this proves nothing"
    );

    assert_eq!(
        positive.content_hash(),
        negative.content_hash(),
        "one value must have one content address, whichever door it came through"
    );
}

/// Every node variant and every parameter shape in one document.
///
/// [`REFERENCE`] is the realistic pipeline, and it is the wrong thing to pin
/// the grammar against: it contains no reranker, no extension, no `Bool` and
/// no `List`, so four of the nine tag bytes never reach its hasher. Renumbering
/// any of those four would silently invalidate every stored digest for a
/// pipeline that used them, and the reference digest would not move.
const EVERY_SHAPE: &str = r#"
pipeline:
  inputs: [question]
  nodes:
    - id: dense
      component: retriever
      impl: qdrant_dense
      inputs: [question]
      params: { top_k: 50, metric: cosine, rerank: false }
    - id: fuse
      component: fusion
      impl: rrf
      inputs: [dense]
      params: { k: 60.0, weights: [1, 2.5, -3], empty: [] }
    - id: rerank
      component: reranker
      impl: bge_reranker
      inputs: [question, fuse]
      params: { top_n: 5, cascade: [[1, 2], [3]] }
    - id: hyde
      component: extension
      impl: hyde
      inputs: [rerank]
      params: { enabled: true, prompt: "" }
"#;

#[test]
fn the_digest_of_the_whole_grammar_is_pinned() {
    // The second tripwire, and the one that guards the tag bytes the
    // reference pipeline cannot reach. Both literals must be updated together
    // when the canonical form deliberately changes — and if only one of them
    // moves, the encoding has become inconsistent across the grammar rather
    // than versioned.
    assert_eq!(
        hash_of(EVERY_SHAPE).to_string(),
        "ea280050c4438aeba2f23cbb039847f5de9ebf5f8668a77c7203cac8c09a42fd",
        "the canonical encoding changed; update this literal only deliberately"
    );
}

#[test]
fn the_whole_grammar_fixture_really_does_exercise_every_shape() {
    // The guard on the fixture above. A pin over a document that quietly
    // stopped covering a variant would be a tripwire with nothing under it,
    // and nothing else in this file would notice.
    let pipeline = canonicalize(EVERY_SHAPE);
    let mut seen = std::collections::BTreeSet::new();
    for node in pipeline.nodes() {
        seen.insert(match node {
            LogicalNode::Retriever(_) => "retriever",
            LogicalNode::Fusion(_) => "fusion",
            LogicalNode::Reranker(_) => "reranker",
            LogicalNode::Extension(_) => "extension",
        });
    }
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        vec!["extension", "fusion", "reranker", "retriever"],
        "the fixture must cover all four node variants"
    );

    let mut kinds = std::collections::BTreeSet::new();
    fn note(value: &ParamValue, kinds: &mut std::collections::BTreeSet<&'static str>) {
        let kind = match value {
            ParamValue::String(_) => "string",
            ParamValue::Int(_) => "int",
            ParamValue::Float(_) => "float",
            ParamValue::Bool(_) => "bool",
            ParamValue::List(values) => {
                for value in values {
                    note(value, kinds);
                }
                "list"
            }
        };
        kinds.insert(kind);
    }
    for node in pipeline.nodes() {
        let params = match node {
            LogicalNode::Retriever(n) => &n.params,
            LogicalNode::Fusion(n) => &n.params,
            LogicalNode::Reranker(n) => &n.params,
            LogicalNode::Extension(n) => &n.params,
        };
        for value in params.values() {
            note(value, &mut kinds);
        }
    }
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        vec!["bool", "float", "int", "list", "string"],
        "the fixture must cover all five parameter variants"
    );
}

#[test]
fn a_list_nested_in_a_list_is_hashed_through() {
    // `feed_param_value` recurses, and `cascade: [[1, 2], [3]]` in the fixture
    // above is the only nesting in the suite. Regrouping its elements without
    // changing their order or count must still change the digest — the
    // recursion's own framing, one level down from the one the field tests
    // cover.
    let regrouped = EVERY_SHAPE.replace("cascade: [[1, 2], [3]]", "cascade: [[1], [2, 3]]");
    assert_ne!(regrouped, EVERY_SHAPE, "the substitution must have applied");
    assert_ne!(
        hash_of(EVERY_SHAPE),
        hash_of(&regrouped),
        "a nested list's grouping is part of the value"
    );
}
