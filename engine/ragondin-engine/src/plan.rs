//! Physical planning: `LogicalPipeline` + `EngineContext` → `PhysicalPipeline`
//! (#15).
//!
//! The logical-to-physical seam (`docs/code-architecture.md` §6.3, ADR-C2):
//! planning resolves each node's `impl:` name to a **constructed component**
//! through the registry, and verifies end-to-end kind compatibility. The
//! optimization phase between the two levels is **the identity function**
//! (frozen decision): the seam is reserved, the optimizer is not built.
//!
//! Planning is the **second** of ADR-C16's two kind-check layers, over the one
//! derivation `ragondin-pipeline` exposes — never a second copy of it, which
//! would drift from the one `ragondin validate` runs.

use std::collections::{HashMap, HashSet};

use ragondin_contracts::{Fusion, Reranker, Retriever};
use ragondin_pipeline::{
    consumed_kinds, produced_kind, LogicalNode, LogicalPipeline, NodeId, PortSpec, ValueKind,
};

use crate::context::EngineContext;
use crate::error::PlanError;

/// The component a planned node runs, one variant per [`LogicalNode`] variant
/// that resolves to one.
///
/// `Embedder` and `VectorStore` are absent deliberately: they are not pipeline
/// nodes (see [`crate::ComponentFamily`]) but components a dense retriever is
/// built from, so they never appear as a node of a plan.
pub(crate) enum ResolvedComponent {
    /// A resolved [`LogicalNode::Retriever`].
    Retriever(Box<dyn Retriever>),
    /// A resolved [`LogicalNode::Fusion`].
    Fusion(Box<dyn Fusion>),
    /// A resolved [`LogicalNode::Reranker`].
    Reranker(Box<dyn Reranker>),
}

/// One node of a plan: its logical form, and the component it resolved to.
///
/// The logical node is carried whole rather than reduced to an id and an input
/// list, because ADR-C16 requires a node's port kinds to be **derived from its
/// `LogicalNode` variant on demand** rather than stored — which is only
/// possible if the variant is still here. It also carries the node's `params`,
/// whose per-call half (§6.3) the executor needs at every call.
pub(crate) struct PhysicalNode {
    node: LogicalNode,
    component: ResolvedComponent,
}

impl PhysicalNode {
    /// The node's logical form: its id, its inputs in port order, its params,
    /// and the variant every kind derivation reads.
    pub(crate) fn logical(&self) -> &LogicalNode {
        &self.node
    }

    /// The constructed component this node runs.
    pub(crate) fn component(&self) -> &ResolvedComponent {
        &self.component
    }
}

/// An executable plan: every `impl:` name resolved to a constructed component.
///
/// **Not serializable, and not to be made so** — it holds `Box<dyn Trait>`.
/// That is `docs/OPEN_QUESTIONS.md` #3, left open.
///
/// Its payload is private to this crate (INV-2) so that it can evolve as the
/// executor and later milestones need it to.
pub struct PhysicalPipeline {
    inputs: Vec<NodeId>,
    nodes: Vec<PhysicalNode>,
}

impl PhysicalPipeline {
    /// The ids of the values this plan receives from its caller (ADR-C18),
    /// carried through from the [`LogicalPipeline`] verbatim.
    ///
    /// The executor seeds its value table from these, which is the whole
    /// reason they are here: a declared input and a dangling one are
    /// indistinguishable from the node list alone — both are ids naming no
    /// node — so an executor without the declaration could not tell the
    /// pipeline's entry point from a wiring mistake.
    pub(crate) fn inputs(&self) -> &[NodeId] {
        &self.inputs
    }

    /// The plan's nodes, in the canonical order [`LogicalPipeline`] holds them
    /// (sorted by [`NodeId`]).
    ///
    /// **That order is not an execution order.** A fusion node sorts before the
    /// retrievers it consumes whenever its id does; scheduling is the
    /// executor's job, over the data-flow edges.
    pub(crate) fn nodes(&self) -> &[PhysicalNode] {
        &self.nodes
    }
}

