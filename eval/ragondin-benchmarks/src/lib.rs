//! # ragondin-benchmarks
//!
//! The `BenchmarkAdapter` contract and its implementations. A benchmark is a
//! corpus plus queries plus ground truth; an adapter presents any dataset
//! through one shape the harness can drive.
//!
//! §5.3 models a benchmark as a quadruple — corpus + queries + qrels +
//! reference answers — where the pieces a benchmark *carries* mechanically
//! determine which metrics are computable (ADR-8). [`Benchmark`] holds all
//! four, and [`Benchmark::carries`] reports which of qrels and reference
//! answers it carries, so the harness reads the regime off the value rather
//! than from a flag. Reference answers are data, never a judge's output.
//!
//! Two adapters: [`BeirAdapter`] reads a BEIR directory — the retrieval
//! triple, plus `answers.jsonl` as reference answers when asked to — and
//! [`SquadAdapter`] reads a SQuAD v1.1 file as a retrieval corpus with
//! reference answers (ADR-C30 § 2).
//!
//! This crate **owns the qrels type** and depends on `ragondin-metrics` in
//! neither direction: the metrics functions borrow a `&BTreeMap<DocId, u8>`,
//! which is exactly what [`Qrels::for_query`] hands out.
//!
//! Reading dataset files from disk here is correct: INV-3 (no I/O) names
//! `ragondin-types` and `ragondin-pipeline`, not this crate. See
//! `ARCHITECTURE.md`.

#![warn(missing_docs)]

pub mod beir;
mod benchmark;
mod error;
pub mod squad;

pub use beir::BeirAdapter;
pub use benchmark::{Benchmark, BenchmarkAdapter, CarriedPieces, Qrels, ReferenceAnswers};
pub use error::BenchmarkError;
pub use squad::SquadAdapter;
