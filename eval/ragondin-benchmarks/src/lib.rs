//! # ragondin-benchmarks
//!
//! The `BenchmarkAdapter` contract and its implementations. A benchmark is a
//! corpus plus queries plus ground truth; an adapter presents any dataset
//! through one shape the harness can drive.
//!
//! §5.3 models a benchmark as a quadruple — corpus + queries + qrels +
//! reference answers — where the pieces *present* mechanically determine which
//! metrics are computable. This crate implements the **retrieval family**
//! (corpus + queries + qrels): it needs no LLM judge and yields fully
//! deterministic metrics. Reference answers arrive with the milestone that
//! computes generation metrics, additively.
//!
//! This crate **owns the qrels type** and depends on `ragondin-metrics` in
//! neither direction: the metrics functions borrow a `&BTreeMap<DocId, u8>`,
//! which is exactly what [`Qrels::for_query`] hands out.
//!
//! Reading dataset files from disk here is correct: INV-3 (no I/O) names
//! `ragondin-types` and `ragondin-pipeline`, not this crate. See
//! `ARCHITECTURE.md`.

#![warn(missing_docs)]

mod benchmark;
mod error;

pub use benchmark::{Benchmark, BenchmarkAdapter, Qrels};
pub use error::BenchmarkError;
