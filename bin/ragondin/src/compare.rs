//! The `compare` subcommand: read two stored runs, print their diff.
//!
//! ADR-C15 keeps the comparison itself in `ragondin-experiments`
//! ([`ragondin_experiments::compare()`], reached here through
//! [`FileSystemRunStore::compare`]); this module renders what that function
//! returns — the metrics, then the configuration parameters the two runs
//! differ in — and computes nothing of its own: no metric, no configuration
//! difference, no re-execution.

use std::path::Path;

use anyhow::{Context, Result};
use ragondin_experiments::{
    ConfigurationComparison, FileSystemRunStore, MetricComparison, ParameterKey, RunComparison,
    RunId, Side,
};
use ragondin_pipeline::ParamValue;

/// Loads the runs named by `run_a` and `run_b` from the store rooted at
/// `store_root` and prints their diff: metric by metric, then the
/// configuration parameters they differ in.
///
/// A `run_id` that does not parse, or that names no run in the store, is
/// reported through [`RunStoreError`](ragondin_experiments::RunStoreError)'s
/// own wording rather than re-derived here — the same discipline `validate`
/// follows for everything but the one error it renders specially.
pub fn run(store_root: &Path, run_a: &str, run_b: &str) -> Result<()> {
    let left: RunId = run_a
        .parse()
        .with_context(|| format!("`{run_a}` is not a run id"))?;
    let right: RunId = run_b
        .parse()
        .with_context(|| format!("`{run_b}` is not a run id"))?;

    let store = FileSystemRunStore::new(store_root);
    let comparison = store.compare(&left, &right)?;

    print!("{}", render(&comparison));

    Ok(())
}

/// Renders a comparison as one line per metric either run recorded, followed
/// by `metrics: identical` when the two runs agree on every one of them, and
/// then the configuration block ([`render_configuration`]).
///
/// Two runs with no metrics at all get `metrics: identical` too
/// ([`RunComparison::is_identical`]'s own rule) — there is nothing they
/// disagree about, and this renders exactly that empty table plus the line.
/// That line speaks of the metrics only; the configuration block below it
/// says whether the configurations agree.
fn render(comparison: &RunComparison) -> String {
    let mut report = format!("{} vs {}\n", comparison.left, comparison.right);
    for metric in &comparison.metrics {
        report.push_str(&render_metric(metric));
        report.push('\n');
    }
    if comparison.is_identical() {
        report.push_str("metrics: identical\n");
    }
    report.push_str(&render_configuration(&comparison.configuration));
    report
}

/// The configuration block: one heading line, then one indented line per
/// differing parameter — `<node> component:`, `<node> impl:` or
/// `<node> params.<key>:`, both values, `-` for a side that does not set it —
/// in the order `ragondin-experiments` returns them, node id then key.
fn render_configuration(configuration: &ConfigurationComparison) -> String {
    match configuration {
        ConfigurationComparison::Compared {
            differences,
            same_logical_form,
        } if differences.is_empty() => {
            // `identical` only when the canonical forms hash equal. Every
            // node's family, `impl:` and params are listed keys, so what the
            // hash sees beyond them is the wiring: a node's `inputs` and the
            // declared inputs. Two configurations wired differently share
            // every parameter and are still two, with two run identities.
            if *same_logical_form {
                "configuration: identical\n".to_owned()
            } else {
                "configuration: no parameter differs; the wiring does\n".to_owned()
            }
        }
        ConfigurationComparison::Compared { differences, .. } => {
            let mut block = match differences.len() {
                1 => "configuration: 1 parameter differs\n".to_owned(),
                count => format!("configuration: {count} parameters differ\n"),
            };
            for difference in differences {
                let key = match &difference.key {
                    ParameterKey::Component => "component".to_owned(),
                    ParameterKey::Impl => "impl".to_owned(),
                    ParameterKey::Param(key) => format!("params.{key}"),
                };
                block.push_str(&format!(
                    "  {} {key}: {} vs {}\n",
                    difference.node.as_str(),
                    render_side(difference.left.as_ref()),
                    render_side(difference.right.as_ref()),
                ));
            }
            block
        }
        ConfigurationComparison::Unavailable { side, reason } => {
            let side = match side {
                Side::Left => "left",
                Side::Right => "right",
            };
            format!("configuration: not compared, the {side} run's: {reason}\n")
        }
    }
}

fn render_side(value: Option<&ParamValue>) -> String {
    value.map_or_else(|| "-".to_owned(), render_value)
}

