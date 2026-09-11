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
//! Three modules:
//!
//! - [`run`] — [`RunId`] and the [`Run`] record: the identity tuple's
//!   components, the metrics, the configuration and the per-query traces.
//! - [`store`] — [`FileSystemRunStore`], a directory per run. Native by
//!   decision (ADR-13), and filesystem-backed by a choice that module argues.
//! - [`mod@compare`] — [`compare()`], the metric-by-metric diff behind
//!   `ragondin compare` and, later, the comparison view (§6.5).
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

pub use compare::{compare, MetricComparison, RunComparison};
pub use run::{ConfigDocument, Metrics, Run, RunId, RunIdParseError, RunInputs, TraceDocument};
pub use store::{FileSystemRunStore, RunStoreError};
