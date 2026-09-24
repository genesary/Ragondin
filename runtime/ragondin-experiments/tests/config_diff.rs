//! The configuration half of a comparison: which node-level parameters and
//! `impl:` names two runs' stored configurations disagree on.
//!
//! The documents are lowered to `LogicalPipeline` before they are compared, so
//! two spellings of one configuration compare as one configuration (the spirit
//! of INV-8) — a difference in formatting is not a difference in what ran.

use std::collections::BTreeMap;

use ragondin_experiments::{
    compare, ConfigDocument, ConfigurationComparison, ParameterDifference, ParameterKey, Run,
    RunId, RunInputs, Side,
};
use ragondin_pipeline::{NodeId, ParamValue, PipelineHash};

fn a_run(byte: u8, config: &str) -> Run {
    Run {
        id: RunId::from_digest([byte; 32]),
        inputs: RunInputs {
            pipeline: PipelineHash::from_digest([0xbe; 32]),
            dataset_version: "beir/scifact@2021-05-01".to_owned(),
            index_version: "bm25-ram@7".to_owned(),
            model_hashes: BTreeMap::new(),
            engine_version: "0.0.0".to_owned(),
        },
        metrics: [("ndcg@10", 0.5)].into_iter().collect(),
        config: ConfigDocument::new(config),
        traces: BTreeMap::new(),
    }
}

const BASELINE: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: sparse
      component: retriever
      impl: bm25
      inputs: [question]
      params: { top_k: 10, k1: 1.2 }
";

fn differences(comparison: &ConfigurationComparison) -> &[ParameterDifference] {
    match comparison {
        ConfigurationComparison::Compared { differences, .. } => differences,
        other => panic!("both documents lower, so they are compared: {other:?}"),
    }
}

#[test]
fn one_differing_parameter_names_the_node_the_key_and_both_values() {
    let candidate = BASELINE.replace("top_k: 10", "top_k: 20");

    let comparison = compare(&a_run(0x01, BASELINE), &a_run(0x02, &candidate));

    assert_eq!(
        differences(&comparison.configuration),
        [ParameterDifference {
            node: NodeId::new("sparse"),
            key: ParameterKey::Param("top_k".to_owned()),
            left: Some(ParamValue::Int(10)),
            right: Some(ParamValue::Int(20)),
        }]
    );
}

#[test]
fn identical_configurations_report_no_difference_however_they_are_spelled() {
    let respelled = "\
pipeline:
  inputs: [question]
  nodes:
    - id: sparse
      component: retriever
      impl: bm25
      inputs: [question]
      params:
        k1: 1.2
        top_k: 10
";

    let comparison = compare(&a_run(0x01, BASELINE), &a_run(0x02, respelled));

    assert_eq!(
        comparison.configuration,
        ConfigurationComparison::Compared {
            differences: Vec::new(),
            same_logical_form: true,
        }
    );
}

#[test]
fn a_difference_in_wiring_alone_is_no_parameter_difference_and_not_an_identical_configuration() {
    let rewired = BASELINE.replace("[question]", "[query]");

    let comparison = compare(&a_run(0x01, BASELINE), &a_run(0x02, &rewired));

    assert_eq!(
        comparison.configuration,
        ConfigurationComparison::Compared {
            differences: Vec::new(),
            same_logical_form: false,
        }
    );
}

#[test]
fn a_document_stored_under_a_schema_version_this_build_cannot_read_says_so() {
    let future = format!("version: 99\n{BASELINE}");

    let comparison = compare(&a_run(0x01, BASELINE), &a_run(0x02, &future));

    match &comparison.configuration {
        ConfigurationComparison::Unavailable { side, reason } => {
            assert_eq!(*side, Side::Right);
            assert!(
                reason.starts_with("stored under a schema version this build cannot read"),
                "got: {reason}"
            );
        }
        other => panic!("the right document is from a newer schema: {other:?}"),
    }
}

#[test]
fn a_different_impl_is_a_parameter_of_the_node() {
    let candidate = BASELINE.replace("impl: bm25", "impl: splade");

    let comparison = compare(&a_run(0x01, BASELINE), &a_run(0x02, &candidate));

    assert_eq!(
        differences(&comparison.configuration),
        [ParameterDifference {
            node: NodeId::new("sparse"),
            key: ParameterKey::Impl,
            left: Some(ParamValue::String("bm25".to_owned())),
            right: Some(ParamValue::String("splade".to_owned())),
        }]
    );
}

#[test]
fn a_node_only_one_run_has_is_reported_key_by_key_sorted_by_node_then_key() {
    let candidate = format!(
        "{BASELINE}    - id: reorder
      component: reranker
      impl: cross_encoder
      inputs: [question, sparse]
      params: {{ top_k: 5 }}
    - id: alpha
      component: reranker
      impl: identity
      inputs: [question, reorder]
"
    );

    let comparison = compare(&a_run(0x01, BASELINE), &a_run(0x02, &candidate));

    let found: Vec<(&str, &ParameterKey, bool, bool)> = differences(&comparison.configuration)
        .iter()
        .map(|d| (d.node.as_str(), &d.key, d.left.is_some(), d.right.is_some()))
        .collect();
    assert_eq!(
        found,
        [
            ("alpha", &ParameterKey::Impl, false, true),
            ("reorder", &ParameterKey::Impl, false, true),
            (
                "reorder",
                &ParameterKey::Param("top_k".to_owned()),
                false,
                true
            ),
        ]
    );
}

#[test]
fn a_parameter_only_one_run_sets_is_reported_on_its_side() {
    let candidate = BASELINE.replace(", k1: 1.2", "");

    let comparison = compare(&a_run(0x01, BASELINE), &a_run(0x02, &candidate));

    assert_eq!(
        differences(&comparison.configuration),
        [ParameterDifference {
            node: NodeId::new("sparse"),
            key: ParameterKey::Param("k1".to_owned()),
            left: Some(ParamValue::Float(1.2)),
            right: None,
        }]
    );
}

#[test]
fn a_document_that_does_not_lower_is_reported_and_the_metrics_are_still_compared() {
    let comparison = compare(
        &a_run(0x01, "schema_version: 1\nnodes: []\n"),
        &a_run(0x02, BASELINE),
    );

    match &comparison.configuration {
        ConfigurationComparison::Unavailable { side, reason } => {
            assert_eq!(*side, Side::Left);
            assert!(!reason.is_empty(), "the reason says what went wrong");
        }
        other => panic!("the left document does not lower: {other:?}"),
    }
    assert_eq!(comparison.metrics.len(), 1);
}
