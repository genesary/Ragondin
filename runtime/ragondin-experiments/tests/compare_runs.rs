//! The comparison of several runs against a baseline: the metric table, with
//! the best value of each row and each run's delta to the baseline, and the
//! configuration matrix of every parameter not identical across the runs.
//! Two runs compare as `compare` compares them, and runs evaluated on
//! different benchmarks are refused.

use std::collections::BTreeMap;

use ragondin_experiments::{
    compare, compare_runs, ConfigDocument, ConfigurationComparison, ConfigurationMatrix, Direction,
    NotComparable, ParameterKey, ParameterRow, Run, RunId, RunInputs, Side,
};
use ragondin_pipeline::{NodeId, ParamValue, PipelineHash};

const SCIFACT: &str = "beir/scifact@2021-05-01";

fn a_run(byte: u8, config: &str, metrics: &[(&str, f64)]) -> Run {
    Run {
        id: RunId::from_digest([byte; 32]),
        inputs: RunInputs {
            pipeline: PipelineHash::from_digest([byte; 32]),
            dataset_version: SCIFACT.to_owned(),
            index_version: "bm25-ram@7".to_owned(),
            model_hashes: BTreeMap::new(),
            engine_version: "0.0.0".to_owned(),
        },
        metrics: metrics.iter().copied().collect(),
        config: ConfigDocument::new(config),
        traces: BTreeMap::new(),
        bindings: Vec::new(),
    }
}

const DENSE: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
      params: { top_k: 100, model: bge-small }
";

const HYBRID: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
      params: { top_k: 50, model: bge-small }
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
      params: { top_k: 50 }
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [dense, bm25]
      params: { k: 60 }
";

const HYBRID_RERANK: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
      params: { top_k: 50, model: bge-small }
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
      params: { top_k: 50 }
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [dense, bm25]
      params: { k: 60 }
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, rrf]
      params: { top_k: 10 }
";

fn three_runs() -> (Run, Run, Run) {
    (
        a_run(
            0x01,
            DENSE,
            &[
                ("ndcg@10", 0.6483),
                ("recall@100", 0.9233),
                ("latency_p50_ms", 61.0),
            ],
        ),
        a_run(
            0x02,
            HYBRID,
            &[
                ("ndcg@10", 0.6912),
                ("recall@100", 0.96),
                ("latency_p50_ms", 63.0),
            ],
        ),
        a_run(
            0x03,
            HYBRID_RERANK,
            &[
                ("ndcg@10", 0.7217),
                ("mrr", 0.6875),
                ("latency_p50_ms", 412.0),
            ],
        ),
    )
}

#[test]
fn every_metric_any_run_recorded_is_a_row_with_one_value_per_run_in_name_order() {
    let (dense, hybrid, rerank) = three_runs();

    let comparison = compare_runs(&dense, &[&hybrid, &rerank]).expect("one benchmark");

    assert_eq!(comparison.runs, [dense.id, hybrid.id, rerank.id]);
    let rows: Vec<(&str, Vec<Option<f64>>)> = comparison
        .metrics
        .iter()
        .map(|row| (row.name.as_str(), row.values.clone()))
        .collect();
    assert_eq!(
        rows,
        [
            ("latency_p50_ms", vec![Some(61.0), Some(63.0), Some(412.0)]),
            ("mrr", vec![None, None, Some(0.6875)]),
            ("ndcg@10", vec![Some(0.6483), Some(0.6912), Some(0.7217)]),
            ("recall@100", vec![Some(0.9233), Some(0.96), None]),
        ]
    );
}

#[test]
fn each_row_names_its_best_run_by_the_direction_its_name_implies() {
    let (dense, hybrid, rerank) = three_runs();

    let comparison = compare_runs(&dense, &[&hybrid, &rerank]).unwrap();
    let row = |name: &str| {
        comparison
            .metrics
            .iter()
            .find(|row| row.name == name)
            .unwrap()
    };

    assert_eq!(row("ndcg@10").direction, Direction::HigherIsBetter);
    assert_eq!(row("ndcg@10").best(), [2]);
    assert_eq!(row("latency_p50_ms").direction, Direction::LowerIsBetter);
    assert_eq!(row("latency_p50_ms").best(), [0]);
    // A run that did not record the metric is never the best of its row.
    assert_eq!(row("recall@100").best(), [1]);
    assert_eq!(row("mrr").best(), [2]);
}

