//! The executor: runs a [`PhysicalPipeline`] and **returns** its
//! [`ExecutionTrace`] (INV-10, ADR-C9).
//!
//! The trace is a return value, never a log: no `tracing` macro appears in
//! this crate, and the signature pairs the trace with the result rather than
//! wrapping it, so a failed run is inspectable up to the node that failed.
//!
//! # How a plan runs
//!
//! Nodes are scheduled in a **topological order over the data-flow edges** —
//! a node runs once every id its `inputs` name holds a value — and never in
//! the order the plan stores them. That order is canonical (sorted by
//! [`NodeId`], for the content hash) and is not an execution order: a fusion
//! sorts before the retrievers it consumes whenever its id does. Among the
//! nodes that are ready, the canonical order breaks the tie, so one plan and
//! one query always execute in the same order. Nodes run one at a time; the
//! concurrency two independent retrieval legs would allow is not built.
//!
//! Each edge carries an erased [`NodeValue`] (ADR-C16), and each node's
//! adapter destructures the value it expects, calls the component with typed
//! arguments, and re-wraps the typed result. `NodeValue` is confined to this
//! crate.
//!
//! The value table is seeded from the pipeline's **declared inputs**
//! (ADR-C18): each receives `NodeValue::Query(input)`. `validate` admits
//! exactly one, and this pass does not re-check that arity — duplicating a
//! check the layer above owns is what ADR-C16 warns against.
//!
//! # Two rules this module fixes
//!
//! **The terminal node.** A node is **terminal** when no other node of the
//! plan consumes it, and a plan must have exactly one: the executor returns
//! one value, and nothing in the representation says which of several
//! unconsumed outputs that would be. Zero is [`ExecError::NoTerminalNode`],
//! more than one is [`ExecError::MultipleTerminalNodes`]. The M2 [`Output`]
//! is that node's `Vec<ScoredChunk>`.
//!
//! **Per-call parameters.** A node's params are split at the seam
//! (`docs/code-architecture.md` §6.3): the constructor already took the half
//! that configures the implementation, and the executor reads the half that
//! varies per call. In M2 that half is one key — `top_k`, on a retriever and
//! on a reranker — read as a [`ParamValue::Int`] and refused as
//! [`ExecError::InvalidParam`] when it is absent, of another kind, or
//! negative. **No default is invented here**: what a component does without a
//! parameter is the component's to decide (§8.1), and a default applied here
//! could only be a second, disagreeing copy of it. A fusion's `FusionParams`
//! carries no key, so nothing is read for one.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ragondin_contracts::{FusionParams, RerankParams, RetrieveParams};
use ragondin_pipeline::{LogicalNode, NodeId, ParamValue, Params, ValueKind};
use ragondin_types::{Query, ScoredChunk};

use crate::error::ExecError;
use crate::plan::{PhysicalNode, PhysicalPipeline, ResolvedComponent};
use crate::trace::{ExecutionTrace, NodeTrace, ValueSummary};

/// What a pipeline returns to its caller.
///
/// In M2 a pipeline retrieves, so its output is its terminal node's ranked
/// chunks. It is an alias rather than a struct because there is nothing yet to
/// carry beside them; a generation-shaped output arrives with M3, and this
/// crate is internal (INV-2), so widening it then breaks nobody's API.
pub type Output = Vec<ScoredChunk>;

/// The erased value travelling along one edge of a plan (ADR-C16).
///
/// **Confined to this crate**, as ADR-C16 requires: it must never appear in
/// `ragondin-types`, `ragondin-pipeline` or `ragondin-contracts`. Component
/// authors never see it — each node's adapter below destructures it and calls
/// the component with typed arguments.
///
/// A closed enum rather than `Box<dyn Any>`: a kind added in M3 turns every
/// adapter that does not handle it into a compiler error, which a downcast
/// would turn into a runtime string.
pub(crate) enum NodeValue {
    /// The query, produced only by a declared pipeline input (ADR-C18).
    Query(Query),
    /// A ranked list of chunks.
    Chunks(Vec<ScoredChunk>),
}

/// The value each producer has produced so far, keyed by its id.
type Table = HashMap<NodeId, NodeValue>;

/// The key of the only per-call parameter this build reads. See the module
/// documentation for the rule.
const TOP_K: &str = "top_k";

/// The executor.
///
/// It carries no state: a plan holds its own constructed components and a
/// query arrives per call, so two runs through one `Engine` share nothing.
/// It exists as a type rather than as a free function because
/// `docs/code-architecture.md` §8.2 gives the executor a receiver, and what a
/// later milestone gives it to hold — a cache, a clock — belongs on it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Engine;

impl Engine {
    /// An executor.
    pub fn new() -> Self {
        Self
    }

    /// Runs `plan` over `input`, returning what it produced **and** the trace
    /// of how it did.
    ///
    /// The pair is deliberate, and is INV-10 in the signature: a
    /// `Result<(Output, ExecutionTrace), ExecError>` would discard the trace
    /// on exactly the runs whose trace is worth reading. What ran before the
    /// failure is in the returned trace, with the failing node last.
    pub async fn execute(
        &self,
        plan: &PhysicalPipeline,
        input: Query,
    ) -> (Result<Output, ExecError>, ExecutionTrace) {
        let mut trace = ExecutionTrace::default();
        let output = run(plan, input, &mut trace).await;
        (output, trace)
    }
}