/// A value as a reader tells the grammar's five shapes apart: a string
/// quoted, a float never rendered as an integer would be — `k: 60` and
/// `k: 60.0` are two configurations, and must not render as one.
fn render_value(value: &ParamValue) -> String {
    match value {
        ParamValue::String(text) => format!("{text:?}"),
        ParamValue::Int(number) => number.to_string(),
        ParamValue::Float(number) => format!("{number:?}"),
        ParamValue::Bool(flag) => flag.to_string(),
        ParamValue::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(render_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// One metric's line: both runs' values, and which side scored higher.
///
/// "Higher" is reported, never "better": this module knows no metric's
/// direction — whether nDCG or wall-clock latency improves by going up or
/// down — because [`Metrics`](ragondin_experiments::Metrics) fixes no
/// catalogue of names (quality, cost and latency all land in the same map,
/// per `docs/system-architecture.md` §6.5). A reader supplies the direction
/// for the metric they are looking at, exactly as they would reading the
/// store's own `metrics.json` beside it.
fn render_metric(metric: &MetricComparison) -> String {
    match (metric.left, metric.right) {
        (Some(left), Some(right)) if metric.is_identical() => {
            format!("{}: {left:.4} vs {right:.4} -> identical", metric.name)
        }
        (Some(left), Some(right)) => {
            let delta = metric.delta().expect("both sides are present");
            let (winner, magnitude) = if delta > 0.0 {
                ("right", delta)
            } else {
                ("left", -delta)
            };
            format!(
                "{}: {left:.4} vs {right:.4} -> {winner} +{magnitude:.4}",
                metric.name
            )
        }
        (Some(left), None) => {
            format!("{}: {left:.4} vs - -> only in the left run", metric.name)
        }
        (None, Some(right)) => {
            format!("{}: - vs {right:.4} -> only in the right run", metric.name)
        }
        (None, None) => {
            unreachable!("compare() names a metric only when at least one run recorded it")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ragondin_experiments::ParameterDifference;
    use ragondin_pipeline::NodeId;

    #[test]
    fn identical_configurations_say_so_in_one_line() {
        assert_eq!(
            render_configuration(&ConfigurationComparison::Compared {
                differences: Vec::new(),
                same_logical_form: true,
            }),
            "configuration: identical\n"
        );
    }

    #[test]
    fn no_differing_parameter_under_a_different_logical_form_names_the_wiring() {
        assert_eq!(
            render_configuration(&ConfigurationComparison::Compared {
                differences: Vec::new(),
                same_logical_form: false,
            }),
            "configuration: no parameter differs; the wiring does\n"
        );
    }

    #[test]
    fn each_differing_parameter_names_its_node_its_key_and_both_values() {
        let differences = vec![
            ParameterDifference {
                node: NodeId::new("sparse"),
                key: ParameterKey::Component,
                left: Some(ParamValue::String("retriever".to_owned())),
                right: Some(ParamValue::String("extension".to_owned())),
            },
            ParameterDifference {
                node: NodeId::new("sparse"),
                key: ParameterKey::Impl,
                left: Some(ParamValue::String("bm25".to_owned())),
                right: Some(ParamValue::String("splade".to_owned())),
            },
            ParameterDifference {
                node: NodeId::new("sparse"),
                key: ParameterKey::Param("k".to_owned()),
                left: Some(ParamValue::Int(60)),
                right: Some(ParamValue::Float(60.0)),
            },
            ParameterDifference {
                node: NodeId::new("tail"),
                key: ParameterKey::Param("fields".to_owned()),
                left: None,
                right: Some(ParamValue::List(vec![
                    ParamValue::String("title".to_owned()),
                    ParamValue::Bool(true),
                ])),
            },
        ];

        assert_eq!(
            render_configuration(&ConfigurationComparison::Compared {
                differences,
                same_logical_form: false,
            }),
            "configuration: 4 parameters differ\n\
             \x20 sparse component: \"retriever\" vs \"extension\"\n\
             \x20 sparse impl: \"bm25\" vs \"splade\"\n\
             \x20 sparse params.k: 60 vs 60.0\n\
             \x20 tail params.fields: - vs [\"title\", true]\n"
        );
    }

    #[test]
    fn a_configuration_that_could_not_be_compared_says_which_side_and_why() {
        let unavailable = ConfigurationComparison::Unavailable {
            side: Side::Right,
            reason: "the stored configuration does not parse: missing field `pipeline`".to_owned(),
        };

        assert_eq!(
            render_configuration(&unavailable),
            "configuration: not compared, the right run's: the stored configuration does not parse: missing field `pipeline`\n"
        );
    }

    #[test]
    fn a_metric_the_two_runs_scored_differently_names_the_higher_side_and_the_gap() {
        let metric = MetricComparison {
            name: "ndcg@10".to_owned(),
            left: Some(0.64),
            right: Some(0.71),
        };

        assert_eq!(
            render_metric(&metric),
            "ndcg@10: 0.6400 vs 0.7100 -> right +0.0700"
        );
    }

    #[test]
    fn a_metric_the_two_runs_scored_identically_says_so() {
        let metric = MetricComparison {
            name: "recall@10".to_owned(),
            left: Some(0.75),
            right: Some(0.75),
        };

        assert_eq!(
            render_metric(&metric),
            "recall@10: 0.7500 vs 0.7500 -> identical"
        );
    }

    #[test]
    fn a_metric_only_one_run_recorded_says_which_side() {
        let metric = MetricComparison {
            name: "ndcg@10".to_owned(),
            left: Some(0.64),
            right: None,
        };

        assert_eq!(
            render_metric(&metric),
            "ndcg@10: 0.6400 vs - -> only in the left run"
        );
    }
}