#[test]
fn a_tie_for_the_best_value_names_every_run_holding_it() {
    let base = a_run(0x01, DENSE, &[("ndcg@10", 0.5)]);
    let same = a_run(0x02, DENSE, &[("ndcg@10", 0.5)]);
    let worse = a_run(0x03, DENSE, &[("ndcg@10", 0.4)]);

    let comparison = compare_runs(&base, &[&same, &worse]).unwrap();

    assert_eq!(comparison.metrics[0].best(), [0, 1]);
}

#[test]
fn each_run_s_delta_is_against_the_baseline_and_absent_when_either_side_is() {
    let (dense, hybrid, rerank) = three_runs();

    let comparison = compare_runs(&dense, &[&hybrid, &rerank]).unwrap();
    let ndcg = comparison
        .metrics
        .iter()
        .find(|row| row.name == "ndcg@10")
        .unwrap();
    let recall = comparison
        .metrics
        .iter()
        .find(|row| row.name == "recall@100")
        .unwrap();

    assert_eq!(
        ndcg.deltas(),
        [Some(0.0), Some(0.6912 - 0.6483), Some(0.7217 - 0.6483)]
    );
    assert_eq!(recall.deltas(), [Some(0.0), Some(0.96 - 0.9233), None]);
    let mrr = comparison
        .metrics
        .iter()
        .find(|row| row.name == "mrr")
        .unwrap();
    assert_eq!(mrr.deltas(), [None, None, None]);
}

#[test]
fn the_matrix_holds_exactly_the_parameters_not_identical_across_the_runs() {
    let (dense, hybrid, rerank) = three_runs();

    let comparison = compare_runs(&dense, &[&hybrid, &rerank]).unwrap();

    let ConfigurationMatrix::Compared {
        parameters,
        same_logical_form,
    } = &comparison.configuration
    else {
        panic!("every document lowers: {:?}", comparison.configuration);
    };
    assert!(!same_logical_form);
    let s = |text: &str| Some(ParamValue::String(text.to_owned()));
    let row = |node: &str, key: ParameterKey, values: Vec<Option<ParamValue>>| ParameterRow {
        node: NodeId::new(node),
        key,
        values,
    };
    let param = |name: &str| ParameterKey::Param(name.to_owned());
    // `dense`'s family, `impl:` and `model` are one value across the three,
    // so they are not rows; its `top_k` is.
    assert_eq!(
        parameters,
        &[
            row(
                "bm25",
                ParameterKey::Component,
                vec![None, s("retriever"), s("retriever")]
            ),
            row("bm25", ParameterKey::Impl, vec![None, s("bm25"), s("bm25")]),
            row(
                "bm25",
                param("top_k"),
                vec![None, Some(ParamValue::Int(50)), Some(ParamValue::Int(50))]
            ),
            row(
                "dense",
                param("top_k"),
                vec![
                    Some(ParamValue::Int(100)),
                    Some(ParamValue::Int(50)),
                    Some(ParamValue::Int(50))
                ]
            ),
            row(
                "rerank",
                ParameterKey::Component,
                vec![None, None, s("reranker")]
            ),
            row(
                "rerank",
                ParameterKey::Impl,
                vec![None, None, s("cross_encoder")]
            ),
            row(
                "rerank",
                param("top_k"),
                vec![None, None, Some(ParamValue::Int(10))]
            ),
            row(
                "rrf",
                ParameterKey::Component,
                vec![None, s("fusion"), s("fusion")]
            ),
            row("rrf", ParameterKey::Impl, vec![None, s("rrf"), s("rrf")]),
            row(
                "rrf",
                param("k"),
                vec![None, Some(ParamValue::Int(60)), Some(ParamValue::Int(60))]
            ),
        ]
    );
}

#[test]
fn identical_configurations_leave_the_matrix_empty_and_one_logical_form() {
    let comparison = compare_runs(
        &a_run(0x01, DENSE, &[]),
        &[&a_run(0x02, DENSE, &[]), &a_run(0x03, DENSE, &[])],
    )
    .unwrap();

    assert_eq!(
        comparison.configuration,
        ConfigurationMatrix::Compared {
            parameters: Vec::new(),
            same_logical_form: true,
        }
    );
}

