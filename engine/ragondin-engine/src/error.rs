//! What physical planning refuses, and why.
//!
//! `PlanError` is defined here rather than beside the registry because it is
//! the whole planning pass's error: resolving a name against the registry is
//! the first thing planning does, and the kind checks and `Extension`
//! resolution that follow report through the same type.

use std::fmt;

use ragondin_pipeline::{NodeId, ValueKind};

/// The component families the registry keeps one table for.
///
/// Present in every [`PlanError`] the registry raises because the family is
/// part of the lookup, not just of the message: `impl: bm25` under a `fusion:`
/// node is a different question from `impl: bm25` under a `retriever:` node,
/// and an error that named only `bm25` would send the reader looking for a
/// missing registration that is in fact present under another family.
///
/// Wider than [`ragondin_pipeline::LogicalNode`]'s variants, and deliberately:
/// `Embedder` and `VectorStore` are not pipeline nodes. They are components a
/// dense retriever is built from, registered through the same public
/// `register_*` API as every other family (INV-7) and **injected into that
/// retriever at the composition root** (#31), whose registration closure
/// captures them.
///
/// Nothing resolves one from a node, and nothing can: physical planning matches
/// a `Retriever`, a `Fusion` and a `Reranker`, and a [`crate::ComponentCtor`]
/// receives the node's `Params` and never the [`crate::EngineContext`]. So both
/// families are here for the error rather than for a lookup — the registry
/// keeps one table per family, and the family is part of every name it fails to
/// find — and their tables have no consumer in planning today, nor do
/// `build_embedder` and `build_vector_store` outside tests. That is an open
/// question, not a settled design: how a `Remote` embedder or vector store is
/// built is #101. Recorded here so it is read as a gap rather than inferred
/// from an unused table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentFamily {
    /// [`ragondin_contracts::Retriever`].
    Retriever,
    /// [`ragondin_contracts::Fusion`].
    Fusion,
    /// [`ragondin_contracts::Reranker`].
    Reranker,
    /// [`ragondin_contracts::Embedder`].
    Embedder,
    /// [`ragondin_contracts::VectorStore`].
    VectorStore,
}

impl fmt::Display for ComponentFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Retriever => "retriever",
            Self::Fusion => "fusion",
            Self::Reranker => "reranker",
            Self::Embedder => "embedder",
            Self::VectorStore => "vector store",
        };
        f.write_str(name)
    }
}

/// What a constructor reports when it cannot build its component.
///
/// Boxed rather than a fixed enum because the reason belongs to the component,
/// not to the engine: a missing ONNX model file, an unparseable parameter, a
/// store client that cannot resolve its URL. The registry wraps whatever
/// arrives in [`PlanError::Construction`], adding the family and the name —
/// the two things it knows and the constructor does not.
pub type ConstructionError = Box<dyn std::error::Error + Send + Sync>;

