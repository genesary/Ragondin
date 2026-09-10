//! What physical planning and execution refuse, and why.
//!
//! `PlanError` is defined here rather than beside the registry because it is
//! the whole planning pass's error: resolving a name against the registry is
//! the first thing planning does, and the kind checks and `Extension`
//! resolution that follow report through the same type. `ExecError` is here
//! for the same reason, one pass later: it is the whole execution pass's
//! error, raised by the scheduler and by each node's adapter alike.

use std::fmt;

use ragondin_contracts::ComponentError;
use ragondin_pipeline::{NodeId, ParamValue, ValueKind};

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
        "node `{}` port {port} (fed by `{}`): {}found `{found}`",
        consumer.as_str(),
        producer.as_str(),
        kind_mismatch_expected_clause(expected)
    )]
    KindMismatch {
        /// The node consuming the mismatched edge.
        consumer: NodeId,
        /// The position, within `consumer`'s `inputs`, of the mismatched edge.
        port: usize,
        /// The node — or, since ADR-C18, the declared pipeline input —
        /// producing the value on the mismatched edge. Kept word for word
        /// with `ragondin_pipeline::ValidationError::KindMismatch`'s, which
        /// is what the "deliberately the same wording" claim above obliges.
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

/// What execution refuses.
///
/// **Exhaustive, deliberately**, for the reason [`PlanError`] states: this
/// enum is not on a stable boundary (INV-2), so a `match` that stops compiling
/// when a variant arrives is the intended signal that a new refusal needs
/// reporting.
///
/// Four of these variants report a **defect upstream** rather than a
/// condition to absorb — [`ExecError::KindMismatch`], [`ExecError::Cycle`],
/// [`ExecError::DanglingInput`] and [`ExecError::DuplicateNodeIds`] — and
/// their messages say so. `ragondin_pipeline::validate` refuses the shape each
/// of them names, and [`plan_physical`] refuses the first as well, so a plan
/// that came through both cannot raise them; a `LogicalPipeline` deserialized
/// straight from a store or a wire is the shape that can.
///
/// [`plan_physical`]: crate::plan_physical
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    /// The node's component was called, and failed.
    ///
    /// The node id is the whole diagnostic the engine can add: a
    /// [`ComponentError`] says what went wrong and never which node it went
    /// wrong in, because a component does not know its own node. Kept as the
    /// [`source`] as well as rendered, so a caller can walk to the cause.
    ///
    /// [`source`]: std::error::Error::source
    #[error("node `{}`: {source}", node.as_str())]
    Component {
        /// The node whose component failed.
        node: NodeId,
        /// What the component reported.
        #[source]
        source: ComponentError,
    },

    /// The node declares fewer inputs than its variant consumes.
    ///
    /// "A node has too few inputs" is deliberately unchecked at validation and
    /// at planning — both walk the `inputs` a node *has* — so ADR-C18 leaves
    /// it here. Unlike the three defect variants above, this one is reachable
    /// from a pipeline that came through `validate`, and it is a user error:
    /// a node was written without the edge it needs.
    #[error(
        "node `{}` has no input at port {port}, where a `{expected}` is required",
        consumer.as_str()
    )]
    MissingInput {
        /// The node missing an edge.
        consumer: NodeId,
        /// The position, within `consumer`'s ports, that carries no edge.
        port: usize,
        /// The kind `consumer` declares at `port`.
        expected: ValueKind,
    },

    /// An edge names a producer this plan does not have.
    ///
    /// A defect upstream: `validate` refuses a dangling `inputs` entry, and
    /// planning skips one rather than growing a second referential-integrity
    /// check, which leaves the report here.
    #[error(
        "node `{}` port {port} names `{}`, which is neither a node of this plan nor one of its declared inputs — validation rejects this wiring, so a plan holding it did not come through it",
        consumer.as_str(),
        input.as_str()
    )]
    DanglingInput {
        /// The node whose edge names nothing.
        consumer: NodeId,
        /// The position, within `consumer`'s `inputs`, of that edge.
        port: usize,
        /// The id the edge names.
        input: NodeId,
    },

    /// The value on an edge is not of the kind the consuming port declares.
    ///
    /// ADR-C16's **backstop**, and reaching it is a defect in validation or in
    /// planning, which both check this edge before execution — not a condition
    /// to absorb here. Worded to send the reader upstream.
    #[error(
        "node `{}` port {port} (fed by `{}`): expected `{expected}`, found `{found}` — validation and planning both check this edge, so reaching it at execution is a defect in one of them",
        consumer.as_str(),
        producer.as_str()
    )]
    KindMismatch {
        /// The node consuming the mismatched edge.
        consumer: NodeId,
        /// The position, within `consumer`'s `inputs`, of the mismatched edge.
        port: usize,
        /// The node — or declared pipeline input — on the other end.
        producer: NodeId,
        /// The kind `consumer` declares at `port`.
        expected: ValueKind,
        /// The kind the value actually is.
        found: ValueKind,
    },

    /// No node of this plan is terminal, so there is nothing to return.
    ///
    /// A **terminal** node is one whose output no other node consumes. A plan
    /// with none is either empty or cyclic.
    #[error("this plan has no terminal node — no node's output is left unconsumed, so nothing is the pipeline's result")]
    NoTerminalNode,

    /// Several nodes of this plan are terminal, so the result is ambiguous.
    ///
    /// See [`ExecError::NoTerminalNode`] for what terminal means. Exactly one
    /// is required: the executor returns one value, and nothing in the
    /// representation says which of several unconsumed outputs it would be.
    #[error(
        "this plan has {} terminal nodes ({}) — exactly one node's output must be left unconsumed",
        nodes.len(),
        node_list(nodes)
    )]
    MultipleTerminalNodes {
        /// The unconsumed nodes, in the plan's canonical order.
        nodes: Vec<NodeId>,
    },

    /// A per-call parameter is absent, of another kind than the executor
    /// reads, or negative.
    ///
    /// The executor reads a node's per-call keys from its `Params` (§6.3) and
    /// **invents no default**: what a component does in the absence of a
    /// parameter is the component's to decide, and a default applied here
    /// could only be a second, disagreeing copy of it.
    #[error(
        "node `{}`: the per-call parameter `{key}` must be a non-negative integer, {}",
        node.as_str(),
        param_found_clause(found)
    )]
    InvalidParam {
        /// The node whose parameter is missing or unusable.
        node: NodeId,
        /// The parameter's key.
        key: &'static str,
        /// What the node declared under that key instead, if anything.
        found: Option<ParamValue>,
    },

    /// The data-flow edges form a cycle, so none of these nodes can be
    /// scheduled.
    ///
    /// A defect upstream: `validate` rejects a cyclic pipeline. Reported
    /// rather than spun on, because a scheduler that waits for a value nothing
    /// will produce never returns.
    #[error(
        "the data-flow edges of {} form a cycle, so none of them can be scheduled — validation rejects a cyclic pipeline, so a plan holding one did not come through it",
        node_list(nodes)
    )]
    Cycle {
        /// The nodes that never became ready, in the plan's canonical order.
        nodes: Vec<NodeId>,
    },

    /// Several nodes of this plan share an id, so the scheduler can never
    /// account for all of them.
    ///
    /// A defect upstream: `validate` rejects a duplicate id. The scheduler
    /// tracks the ids it has run, and a plan holding more nodes than ids
    /// stalls with every remaining node already counted as done — a stall
    /// that is not a cycle, and is reported as what it is rather than as a
    /// cycle over no nodes.
    #[error(
        "the ids {} each name more than one node of this plan — validation rejects a duplicate id, so a plan holding one did not come through it",
        node_list(nodes)
    )]
    DuplicateNodeIds {
        /// Each id that names more than one node, once, in the plan's
        /// canonical order.
        nodes: Vec<NodeId>,
    },
}

/// Renders a node list for a message: `` `a`, `b`, `c` ``.
fn node_list(nodes: &[NodeId]) -> String {
    nodes
        .iter()
        .map(|node| format!("`{}`", node.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The part of [`ExecError::InvalidParam`]'s message that says what the node
/// declared instead. A wrong *value* is named (a `top_k` of `-1` is worth
/// reading back); a wrong *kind* is named by kind, since printing a whole
/// list into an error message helps nobody.
fn param_found_clause(found: &Option<ParamValue>) -> String {
    match found {
        None => "and this node declares none".to_string(),
        Some(ParamValue::Int(value)) => format!("and this node declares `{value}`"),
        Some(ParamValue::String(_)) => "and this node declares a string".to_string(),
        Some(ParamValue::Float(_)) => "and this node declares a float".to_string(),
        Some(ParamValue::Bool(_)) => "and this node declares a boolean".to_string(),
        Some(ParamValue::List(_)) => "and this node declares a list".to_string(),
    }
}
