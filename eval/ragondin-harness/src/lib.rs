//! # ragondin-harness
//!
//! The evaluation harness: it drives the **same** `ragondin-engine` with an
//! iterator over a benchmark (`ragondin-benchmarks`), collects each
//! `ExecutionTrace` and the metrics (`ragondin-metrics`), and assembles a run
//! identified by the content-addressed tuple (`ragondin-experiments`).
//!
//! It is a **thin driver**. The serving driver (`ragondin-server`) wraps the
//! very same engine; there is literally one execution path, and Cargo proves it
//! — which is what makes evaluation/serving skew impossible (P1, ADR-4). No
//! execution logic is reimplemented here: [`evaluate`] plans through
//! `plan_physical` and runs through `Engine::execute`, and what it adds is the
//! loop, the scoring and the identity.
//!
//! # What a run is made of
//!
//! - [`Evaluation`] — the request: a validated pipeline and its verbatim
//!   configuration text, a loaded benchmark, the [`CorpusIndex`] the caller's
//!   components were constructed from, the rank cutoff, and the model hashes
//!   the caller knows.
//! - [`CorpusIndex`] — the corpus prepared for retrieval. **Ad hoc, and not a
//!   pipeline**: question 5 of `docs/OPEN_QUESTIONS.md` is unresolved, and this
//!   crate does not resolve it. Built by the caller, not by this crate
//!   (ADR-C26): the composition root is the one place that holds both the
//!   engine and the concrete components it constructs from these chunks.
//!   Defined in `ragondin_benchmarks::identity` with the dataset digest, and
//!   re-exported here: a reader verifying a stored run derives the same chunk
//!   set without reaching the engine (ADR-C36 § 4).
//! - The `Run` — the record, named by `hash(pipeline_config, dataset_version,
//!   index_version, model_hashes, engine_version)` (§7.1). Identical inputs
//!   yield an identical `run_id` and identical metrics (P4).
//!   [`run_identity`] computes that id from an [`Evaluation`] without running
//!   it, through the construction `evaluate` names the finished run by, so a
//!   caller can announce a run before executing it.
//!
//! # What a run scores
//!
//! The regime follows the pieces the benchmark carries (ADR-8), never a flag:
//! qrels score nDCG@k, recall@k and MRR; reference answers score `exact_match`
//! and `token_f1` (ADR-C30 § 1); a benchmark carrying both scores both, each
//! family averaged over the queries that carry its piece. Once a pipeline ends
//! in an answer, the retrieval metrics read the ranking that fed its context
//! builder, found by port position (ADR-C30 § 3), and the answer is read from
//! the generator's entry in the trace.
//!
//! # Watching and stopping a run
//!
//! [`evaluate_observed`] is the loop behind [`evaluate`], with an observer
//! called once per executed query — its position, the total, the engine's
//! time, and the rendered trace, as a [`QueryProgress`] — and a cancellation
//! signal read between two queries and never inside one
//! ([`HarnessError::Cancelled`]). The traces the observer received are the
//! partial record of a run that stopped.
//!
//! The harness **returns** the run rather than storing it: the store root is
//! the caller's, and `FileSystemRunStore::save` is one call away. What belongs
//! here is the record; where it is written down is the composition root's.
//!
//! # What is not here
//!
//! No serving path and no Tower (that is `ragondin-server`). No judge: every
//! metric is deterministic (ADR-10). No cache: question 6 of `docs/OPEN_QUESTIONS.md` is
//! unresolved. And no component: a driver is handed a ready `EngineContext`
//! and constructs nothing, so this crate depends on nothing under
//! `components/`.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

// Private modules with a flat re-export: one path to each item. The two names a
// caller needs are `evaluate` and `Evaluation`; a caller that watches or stops
// a run adds `evaluate_observed` and `QueryProgress`, and one that must name a
// run before running it adds `run_identity`.
mod error;
mod evaluate;
mod identity;
mod trace;

// Re-exported, not defined: the chunk derivation and the `index_version` it
// names live in `ragondin_benchmarks::identity` (ADR-C36 § 4), and this path
// keeps every caller that names `ragondin_harness::CorpusIndex` compiling.
pub use error::{HarnessError, RankingWalkError};
pub use evaluate::{evaluate, evaluate_observed, Evaluation, QueryProgress};
pub use identity::run_identity;
pub use ragondin_benchmarks::identity::CorpusIndex;
