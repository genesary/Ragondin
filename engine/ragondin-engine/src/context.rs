//! The component registry and the context that carries it.
//!
//! `EngineContext` is the composition root (`docs/code-architecture.md` §8.1),
//! modelled on DataFusion's session context: one table per family a pipeline
//! node names, mapping an `impl:` name to a constructor. It is **passed
//! explicitly, never global** (INV-6), so several contexts can exist in one
//! process — which is what lets the evaluation harness hold two configurations
//! side by side.
//!
//! **No privilege for built-ins (INV-7).** There is one `register_*` per
//! family and no second way in. A first-party component, a third-party crate
//! and (later) a `Remote<T>` adapter all arrive through the same call, because
//! a shortcut for built-ins is what turns a contribution funnel into a
//! two-tier system.

use std::collections::BTreeMap;

use ragondin_contracts::{ContextBuilder, Fusion, Generator, Reranker, Retriever};
use ragondin_pipeline::Params;

use crate::error::{ComponentFamily, ConstructionError, PlanError};

/// A constructor for a component of trait `T`.
///
/// It receives the node's **configuration** — the untyped parameter map — and
/// not the trait's per-call params struct (`docs/code-architecture.md` §6.3):
/// what a component needs to be *built* (a model path, a device, BM25's `k1`)
/// is fixed for its lifetime, while what varies per call travels with the call.
///
/// `Fn` rather than `FnOnce`: one registration answers every node that names
/// it, in every plan built from this context.
pub type ComponentCtor<T> = Box<dyn Fn(&Params) -> Result<Box<T>, ConstructionError> + Send + Sync>;

/// Constructs a [`Retriever`].
pub type RetrieverCtor = ComponentCtor<dyn Retriever>;
/// Constructs a [`Fusion`].
pub type FusionCtor = ComponentCtor<dyn Fusion>;
/// Constructs a [`Reranker`].
pub type RerankerCtor = ComponentCtor<dyn Reranker>;
/// Constructs a [`ContextBuilder`].
pub type ContextBuilderCtor = ComponentCtor<dyn ContextBuilder>;
/// Constructs a [`Generator`].
pub type GeneratorCtor = ComponentCtor<dyn Generator>;

/// One family's table of constructors, keyed by `impl:` name.
///
/// It carries its own [`ComponentFamily`] so that a failed lookup can say which
/// table was consulted without every call site repeating it.
struct Registry<T: ?Sized> {
    family: ComponentFamily,
    entries: BTreeMap<String, ComponentCtor<T>>,
}

impl<T: ?Sized> Registry<T> {
    fn new(family: ComponentFamily) -> Self {
        Self {
            family,
            entries: BTreeMap::new(),
        }
    }

    fn register(&mut self, name: &str, ctor: ComponentCtor<T>) {
        self.entries.insert(name.to_string(), ctor);
    }

    fn build(&self, name: &str, config: &Params) -> Result<Box<T>, PlanError> {
        let ctor = self
            .entries
            .get(name)
            .ok_or_else(|| PlanError::UnknownImpl {
                family: self.family,
                name: name.to_string(),
            })?;

        ctor(config).map_err(|source| PlanError::Construction {
            family: self.family,
            name: name.to_string(),
            source,
        })
    }
}

/// The component registry, passed explicitly to everything that needs it.
///
/// Construct one, register the components the binary was built with, and hand
/// it to physical planning. Registering is `&mut self` and building is
/// `&self`, so a context is populated once at composition time and then only
/// read.
///
/// It is shareable across threads — the harness (#29) plans two configurations
/// concurrently against one context — but the `&mut`/`&` split is not what
/// makes that sound: every stored value being `Send + Sync` is. The
/// `const _: fn()` block below is where that guarantee lives, next to what it
/// constrains rather than in a test whose deletion would remove it silently;
/// `ragondin-contracts` carries the same kind of block, over each of its
/// trait objects and `ComponentError`. Adding a field that is not `Sync` — an
/// `Rc`, a bare `RefCell` cache — breaks the build here rather than at a
/// `tokio::spawn` boundary in another crate.
pub struct EngineContext {
    retrievers: Registry<dyn Retriever>,
    fusions: Registry<dyn Fusion>,
    rerankers: Registry<dyn Reranker>,
    context_builders: Registry<dyn ContextBuilder>,
    generators: Registry<dyn Generator>,
}

