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
//! - The `Run` — the record, named by `hash(pipeline_config, dataset_version,
//!   index_version, model_hashes, engine_version)` (§7.1). Identical inputs
//!   yield an identical `run_id` and identical metrics (P4).
//!
//! The harness **returns** the run rather than storing it: the store root is
//! the caller's, and `FileSystemRunStore::save` is one call away. What belongs
//! here is the record; where it is written down is the composition root's.
//!
//! # What is not here
//!
//! No serving path and no Tower (that is `ragondin-server`). No judge and no
//! generation — retrieval metrics only, which is what makes M2 defensible
//! without an LLM (ADR-10). No cache: question 6 of `docs/OPEN_QUESTIONS.md` is
//! unresolved. And no component: a driver is handed a ready `EngineContext`
//! and constructs nothing, so this crate depends on nothing under
//! `components/`.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

// Private modules with a flat re-export: one path to each item. The two names a
// caller needs are `evaluate` and `Evaluation`.
mod corpus;
mod error;
mod evaluate;
mod identity;
mod trace;

pub use corpus::CorpusIndex;
pub use error::HarnessError;
pub use evaluate::{evaluate, Evaluation};