/// Turns a validated pipeline into an executable one against `ctx`'s registry.
///
/// Three passes, cheapest first, so that a pipeline nothing in this build can
/// run is refused before any constructor is asked to load a model:
///
/// 1. every [`LogicalNode::Extension`] is refused
///    ([`PlanError::ExtensionUnsupported`]) — see that variant, and #93, for
///    why the check ADR-C16 reserves for planning cannot yet apply to them;
/// 2. every edge's kinds are checked, and a mismatch refused
///    ([`PlanError::KindMismatch`]) — see that variant for what the check is,
///    and for why a pipeline that came through `validate` never reaches it;
/// 3. every remaining node's `impl:` name is resolved through the registry,
///    which constructs the component from the node's params.
///
/// A node whose `inputs` names a node that does not exist is **not** refused
/// here: [`LogicalPipeline`] is validated by construction, so the only way to
/// hold one with a dangling input is to have bypassed `validate`, and the
/// executor owns that failure, and reports it as
/// [`crate::ExecError::DanglingInput`]. Planning skips the edge rather than
/// growing a second referential-integrity check.
pub fn plan_physical(
    logical: &LogicalPipeline,
    ctx: &EngineContext,
) -> Result<PhysicalPipeline, PlanError> {
    let logical = optimize(logical);
    let nodes = logical.nodes();

    for node in nodes {
        if let LogicalNode::Extension(extension) = node {
            return Err(PlanError::ExtensionUnsupported {
                node: extension.id.clone(),
                kind: extension.kind.clone(),
            });
        }
    }

    let index: HashMap<&NodeId, &LogicalNode> =
        nodes.iter().map(|node| (node.id(), node)).collect();
    let declared: HashSet<&NodeId> = logical.inputs().iter().collect();
    check_kinds(nodes, &index, &declared)?;

    let nodes = nodes
        .iter()
        .map(|node| {
            Ok(PhysicalNode {
                node: node.clone(),
                component: resolve(node, ctx)?,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;

    Ok(PhysicalPipeline {
        inputs: logical.inputs().to_vec(),
        nodes,
    })
}

/// The optimization phase between the logical and physical levels: **the
/// identity function**, deliberately (`docs/code-architecture.md` §6.3, a
/// frozen decision). The seam is an architectural boundary that is expensive
/// to introduce after the fact; the optimizer is a subsystem we are not
/// building. Do not add passes here.
fn optimize(logical: &LogicalPipeline) -> &LogicalPipeline {
    logical
}

/// Checks every edge's kinds line up, ADR-C16's second layer over the same
/// derivation validation ran ([`consumed_kinds`], [`produced_kind`] — never a
/// copy of *them*).
///
/// The *comparison* around that derivation is, however, a second copy of
/// `ragondin-pipeline`'s, which is private to that crate and returns its own
/// error type. ADR-C16's "one derivation, two call sites" therefore holds for
/// the derivation and not for the loop, so the loop is written to stay in step
/// with the other one clause for clause — including the `Extension`-producer
/// clause below, which nothing here can currently reach.
///
/// An `inputs` entry `index` does not know is a **declared pipeline input**
/// (ADR-C18) when `declared` holds it, and produces [`ValueKind::Query`]; only
/// an id that is neither is left to the executor.
///
/// What this layer is *for* is an [`LogicalNode::Extension`] node, whose kinds
/// only the registry knows. There is no registry entry for one to consult (see
/// [`PlanError::ExtensionUnsupported`] and #93), and every `Extension` is
/// refused before this runs — so what remains reachable is a **backstop** over
/// primitives, and only for a [`LogicalPipeline`] that did not come through
/// `validate`. It is kept because the layer ADR-C16 asks for has to exist at
/// the point where the extension check will slot into it. That slotting is
/// **not** a no-op: the `PortSpec::Unknown` arm below must stop being
/// unreachable for an `Extension` *consumer* to be checked at all.
fn check_kinds(
    nodes: &[LogicalNode],
    index: &HashMap<&NodeId, &LogicalNode>,
    declared: &HashSet<&NodeId>,
) -> Result<(), PlanError> {
    for node in nodes {
        let spec = consumed_kinds(node);

        for (port, input_id) in node.inputs().iter().enumerate() {
            let expected = match &spec {
                PortSpec::Fixed(kinds) => kinds.get(port).copied(),
                PortSpec::Variadic(kind) => Some(*kind),
                PortSpec::Unknown => unreachable!(
                    "only Extension returns PortSpec::Unknown, and it is refused before this runs"
                ),
            };

            // Derived from the *producer*, never from `node`: indistinguishable
            // today, since every primitive produces `Chunks`, and a live bug the
            // moment a node kind that does not arrives (#93, or a generator).
            let found = match index.get(input_id) {
                Some(producer) => {
                    // The clause `ragondin-pipeline`'s check carries, kept here
                    // in step with it: once a port is known to exist, an
                    // `Extension` producer's kind stays unguessed (ADR-C16).
                    // Unreachable today, since every `Extension` is refused
                    // before this runs — but its *absence* is what would make
                    // the two layers disagree the day #93 lands, rejecting an
                    // edge `validate` accepts (which `ragondin-pipeline`'s own
                    // `an_extension_feeding_a_primitive_and_fed_by_one_validates`
                    // requires to stay legal).
                    if expected.is_some() && matches!(producer, LogicalNode::Extension(_)) {
                        continue;
                    }
                    produced_kind(producer)
                }
                // A declared pipeline input (ADR-C18) produces `Query`.
                // Resolving it here is not optional bookkeeping: without it
                // this loop would fall through to the arm below and skip the
                // one edge the declaration exists to carry, silently accepting
                // a wiring `validate` rejects.
                None if declared.contains(input_id) => ValueKind::Query,
                // Neither a node nor a declaration: still the executor's
                // business (see `plan_physical`).
                None => continue,
            };

            if expected != Some(found) {
                return Err(PlanError::KindMismatch {
                    consumer: node.id().clone(),
                    port,
                    producer: input_id.clone(),
                    expected,
                    found,
                });
            }
        }
    }
    Ok(())
}

/// Resolves one node's `impl:` name against `ctx`, constructing the component
/// from the node's params.
///
/// #15's "applying default params" is discharged here by doing nothing, and
/// deliberately: a constructor receives the node's configuration and is the
/// only thing that knows its own defaults (§8.1, and `EngineContext` has
/// nowhere to declare one), so a default applied here could only be a second,
/// disagreeing copy of what the component already does — the shape #14's
/// `counting_retriever` established when it read a missing `count` as `0`.
///
/// The match is on the node's **variant**, which is what selects the family's
/// registry — never on the implementation name (INV-7): a built-in resolves
/// through exactly the path a third-party crate's registration does.
fn resolve(node: &LogicalNode, ctx: &EngineContext) -> Result<ResolvedComponent, PlanError> {
    match node {
        LogicalNode::Retriever(node) => ctx
            .build_retriever(&node.implementation, &node.params)
            .map(ResolvedComponent::Retriever),
        LogicalNode::Fusion(node) => ctx
            .build_fusion(&node.implementation, &node.params)
            .map(ResolvedComponent::Fusion),
        LogicalNode::Reranker(node) => ctx
            .build_reranker(&node.implementation, &node.params)
            .map(ResolvedComponent::Reranker),
        LogicalNode::Extension(_) => {
            unreachable!("every Extension node is refused before resolution")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ragondin_contracts::{
        ComponentError, FusionParams, RerankParams, Reranker, RetrieveParams, Retriever,
    };
    use ragondin_pipeline::{
        validate, ParamValue, Params, RawGraph, RawNode, RawParamValue, RawPipeline, SchemaVersion,
        ValidationError, ValueKind,
    };
    use ragondin_types::{Chunk, ChunkId, DocId, Query, QueryId, ScoredChunk};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::error::{ComponentFamily, ConstructionError};

    struct StubRetriever;

    #[async_trait]
    impl Retriever for StubRetriever {
        async fn retrieve(
            &self,
            _query: &Query,
            _params: &RetrieveParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            Ok(Vec::new())
        }
    }

    /// Returns `count` chunks, where `count` came from its **configuration**.
    /// Empty-vector stubs cannot tell "the node's params reached the
    /// constructor" from "an empty map did"; this one can.
    struct ConfiguredRetriever {
        count: usize,
    }

    #[async_trait]
    impl Retriever for ConfiguredRetriever {
        async fn retrieve(
            &self,
            _query: &Query,
            _params: &RetrieveParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            Ok((0..self.count)
                .map(|i| ScoredChunk {
                    chunk: Chunk {
                        id: ChunkId::new(format!("c{i}")),
                        text: String::new(),
                        document_id: DocId::new("doc"),
                    },
                    score: 1.0,
                })
                .collect())
        }
    }

    /// Refuses a configuration without `count`, so a constructor handed the
    /// wrong params fails loudly rather than quietly building the same thing.
    fn configured_retriever(config: &Params) -> Result<Box<dyn Retriever>, ConstructionError> {
        let Some(ParamValue::Int(count)) = config.get("count") else {
            return Err("`count` must be an integer, and this node has none".into());
        };
        Ok(Box::new(ConfiguredRetriever {
            count: *count as usize,
        }))
    }

    struct StubFusion;

    #[async_trait]
    impl Fusion for StubFusion {
        async fn fuse(
            &self,
            _inputs: Vec<Vec<ScoredChunk>>,
            _params: &FusionParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            Ok(Vec::new())
        }
    }

    struct StubReranker;

    #[async_trait]
    impl Reranker for StubReranker {
        async fn rerank(
            &self,
            _query: &Query,
            _chunks: Vec<ScoredChunk>,
            _params: &RerankParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            Ok(Vec::new())
        }
    }

    /// A context registering one implementation per family a node can name.
    fn context() -> EngineContext {
        let mut ctx = EngineContext::new();
        ctx.register_retriever("bm25", Box::new(|_| Ok(Box::new(StubRetriever))));
        ctx.register_retriever("dense", Box::new(|_| Ok(Box::new(StubRetriever))));
        ctx.register_fusion("rrf", Box::new(|_| Ok(Box::new(StubFusion))));
        ctx.register_reranker("cross_encoder", Box::new(|_| Ok(Box::new(StubReranker))));
        ctx.register_retriever("configured", Box::new(configured_retriever));
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

    fn raw_counting(id: &str, count: i64) -> RawNode {
        let mut node = raw(id, "retriever", "configured", &[]);
        node.params
            .insert("count".to_string(), RawParamValue::Int(count));
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

    /// A `LogicalPipeline` `validate` would have refused.
    ///
    /// Planning's kind check is a second layer over the derivation validation
    /// already ran, so nothing that came through `validate` can reach it.
    /// `serde` is the only way in: `LogicalPipeline` derives `Deserialize`, and
    /// that path re-runs no check. Not a wire path — anything arriving over one
    /// goes `RawPipeline` → `validate` (INV-9) — but the two doors out of this
    /// crate are `validate` and that derive, and only one of them checks.
    ///
    /// It also bypasses canonical node ordering, so a fixture must be written
    /// in [`NodeId`] order to satisfy what [`PhysicalPipeline::nodes`]
    /// documents.
    fn forged(json: &str) -> LogicalPipeline {
        serde_json::from_str(json).expect("the forged JSON must match LogicalPipeline's shape")
    }

    /// The family a resolved component belongs to. Not a kind derivation — it
    /// reads the resolved variant, and exists so a test can assert *what* each
    /// node resolved to.
    fn family(component: &ResolvedComponent) -> ComponentFamily {
        match component {
            ResolvedComponent::Retriever(_) => ComponentFamily::Retriever,
            ResolvedComponent::Fusion(_) => ComponentFamily::Fusion,
            ResolvedComponent::Reranker(_) => ComponentFamily::Reranker,
        }
    }

    #[test]
    fn a_hybrid_pipeline_plans_to_its_resolved_components() {
        let pipeline = logical(vec![
            raw("bm25_leg", "retriever", "bm25", &[]),
            raw("dense_leg", "retriever", "dense", &[]),
            raw("fuse", "fusion", "rrf", &["bm25_leg", "dense_leg"]),
        ]);

        let plan = plan_physical(&pipeline, &context()).expect("every impl is registered");

        let resolved: Vec<(&str, ComponentFamily)> = plan
            .nodes()
            .iter()
            .map(|node| (node.logical().id().as_str(), family(node.component())))
            .collect();
        assert_eq!(
            resolved,
            vec![
                ("bm25_leg", ComponentFamily::Retriever),
                ("dense_leg", ComponentFamily::Retriever),
                ("fuse", ComponentFamily::Fusion),
            ],
            "every node must resolve to a component of its own family"
        );
    }

    #[test]
    fn a_planned_node_keeps_its_inputs_in_port_order() {
        // ADR-C16: `inputs` is positional. The executor reads them from the
        // plan, so losing or reordering them here would silently rewire the
        // pipeline.
        let pipeline = logical(vec![
            raw("a", "retriever", "bm25", &[]),
            raw("b", "retriever", "dense", &[]),
            raw("fuse", "fusion", "rrf", &["b", "a"]),
        ]);

        let plan = plan_physical(&pipeline, &context()).expect("every impl is registered");

        let fusion = plan
            .nodes()
            .iter()
            .find(|node| node.logical().id().as_str() == "fuse")
            .expect("the fusion node must be in the plan");
        let inputs: Vec<&str> = fusion
            .logical()
            .inputs()
            .iter()
            .map(NodeId::as_str)
            .collect();
        assert_eq!(inputs, vec!["b", "a"], "port order must survive planning");
    }

    #[test]
    fn an_unregistered_impl_is_refused_with_unknown_impl() {
        let pipeline = logical(vec![raw("leg", "retriever", "splade", &[])]);

        let Err(err) = plan_physical(&pipeline, &context()) else {
            panic!("`splade` is registered nowhere")
        };

        assert!(
            matches!(
                &err,
                PlanError::UnknownImpl { family: ComponentFamily::Retriever, name } if name == "splade"
            ),
            "expected UnknownImpl, got {err:?}"
        );
    }

    #[test]
    fn a_constructor_that_refuses_surfaces_as_a_construction_error() {
        let pipeline = logical(vec![raw("leg", "retriever", "refuses", &[])]);
        let mut ctx = context();
        ctx.register_retriever("refuses", Box::new(|_| Err("no model at that path".into())));

        let Err(err) = plan_physical(&pipeline, &ctx) else {
            panic!("the constructor always refuses")
        };

        assert!(
            matches!(
                &err,
                PlanError::Construction { family: ComponentFamily::Retriever, name, .. } if name == "refuses"
            ),
            "expected Construction, got {err:?}"
        );
        assert!(
            err.to_string().contains("no model at that path"),
            "the constructor's own reason must reach the message: {err}"
        );
    }

    #[test]
    fn an_extension_node_is_refused_as_unsupported() {
        let pipeline = logical(vec![raw("qx", "extension", "hyde", &[])]);

        let Err(err) = plan_physical(&pipeline, &context()) else {
            panic!("no Extension node has a physical planner in this build")
        };

        assert!(
            matches!(
                &err,
                PlanError::ExtensionUnsupported { node, kind } if node.as_str() == "qx" && kind == "hyde"
            ),
            "expected ExtensionUnsupported, got {err:?}"
        );
        let message = err.to_string();
        assert!(
            message.contains("`qx`") && message.contains("hyde"),
            "the message must name the node and its extension type: {message}"
        );
    }

    #[test]
    fn an_extension_is_refused_before_any_component_is_constructed() {
        // The order of planning's passes, pinned: a plan holding an Extension
        // cannot run at all, so refusing it must not wait behind a constructor
        // loading a model.
        let pipeline = logical(vec![
            raw("qx", "extension", "hyde", &[]),
            raw("leg", "retriever", "splade", &[]),
        ]);

        let Err(err) = plan_physical(&pipeline, &context()) else {
            panic!("the pipeline holds both an Extension and an unregistered impl")
        };

        assert!(
            matches!(&err, PlanError::ExtensionUnsupported { .. }),
            "the Extension must be refused before resolution is attempted, got {err:?}"
        );
    }

    #[test]
    fn a_miswired_edge_is_refused_with_a_kind_mismatch_naming_the_edge() {
        // A reranker's port 0 wants a `Query`; `leg` produces `Chunks`. Every
        // impl below is registered, so the kind check is the only thing that
        // can refuse this plan.
        let pipeline = forged(
            r#"{"inputs":[],"nodes":[
                {"Retriever":{"id":"leg","implementation":"bm25","inputs":[],"params":{}}},
                {"Reranker":{"id":"rank","implementation":"cross_encoder","inputs":["leg","leg"],"params":{}}}
            ]}"#,
        );

        let Err(err) = plan_physical(&pipeline, &context()) else {
            panic!("a retriever cannot feed a reranker's query port")
        };

        assert!(
            matches!(
                &err,
                PlanError::KindMismatch {
                    consumer,
                    port: 0,
                    producer,
                    expected: Some(ValueKind::Query),
                    found: ValueKind::Chunks,
                } if consumer.as_str() == "rank" && producer.as_str() == "leg"
            ),
            "expected KindMismatch at port 0, got {err:?}"
        );
        let message = err.to_string();
        assert!(
            message.contains("`rank`") && message.contains("`leg`"),
            "the message must name both ends of the edge: {message}"
        );
        assert!(
            message.contains("query") && message.contains("chunks"),
            "the message must name the expected and the found kind: {message}"
        );
    }

    #[test]
    fn a_retriever_fed_by_a_retriever_is_refused_at_port_zero() {
        // Named for what it proves: port 0 faults first, so this never reaches
        // the second input. The `expected: None` case has its own test below —
        // conflating the two is how a branch goes uncovered while looking
        // covered.
        let pipeline = forged(
            r#"{"inputs":[],"nodes":[
                {"Retriever":{"id":"a","implementation":"bm25","inputs":[],"params":{}}},
                {"Retriever":{"id":"b","implementation":"dense","inputs":["a","a"],"params":{}}}
            ]}"#,
        );

        let Err(err) = plan_physical(&pipeline, &context()) else {
            panic!("a retriever's only port wants a query, and `a` produces chunks")
        };

        assert!(
            matches!(
                &err,
                PlanError::KindMismatch {
                    consumer,
                    port: 0,
                    expected: Some(ValueKind::Query),
                    found: ValueKind::Chunks,
                    ..
                } if consumer.as_str() == "b"
            ),
            "port 0 is the first fault, and it is a kind mismatch: {err:?}"
        );
    }

    #[test]
    fn an_edge_beyond_a_fixed_arity_variant_is_refused_with_no_expected_kind() {
        // A retriever declares one port. Port 0 is dangling and therefore
        // skipped, which lets port 1 — a position the variant never declared —
        // carry the fault: no kind to compare against, only an edge that should
        // not exist. That skip is the only route to `expected: None` in this
        // build, since every primitive produces `Chunks` and every fixed port 0
        // wants `Query`, so no edge can match port 0 and then overflow arity.
        let pipeline = forged(
            r#"{"inputs":[],"nodes":[
                {"Retriever":{"id":"a","implementation":"bm25","inputs":[],"params":{}}},
                {"Retriever":{"id":"b","implementation":"dense","inputs":["nowhere","a"],"params":{}}}
            ]}"#,
        );

        let Err(err) = plan_physical(&pipeline, &context()) else {
            panic!("a retriever declares exactly one port")
        };

        assert!(
            matches!(
                &err,
                PlanError::KindMismatch {
                    consumer,
                    port: 1,
                    producer,
                    expected: None,
                    found: ValueKind::Chunks,
                } if consumer.as_str() == "b" && producer.as_str() == "a"
            ),
            "expected an arity fault at port 1 with no declared kind, got {err:?}"
        );
        assert!(
            err.to_string()
                .contains("no port declared at this position"),
            "the message must say no port exists there, not name a kind: {err}"
        );
    }

    #[test]
    fn a_rerankers_ports_are_read_by_position_not_by_first() {
        // The reranker is the only node with heterogeneous ports — `[Query,
        // Chunks]` — so it is the only node that can tell a positional lookup
        // from one that always reads the first declared kind. Port 0 dangles
        // and is skipped; port 1 legitimately takes the chunks a retriever
        // produces, so this must plan.
        let pipeline = forged(
            r#"{"inputs":[],"nodes":[
                {"Retriever":{"id":"leg","implementation":"bm25","inputs":[],"params":{}}},
                {"Reranker":{"id":"rank","implementation":"cross_encoder","inputs":["nowhere","leg"],"params":{}}}
            ]}"#,
        );

        let plan = plan_physical(&pipeline, &context())
            .expect("port 1 of a reranker takes chunks, which `leg` produces");

        let families: Vec<ComponentFamily> =
            plan.nodes().iter().map(|n| family(n.component())).collect();
        assert_eq!(
            families,
            vec![ComponentFamily::Retriever, ComponentFamily::Reranker],
            "and a reranker node resolves through the reranker registry"
        );
    }

    #[test]
    fn a_variadic_port_accepts_any_number_of_edges_of_its_kind() {
        let pipeline = logical(vec![
            raw("a", "retriever", "bm25", &[]),
            raw("b", "retriever", "dense", &[]),
            raw("c", "retriever", "bm25", &[]),
            raw("fuse", "fusion", "rrf", &["a", "b", "c"]),
        ]);

        assert!(
            plan_physical(&pipeline, &context()).is_ok(),
            "a fusion consumes any number of chunk-producing legs"
        );
    }

    #[test]
    fn a_dangling_input_is_left_for_the_executor_to_report() {
        // Referential integrity is `validate`'s and the executor's.
        // Planning must not grow a third copy of it — and must not panic on
        // one either, which is what the lookup's `else { continue }` is for.
        let pipeline = forged(
            r#"{"inputs":[],"nodes":[
                {"Fusion":{"id":"fuse","implementation":"rrf","inputs":["nowhere"],"params":{}}}
            ]}"#,
        );

        assert!(
            plan_physical(&pipeline, &context()).is_ok(),
            "planning skips an edge it cannot resolve rather than refusing it"
        );
    }

    #[test]
    fn a_dangling_input_does_not_abandon_the_rest_of_the_kind_check() {
        // "Skip this edge" and "stop checking" are indistinguishable on a
        // one-node graph. Here the dangling edge comes first and a genuine
        // fault comes after it, so only the former still reports.
        let pipeline = forged(
            r#"{"inputs":[],"nodes":[
                {"Fusion":{"id":"fuse","implementation":"rrf","inputs":["nowhere"],"params":{}}},
                {"Reranker":{"id":"rank","implementation":"cross_encoder","inputs":["fuse","fuse"],"params":{}}}
            ]}"#,
        );

        let Err(err) = plan_physical(&pipeline, &context()) else {
            panic!("`fuse` produces chunks, and a reranker's port 0 wants a query")
        };

        assert!(
            matches!(
                &err,
                PlanError::KindMismatch { consumer, port: 0, .. } if consumer.as_str() == "rank"
            ),
            "the fault after the skipped edge must still be reported: {err:?}"
        );
    }

    #[tokio::test]
    async fn each_node_is_constructed_from_its_own_params() {
        // §6.3: the constructor receives the node's configuration. Two nodes,
        // one implementation, two configurations — so a planner that passed an
        // empty map, or one node's params to another, cannot pass this.
        let pipeline = logical(vec![raw_counting("few", 2), raw_counting("many", 5)]);

        let plan = plan_physical(&pipeline, &context()).expect("`configured` is registered");

        let mut counts = Vec::new();
        for node in plan.nodes() {
            let ResolvedComponent::Retriever(retriever) = node.component() else {
                panic!("both nodes are retrievers")
            };
            let hits = retriever
                .retrieve(
                    &Query {
                        id: QueryId::new("q"),
                        text: "anything".to_string(),
                    },
                    &RetrieveParams::new(10),
                )
                .await
                .expect("the stub does not fail");
            counts.push((node.logical().id().as_str(), hits.len()));
        }

        assert_eq!(
            counts,
            vec![("few", 2), ("many", 5)],
            "each component must be built from the params of its own node"
        );
    }

    #[test]
    fn two_pipelines_planned_from_one_context_each_get_their_own_configuration() {
        // The harness scenario (#29): a baseline and a candidate planned from
        // one context. `ComponentCtor` is `Fn`, so one registration answers
        // both — and each plan carries its own configuration, not the first's.
        let ctx = context();
        let baseline = plan_physical(&logical(vec![raw_counting("leg", 1)]), &ctx)
            .expect("`configured` is registered");
        let candidate = plan_physical(&logical(vec![raw_counting("leg", 9)]), &ctx)
            .expect("`configured` is registered");

        // The configuration is only observable through the built component, so
        // compare what each plan kept of its own node.
        let params_of = |plan: &PhysicalPipeline| match plan.nodes()[0].logical() {
            LogicalNode::Retriever(node) => node.params.get("count").cloned(),
            other => panic!("expected a retriever, got {other:?}"),
        };
        assert_eq!(params_of(&baseline), Some(ParamValue::Int(1)));
        assert_eq!(params_of(&candidate), Some(ParamValue::Int(9)));
    }

    #[test]
    fn a_miswired_edge_is_refused_before_any_component_is_constructed() {
        // The other half of the pass ordering `plan_physical` documents: the
        // kind check is cheap and construction may load a model, so a plan that
        // cannot run must be refused before a constructor is called.
        let constructions = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&constructions);
        let mut ctx = context();
        ctx.register_retriever(
            "counted",
            Box::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(StubRetriever))
            }),
        );
        let pipeline = forged(
            r#"{"inputs":[],"nodes":[
                {"Retriever":{"id":"leg","implementation":"counted","inputs":[],"params":{}}},
                {"Reranker":{"id":"rank","implementation":"cross_encoder","inputs":["leg","leg"],"params":{}}}
            ]}"#,
        );

        let Err(err) = plan_physical(&pipeline, &ctx) else {
            panic!("a retriever cannot feed a reranker's query port")
        };

        assert!(matches!(&err, PlanError::KindMismatch { .. }), "{err:?}");
        assert_eq!(
            constructions.load(Ordering::SeqCst),
            0,
            "no component may be constructed for a plan the kind check refuses"
        );
    }

    #[test]
    fn a_declared_input_feeding_a_chunks_port_is_refused_at_planning() {
        // The trap this issue exists to avoid. Planning resolves a producer
        // through its node index; a declared input (ADR-C18) is not in it, and
        // the arm that used to catch a miss simply skipped the edge. If this
        // layer ever falls back to that skip, planning silently accepts a
        // wiring `validate` rejects — and the two layers ADR-C16 calls "one
        // derivation, two call sites" disagree.
        //
        // Forged, because `validate` refuses exactly this: the point is what
        // the *second* layer does with a pipeline that did not come through
        // the first.
        let pipeline = forged(
            r#"{"inputs":["question"],"nodes":[
                {"Fusion":{"id":"fuse","implementation":"rrf","inputs":["question"],"params":{}}}
            ]}"#,
        );

        let Err(err) = plan_physical(&pipeline, &context()) else {
            panic!("the pipeline's query cannot feed a fusion's chunk port")
        };

        assert!(
            matches!(
                &err,
                PlanError::KindMismatch {
                    consumer,
                    port: 0,
                    producer,
                    expected: Some(ValueKind::Chunks),
                    found: ValueKind::Query,
                } if consumer.as_str() == "fuse" && producer.as_str() == "question"
            ),
            "expected KindMismatch at port 0 naming the declared input, got {err:?}"
        );
    }

    #[test]
    fn an_id_that_is_neither_a_node_nor_a_declaration_is_still_left_to_the_executor() {
        // The third arm, kept distinct from the one above: a genuinely
        // unresolvable id is not this layer's fault to report. Only a
        // `LogicalPipeline` that bypassed `validate` can hold one.
        let pipeline = forged(
            r#"{"inputs":[],"nodes":[
                {"Fusion":{"id":"fuse","implementation":"rrf","inputs":["nowhere"],"params":{}}}
            ]}"#,
        );
        plan_physical(&pipeline, &context()).expect("a dangling input is not planning's to refuse");
    }

    #[test]
    fn a_reranker_wired_to_the_declared_input_plans_end_to_end() {
        // The acceptance criterion of ADR-C18, and what was impossible before
        // it: a reranker's port 0 wants a `Query`, no primitive produces one,
        // and the only previous filler was an `Extension` node — which
        // `plan_physical` refuses outright (see
        // `an_extension_node_is_refused_as_unsupported`). With the query
        // declared, the whole graph resolves with no extension anywhere.
        let pipeline = logical(vec![
            raw("leg", "retriever", "bm25", &["question"]),
            raw("fuse", "fusion", "rrf", &["leg"]),
            raw("rank", "reranker", "cross_encoder", &["question", "fuse"]),
        ]);

        let physical = plan_physical(&pipeline, &context())
            .expect("a reranker fed by the declared input must plan");
        assert_eq!(physical.nodes().len(), 3);
        assert!(
            !pipeline
                .nodes()
                .iter()
                .any(|n| matches!(n, LogicalNode::Extension(_))),
            "the point is that no Extension is needed"
        );
    }

    #[test]
    fn the_two_kind_check_layers_report_one_fault_identically() {
        // ADR-C16's "one derivation, two call sites" is only true for a reader
        // if both layers render the same edge the same way. The derivation is
        // shared; these two `Display`s are not, so nothing but this test keeps
        // them from drifting apart.
        for expected in [Some(ValueKind::Query), None] {
            let planning = PlanError::KindMismatch {
                consumer: NodeId::new("rank"),
                port: 1,
                producer: NodeId::new("leg"),
                expected,
                found: ValueKind::Chunks,
            };
            let validation = ValidationError::KindMismatch {
                consumer: NodeId::new("rank"),
                port: 1,
                producer: NodeId::new("leg"),
                expected,
                found: ValueKind::Chunks,
            };
            assert_eq!(
                planning.to_string(),
                validation.to_string(),
                "the planning and validation layers must word one fault alike"
            );
        }
    }
}