/// Runs the plan, appending to `trace` as it goes.
///
/// Split from [`Engine::execute`] so that the trace is built through `&mut`
/// and returned whatever this function does with `?`: an early return here
/// cannot drop what has already been recorded.
async fn run(
    plan: &PhysicalPipeline,
    input: Query,
    trace: &mut ExecutionTrace,
) -> Result<Output, ExecError> {
    // Before anything runs: a plan with no single output has nothing to
    // return, and finding that out after calling components would waste the
    // calls.
    let terminal = terminal_node(plan)?;

    let mut table = Table::new();
    for declared in plan.inputs() {
        table.insert(declared.clone(), NodeValue::Query(input.clone()));
    }

    let mut done: HashSet<&NodeId> = HashSet::new();
    while done.len() < plan.nodes().len() {
        // The topological step: the first node, in canonical order, that has
        // not run and whose every input is in the table. Canonical order is
        // the tie-break only — readiness is what decides.
        let Some(next) = plan
            .nodes()
            .iter()
            .find(|node| !done.contains(node.logical().id()) && is_ready(node, &table))
        else {
            return Err(stalled(plan, &table, &done));
        };

        let id = next.logical().id();
        let inputs = summarize_inputs(next, &table);
        let started = Instant::now();
        let outcome = call(next, &table).await;
        let duration = started.elapsed();

        match outcome {
            Ok(value) => {
                trace.nodes.push(NodeTrace {
                    node: id.clone(),
                    inputs,
                    output: Some(ValueSummary::of(&value)),
                    duration,
                    error: None,
                });
                table.insert(id.clone(), value);
                done.insert(id);
            }
            Err(err) => {
                trace.nodes.push(NodeTrace {
                    node: id.clone(),
                    inputs,
                    output: None,
                    duration,
                    error: Some(err.to_string()),
                });
                return Err(err);
            }
        }
    }

    match table.remove(&terminal) {
        Some(NodeValue::Chunks(chunks)) => Ok(chunks),
        // Exhaustive on purpose (ADR-C16): a kind added in M3 that a pipeline
        // can end on must be handled here, and this match is where the
        // compiler says so.
        Some(NodeValue::Query(_)) => {
            unreachable!("no node produces a query; only a declared pipeline input does")
        }
        None => unreachable!("the terminal node is a node of the plan, and every node ran"),
    }
}

/// The one node no other node consumes — see the module documentation for the
/// rule this enforces.
fn terminal_node(plan: &PhysicalPipeline) -> Result<NodeId, ExecError> {
    let consumed: HashSet<&NodeId> = plan
        .nodes()
        .iter()
        .flat_map(|node| node.logical().inputs())
        .collect();
    let terminals: Vec<NodeId> = plan
        .nodes()
        .iter()
        .map(|node| node.logical().id())
        .filter(|id| !consumed.contains(id))
        .cloned()
        .collect();

    match terminals.as_slice() {
        [terminal] => Ok(terminal.clone()),
        [] => Err(ExecError::NoTerminalNode),
        _ => Err(ExecError::MultipleTerminalNodes { nodes: terminals }),
    }
}

/// Whether every id this node's `inputs` name already holds a value.
fn is_ready(node: &PhysicalNode, table: &Table) -> bool {
    node.logical()
        .inputs()
        .iter()
        .all(|input| table.contains_key(input))
}

/// Why no node is ready while nodes remain.
///
/// A **dangling** edge outranks a cycle: an id naming nothing is a definite
/// fault, while "not ready" is the shape a cycle shares with a node merely
/// waiting behind one. Reporting the cycle first would misname a plan whose
/// only defect is a typo in an id.
///
/// A stall with **no node remaining** is neither: `done` is a set of ids, so a
/// plan whose nodes outnumber its ids can never reach its node count, and
/// every node it holds is already counted. That is a duplicate id, reported
/// as one rather than as a cycle over an empty list.
fn stalled(plan: &PhysicalPipeline, table: &Table, done: &HashSet<&NodeId>) -> ExecError {
    let known: HashSet<&NodeId> = plan
        .nodes()
        .iter()
        .map(|node| node.logical().id())
        .collect();
    let remaining: Vec<&PhysicalNode> = plan
        .nodes()
        .iter()
        .filter(|node| !done.contains(node.logical().id()))
        .collect();

    if remaining.is_empty() {
        return ExecError::DuplicateNodeIds {
            nodes: duplicate_ids(plan),
        };
    }

    for node in &remaining {
        for (port, input) in node.logical().inputs().iter().enumerate() {
            if !table.contains_key(input) && !known.contains(input) {
                return ExecError::DanglingInput {
                    consumer: node.logical().id().clone(),
                    port,
                    input: input.clone(),
                };
            }
        }
    }

    ExecError::Cycle {
        nodes: remaining
            .iter()
            .map(|node| node.logical().id().clone())
            .collect(),
    }
}

/// Each id that names more than one node of `plan`, once, in canonical order.
fn duplicate_ids(plan: &PhysicalPipeline) -> Vec<NodeId> {
    let mut seen: HashSet<&NodeId> = HashSet::new();
    let mut duplicates: Vec<NodeId> = Vec::new();
    for id in plan.nodes().iter().map(|node| node.logical().id()) {
        if !seen.insert(id) && !duplicates.contains(id) {
            duplicates.push(id.clone());
        }
    }
    duplicates
}

/// What this node is about to receive, in port order.
///
/// Read before the call rather than after: a node that fails still records
/// what it was given, which is what makes a failed run diagnosable.
fn summarize_inputs(node: &PhysicalNode, table: &Table) -> Vec<ValueSummary> {
    node.logical()
        .inputs()
        .iter()
        .filter_map(|input| table.get(input))
        .map(ValueSummary::of)
        .collect()
}

