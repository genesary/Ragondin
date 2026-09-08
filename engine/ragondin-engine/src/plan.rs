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

use std::collections::HashMap;

use ragondin_contracts::{Fusion, Reranker, Retriever};
use ragondin_pipeline::{
    consumed_kinds, produced_kind, LogicalNode, LogicalPipeline, NodeId, PortSpec,
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
    //
    // The trait object is written by planning and read by the executor (#16),
    // which lands in its own issue — so until it does, nothing outside this
    // crate's tests reads it. The `allow` is per variant, and not on the enum,
    // so that a variant added later does not inherit the exemption unnoticed.
    #[allow(dead_code)]
    Retriever(Box<dyn Retriever>),
    /// A resolved [`LogicalNode::Fusion`].
    #[allow(dead_code)]
    Fusion(Box<dyn Fusion>),
    /// A resolved [`LogicalNode::Reranker`].
    #[allow(dead_code)]
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
    //
    // `allow` rather than `expect`, and per method: the executor (#16) is the
    // caller and lands in its own issue, so until it does this crate's tests
    // are what exercises these — while a method added later must not inherit
    // the exemption unnoticed. In the `cfg(test)` build the tests do use them,
    // so an `expect` would go unfulfilled and fail `clippy -D warnings`.
    #[allow(dead_code)]
    pub(crate) fn logical(&self) -> &LogicalNode {
        &self.node
    }

    /// The constructed component this node runs.
    #[allow(dead_code)]
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
    nodes: Vec<PhysicalNode>,
}

impl PhysicalPipeline {
    /// The plan's nodes, in the canonical order [`LogicalPipeline`] holds them
    /// (sorted by [`NodeId`]).
    ///
    /// **That order is not an execution order.** A fusion node sorts before the
    /// retrievers it consumes whenever its id does; scheduling is the
    /// executor's job, over the data-flow edges (#16).
    #[allow(dead_code)] // See `PhysicalNode::logical`.
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
/// 2. every edge's kinds are checked ([`check_kinds`]);
/// 3. every remaining node's `impl:` name is resolved through the registry,
///    which constructs the component from the node's params.
///
/// A node whose `inputs` names a node that does not exist is **not** refused
/// here: [`LogicalPipeline`] is validated by construction, so the only way to
/// hold one with a dangling input is to have bypassed `validate`, and the
/// executor already owns that failure (#16). Planning skips the edge rather
/// than growing a second referential-integrity check.
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
    check_kinds(nodes, &index)?;

    let nodes = nodes
        .iter()
        .map(|node| {
            Ok(PhysicalNode {
                node: node.clone(),
                component: resolve(node, ctx)?,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;

    Ok(PhysicalPipeline { nodes })
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
/// copy of them).
///
/// What this layer is *for* is an [`LogicalNode::Extension`] node, whose kinds
/// only the registry knows. There is no registry entry for one to consult (see
/// [`PlanError::ExtensionUnsupported`] and #93), and every `Extension` is
/// refused before this runs — so what remains here is a **backstop** over
/// primitives,
/// reachable only by a [`LogicalPipeline`] that did not come through
/// `validate`. It is kept, and kept honest by its own test, because the layer
/// ADR-C16 asks for has to exist at the point where the extension check will
/// slot into it.
fn check_kinds(
    nodes: &[LogicalNode],
    index: &HashMap<&NodeId, &LogicalNode>,
) -> Result<(), PlanError> {
    for node in nodes {
        let spec = consumed_kinds(node);

        for (port, input_id) in node.inputs().iter().enumerate() {
            // A dangling input belongs to the executor (see `plan_physical`).
            let Some(producer) = index.get(input_id) else {
                continue;
            };

            let expected = match &spec {
                PortSpec::Fixed(kinds) => kinds.get(port).copied(),
                PortSpec::Variadic(kind) => Some(*kind),
                PortSpec::Unknown => unreachable!(
                    "only Extension returns PortSpec::Unknown, and it is refused before this runs"
                ),
            };

            let found = produced_kind(producer);
            if expected != Some(found) {
                return Err(PlanError::KindMismatch {
                    consumer: node.id().clone(),
                    port,
                    producer: producer.id().clone(),
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
    use ragondin_pipeline::{validate, RawGraph, RawNode, RawPipeline, ValueKind};
    use ragondin_types::{Query, ScoredChunk};
    use std::collections::BTreeMap;

    use crate::error::ComponentFamily;

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

    /// The legitimate way to a `LogicalPipeline`: through validation.
    fn logical(nodes: Vec<RawNode>) -> LogicalPipeline {
        validate(RawPipeline {
            version: Default::default(),
            pipeline: RawGraph { nodes },
        })
        .expect("the fixture must validate")
    }

    /// A `LogicalPipeline` `validate` would have refused.
    ///
    /// Planning's kind check is a second layer over the derivation validation
    /// already ran, so nothing that came through `validate` can reach it.
    /// `serde` is the only way in: `LogicalPipeline` derives `Deserialize`, and
    /// that path re-runs no check. This is the shape a `LogicalPipeline`
    /// deserialized from a run store or sent over a wire could have — which is
    /// why planning does not simply trust its input.
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
            r#"{"nodes":[
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
    fn an_edge_beyond_a_fixed_arity_variant_is_refused_with_no_expected_kind() {
        // A retriever declares one port. A second input reaches a position it
        // never declared: there is no kind to compare against, only an edge
        // that should not exist.
        let pipeline = forged(
            r#"{"nodes":[
                {"Retriever":{"id":"a","implementation":"bm25","inputs":[],"params":{}}},
                {"Retriever":{"id":"b","implementation":"dense","inputs":["a","a"],"params":{}}}
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
        // Referential integrity is `validate`'s (#9) and the executor's (#16).
        // Planning must not grow a third copy of it — and must not panic on
        // one either.
        let pipeline = forged(
            r#"{"nodes":[
                {"Fusion":{"id":"fuse","implementation":"rrf","inputs":["nowhere"],"params":{}}}
            ]}"#,
        );

        assert!(
            plan_physical(&pipeline, &context()).is_ok(),
            "planning skips an edge it cannot resolve rather than refusing it"
        );
    }

    #[test]
    fn planning_twice_from_one_context_yields_two_independent_plans() {
        // A registration answers every node that names it, in every plan built
        // from the context (`ComponentCtor` is `Fn`, not `FnOnce`) — which is
        // what lets the harness (#29) plan a baseline and a candidate from one
        // context.
        let pipeline = logical(vec![raw("leg", "retriever", "bm25", &[])]);
        let ctx = context();

        let first = plan_physical(&pipeline, &ctx).expect("registered");
        let second = plan_physical(&pipeline, &ctx).expect("registered");

        assert_eq!(first.nodes().len(), 1);
        assert_eq!(second.nodes().len(), 1);
    }
}