#[test]
fn a_configuration_that_does_not_lower_names_its_run_and_the_metrics_are_still_compared() {
    let future = format!("version: 99\n{DENSE}");
    let unreadable = a_run(0x03, &future, &[("ndcg@10", 0.1)]);

    let comparison = compare_runs(
        &a_run(0x01, DENSE, &[("ndcg@10", 0.2)]),
        &[&a_run(0x02, DENSE, &[]), &unreadable],
    )
    .unwrap();

    match &comparison.configuration {
        ConfigurationMatrix::Unavailable { run, reason, .. } => {
            assert_eq!(*run, unreadable.id);
            assert!(
                reason.starts_with("stored under a schema version"),
                "{reason}"
            );
        }
        other => panic!("the third document does not lower: {other:?}"),
    }
    assert_eq!(comparison.metrics[0].values, [Some(0.2), None, Some(0.1)]);
}

#[test]
fn runs_evaluated_on_different_benchmarks_are_refused_naming_both_versions() {
    let (dense, hybrid, _) = three_runs();
    let mut elsewhere = a_run(0x09, DENSE, &[]);
    elsewhere.inputs.dataset_version = "beir/nfcorpus@2021-05-01".to_owned();

    let refusal = compare_runs(&dense, &[&hybrid, &elsewhere]).unwrap_err();

    assert_eq!(
        refusal,
        NotComparable::DatasetsDiffer {
            baseline: dense.id,
            baseline_version: SCIFACT.to_owned(),
            run: elsewhere.id,
            run_version: "beir/nfcorpus@2021-05-01".to_owned(),
        }
    );
    let message = refusal.to_string();
    assert!(message.contains(SCIFACT), "{message}");
    assert!(message.contains("beir/nfcorpus@2021-05-01"), "{message}");
}

#[test]
fn two_runs_compare_as_the_pairwise_comparison_compares_them() {
    for (left, right) in [
        (
            a_run(0x01, DENSE, &[("ndcg@10", 0.6), ("mrr", 0.5)]),
            a_run(0x02, HYBRID, &[("ndcg@10", 0.7), ("recall@10", 0.9)]),
        ),
        (
            a_run(0x01, DENSE, &[("ndcg@10", 0.6)]),
            a_run(0x02, &format!("version: 99\n{DENSE}"), &[("ndcg@10", 0.6)]),
        ),
        (
            a_run(0x01, &format!("version: 99\n{DENSE}"), &[]),
            a_run(0x02, DENSE, &[]),
        ),
    ] {
        let pairwise = compare(&left, &right);
        let many = compare_runs(&left, &[&right]).unwrap();

        assert_eq!(many.runs, [pairwise.left, pairwise.right]);
        assert_eq!(many.metrics.len(), pairwise.metrics.len());
        for (row, metric) in many.metrics.iter().zip(&pairwise.metrics) {
            assert_eq!(row.name, metric.name);
            assert_eq!(row.values, [metric.left, metric.right]);
            assert_eq!(row.deltas()[1], metric.delta());
        }
        match (&many.configuration, &pairwise.configuration) {
            (
                ConfigurationMatrix::Compared {
                    parameters,
                    same_logical_form,
                },
                ConfigurationComparison::Compared {
                    differences,
                    same_logical_form: pairwise_same,
                },
            ) => {
                assert_eq!(same_logical_form, pairwise_same);
                assert_eq!(parameters.len(), differences.len());
                for (row, difference) in parameters.iter().zip(differences) {
                    assert_eq!(row.node, difference.node);
                    assert_eq!(row.key, difference.key);
                    assert_eq!(
                        row.values,
                        [difference.left.clone(), difference.right.clone()]
                    );
                }
            }
            (
                ConfigurationMatrix::Unavailable { run, reason, .. },
                ConfigurationComparison::Unavailable {
                    side,
                    reason: pairwise_reason,
                },
            ) => {
                let expected = match side {
                    Side::Left => left.id,
                    Side::Right => right.id,
                };
                assert_eq!(*run, expected);
                assert_eq!(reason, pairwise_reason);
            }
            (many, pairwise) => panic!("the two disagree: {many:?} against {pairwise:?}"),
        }
    }
}