/// Calls one node's component: the adapter ADR-C16 describes.
///
/// It destructures the erased value on each port, builds the typed per-call
/// params from the node's own `Params`, and re-wraps the typed result. The
/// match is on the node's **variant** and never on its implementation name
/// (INV-7).
///
/// **Exhaustive over [`LogicalNode`], on purpose.** ADR-C16 chose a closed
/// enum so that a kind added later turns every site that does not handle it
/// into a compiler error; a match over the `(node, component)` pair with a
/// catch-all arm would turn it into a runtime panic instead. So the variant is
/// matched first and without a wildcard — a `LogicalNode` variant added in M3
/// (#93) fails to compile here — and the component is destructured inside
/// each arm, where the only other pairing is the one planning rules out.
async fn call(node: &PhysicalNode, table: &Table) -> Result<NodeValue, ExecError> {
    match node.logical() {
        LogicalNode::Retriever(logical) => {
            let ResolvedComponent::Retriever(component) = node.component() else {
                unreachable!("planning resolves a Retriever node through the retriever registry")
            };
            let query = query_at(&logical.id, &logical.inputs, 0, table)?;
            let params = RetrieveParams::new(per_call_top_k(&logical.id, &logical.params)?);
            let chunks = component
                .retrieve(query, &params)
                .await
                .map_err(|source| component_failed(&logical.id, source))?;
            Ok(NodeValue::Chunks(chunks))
        }
        LogicalNode::Fusion(logical) => {
            let ResolvedComponent::Fusion(component) = node.component() else {
                unreachable!("planning resolves a Fusion node through the fusion registry")
            };
            // Variadic: every port is a leg, and the legs reach the component
            // in the order the pipeline wires them, which `Fusion::fuse`'s
            // contract requires.
            let mut legs = Vec::with_capacity(logical.inputs.len());
            for port in 0..logical.inputs.len() {
                legs.push(chunks_at(&logical.id, &logical.inputs, port, table)?.to_vec());
            }
            let chunks = component
                .fuse(legs, &FusionParams::new())
                .await
                .map_err(|source| component_failed(&logical.id, source))?;
            Ok(NodeValue::Chunks(chunks))
        }
        LogicalNode::Reranker(logical) => {
            let ResolvedComponent::Reranker(component) = node.component() else {
                unreachable!("planning resolves a Reranker node through the reranker registry")
            };
            let query = query_at(&logical.id, &logical.inputs, 0, table)?;
            let chunks = chunks_at(&logical.id, &logical.inputs, 1, table)?.to_vec();
            let params = RerankParams::new(per_call_top_k(&logical.id, &logical.params)?);
            let reranked = component
                .rerank(query, chunks, &params)
                .await
                .map_err(|source| component_failed(&logical.id, source))?;
            Ok(NodeValue::Chunks(reranked))
        }
        // `PhysicalNode` is built in one place, and `plan_physical` refuses
        // every `Extension` before it builds any — so no plan holds one, and
        // `ResolvedComponent` has no variant it could carry.
        LogicalNode::Extension(_) => {
            unreachable!("`plan_physical` refuses every Extension node, so no plan holds one")
        }
    }
}

/// Adds the node id a [`ragondin_contracts::ComponentError`] cannot carry.
fn component_failed(node: &NodeId, source: ragondin_contracts::ComponentError) -> ExecError {
    ExecError::Component {
        node: node.clone(),
        source,
    }
}

/// The query on `port`, or the typed refusal.
fn query_at<'t>(
    consumer: &NodeId,
    inputs: &[NodeId],
    port: usize,
    table: &'t Table,
) -> Result<&'t Query, ExecError> {
    let producer = producer_at(consumer, inputs, port, ValueKind::Query)?;
    match value_of(producer, table) {
        NodeValue::Query(query) => Ok(query),
        NodeValue::Chunks(_) => Err(ExecError::KindMismatch {
            consumer: consumer.clone(),
            port,
            producer: producer.clone(),
            expected: ValueKind::Query,
            found: ValueKind::Chunks,
        }),
    }
}

/// The chunk list on `port`, or the typed refusal.
fn chunks_at<'t>(
    consumer: &NodeId,
    inputs: &[NodeId],
    port: usize,
    table: &'t Table,
) -> Result<&'t [ScoredChunk], ExecError> {
    let producer = producer_at(consumer, inputs, port, ValueKind::Chunks)?;
    match value_of(producer, table) {
        NodeValue::Chunks(chunks) => Ok(chunks),
        NodeValue::Query(_) => Err(ExecError::KindMismatch {
            consumer: consumer.clone(),
            port,
            producer: producer.clone(),
            expected: ValueKind::Chunks,
            found: ValueKind::Query,
        }),
    }
}

/// The id feeding `port`, or [`ExecError::MissingInput`] when the node
/// declares no edge there.
fn producer_at<'i>(
    consumer: &NodeId,
    inputs: &'i [NodeId],
    port: usize,
    expected: ValueKind,
) -> Result<&'i NodeId, ExecError> {
    inputs.get(port).ok_or_else(|| ExecError::MissingInput {
        consumer: consumer.clone(),
        port,
        expected,
    })
}

/// The value a producer left in the table.
fn value_of<'t>(producer: &NodeId, table: &'t Table) -> &'t NodeValue {
    table
        .get(producer)
        .expect("a node runs only once every id its `inputs` name holds a value")
}

/// The per-call `top_k` a node declares. See the module documentation for the
/// rule, including why no default is applied here.
fn per_call_top_k(node: &NodeId, params: &Params) -> Result<usize, ExecError> {
    let found = params.get(TOP_K);
    let refuse = || ExecError::InvalidParam {
        node: node.clone(),
        key: TOP_K,
        found: found.cloned(),
    };
    match found {
        // `top_k` is a `usize` on the contract, and `-1 as usize` is a very
        // large count rather than an error, so the conversion is checked
        // rather than cast.
        Some(ParamValue::Int(value)) => usize::try_from(*value).map_err(|_| refuse()),
        _ => Err(refuse()),
    }
}

