//! # ragondin-experiments
//!
//! The experiment plane's state (`docs/system-architecture.md` §6.2): the
//! **native run store** — the history of runs, their metrics and their traces
//! — and the **run comparison** that store exists to serve.
//!
//! A run is identified by the content-addressed tuple of its inputs (§7.1), so
//! a run whose id is already stored need not be executed again. The digest is
//! assembled by the harness, which holds every piece of the tuple; this crate
//! defines the record, stores it by that id, and diffs two of them.
//!
//! The modules:
//!
//! - [`run`] — [`RunId`] and the [`Run`] record: the identity tuple's
//!   components, the metrics, the configuration and the per-query traces, and,
//!   outside identity, the bindings, the [`RunTimes`] it ran at and the
//!   [`RunProvenance`] it was launched under.
//! - [`store`] — the [`RunStore`] trait, and [`FileSystemRunStore`], its
//!   first implementation, a directory per run. Native by decision (ADR-13),
//!   and filesystem-backed by a choice that module argues.
//! - [`mod@trace`] — [`Trace`], the one typed definition of a stored trace
//!   document, read out of a [`TraceDocument`] and written back into one
//!   (ADR-C36 § 2). The store itself never parses a trace.
//! - [`walk`] — [`terminal`] and [`ranking_node`], the one definition of
//!   ADR-C30 § 3's walk to the ranking a pipeline is scored on, called by the
//!   harness that writes a run's metrics and by the API that reads them back.
//! - `conformance`, behind the `conformance` feature — the suite every
//!   [`RunStore`] backend passes.
//! - [`mod@compare`] — [`compare()`], the diff behind `ragondin compare`:
//!   metric by metric, and the configuration parameters the two runs differ
//!   in; and [`compare_runs`], the same table and matrix over a baseline and
//!   further runs of one benchmark, behind the comparison view (§6.5).
//!
//! Not here, and deliberately: **export adapters** to MLflow or OpenTelemetry
//! (additive to the plane, and not what a local benchmark needs), the
//! **registry** of benchmarks, datasets and indexes, the **user interface**,
//! **metric computation** (`ragondin-metrics`), and **execution** of anything
//! at all (the harness and the engine).

#![warn(missing_docs)]

pub mod compare;
pub mod run;
pub mod store;
pub mod trace;
pub mod walk;

#[cfg(feature = "conformance")]
pub mod conformance;

pub use compare::{
    compare, compare_runs, lower_configuration, Comparison, ConfigurationComparison,
    ConfigurationMatrix, Direction, MetricComparison, MetricRow, NotComparable,
    ParameterDifference, ParameterKey, ParameterRow, RunComparison, Side,
};
pub use run::{
    ConfigDocument, Metrics, PrefixOf, Run, RunBinding, RunId, RunIdParseError, RunInputs,
    RunProvenance, RunTimes, TraceDocument, UnixMillis,
};
pub use store::{FileSystemRunStore, RunStore, RunStoreError};
pub use trace::{Trace, TraceChunk, TraceError, TraceNode, TraceProblem, TraceSummary};
pub use walk::{ranking_node, terminal, WalkError};
