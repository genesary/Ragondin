//! The `compare` subcommand: read two stored runs, print their diff.
//!
//! ADR-C15 keeps the comparison itself in `ragondin-experiments`
//! ([`ragondin_experiments::compare()`], reached here through
//! [`FileSystemRunStore::compare`]); this module renders what that function
//! returns and computes nothing of its own — no metric, no re-execution.

use std::path::Path;

use anyhow::{Context, Result};
use ragondin_experiments::{FileSystemRunStore, MetricComparison, RunComparison, RunId};

/// Loads the runs named by `run_a` and `run_b` from the store rooted at
/// `store_root` and prints their metric-by-metric diff.
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
/// by `identical` when the two runs agree on every one of them.
///
/// Two runs with no metrics at all are `identical` too
/// ([`RunComparison::is_identical`]'s own rule) — there is nothing they
/// disagree about, and this renders exactly that empty table plus the line.
fn render(comparison: &RunComparison) -> String {
    let mut report = format!("{} vs {}\n", comparison.left, comparison.right);
    for metric in &comparison.metrics {
        report.push_str(&render_metric(metric));
        report.push('\n');
    }
    if comparison.is_identical() {
        report.push_str("identical\n");
    }
    report
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
