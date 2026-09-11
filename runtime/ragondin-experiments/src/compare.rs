//! Run comparison: [`compare`], [`RunComparison`] and [`MetricComparison`].
//!
//! The diff between two runs is the view the platform exists for
//! (`docs/system-architecture.md` §6.5): *this configuration against that one,
//! side by side*. This module is its data — the `ragondin compare` command and
//! any later interface render what [`compare`] returns.
//!
//! It compares **metrics**, not configurations. Which inputs differ is already
//! legible in the two runs' [`RunInputs`](crate::RunInputs), and a run store
//! that also diffed graphs would be reimplementing the pipeline crate's job.

use std::collections::BTreeSet;

use crate::run::{Run, RunId};

/// Compares two runs, metric by metric.
///
/// Every metric either run recorded appears in the result, in name order, so a
/// caller can render the two columns without deciding what to show.
pub fn compare(left: &Run, right: &Run) -> RunComparison {
    let names: BTreeSet<&str> = left
        .metrics
        .iter()
        .chain(right.metrics.iter())
        .map(|(name, _)| name)
        .collect();

    RunComparison {
        left: left.id,
        right: right.id,
        metrics: names
            .into_iter()
            .map(|name| MetricComparison {
                name: name.to_owned(),
                left: left.metrics.get(name),
                right: right.metrics.get(name),
            })
            .collect(),
    }
}

/// What two runs scored, metric by metric.
#[derive(Clone, Debug, PartialEq)]
pub struct RunComparison {
    /// The run on the left-hand side.
    pub left: RunId,
    /// The run on the right-hand side.
    pub right: RunId,
    /// One entry per metric either run recorded, in name order.
    pub metrics: Vec<MetricComparison>,
}

impl RunComparison {
    /// Whether the two runs agree on every metric.
    ///
    /// Two runs with no metrics at all agree vacuously — there is nothing they
    /// disagree about, and reporting a difference would be inventing one.
    pub fn is_identical(&self) -> bool {
        self.metrics.iter().all(MetricComparison::is_identical)
    }

    /// The metrics the two runs do not agree on.
    pub fn differences(&self) -> impl Iterator<Item = &MetricComparison> {
        self.metrics.iter().filter(|metric| !metric.is_identical())
    }
}

/// One metric, as each run recorded it.
///
/// A side is `None` when that run did not record the metric — which is not the
/// same as recording zero, and is why this is an `Option` rather than a
/// defaulted number.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricComparison {
    /// The metric's name.
    pub name: String,
    /// What the left-hand run scored, if it recorded this metric.
    pub left: Option<f64>,
    /// What the right-hand run scored, if it recorded this metric.
    pub right: Option<f64>,
}

impl MetricComparison {
    /// Right minus left, when both runs recorded the metric.
    ///
    /// The sign therefore reads as *what moved when going from left to right*,
    /// which is the direction a comparison is read in: the left-hand run is
    /// the baseline and the right-hand one is the candidate.
    pub fn delta(&self) -> Option<f64> {
        match (self.left, self.right) {
            (Some(left), Some(right)) => Some(right - left),
            _ => None,
        }
    }

    /// Whether both runs recorded this metric with the same value.
    ///
    /// Compared exactly, not within a tolerance: these are two recorded
    /// numbers read back from a store, not two computations of one number, and
    /// a store that smoothed a difference away would hide the thing it is
    /// being asked about. A metric recorded as `NaN` is therefore never
    /// identical to itself, which is IEEE 754 and not a decision taken here;
    /// two runs *read back from a store* cannot hit it, because the store
    /// refuses a non-finite metric on the way in
    /// ([`NotFinite`](crate::RunStoreError::NotFinite)).
    pub fn is_identical(&self) -> bool {
        matches!((self.left, self.right), (Some(left), Some(right)) if left == right)
    }
}