impl EngineContext {
    /// An empty context, registering nothing.
    ///
    /// Empty rather than pre-populated, and that is INV-7 in the constructor:
    /// a context that arrived knowing the first-party components would make
    /// them reachable by a path a third-party crate has no equivalent of. The
    /// binary is the composition root, so the binary registers.
    pub fn new() -> Self {
        Self {
            retrievers: Registry::new(ComponentFamily::Retriever),
            fusions: Registry::new(ComponentFamily::Fusion),
            rerankers: Registry::new(ComponentFamily::Reranker),
            context_builders: Registry::new(ComponentFamily::ContextBuilder),
            generators: Registry::new(ComponentFamily::Generator),
        }
    }

    /// Registers a [`Retriever`] constructor under `name`.
    ///
    /// Registering a name twice replaces the earlier constructor: the last
    /// registration wins. A context is built by one composition root that knows
    /// what it is assembling, so a collision is that root's own doing, and
    /// refusing it would buy an error path nobody can act on.
    pub fn register_retriever(&mut self, name: &str, ctor: RetrieverCtor) {
        self.retrievers.register(name, ctor);
    }

    /// Registers a [`Fusion`] constructor under `name`. See
    /// [`register_retriever`](Self::register_retriever) on re-registration.
    pub fn register_fusion(&mut self, name: &str, ctor: FusionCtor) {
        self.fusions.register(name, ctor);
    }

    /// Registers a [`Reranker`] constructor under `name`. See
    /// [`register_retriever`](Self::register_retriever) on re-registration.
    pub fn register_reranker(&mut self, name: &str, ctor: RerankerCtor) {
        self.rerankers.register(name, ctor);
    }

    /// Registers a [`ContextBuilder`] constructor under `name`. See
    /// [`register_retriever`](Self::register_retriever) on re-registration.
    pub fn register_context_builder(&mut self, name: &str, ctor: ContextBuilderCtor) {
        self.context_builders.register(name, ctor);
    }

    /// Registers a [`Generator`] constructor under `name`. See
    /// [`register_retriever`](Self::register_retriever) on re-registration.
    pub fn register_generator(&mut self, name: &str, ctor: GeneratorCtor) {
        self.generators.register(name, ctor);
    }
}

/// The resolution half of the registry: crate-internal, because physical
/// planning is the only caller and `ragondin-engine` is not an API boundary
/// (INV-2).
///
/// That caller is [`crate::plan_physical`], which resolves each node's `impl:`
/// name through these methods — one per table, and so one per
/// [`crate::ComponentFamily`]. No method here carries a `dead_code` exemption:
/// a table planning stopped reading would show up as a warning, not sit unused.
impl EngineContext {
    /// Builds the [`Retriever`] registered under `name` from `config`.
    pub(crate) fn build_retriever(
        &self,
        name: &str,
        config: &Params,
    ) -> Result<Box<dyn Retriever>, PlanError> {
        self.retrievers.build(name, config)
    }

    /// Builds the [`Fusion`] registered under `name` from `config`.
    pub(crate) fn build_fusion(
        &self,
        name: &str,
        config: &Params,
    ) -> Result<Box<dyn Fusion>, PlanError> {
        self.fusions.build(name, config)
    }

    /// Builds the [`Reranker`] registered under `name` from `config`.
    pub(crate) fn build_reranker(
        &self,
        name: &str,
        config: &Params,
    ) -> Result<Box<dyn Reranker>, PlanError> {
        self.rerankers.build(name, config)
    }

    /// Builds the [`ContextBuilder`] registered under `name` from `config`.
    pub(crate) fn build_context_builder(
        &self,
        name: &str,
        config: &Params,
    ) -> Result<Box<dyn ContextBuilder>, PlanError> {
        self.context_builders.build(name, config)
    }

