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
//! This crate also owns the **identity of a dataset and of the chunk set
//! derived from it** — [`identity::dataset_version`],
//! [`identity::CorpusIndex`] (one chunk per document) and
//! [`identity::index_version`] — so that the harness that records them and the
//! experiment plane that verifies a stored run against them share one
//! definition without the second reaching the engine (ADR-C36 § 4).
//!
//! And it owns the **datasets directory**: [`manifest`] lists the datasets
//! this build can obtain, each pinned by digest, and [`datasets`] puts one on
//! disk through a digest-verified download, verifies what a directory holds
//! against the `dataset_version` expected of it, and imports a local corpus.
//! That download is the one network fetch in this crate, and it only puts a
//! frozen snapshot on disk: nothing that loads a benchmark for a run reads the
//! network.
//!
//! Reading dataset files from disk here is correct: INV-3 (no I/O) names
//! `ragondin-types` and `ragondin-pipeline`, not this crate. See
//! `ARCHITECTURE.md`.

#![warn(missing_docs)]

pub mod beir;
mod benchmark;
pub mod datasets;
mod error;
pub mod identity;
pub mod manifest;
pub mod squad;

pub use beir::BeirAdapter;
pub use benchmark::{Benchmark, BenchmarkAdapter, CarriedPieces, Qrels, ReferenceAnswers};
pub use error::BenchmarkError;
pub use squad::SquadAdapter;
