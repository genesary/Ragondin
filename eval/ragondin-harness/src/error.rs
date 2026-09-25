//! [`HarnessError`]: what stops an evaluation run.

use ragondin_engine::{ExecError, PlanError};
use ragondin_experiments::TraceDocument;
use ragondin_pipeline::NodeId;
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

    /// The benchmark carries neither qrels nor reference answers, so no
    /// metric family scored any query.
    ///
    /// A mean over no query is not zero, it is undefined — and a `NaN` metric
    /// is refused by the run store at the far end of this driver. Naming the
    /// cause here reports it while the cause is still known. A benchmark that
    /// carries either piece always scores its family over at least one query
    /// (ADR-C30 § 5), so this is the regime with nothing in it.
    #[error("no query of this benchmark carries qrels or a reference answer, so no metric can be computed")]
    NothingToScore,

    /// The benchmark carries reference answers, and a query's pipeline
    /// produced no answer: its terminal node returned a ranking or a context.
    ///
    /// Refused rather than scored on the retrieval metrics alone (ADR-C30
    /// § 5): the benchmark asked for answers to be scored, and a run that
    /// dropped that family would read as a smaller evaluation reported as the
    /// whole one. Checked for every query once the benchmark carries
    /// references, judged or not, so the refusal does not depend on which
    /// queries happen to hold one.
    #[error(
        "query `{}`: the benchmark carries reference answers, and the pipeline's output is of kind `{kind}`, not an answer",
        query.as_str()
    )]
    NoAnswer {
        /// The query whose pipeline produced no answer.
        query: QueryId,
        /// What the executor recorded for that query, rendered.
        ///
        /// Carried for the reason [`HarnessError::Execute`] carries its own:
        /// what the pipeline returned instead is named in this trace and
        /// nowhere else.
        trace: TraceDocument,
        /// The kind the pipeline returned instead, as a configuration names
        /// it: `"chunks"` or `"context"`.
        kind: &'static str,
    },

    /// The benchmark carries qrels, and the ranking the retrieval metrics read
    /// could not be found for a query (ADR-C30 § 3).
    ///
    /// Never answered by falling back to the context's own chunks: that list
    /// is cut to the builder's budget, so an nDCG over it would mean one thing
    /// under one builder and another under the next. It is the mirror of
    /// [`HarnessError::NoAnswer`]: a pipeline ending in an answer with no
    /// ranking behind it, run over a benchmark that asks for one. No pipeline
    /// `ragondin-pipeline` validates reaches it through the engine as M3
    /// builds it, since a generator's context port only accepts a context and
    /// a builder's chunks port only a ranking; it is stated so that a pipeline
    /// that does is refused by name rather than scored on something else.
    #[error("query `{}`: no ranking to score the benchmark's qrels on: {walk}", query.as_str())]
    NoRanking {
        /// The query whose ranking was not found.
        query: QueryId,
        /// What the executor recorded for that query, rendered.
        trace: TraceDocument,
        /// Where the walk to the ranking stopped.
        walk: RankingWalkError,
    },
}

/// Where the walk from a pipeline's terminal node to the ranking behind it
/// stopped (ADR-C30 § 3).
///
/// The walk goes by port position, never by node name: a generator's context
/// port (`inputs[1]`) names its context builder, and a builder's chunks port
/// (`inputs[1]`) names the node whose output entry in the trace is the ranking.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RankingWalkError {
    /// The pipeline has no single terminal node — no node, or several, that no
    /// other node consumes. The executor refuses such a plan before this walk
    /// could run, so it is named for completeness rather than expected.
    #[error("the pipeline has no single terminal node")]
    NoTerminalNode,

    /// A generator's or a context builder's port the walk reads is absent.
    #[error("node `{}` has no input on port {port}", node.as_str())]
    MissingPort {
        /// The node whose port is absent.
        node: NodeId,
        /// The port position the walk reads.
        port: usize,
    },

    /// The generator's context port names something that is not a context
    /// builder.
    #[error(
        "generator `{}` takes its context from `{}`, which is not a context builder",
        generator.as_str(),
        context.as_str()
    )]
    ContextNotFromBuilder {
        /// The terminal generator.
        generator: NodeId,
        /// What its context port names instead.
        context: NodeId,
    },

    /// The node the ranking is read from has no ranking as its output entry in
    /// the trace — it is no node at all, it has no entry, or its entry is of
    /// another kind.
    #[error("`{}` has no ranked chunks as its output in the trace", node.as_str())]
    NoRankedChunks {
        /// The node, or the pipeline input, the walk arrived at.
        node: NodeId,
    },
}
