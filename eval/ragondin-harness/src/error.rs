//! [`HarnessError`]: what stops an evaluation run.

use ragondin_engine::{ExecError, PlanError};
use ragondin_experiments::TraceDocument;
use ragondin_types::QueryId;

/// Why an evaluation did not produce a run.
///
/// Typed, and `thiserror` rather than `anyhow`: this is a library, and a driver
/// that erased its failures would make the binary above it guess at them.
#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    /// The pipeline could not be planned against the context it was given —
    /// an `impl:` name nothing registered, or a constructor that refused its
    /// configuration. Planning happens once, before any query runs.
    #[error("the pipeline could not be planned: {0}")]
    Plan(#[from] PlanError),

    /// One query failed to execute. The run stops there: metrics averaged over
    /// the queries that happened to succeed would be a smaller benchmark
    /// reported as the whole one.
    #[error("query `{}` failed: {source}", query.as_str())]
    Execute {
        /// The query whose execution failed.
        query: QueryId,
        /// What the executor recorded before it stopped, rendered.
        ///
        /// Carried here for the reason INV-10 pairs the trace with the result
        /// rather than wrapping it: the trace of a *failed* run is the one
        /// worth reading, and an error that dropped it would discard exactly
        /// that. The failing node is its last entry.
        trace: TraceDocument,
        /// The failure the executor returned.
        ///
        /// Boxed to keep `Result<Run, HarnessError>` small: the error is the
        /// cold path, and every successful call would otherwise carry the
        /// widest failure this crate can report (`clippy::result_large_err`).
        source: Box<ExecError>,
    },

    /// No query of the benchmark carries a relevance judgment, so there is
    /// nothing to score.
    ///
    /// A mean over no query is not zero, it is undefined — and a `NaN` metric
    /// is refused by the run store at the far end of this driver. Naming the
    /// cause here reports it while the cause is still known.
    #[error("no query of this benchmark is judged, so no metric can be computed")]
    NothingToScore,

    /// A query's pipeline returned something other than a ranking of chunks —
    /// a context or an answer — and this harness scores rankings only.
    ///
    /// The harness's state until it selects which ranking a generation
    /// pipeline is scored on and scores answers themselves: the engine can
    /// now end a pipeline on a context builder or a generator, and a run that
    /// silently dropped those outputs, or scored a ranking the pipeline did not
    /// return, would report numbers nobody asked for. Refused for every query,
    /// judged or not, so the refusal does not depend on the qrels.
    #[error(
        "query `{}`: the pipeline's output is of kind `{kind}`, and this harness scores only a ranking of chunks",
        query.as_str()
    )]
    UnscorableOutput {
        /// The query whose output cannot be scored.
        query: QueryId,
        /// The kind of what the pipeline returned instead, as a configuration
        /// names it: `"context"` or `"answer"`.
        kind: &'static str,
    },
}