// D-11, as `context.rs` applies it to `EngineContext`: the harness (#29) runs
// two plans from one context across tasks, so what `execute` takes and returns
// must cross a `tokio::spawn` boundary. Asserted next to what it constrains
// rather than in a test whose deletion would remove it silently. A
// `PhysicalPipeline` is `Send + Sync` because every contract trait is; an
// `ExecError` because `ComponentError` is.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Engine>();
    assert_send_sync::<PhysicalPipeline>();
    assert_send_sync::<ExecutionTrace>();
    assert_send_sync::<ExecError>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ragondin_contracts::{ComponentError, Fusion, Reranker, Retriever};
    use ragondin_pipeline::{
        validate, LogicalPipeline, RawGraph, RawNode, RawParamValue, RawPipeline, SchemaVersion,
    };
    use ragondin_types::{Chunk, ChunkId, DocId, QueryId};
    use std::collections::BTreeMap;
    use std::time::Duration;

    use crate::context::EngineContext;
    use crate::plan::plan_physical;

    fn chunk(id: &str) -> Chunk {
        Chunk {
            id: ChunkId::new(id),
            text: format!("text of {id}"),
            document_id: DocId::new("doc"),
        }
    }

    fn scored(id: &str, score: f32) -> ScoredChunk {
        ScoredChunk {
            chunk: chunk(id),
            score,
        }
    }

    fn query() -> Query {
        Query {
            id: QueryId::new("q1"),
            text: "why".to_string(),
        }
    }

    /// Returns a fixed ranked list, truncated to the per-call `top_k`.
    struct ListRetriever {
        ids: Vec<&'static str>,
    }

    #[async_trait]
    impl Retriever for ListRetriever {
        async fn retrieve(
            &self,
            _query: &Query,
            params: &RetrieveParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            Ok(self
                .ids
                .iter()
                .take(params.top_k)
                .enumerate()
                .map(|(rank, id)| scored(id, 1.0 - rank as f32 / 10.0))
                .collect())
        }
    }

    /// Returns exactly `top_k` chunks, so a test can read the per-call
    /// parameter back out of the result.
    struct CountingRetriever;

    #[async_trait]
    impl Retriever for CountingRetriever {
        async fn retrieve(
            &self,
            _query: &Query,
            params: &RetrieveParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            Ok((0..params.top_k)
                .map(|i| scored(&format!("hit-{i}"), 1.0 - i as f32 / 10.0))
                .collect())
        }
    }

    /// Always fails, the way an unreachable `Remote` component would.
    struct OfflineRetriever;

    #[async_trait]
    impl Retriever for OfflineRetriever {
        async fn retrieve(
            &self,
            _query: &Query,
            _params: &RetrieveParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            Err(ComponentError::Unavailable("the index is offline".into()))
        }
    }

    /// Answers after a delay, the way a `Remote` component waits on the wire.
    ///
    /// What a trace's `duration` measures is only observable through a
    /// component that takes measurable time: every other stub here answers
    /// in microseconds, and a zeroed duration would pass beside them.
    struct SlowRetriever;

    #[async_trait]
    impl Retriever for SlowRetriever {
        async fn retrieve(
            &self,
            _query: &Query,
            _params: &RetrieveParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            tokio::time::sleep(SLOW_RETRIEVER_DELAY).await;
            Ok(vec![scored("c1", 1.0)])
        }
    }

    const SLOW_RETRIEVER_DELAY: Duration = Duration::from_millis(20);

    /// Concatenates its legs in the order they arrive.
    ///
    /// **Order-sensitive on purpose**: `RrfFusion` below scores each chunk by
    /// its rank within a leg and never by which leg it came from, so `[a, b]`
    /// and `[b, a]` fuse identically through it. Only a fusion whose output
    /// depends on leg order can observe that the adapter hands the legs over
    /// in wiring order, which `Fusion::fuse`'s contract requires.
    struct ConcatFusion;

    #[async_trait]
    impl Fusion for ConcatFusion {
        async fn fuse(
            &self,
            inputs: Vec<Vec<ScoredChunk>>,
            _params: &FusionParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            Ok(inputs.into_iter().flatten().collect())
        }
    }

    /// Reciprocal rank fusion over its legs, with `k = 60`.
    struct RrfFusion;

    #[async_trait]
    impl Fusion for RrfFusion {
        async fn fuse(
            &self,
            inputs: Vec<Vec<ScoredChunk>>,
            _params: &FusionParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            let mut fused: BTreeMap<String, (ScoredChunk, f32)> = BTreeMap::new();
            for leg in inputs {
                for (rank, hit) in leg.into_iter().enumerate() {
                    let contribution = 1.0 / (60.0 + rank as f32 + 1.0);
                    let entry = fused
                        .entry(hit.chunk.id.as_str().to_string())
                        .or_insert((hit, 0.0));
                    entry.1 += contribution;
                }
            }
            let mut out: Vec<ScoredChunk> = fused
                .into_values()
                .map(|(mut hit, score)| {
                    hit.score = score;
                    hit
                })
                .collect();
            out.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .expect("the stub produces finite scores")
                    .then_with(|| a.chunk.id.as_str().cmp(b.chunk.id.as_str()))
            });
            Ok(out)
        }
    }

    /// Reorders by chunk id, descending, then keeps the per-call `top_k`.
    struct ByIdReranker;

    #[async_trait]
    impl Reranker for ByIdReranker {
        async fn rerank(
            &self,
            _query: &Query,
            mut chunks: Vec<ScoredChunk>,
            params: &RerankParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            chunks.sort_by(|a, b| b.chunk.id.as_str().cmp(a.chunk.id.as_str()));
            chunks.truncate(params.top_k);
            for (rank, hit) in chunks.iter_mut().enumerate() {
                hit.score = 1.0 - rank as f32 / 10.0;
            }
            Ok(chunks)
        }
    }

    /// A context registering one implementation per family a test names.
    fn context() -> EngineContext {
        let mut ctx = EngineContext::new();
        ctx.register_retriever(
            "bm25",
            Box::new(|_| {
                Ok(Box::new(ListRetriever {
                    ids: vec!["c1", "c2", "c3"],
                }))
            }),
        );
        ctx.register_retriever(
            "dense",
            Box::new(|_| {
                Ok(Box::new(ListRetriever {
                    ids: vec!["c3", "c1", "c4"],
                }))
            }),
        );
        ctx.register_retriever("counting", Box::new(|_| Ok(Box::new(CountingRetriever))));
        ctx.register_retriever("offline", Box::new(|_| Ok(Box::new(OfflineRetriever))));
        ctx.register_retriever("slow", Box::new(|_| Ok(Box::new(SlowRetriever))));
        ctx.register_fusion("rrf", Box::new(|_| Ok(Box::new(RrfFusion))));
        ctx.register_fusion("concat", Box::new(|_| Ok(Box::new(ConcatFusion))));
        ctx.register_reranker("by_id", Box::new(|_| Ok(Box::new(ByIdReranker))));
        ctx
    }

    fn raw(id: &str, component: &str, implementation: &str, inputs: &[&str]) -> RawNode {
        RawNode {
            id: id.to_string(),
            component: component.to_string(),
            implementation: implementation.to_string(),
            inputs: inputs.iter().map(|i| (*i).to_string()).collect(),
            params: BTreeMap::new(),
        }
    }

    /// A node carrying the per-call parameter the executor reads.
    fn raw_top_k(
        id: &str,
        component: &str,
        implementation: &str,
        inputs: &[&str],
        top_k: RawParamValue,
    ) -> RawNode {
        let mut node = raw(id, component, implementation, inputs);
        node.params.insert("top_k".to_string(), top_k);
        node
    }

    /// The legitimate way to a `LogicalPipeline`: through validation.
    fn logical(nodes: Vec<RawNode>) -> LogicalPipeline {
        validate(RawPipeline {
            version: SchemaVersion::CURRENT,
            pipeline: RawGraph {
                inputs: vec!["question".to_string()],
                nodes,
            },
        })
        .expect("the fixture must validate")
    }

    /// A `LogicalPipeline` `validate` would have refused — the only door to
    /// one, since `validate` is the only other way to hold one and it checks.
    /// See `plan.rs`'s helper of the same name; a fixture must be written in
    /// `NodeId` order, because this path does no canonicalization either.
    fn forged(json: &str) -> LogicalPipeline {
        serde_json::from_str(json).expect("the forged JSON must match LogicalPipeline's shape")
    }

    fn plan(nodes: Vec<RawNode>) -> PhysicalPipeline {
        plan_physical(&logical(nodes), &context()).expect("the fixture must plan")
    }

    fn plan_forged(json: &str) -> PhysicalPipeline {
        plan_physical(&forged(json), &context()).expect("the fixture must plan")
    }

    fn ids(chunks: &[ScoredChunk]) -> Vec<&str> {
        chunks.iter().map(|hit| hit.chunk.id.as_str()).collect()
    }

    fn traced(trace: &ExecutionTrace) -> Vec<&str> {
        trace.nodes.iter().map(|node| node.node.as_str()).collect()
    }

    #[tokio::test]
    async fn a_hybrid_pipeline_returns_its_fusions_result_and_one_trace_per_node() {
        // The acceptance criterion: two retrieval legs into an RRF fusion.
        // `bm25` ranks c1, c2, c3 and `dense` ranks c3, c1, c4, so RRF with
        // k = 60 puts c1 first (two high ranks), then c3, then the singletons
        // in rank order — an order neither leg produces, which is what makes
        // "the output is the fusion's result" observable.
        let plan = plan(vec![
            raw_top_k(
                "bm25_leg",
                "retriever",
                "bm25",
                &["question"],
                RawParamValue::Int(3),
            ),
            raw_top_k(
                "dense_leg",
                "retriever",
                "dense",
                &["question"],
                RawParamValue::Int(3),
            ),
            raw("fuse", "fusion", "rrf", &["bm25_leg", "dense_leg"]),
        ]);

        let (output, trace) = Engine::new().execute(&plan, query()).await;

        let output = output.expect("every node of this plan can run");
        assert_eq!(
            ids(&output),
            vec!["c1", "c3", "c2", "c4"],
            "the pipeline's output is the terminal fusion's result"
        );
        assert_eq!(
            trace.nodes.len(),
            3,
            "the trace holds one entry per node: {:?}",
            traced(&trace)
        );
        let mut traced_ids = traced(&trace);
        traced_ids.sort_unstable();
        assert_eq!(traced_ids, vec!["bm25_leg", "dense_leg", "fuse"]);
    }

    #[tokio::test]
    async fn a_fusion_receives_its_legs_in_wiring_order() {
        // `Fusion::fuse`'s contract: the legs arrive in the order the node
        // wires them. `RrfFusion` cannot tell `[a, b]` from `[b, a]`, so the
        // acceptance test above passes with the legs reversed; `concat` can,
        // and two plans that differ only in wiring order must fuse differently.
        let legs = |first: &str, second: &str| {
            plan(vec![
                raw_top_k(
                    "bm25_leg",
                    "retriever",
                    "bm25",
                    &["question"],
                    RawParamValue::Int(3),
                ),
                raw_top_k(
                    "dense_leg",
                    "retriever",
                    "dense",
                    &["question"],
                    RawParamValue::Int(3),
                ),
                raw("fuse", "fusion", "concat", &[first, second]),
            ])
        };
        let engine = Engine::new();

        let (bm25_first, _) = engine
            .execute(&legs("bm25_leg", "dense_leg"), query())
            .await;
        let (dense_first, _) = engine
            .execute(&legs("dense_leg", "bm25_leg"), query())
            .await;

        assert_eq!(
            ids(&bm25_first.expect("every node of this plan can run")),
            vec!["c1", "c2", "c3", "c3", "c1", "c4"],
            "port 0 is `bm25_leg`, so its chunks come first"
        );
        assert_eq!(
            ids(&dense_first.expect("every node of this plan can run")),
            vec!["c3", "c1", "c4", "c1", "c2", "c3"],
            "port 0 is `dense_leg` here, so the order must flip with the wiring"
        );
    }

    #[tokio::test]
    async fn a_trace_measures_how_long_each_nodes_component_took() {
        // ADR-C9 lists duration among what per-node replay reads. `slow`
        // waits a known time before answering, so the trace must show at
        // least that much — `> ZERO` would also be satisfied by the adapter's
        // own overhead, and would not prove the component's call is what is
        // measured.
        let plan = plan(vec![
            raw_top_k(
                "leg",
                "retriever",
                "slow",
                &["question"],
                RawParamValue::Int(1),
            ),
            raw("fuse", "fusion", "rrf", &["leg"]),
        ]);

        let (output, trace) = Engine::new().execute(&plan, query()).await;

        output.expect("every node of this plan can run");
        assert_eq!(traced(&trace), vec!["leg", "fuse"]);
        assert!(
            trace.nodes[0].duration >= SLOW_RETRIEVER_DELAY,
            "the slow leg waited {SLOW_RETRIEVER_DELAY:?}, but its trace records {:?}",
            trace.nodes[0].duration
        );
    }

    #[tokio::test]
    async fn a_trace_records_what_each_node_received_and_produced() {
        let plan = plan(vec![
            raw_top_k(
                "leg",
                "retriever",
                "counting",
                &["question"],
                RawParamValue::Int(2),
            ),
            raw("fuse", "fusion", "rrf", &["leg"]),
        ]);

        let (_, trace) = Engine::new().execute(&plan, query()).await;

        let leg = &trace.nodes[0];
        assert_eq!(leg.node.as_str(), "leg");
        assert_eq!(
            leg.inputs,
            vec![ValueSummary::Query {
                id: QueryId::new("q1")
            }],
            "a retriever's port 0 carries the declared input's query"
        );
        assert_eq!(leg.output, Some(ValueSummary::Chunks { count: 2 }));
        assert_eq!(leg.error, None);

        let fuse = &trace.nodes[1];
        assert_eq!(fuse.inputs, vec![ValueSummary::Chunks { count: 2 }]);
        assert_eq!(fuse.output, Some(ValueSummary::Chunks { count: 2 }));
    }

    #[tokio::test]
    async fn execution_schedules_over_the_edges_not_the_canonical_node_order() {
        // The trap this issue names. `LogicalPipeline` sorts its nodes by id,
        // and "fuse" sorts before "x1" and "x2" — so the stored order runs the
        // fusion before the retrievers it consumes. Only a topological
        // schedule survives this.
        let plan = plan(vec![
            raw("fuse", "fusion", "rrf", &["x1", "x2"]),
            raw_top_k(
                "x1",
                "retriever",
                "bm25",
                &["question"],
                RawParamValue::Int(3),
            ),
            raw_top_k(
                "x2",
                "retriever",
                "dense",
                &["question"],
                RawParamValue::Int(3),
            ),
        ]);
        assert_eq!(
            plan.nodes()
                .iter()
                .map(|node| node.logical().id().as_str())
                .collect::<Vec<_>>(),
            vec!["fuse", "x1", "x2"],
            "the fixture must be stored in a non-topological order, or it proves nothing"
        );

        let (output, trace) = Engine::new().execute(&plan, query()).await;

        assert_eq!(
            ids(&output.expect("a topological schedule runs this plan")),
            vec!["c1", "c3", "c2", "c4"]
        );
        assert_eq!(
            traced(&trace),
            vec!["x1", "x2", "fuse"],
            "the trace records the execution order, which is topological"
        );
    }

    #[tokio::test]
    async fn a_dangling_input_is_refused_and_the_partial_trace_is_still_returned() {
        // `fuse` names `nowhere`, which is neither a node nor a declared
        // input. `leg` does not depend on it, so it runs first — which is what
        // makes the returned trace *partial* rather than empty.
        let plan = plan_forged(
            r#"{"inputs":["question"],"nodes":[
                {"Fusion":{"id":"fuse","implementation":"rrf","inputs":["leg","nowhere"],"params":{}}},
                {"Retriever":{"id":"leg","implementation":"counting","inputs":["question"],"params":{"top_k":{"Int":1}}}}
            ]}"#,
        );

        let (output, trace) = Engine::new().execute(&plan, query()).await;

        let Err(err) = output else {
            panic!("`nowhere` produces nothing")
        };
        assert!(
            matches!(
                &err,
                ExecError::DanglingInput { consumer, port: 1, input }
                    if consumer.as_str() == "fuse" && input.as_str() == "nowhere"
            ),
            "expected DanglingInput at port 1, got {err:?}"
        );
        assert_eq!(
            traced(&trace),
            vec!["leg"],
            "the nodes that did run must still be inspectable"
        );
    }

    #[tokio::test]
    async fn a_reranker_reads_the_declared_input_at_port_zero() {
        // ADR-C18's shape end to end: `inputs: [question, fuse]`, query first.
        let plan = plan(vec![
            raw_top_k(
                "leg",
                "retriever",
                "bm25",
                &["question"],
                RawParamValue::Int(3),
            ),
            raw("fuse", "fusion", "rrf", &["leg"]),
            raw_top_k(
                "rank",
                "reranker",
                "by_id",
                &["question", "fuse"],
                RawParamValue::Int(2),
            ),
        ]);

        let (output, trace) = Engine::new().execute(&plan, query()).await;

        assert_eq!(
            ids(&output.expect("the declared input feeds the reranker's query port")),
            vec!["c3", "c2"],
            "the reranker reorders by id, descending, and keeps two"
        );
        assert_eq!(traced(&trace), vec!["leg", "fuse", "rank"]);
        assert_eq!(
            trace.nodes[2].inputs,
            vec![
                ValueSummary::Query {
                    id: QueryId::new("q1")
                },
                ValueSummary::Chunks { count: 3 },
            ],
            "port 0 is the query and port 1 the chunks, in that order"
        );
    }

    #[tokio::test]
    async fn a_component_failure_names_its_node_and_is_recorded_in_the_trace() {
        let plan = plan(vec![
            raw_top_k(
                "leg",
                "retriever",
                "offline",
                &["question"],
                RawParamValue::Int(3),
            ),
            raw("fuse", "fusion", "rrf", &["leg"]),
        ]);

        let (output, trace) = Engine::new().execute(&plan, query()).await;

        let Err(err) = output else {
            panic!("the retriever always fails")
        };
        assert!(
            matches!(&err, ExecError::Component { node, .. } if node.as_str() == "leg"),
            "expected a Component failure naming `leg`, got {err:?}"
        );
        assert!(
            err.to_string().contains("the index is offline"),
            "the component's own reason must reach the message: {err}"
        );
        assert_eq!(
            traced(&trace),
            vec!["leg"],
            "the failing node is traced, and nothing downstream of it runs"
        );
        let failed = &trace.nodes[0];
        assert_eq!(failed.output, None);
        assert!(
            failed
                .error
                .as_deref()
                .is_some_and(|message| message.contains("the index is offline")),
            "the trace records the failure: {:?}",
            failed.error
        );
    }

    #[tokio::test]
    async fn a_plan_whose_output_nothing_consumes_twice_is_refused() {
        // The terminal rule: a terminal node is one no other node consumes,
        // and a plan must have exactly one — otherwise there is no single
        // value to return.
        let plan = plan(vec![
            raw_top_k(
                "a",
                "retriever",
                "bm25",
                &["question"],
                RawParamValue::Int(1),
            ),
            raw_top_k(
                "b",
                "retriever",
                "dense",
                &["question"],
                RawParamValue::Int(1),
            ),
        ]);

        let (output, trace) = Engine::new().execute(&plan, query()).await;

        let Err(err) = output else {
            panic!("neither leg is consumed, so the plan has two terminal nodes")
        };
        assert!(
            matches!(&err, ExecError::MultipleTerminalNodes { nodes } if nodes.len() == 2),
            "expected MultipleTerminalNodes, got {err:?}"
        );
        assert!(
            trace.nodes.is_empty(),
            "a plan with no single output runs no node"
        );
    }

    #[tokio::test]
    async fn a_plan_with_no_node_at_all_has_no_terminal_node() {
        let plan = plan_forged(r#"{"inputs":["question"],"nodes":[]}"#);

        let (output, _) = Engine::new().execute(&plan, query()).await;

        let Err(err) = output else {
            panic!("an empty plan produces nothing")
        };
        assert!(
            matches!(&err, ExecError::NoTerminalNode),
            "expected NoTerminalNode, got {err:?}"
        );
    }

    #[tokio::test]
    async fn a_cycle_among_the_edges_is_refused_as_unschedulable() {
        // `validate` rejects a cyclic pipeline, so only the forged door
        // reaches this. `t` is the single terminal, and `a` and `b` consume
        // each other — so nothing is ever ready and the schedule must say so
        // rather than spin.
        let plan = plan_forged(
            r#"{"inputs":["question"],"nodes":[
                {"Fusion":{"id":"a","implementation":"rrf","inputs":["b"],"params":{}}},
                {"Fusion":{"id":"b","implementation":"rrf","inputs":["a"],"params":{}}},
                {"Fusion":{"id":"t","implementation":"rrf","inputs":["a"],"params":{}}}
            ]}"#,
        );

        let (output, trace) = Engine::new().execute(&plan, query()).await;

        let Err(err) = output else {
            panic!("no node of a cycle can ever become ready")
        };
        assert!(
            matches!(&err, ExecError::Cycle { nodes } if nodes.len() == 3),
            "expected Cycle, got {err:?}"
        );
        assert!(trace.nodes.is_empty(), "no node of this plan can run");
    }

    #[tokio::test]
    async fn a_plan_with_two_nodes_under_one_id_is_refused_as_a_duplicate() {
        // `validate` rejects a duplicate id, so only the forged door reaches
        // this. The scheduler counts *ids* it has run, and a plan with more
        // nodes than ids can never reach its node count — the shape a cycle
        // report would misname, with nothing left to list. `leg` is consumed
        // so that the plan has one terminal and the stall is reached at all.
        let plan = plan_forged(
            r#"{"inputs":["question"],"nodes":[
                {"Fusion":{"id":"fuse","implementation":"rrf","inputs":["leg"],"params":{}}},
                {"Retriever":{"id":"leg","implementation":"counting","inputs":["question"],"params":{"top_k":{"Int":1}}}},
                {"Retriever":{"id":"leg","implementation":"counting","inputs":["question"],"params":{"top_k":{"Int":1}}}}
            ]}"#,
        );

        let (output, trace) = Engine::new().execute(&plan, query()).await;

        let Err(err) = output else {
            panic!("two nodes share the id `leg`")
        };
        assert!(
            matches!(
                &err,
                ExecError::DuplicateNodeIds { nodes } if nodes.len() == 1 && nodes[0].as_str() == "leg"
            ),
            "expected DuplicateNodeIds naming `leg` once, got {err:?}"
        );
        assert!(
            err.to_string().contains("`leg`") && !err.to_string().contains("  "),
            "the message names the id and renders cleanly: {err}"
        );
        assert_eq!(
            traced(&trace),
            vec!["leg", "fuse"],
            "the nodes that ran before the stall are still inspectable"
        );
    }

    #[test]
    fn the_execute_future_is_send() {
        // The harness (#29) drives two runs from one context across tasks, so
        // the future `execute` returns must cross a `tokio::spawn` boundary.
        // Checked here rather than at that boundary in another crate, the way
        // `context.rs` checks `EngineContext` (D-11).
        fn assert_send<F: std::future::Future + Send>(_: &F) {}
        let engine = Engine::new();
        let plan = plan(vec![raw_top_k(
            "leg",
            "retriever",
            "counting",
            &["question"],
            RawParamValue::Int(1),
        )]);

        let future = engine.execute(&plan, query());

        assert_send(&future);
    }

    #[tokio::test]
    async fn a_node_with_too_few_inputs_is_refused_with_the_port_it_lacks() {
        // Arity is deliberately unchecked at validation (ADR-C18) and left
        // here: a retriever declaring no input validates and plans today.
        let plan = plan(vec![raw_top_k(
            "leg",
            "retriever",
            "counting",
            &[],
            RawParamValue::Int(1),
        )]);

        let (output, trace) = Engine::new().execute(&plan, query()).await;

        let Err(err) = output else {
            panic!("a retriever needs a query at port 0")
        };
        assert!(
            matches!(
                &err,
                ExecError::MissingInput { consumer, port: 0, expected: ValueKind::Query }
                    if consumer.as_str() == "leg"
            ),
            "expected MissingInput at port 0, got {err:?}"
        );
        assert_eq!(
            traced(&trace),
            vec!["leg"],
            "the node that refused is traced, with its failure"
        );
        assert!(trace.nodes[0].error.is_some());
    }

    #[tokio::test]
    async fn a_retriever_is_called_with_the_top_k_its_node_declares() {
        // §6.3: the per-call half of a node's params reaches the component as
        // its typed params struct. `counting` returns exactly `top_k` chunks,
        // so the parameter is observable in the result.
        let plan = plan(vec![
            raw_top_k(
                "leg",
                "retriever",
                "counting",
                &["question"],
                RawParamValue::Int(4),
            ),
            raw("fuse", "fusion", "rrf", &["leg"]),
        ]);

        let (output, _) = Engine::new().execute(&plan, query()).await;

        assert_eq!(
            output.expect("the node declares its top_k").len(),
            4,
            "the component was called with top_k = 4"
        );
    }

    #[tokio::test]
    async fn a_node_without_its_per_call_parameter_is_refused() {
        // No default is invented here: what a component does without a
        // parameter is the component's to decide, and the executor has no
        // business guessing (§6.3, §8.1).
        let plan = plan(vec![raw("leg", "retriever", "counting", &["question"])]);

        let (output, _) = Engine::new().execute(&plan, query()).await;

        let Err(err) = output else {
            panic!("the node declares no top_k")
        };
        assert!(
            matches!(
                &err,
                ExecError::InvalidParam { node, key: "top_k", found: None }
                    if node.as_str() == "leg"
            ),
            "expected InvalidParam, got {err:?}"
        );
    }

    #[tokio::test]
    async fn a_per_call_parameter_of_the_wrong_kind_is_refused() {
        let plan = plan(vec![raw_top_k(
            "leg",
            "retriever",
            "counting",
            &["question"],
            RawParamValue::String("many".to_string()),
        )]);

        let (output, _) = Engine::new().execute(&plan, query()).await;

        let Err(err) = output else {
            panic!("`top_k` is a string here")
        };
        assert!(
            matches!(
                &err,
                ExecError::InvalidParam { node, key: "top_k", found: Some(_) }
                    if node.as_str() == "leg"
            ),
            "expected InvalidParam naming what it found, got {err:?}"
        );
        assert!(
            err.to_string().contains("a string"),
            "the message must say what was declared instead: {err}"
        );
    }

    #[tokio::test]
    async fn a_negative_per_call_parameter_is_refused() {
        // `top_k` is a `usize` on the contract, and `-1 as usize` is a very
        // large number rather than an error — so the conversion is checked.
        let plan = plan(vec![raw_top_k(
            "leg",
            "retriever",
            "counting",
            &["question"],
            RawParamValue::Int(-1),
        )]);

        let (output, _) = Engine::new().execute(&plan, query()).await;

        let Err(err) = output else {
            panic!("a negative top_k is not a count")
        };
        assert!(
            matches!(&err, ExecError::InvalidParam { key: "top_k", .. }),
            "expected InvalidParam, got {err:?}"
        );
    }

    #[tokio::test]
    async fn a_reranker_is_called_with_the_top_k_its_node_declares() {
        let plan = plan(vec![
            raw_top_k(
                "leg",
                "retriever",
                "bm25",
                &["question"],
                RawParamValue::Int(3),
            ),
            raw_top_k(
                "rank",
                "reranker",
                "by_id",
                &["question", "leg"],
                RawParamValue::Int(1),
            ),
        ]);

        let (output, _) = Engine::new().execute(&plan, query()).await;

        assert_eq!(
            ids(&output.expect("both nodes declare their top_k")),
            vec!["c3"],
            "the reranker kept one chunk, so it was called with top_k = 1"
        );
    }

    #[test]
    fn a_port_carrying_the_wrong_kind_is_a_kind_mismatch_naming_the_edge() {
        // ADR-C16's backstop, reached directly. No plan `plan_physical`
        // produces can get here: planning checks every edge whose producer it
        // resolves, and an edge it cannot resolve fails as `DanglingInput`
        // above — so the adapter's own check is exercised at its own level.
        let mut table = Table::new();
        table.insert(
            NodeId::new("leg"),
            NodeValue::Chunks(vec![scored("c1", 1.0)]),
        );
        let inputs = vec![NodeId::new("leg")];

        let err = query_at(&NodeId::new("rank"), &inputs, 0, &table)
            .expect_err("`leg` produces chunks, and port 0 wants a query");

        assert!(
            matches!(
                &err,
                ExecError::KindMismatch {
                    consumer,
                    port: 0,
                    producer,
                    expected: ValueKind::Query,
                    found: ValueKind::Chunks,
                } if consumer.as_str() == "rank" && producer.as_str() == "leg"
            ),
            "expected KindMismatch, got {err:?}"
        );
        assert!(
            err.to_string().contains("validation"),
            "the message must read as a defect upstream, not as a condition to absorb: {err}"
        );
    }

    #[tokio::test]
    async fn one_engine_executes_two_plans_independently() {
        // The harness scenario (#29): a baseline and a candidate run through
        // one engine. `execute` takes `&self`, so nothing may accumulate
        // between runs.
        let engine = Engine::new();
        let baseline = plan(vec![raw_top_k(
            "leg",
            "retriever",
            "counting",
            &["question"],
            RawParamValue::Int(1),
        )]);
        let candidate = plan(vec![raw_top_k(
            "leg",
            "retriever",
            "counting",
            &["question"],
            RawParamValue::Int(5),
        )]);

        let (first, first_trace) = engine.execute(&baseline, query()).await;
        let (second, second_trace) = engine.execute(&candidate, query()).await;

        assert_eq!(first.expect("the baseline runs").len(), 1);
        assert_eq!(second.expect("the candidate runs").len(), 5);
        assert_eq!(first_trace.nodes.len(), 1);
        assert_eq!(second_trace.nodes.len(), 1);
    }
}
