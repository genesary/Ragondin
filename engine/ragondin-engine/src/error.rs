//! What physical planning refuses, and why.
//!
//! `PlanError` is defined here rather than beside the registry because it is
//! the whole planning pass's error: resolving a name against the registry is
//! the first thing planning does, and the kind checks and `Extension`
//! resolution that follow report through the same type.

use std::fmt;

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
/// dense retriever is built from, and they are resolved through the same
/// registry so that the retriever's constructor can reach them.
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
/// (INV-2), and physical planning (#15) will add variants for an unsupported
/// `Extension` and for a kind mismatch. When it does, a `match` in the binary
/// that stops compiling is the intended signal that a new refusal needs
/// reporting — which `#[non_exhaustive]` would suppress.
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
}