    /// Builds the [`Generator`] registered under `name` from `config`.
    pub(crate) fn build_generator(
        &self,
        name: &str,
        config: &Params,
    ) -> Result<Box<dyn Generator>, PlanError> {
        self.generators.build(name, config)
    }
}

impl Default for EngineContext {
    fn default() -> Self {
        Self::new()
    }
}

const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EngineContext>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ragondin_contracts::{
        ComponentError, ContextBuilder, ContextParams, Fusion, FusionParams, GenerateParams,
        Generator, RerankParams, Reranker, RetrieveParams, Retriever,
    };
    use ragondin_pipeline::{ParamValue, Params};
    use ragondin_types::{
        Answer, Chunk, ChunkId, Context, DocId, ModelIdentity, Query, QueryId, ScoredChunk,
    };

    fn params(pairs: &[(&str, ParamValue)]) -> Params {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    fn query(text: &str) -> Query {
        Query {
            id: QueryId::new("q1"),
            text: text.to_string(),
        }
    }

    fn scored(id: &str, score: f32) -> ScoredChunk {
        ScoredChunk {
            chunk: Chunk {
                id: ChunkId::new(id),
                text: format!("text of {id}"),
                document_id: DocId::new("doc"),
            },
            score,
        }
    }

    /// Returns `count` chunks, where `count` came from its **configuration** —
    /// the whole point of the constructor receiving `Params` (§8.1).
    struct CountingRetriever {
        count: usize,
    }

    #[async_trait]
    impl Retriever for CountingRetriever {
        async fn retrieve(
            &self,
            _query: &Query,
            params: &RetrieveParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            let n = self.count.min(params.top_k);
            Ok((0..n)
                .map(|i| scored(&format!("c{i}"), 1.0 - i as f32 / 100.0))
                .collect())
        }
    }

    /// Reads `count` out of the node's configuration, and refuses a value it
    /// cannot read — the failure a constructor must be able to report.
    fn counting_retriever(config: &Params) -> Result<Box<dyn Retriever>, ConstructionError> {
        let count = match config.get("count") {
            None => 0,
            Some(ParamValue::Int(n)) if *n >= 0 => *n as usize,
            Some(other) => {
                return Err(
                    format!("`count` must be a non-negative integer, found {other:?}").into(),
                )
            }
        };
        Ok(Box::new(CountingRetriever { count }))
    }

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

    struct TruncatingReranker;

    #[async_trait]
    impl Reranker for TruncatingReranker {
        async fn rerank(
            &self,
            _query: &Query,
            mut chunks: Vec<ScoredChunk>,
            params: &RerankParams,
        ) -> Result<Vec<ScoredChunk>, ComponentError> {
            chunks.truncate(params.top_k);
            Ok(chunks)
        }
    }

    #[tokio::test]
    async fn a_registered_retriever_is_built_and_dispatches() {
        let mut ctx = EngineContext::new();
        ctx.register_retriever("counting", Box::new(counting_retriever));

        let retriever = ctx
            .build_retriever("counting", &params(&[("count", ParamValue::Int(3))]))
            .expect("the name is registered");
        let hits = retriever
            .retrieve(&query("anything"), &RetrieveParams::new(10))
            .await
            .expect("the stub does not fail");

        assert_eq!(hits.len(), 3, "the constructor's configuration reached it");
    }

    #[tokio::test]
    async fn a_constructor_reads_the_nodes_configuration_not_the_per_call_params() {
        // §6.3: `count` configures the implementation and is fixed for the
        // component's lifetime; `top_k` varies per call. Two calls to one
        // component, one configuration, two different per-call answers.
        let mut ctx = EngineContext::new();
        ctx.register_retriever("counting", Box::new(counting_retriever));
        let retriever = ctx
            .build_retriever("counting", &params(&[("count", ParamValue::Int(5))]))
            .expect("the name is registered");

        let wide = retriever
            .retrieve(&query("q"), &RetrieveParams::new(10))
            .await
            .expect("the stub does not fail");
        let narrow = retriever
            .retrieve(&query("q"), &RetrieveParams::new(2))
            .await
            .expect("the stub does not fail");

        assert_eq!(wide.len(), 5, "configuration caps the retriever at 5");
        assert_eq!(narrow.len(), 2, "the per-call top_k caps this call at 2");
    }

    #[test]
    fn building_an_unregistered_name_is_a_typed_error_not_a_panic() {
        let ctx = EngineContext::new();

        let Err(err) = ctx.build_retriever("nowhere", &Params::new()) else {
            panic!("nothing is registered under that name")
        };

        assert!(
            matches!(
                &err,
                PlanError::UnknownImpl { family: ComponentFamily::Retriever, name } if name == "nowhere"
            ),
            "expected UnknownImpl, got {err:?}"
        );
    }

    #[test]
    fn a_name_registered_for_another_family_does_not_satisfy_this_one() {
        // The family is part of the lookup, which is why it is part of the
        // error: `impl: counting` under the wrong node kind must not resolve.
        let mut ctx = EngineContext::new();
        ctx.register_retriever("counting", Box::new(counting_retriever));

        let Err(err) = ctx.build_fusion("counting", &Params::new()) else {
            panic!("no fusion is registered under that name")
        };

        assert!(
            matches!(
                &err,
                PlanError::UnknownImpl { family: ComponentFamily::Fusion, name } if name == "counting"
            ),
            "expected a Fusion UnknownImpl, got {err:?}"
        );
    }

    #[test]
    fn a_constructor_that_cannot_build_reports_a_typed_construction_failure() {
        let mut ctx = EngineContext::new();
        ctx.register_retriever("counting", Box::new(counting_retriever));

        let config = params(&[("count", ParamValue::String("many".into()))]);
        let Err(err) = ctx.build_retriever("counting", &config) else {
            panic!("`count` is not an integer, so the constructor must refuse")
        };

        assert!(
            matches!(
                &err,
                PlanError::Construction { family: ComponentFamily::Retriever, name, .. } if name == "counting"
            ),
            "expected Construction, got {err:?}"
        );
        assert!(
            std::error::Error::source(&err).is_some(),
            "the constructor's own error stays reachable as the source"
        );
        assert!(
            err.to_string().contains("non-negative integer"),
            "the cause must appear in Display, not only via source(): {err}"
        );
    }

    #[tokio::test]
    async fn two_contexts_coexist_in_one_process_with_different_registrations() {
        // The payoff of INV-6: the harness compares two configurations side by
        // side in one process, which a global registry could not express.
        let mut baseline = EngineContext::new();
        baseline.register_retriever("counting", Box::new(counting_retriever));

        let mut candidate = EngineContext::new();
        candidate.register_retriever("other", Box::new(counting_retriever));

        let config = params(&[("count", ParamValue::Int(1))]);

        assert!(baseline.build_retriever("counting", &config).is_ok());
        assert!(candidate.build_retriever("other", &config).is_ok());
        assert!(
            baseline.build_retriever("other", &config).is_err(),
            "one context does not see the other's registrations"
        );
        assert!(
            candidate.build_retriever("counting", &config).is_err(),
            "and the reverse holds too"
        );
    }

    #[test]
    fn re_registering_a_name_replaces_the_previous_constructor() {
        let mut ctx = EngineContext::new();
        ctx.register_retriever("counting", Box::new(counting_retriever));
        ctx.register_retriever(
            "counting",
            Box::new(|_: &Params| Err("this one always refuses".into())),
        );

        assert!(
            ctx.build_retriever("counting", &Params::new()).is_err(),
            "the last registration under a name is the one that answers"
        );
    }

    #[tokio::test]
    async fn every_family_registers_and_builds_through_the_same_mechanism() {
        // INV-7 in one test: every family the registry keeps, one shape of
        // call, no privileged path for any of them.
        let mut ctx = EngineContext::new();
        ctx.register_retriever("counting", Box::new(counting_retriever));
        ctx.register_fusion("concat", Box::new(|_| Ok(Box::new(ConcatFusion))));
        ctx.register_reranker("truncating", Box::new(|_| Ok(Box::new(TruncatingReranker))));

        let config = Params::new();
        let one = vec![scored("a", 1.0)];

        let fused = ctx
            .build_fusion("concat", &config)
            .expect("registered")
            .fuse(vec![one.clone(), one.clone()], &FusionParams::new())
            .await
            .expect("the stub does not fail");
        assert_eq!(fused.len(), 2);

        let reranked = ctx
            .build_reranker("truncating", &config)
            .expect("registered")
            .rerank(&query("q"), fused, &RerankParams::new(1))
            .await
            .expect("the stub does not fail");
        assert_eq!(reranked.len(), 1);

        assert!(ctx.build_retriever("counting", &config).is_ok());
    }

    struct EmptyBuilder;

    #[async_trait]
    impl ContextBuilder for EmptyBuilder {
        async fn build(
            &self,
            _query: &Query,
            chunks: Vec<ScoredChunk>,
            _params: &ContextParams,
        ) -> Result<Context, ComponentError> {
            Ok(Context {
                chunks: Vec::new(),
                text: format!("{} chunks", chunks.len()),
            })
        }

        async fn model_identity(&self) -> Result<ModelIdentity, ComponentError> {
            Ok(ModelIdentity::new("empty"))
        }
    }

    struct FixedGenerator;

    #[async_trait]
    impl Generator for FixedGenerator {
        async fn generate(
            &self,
            _query: &Query,
            context: &Context,
            params: &GenerateParams,
        ) -> Result<Answer, ComponentError> {
            Ok(Answer {
                text: format!("{} from {}", params.served_model, context.text),
            })
        }

        async fn model_identity(
            &self,
            served_model: &str,
        ) -> Result<ModelIdentity, ComponentError> {
            Ok(ModelIdentity::new(served_model))
        }
    }

    #[tokio::test]
    async fn the_generation_families_register_and_build_through_the_same_mechanism() {
        let mut ctx = EngineContext::new();
        ctx.register_context_builder("empty", Box::new(|_| Ok(Box::new(EmptyBuilder))));
        ctx.register_generator("fixed", Box::new(|_| Ok(Box::new(FixedGenerator))));

        let config = Params::new();
        let context = ctx
            .build_context_builder("empty", &config)
            .expect("registered")
            .build(&query("q"), vec![scored("a", 1.0)], &ContextParams::new(1))
            .await
            .expect("the stub does not fail");
        let answer = ctx
            .build_generator("fixed", &config)
            .expect("registered")
            .generate(
                &query("q"),
                &context,
                &GenerateParams::new("m", "{context}"),
            )
            .await
            .expect("the stub does not fail");

        assert_eq!(answer.text, "m from 1 chunks");
    }

    #[test]
    fn an_unknown_generation_impl_names_its_family() {
        let ctx = EngineContext::new();

        let Err(builder) = ctx.build_context_builder("templated", &Params::new()) else {
            panic!("nothing is registered under that name")
        };
        let Err(generator) = ctx.build_generator("vllm", &Params::new()) else {
            panic!("nothing is registered under that name")
        };

        assert!(matches!(
            &builder,
            PlanError::UnknownImpl { family: ComponentFamily::ContextBuilder, name } if name == "templated"
        ));
        assert!(matches!(
            &generator,
            PlanError::UnknownImpl { family: ComponentFamily::Generator, name } if name == "vllm"
        ));
        assert_eq!(
            builder.to_string(),
            "no context_builder implementation is registered under `templated`",
            "the family is named as a configuration writes its `component:`"
        );
        assert_eq!(
            generator.to_string(),
            "no generator implementation is registered under `vllm`"
        );
    }

    #[test]
    fn an_unknown_impl_error_names_both_the_family_and_the_name() {
        let ctx = EngineContext::new();
        let Err(err) = ctx.build_reranker("cross-encoder", &Params::new()) else {
            panic!("nothing is registered under that name")
        };

        let message = err.to_string();
        assert!(
            message.contains("reranker") && message.contains("cross-encoder"),
            "a diagnostic that names neither is not actionable: {message}"
        );
    }
}