/// A configuration that cannot be turned into something executable.
///
/// Typed, per the error convention (`thiserror` in libraries, `anyhow` only in
/// binaries): the binary reporting this to a user needs to tell an unregistered
/// name apart from a component that refused its configuration, and those two
/// send the reader to different places.
///
/// **Exhaustive, deliberately** — the opposite choice from
/// [`ragondin_contracts::ComponentError`], for the reason `ragondin-pipeline`
/// makes it for `ValidationError`: this enum is not on a stable boundary
/// (INV-2), so a `match` in the binary that stops compiling when a variant
/// arrives is the intended signal that a new refusal needs reporting — which
/// `#[non_exhaustive]` would suppress. Physical planning added the last two
/// that way.
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    /// No implementation is registered under this name for this family.
    ///
    /// The name comes from a configuration's `impl:` value, so this is a user
    /// error — a typo, or a component the binary did not register — not an
    /// internal failure.
    #[error("no {family} implementation is registered under `{name}`")]
    UnknownImpl {
        /// The family whose registry was consulted.
        family: ComponentFamily,
        /// The `impl:` name that was looked up.
        name: String,
    },

    /// The implementation was found, and refused to be built.
    ///
    /// The cause is rendered in `Display` **and** kept as the [`source`], the
    /// same way [`ragondin_contracts::ComponentError::Backend`] handles the
    /// error it wraps: a caller can walk to the cause, and a caller that only
    /// prints this one does not lose it. Which parameter a constructor refused
    /// is the whole diagnostic here — a message saying only that construction
    /// failed sends the reader nowhere.
    ///
    /// [`source`]: std::error::Error::source
    #[error("the {family} implementation `{name}` could not be constructed: {source}")]
    Construction {
        /// The family whose registry was consulted.
        family: ComponentFamily,
        /// The `impl:` name that was looked up.
        name: String,
        /// What the constructor reported.
        #[source]
        source: ConstructionError,
    },

    /// The node is an `Extension`, and nothing in this build can plan one.
    ///
    /// ADR-C16 reserves for physical planning the kind check an `Extension`
    /// node needs, "the only point at which an `Extension` node's kinds are
    /// known" — because the registry knows them. It does not: §8.1 describes
    /// one registry per component *family*, keyed on the implementation name,
    /// and **there is no extension family** (a point `ragondin-pipeline`'s
    /// `ExtensionNode::kind` already records). How an extension is looked up
    /// is unsettled — #93 is the decision issue — so this build plans none, and
    /// says so in one error rather than guessing a mechanism.
    ///
    /// Naming the extension type and not only the node id is what makes this
    /// actionable: it is the word a reader takes to the issue tracker.
    #[error(
        "node `{}`: no physical planner for extension type `{kind}`",
        node.as_str()
    )]
    ExtensionUnsupported {
        /// The node that cannot be planned.
        node: NodeId,
        /// Its [`ragondin_pipeline::ExtensionNode::kind`], e.g. `"hyde"`.
        kind: String,
    },

    /// An edge's value kinds do not line up (ADR-C16), caught at planning.
    ///
    /// Mirrors `ragondin_pipeline::ValidationError::KindMismatch` — the same
    /// edge, the same three facts, deliberately the same wording — because a
    /// reader should not have to tell the two layers apart to read the fault.
    /// `expected` is `None` when `port` is beyond what a fixed-arity variant
    /// declares: no port exists to compare against, only an edge that should
    /// not.
    ///
    /// Reaching this means the pipeline did **not** come through
    /// `ragondin_pipeline::validate`, which refuses the same wiring earlier: a
    /// `LogicalPipeline` deserialized straight from a store or a wire is the
    /// shape that gets here. It is not, in this build, the `Extension` case
    /// ADR-C16 wrote the layer for — see [`PlanError::ExtensionUnsupported`].
    // `thiserror`'s `#[error(...)]` cannot branch on a field's value, so the
    // clause that differs between `Some` and `None` is built by a function and
    // interpolated as one fragment.
    #[error(
        "node `{}` port {port} (fed by node `{}`): {}found `{found}`",
        consumer.as_str(),
        producer.as_str(),
        kind_mismatch_expected_clause(expected)
    )]
    KindMismatch {
        /// The node consuming the mismatched edge.
        consumer: NodeId,
        /// The position, within `consumer`'s `inputs`, of the mismatched edge.
        port: usize,
        /// The node producing the value on the mismatched edge.
        producer: NodeId,
        /// The kind `consumer` declares at `port`, or `None` when `port` is
        /// beyond what a fixed-arity variant declares.
        expected: Option<ValueKind>,
        /// The kind `producer` actually produces.
        found: ValueKind,
    },
}

/// The part of [`PlanError::KindMismatch`]'s message that depends on whether a
/// port exists at that position at all. Kept identical to
/// `ragondin-pipeline`'s helper of the same name: the two layers report one
/// fault, and a reader who has seen one message must recognise the other.
fn kind_mismatch_expected_clause(expected: &Option<ValueKind>) -> String {
    match expected {
        Some(expected) => format!("expected `{expected}`, "),
        None => "no port declared at this position, ".to_string(),
    }
}
