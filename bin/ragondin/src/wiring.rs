//! The composition root's bridge: a node's untyped `Params` on one side, a
//! constructed component on the other.
//!
//! `docs/code-architecture.md` §6.3 puts this bridge in the binary and nowhere
//! else: a component crate is a leaf and never sees `ragondin-pipeline`, so the
//! translation from a configuration map to a typed constructor argument is
//! written here — exactly as a third party would write it for a component of
//! their own (INV-7). Every registration below goes through the ordinary
//! `register_*` call, and the engine depends on none of the crates they
//! construct (INV-5).
//!
//! # What is compiled, and what is only configured
//!
//! Everything that reads a configuration is compiled unconditionally, because
//! it reads *text* and needs no backend: a pipeline that names `dense` is
//! inspected the same way whether or not this build can run one. Only the
//! constructors are feature-gated (ADR-C14), so a lean build still refuses a
//! configuration with a diagnosis rather than a mystery. For a node family the
//! unknown `impl:` comes back from planning, naming the family and the name it
//! looked up. An embedder is not a node family and no plan ever looks one up,
//! so [`check_nodes`] names what is missing itself: an `embedder:` name it
//! does not know, or the feature that would have carried the one it does
//! (ADR-C32 § 4).
//!
//! # Bound components
//!
//! A name bound with `--remote <family>/<name>=<uri>` ([`crate::binding`]) is
//! one this composition root knows as well as its `Local` ones: [`Bound`]
//! holds one lazily connecting channel per binding, [`register`] makes one
//! `register_*` call per binding of a node family, and a bound embedder is
//! resolved inside the `dense` constructor closure (ADR-C32 § 3). A build
//! without the `remote` feature refuses every binding when it is parsed, so
//! in that build nothing here is ever bound.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use ragondin_contracts::EmbeddedChunk;
use ragondin_engine::EngineContext;
use ragondin_pipeline::{LogicalNode, LogicalPipeline, ParamValue, Params};
use ragondin_types::Chunk;

use crate::binding::{Bindings, Family};

/// The `impl:` name of the in-process BM25 retriever.
const BM25: &str = "bm25";
/// The `impl:` name of the dense retriever (an embedder over a vector store).
pub const DENSE: &str = "dense";
/// The `impl:` name of Reciprocal Rank Fusion.
const RRF: &str = "rrf";
/// The `impl:` name of the ONNX cross-encoder reranker.
const CROSS_ENCODER: &str = "cross_encoder";
/// The `impl:` name of the concatenating context builder.
const CONCAT: &str = "concat";
/// The `impl:` name of the stub generator, the one generator a build of this
/// binary can carry in-process. It exists for tests: no production `Local`
/// generator exists, a generator being `Remote` by design (ADR-C31).
const STUB_GENERATOR: &str = "stub_generator";

/// The `embedder:` name of the in-process ONNX embedder (ADR-C32 § 1).
const ONNX_EMBEDDER: &str = "onnx";

/// Every name this composition root gives a `Local` component, by family, in
/// **any** build of it — whatever features this one carries. `--remote`
/// refuses to bind one (ADR-C32 § 2): the registry's last registration wins,
/// so a binding would silently replace the `Local` component, and a list that
/// followed this build's features would let one command line mean two things.
const LOCAL: [(Family, &str); 7] = [
    (Family::Retriever, BM25),
    (Family::Retriever, DENSE),
    (Family::Fusion, RRF),
    (Family::Reranker, CROSS_ENCODER),
    (Family::ContextBuilder, CONCAT),
    (Family::Generator, STUB_GENERATOR),
    (Family::Embedder, ONNX_EMBEDDER),
];

/// Whether this composition root gives `name` to a `Local` component of
/// `family` in any build of it.
pub fn is_local(family: Family, name: &str) -> bool {
    LOCAL.contains(&(family, name))
}

/// The `Local` components **this** build carries, by family, in [`LOCAL`]'s
/// order: its entries whose feature is on. What `ragondin ui` reports as the
/// build's capabilities. `dense` is carried by a build that can construct an
/// embedder for it, `onnx` or `remote`, which is when [`register`] can
/// register it.
#[cfg(feature = "ui")]
pub fn carried() -> impl Iterator<Item = (Family, &'static str)> {
    LOCAL.into_iter().filter(|(_, name)| match *name {
        BM25 => cfg!(feature = "bm25"),
        DENSE => cfg!(any(feature = "onnx", feature = "remote")),
        CROSS_ENCODER | ONNX_EMBEDDER => cfg!(feature = "onnx"),
        STUB_GENERATOR => cfg!(feature = "stub"),
        // Normal dependencies, in every build.
        RRF | CONCAT => true,
        // An entry added to `LOCAL` and not here is reported by no build;
        // the `--all-features` capabilities test lists every entry.
        _ => false,
    })
}

/// The keys every `dense` node may carry, whatever embedder it names
/// (ADR-C32 § 1). `top_k` is the executor's; the rest are this file's.
const DENSE_KEYS: [&str; 4] = ["top_k", "embedder", "query_prefix", "passage_prefix"];
/// The keys an ONNX model-bearing node adds: the model, its tokenizer and its
/// token budget. Shared by a `dense` node over the ONNX embedder and a
/// `cross_encoder` node, which read the same three.
const ONNX_KEYS: [&str; 3] = ["model", "tokenizer", "max_sequence_length"];
/// The key a node over a bound embedder or reranker adds, and requires: the
/// name its service serves the model under (ADR-C32 § 1).
const SERVED_MODEL: &str = "served_model";

/// The role an embedder's model plays in run identity
/// (`docs/system-architecture.md` §7.1). Only a build that can construct an
/// embedder or a reranker — the ONNX ones, or bound ones — reads an identity
/// under this role or the next.
#[cfg(any(feature = "onnx", feature = "remote"))]
const EMBEDDER_ROLE: &str = "embedder";
/// The role a reranker's model plays in run identity.
#[cfg(any(feature = "onnx", feature = "remote"))]
const RERANKER_ROLE: &str = "reranker";
/// The role of a context builder's identity (ADR-C31 § 4).
const CONTEXT_BUILDER_ROLE: &str = "context_builder";
/// The role of a generator's identity (ADR-C31 § 4). Read only in a build
/// that carries a generator: the `stub` one, or one that can bind a `Remote`
/// generator.
#[cfg(any(feature = "stub", feature = "remote", test))]
const GENERATOR_ROLE: &str = "generator";

/// A model and the tokenizer that feeds it, as a node configures them.
///
/// Both ONNX components take the same pair plus a token budget, so one type
/// carries what a node says about either. It holds paths and numbers and no
/// backend type at all, which is what lets this file be read in a build that
/// cannot construct the component it describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelSpec {
    /// The ONNX model file.
    pub model: PathBuf,
    /// Its `tokenizer.json`.
    pub tokenizer: PathBuf,
    /// The longest encoded text the model is given, in tokens. `None` leaves
    /// the component's own default in place — the binary applies none of its
    /// own, because a default filled in here could only be a second copy of
    /// the component's, free to disagree (`docs/code-architecture.md` §6.3).
    pub max_sequence_length: Option<usize>,
}

/// What a `dense` node says about the embedder it retrieves through, by the
/// nature of the embedder its `embedder:` names (ADR-C32 § 1).
///
/// The prefixes travel with the embedder because they decide the vectors: two
/// nodes naming one model under different prefixes are two embedders, and the
/// corpus one of them indexed is not the corpus the other would search
/// (ADR-C17). Each prefix is empty when the node carries none, which is the
/// only spelling of "none" — an empty value is refused when the node is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmbedderSpec {
    /// `embedder: onnx`, the in-process ONNX embedder.
    Onnx {
        /// The model and its tokenizer.
        model: ModelSpec,
        /// Prepended to a query before it is embedded.
        query_prefix: String,
        /// Prepended to a passage before it is embedded.
        passage_prefix: String,
    },
    /// A name bound with `--remote embedder/<name>=<uri>`: the `Remote`
    /// embedder over that binding, which applies the prefixes itself.
    Bound {
        /// The `embedder:` name, and the binding's.
        name: String,
        /// The name the service serves the model under.
        served_model: String,
        /// Prepended to a query before it is sent.
        query_prefix: String,
        /// Prepended to a passage before it is sent.
        passage_prefix: String,
    },
}

impl EmbedderSpec {
    /// The served model every call to this embedder names: `None` for the
    /// ONNX embedder, which answers only for the model it loaded, and the
    /// node's `served_model` for a bound one (ADR-C32 § 4). Compiled only
    /// where an embedder can be constructed, which is where it is read.
    #[cfg(any(feature = "onnx", feature = "remote"))]
    pub fn served_model(&self) -> Option<&str> {
        match self {
            Self::Onnx { .. } => None,
            Self::Bound { served_model, .. } => Some(served_model),
        }
    }
}

/// The bindings of one `bench`, and — in a build with the `remote` feature —
/// the channel each one is served over.
///
/// One channel per binding, built once, lazily connecting, and cloned into
/// every constructor that names the binding, so every node naming it shares
/// it (ADR-C32 § 3). Nothing connects here: an unreachable service is reported
/// at its first call, which is the identity read of [`model_hashes`].
#[derive(Clone, Debug)]
pub struct Bound {
    bindings: Bindings,
    #[cfg(feature = "remote")]
    channels: BTreeMap<(Family, String), tonic::transport::Channel>,
}

impl Bound {
    /// Builds a lazily connecting channel for each binding. With the
    /// `remote` feature, it must be called inside a Tokio runtime, because a
    /// lazy channel spawns the task that will connect it.
    pub fn new(bindings: Bindings) -> Result<Self> {
        Self::build(
            bindings,
            #[cfg(feature = "remote")]
            None,
        )
    }

    /// Builds the channels as [`new`](Self::new) does, each giving up on
    /// connecting after `timeout`: the identity probe of `ragondin ui`, which
    /// answers a person waiting on a screen, would otherwise wait out the
    /// operating system's TCP timeout against an address that drops packets.
    /// `bench` keeps [`new`](Self::new)'s behaviour.
    #[cfg(all(feature = "remote", feature = "ui"))]
    pub fn with_connect_timeout(bindings: Bindings, timeout: std::time::Duration) -> Result<Self> {
        Self::build(bindings, Some(timeout))
    }

    fn build(
        bindings: Bindings,
        #[cfg(feature = "remote")] connect_timeout: Option<std::time::Duration>,
    ) -> Result<Self> {
        #[cfg(feature = "remote")]
        let channels = bindings
            .iter()
            .map(|binding| {
                let mut endpoint = tonic::transport::Endpoint::from_shared(binding.uri.clone())
                    .with_context(|| format!("`{}/{}`", binding.family, binding.name))?;
                if let Some(timeout) = connect_timeout {
                    endpoint = endpoint.connect_timeout(timeout);
                }
                Ok((
                    (binding.family, binding.name.clone()),
                    endpoint.connect_lazy(),
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            bindings,
            #[cfg(feature = "remote")]
            channels,
        })
    }

    /// The bindings.
    pub fn bindings(&self) -> &Bindings {
        &self.bindings
    }

    /// The channel `name` is bound to in `family`.
    #[cfg(feature = "remote")]
    fn channel(&self, family: Family, name: &str) -> Result<tonic::transport::Channel> {
        self.channels
            .get(&(family, name.to_owned()))
            .cloned()
            .with_context(|| format!("`{family}/{name}` is not bound"))
    }
}

/// Registers the components this build carries, over the corpus the caller
/// prepared.
///
/// Every call below is one a crate outside this workspace could write
/// verbatim: one `register_*` per component, with a constructor that reads the
/// node's `Params` and nothing else (INV-7). There is no fast path for a
/// first-party component because there is no second way in. The engine depends
/// on none of the crates constructed here (INV-5) — the binary does, which is
/// what `docs/code-architecture.md` §4.3 means by composition root.
///
/// `chunks` is the corpus the run retrieves over, and `embedded` the same
/// corpus already turned into vectors — `None` when the pipeline names no
/// dense node, or when this build carries no embedder to have made them. Both
/// come from the caller because ingestion happens before construction
/// (ADR-C26): a `ComponentCtor` is synchronous, so a store that has to be
/// filled by an `async` upsert cannot be filled inside one.
///
/// `bound` adds one registration per binding of a node family, under the
/// bound name, of the `Remote` adapter over the binding's channel; an
/// `embedder` binding is resolved inside the `dense` closure instead, since no
/// plan ever looks an embedder up.
///
/// Registration is infallible on purpose: a constructor's failure belongs to
/// the plan that names it, where planning reports it against the node
/// (`PlanError::Construction`). Refusing here would refuse a component this
/// configuration may never mention.
pub fn register(
    ctx: &mut EngineContext,
    // Each corpus argument is read by one backend feature only, so a build
    // without that feature has nothing to hand it to. The lint is lifted on
    // the one parameter, under the one feature's absence, and as an
    // expectation: a build where the parameter stops being unused fails.
    #[cfg_attr(not(feature = "bm25"), expect(unused_variables))] chunks: &[Chunk],
    #[cfg_attr(
        not(any(feature = "onnx", feature = "remote")),
        expect(unused_variables)
    )]
    embedded: Option<&[EmbeddedChunk]>,
    #[cfg_attr(
        not(any(feature = "onnx", feature = "remote")),
        expect(unused_variables)
    )]
    bound: &Bound,
) {
    // Always: rank arithmetic over the legs, with no backend behind it.
    ctx.register_fusion(
        RRF,
        Box::new(|params| {
            let k = optional_usize(params, "k")?;
            Ok(Box::new(match k {
                Some(k) => ragondin_fusion_rrf::ReciprocalRankFusion::new(k),
                None => ragondin_fusion_rrf::ReciprocalRankFusion::default(),
            }))
        }),
    );

    // Always, for the reason RRF is: string arithmetic, no backend.
    ctx.register_context_builder(CONCAT, Box::new(|params| Ok(Box::new(concat(params)?))));

    #[cfg(feature = "stub")]
    ctx.register_generator(
        STUB_GENERATOR,
        Box::new(|params| Ok(Box::new(stub_generator(params)?))),
    );

    #[cfg(feature = "bm25")]
    {
        // The index is built in the constructor, from exactly the chunks
        // `CorpusIndex::version` names — which is what makes the
        // `index_version` the run records true of what was searched.
        let corpus = chunks.to_vec();
        ctx.register_retriever(
            BM25,
            Box::new(move |_params| {
                Ok(Box::new(ragondin_retriever_bm25::Bm25Retriever::new(
                    corpus.clone(),
                )?))
            }),
        );
    }

    // Registered only when the corpus was embedded: a dense retriever over an
    // empty store answers every query with nothing, which is a wrong number
    // rather than an error. With no entries there is no dense node to answer,
    // and planning says so by name.
    #[cfg(any(feature = "onnx", feature = "remote"))]
    if let Some(entries) = embedded {
        let entries = entries.to_vec();
        let bound = bound.clone();
        ctx.register_retriever(
            DENSE,
            Box::new(move |params| {
                // The `embedder:` name is resolved here, inside the closure
                // this composition root writes (ADR-C32 § 3): `onnx`, or a
                // name bound with `--remote embedder/<name>=<uri>`.
                let spec = embedder_of(params, bound.bindings())?;
                let store = ragondin_store_memory::MemoryVectorStore::seeded(entries.clone())?;
                let retriever = ragondin_retriever_dense::DenseRetriever::new(
                    embedder(&spec, &bound)?,
                    Box::new(store),
                );
                Ok(Box::new(match spec.served_model() {
                    Some(served_model) => retriever.with_served_model(served_model),
                    None => retriever,
                }))
            }),
        );
    }

    #[cfg(feature = "onnx")]
    ctx.register_reranker(
        CROSS_ENCODER,
        Box::new(|params| Ok(Box::new(onnx_reranker(params)?))),
    );

    // One registration per binding of a node family, through the call a
    // `Local` component uses (INV-7). The adapter reads no parameter: the
    // executor hands a bound node its `top_k` and `served_model` per call.
    #[cfg(feature = "remote")]
    for ((family, name), channel) in &bound.channels {
        let channel = channel.clone();
        match family {
            Family::Retriever => ctx.register_retriever(
                name,
                Box::new(move |_params| {
                    Ok(Box::new(ragondin_remote::RemoteRetriever::new(
                        channel.clone(),
                    )))
                }),
            ),
            Family::Fusion => ctx.register_fusion(
                name,
                Box::new(move |_params| {
                    Ok(Box::new(ragondin_remote::RemoteFusion::new(
                        channel.clone(),
                    )))
                }),
            ),
            Family::Reranker => ctx.register_reranker(
                name,
                Box::new(move |_params| {
                    Ok(Box::new(ragondin_remote::RemoteReranker::new(
                        channel.clone(),
                    )))
                }),
            ),
            Family::ContextBuilder => ctx.register_context_builder(
                name,
                Box::new(move |_params| {
                    Ok(Box::new(ragondin_remote::RemoteContextBuilder::new(
                        channel.clone(),
                    )))
                }),
            ),
            Family::Generator => ctx.register_generator(
                name,
                Box::new(move |_params| {
                    Ok(Box::new(ragondin_remote::RemoteGenerator::new(
                        channel.clone(),
                    )))
                }),
            ),
            // Resolved inside the `dense` closure above.
            Family::Embedder => {}
        }
    }
}

/// The embedder `spec` describes: the ONNX one with its model session loaded,
/// or the `Remote` adapter over the binding's channel with the node's prefixes.
///
/// A spec this build cannot construct is refused, naming the feature; neither
/// case is reached from `bench`, since [`check_nodes`] refuses the `onnx`
/// embedder in a build without it and a build without `remote` refuses every
/// binding.
#[cfg(any(feature = "onnx", feature = "remote"))]
pub fn embedder(
    spec: &EmbedderSpec,
    #[cfg_attr(not(feature = "remote"), expect(unused_variables))] bound: &Bound,
) -> Result<Box<dyn ragondin_contracts::Embedder>> {
    match spec {
        #[cfg(feature = "onnx")]
        EmbedderSpec::Onnx {
            model,
            query_prefix,
            passage_prefix,
        } => Ok(Box::new(onnx_embedder(
            model,
            query_prefix,
            passage_prefix,
        )?)),
        #[cfg(not(feature = "onnx"))]
        EmbedderSpec::Onnx { .. } => bail!(
            "the `{ONNX_EMBEDDER}` embedder is not in this build: rebuild with the `onnx` feature"
        ),
        #[cfg(feature = "remote")]
        EmbedderSpec::Bound {
            name,
            query_prefix,
            passage_prefix,
            ..
        } => Ok(Box::new(ragondin_remote::RemoteEmbedder::new(
            bound.channel(Family::Embedder, name)?,
            query_prefix.as_str(),
            passage_prefix.as_str(),
        ))),
        #[cfg(not(feature = "remote"))]
        EmbedderSpec::Bound { name, .. } => bail!(
            "the embedder `{name}` is bound, and this build cannot construct a `Remote` \
             component: rebuild with the `remote` feature"
        ),
    }
}

/// The ONNX embedder over `model`, with its session loaded.
#[cfg(feature = "onnx")]
fn onnx_embedder(
    model: &ModelSpec,
    query_prefix: &str,
    passage_prefix: &str,
) -> Result<ragondin_embedder_onnx::OnnxEmbedder> {
    let mut config =
        ragondin_embedder_onnx::OnnxEmbedderConfig::new(&model.model, &model.tokenizer)
            .with_prefixes(query_prefix, passage_prefix);
    if let Some(tokens) = model.max_sequence_length {
        config = config.with_max_sequence_length(nonzero(tokens, "max_sequence_length")?);
    }
    Ok(ragondin_embedder_onnx::OnnxEmbedder::new(config)?)
}

/// The cross-encoder a `cross_encoder` node configures, with its model session
/// loaded.
#[cfg(feature = "onnx")]
fn onnx_reranker(params: &Params) -> Result<ragondin_reranker_onnx::OnnxReranker> {
    let spec = reranker_of(params)?;
    let mut config = ragondin_reranker_onnx::OnnxRerankerConfig::new(&spec.model, &spec.tokenizer);
    if let Some(tokens) = spec.max_sequence_length {
        config.max_sequence_length = nonzero(tokens, "max_sequence_length")?;
    }
    Ok(ragondin_reranker_onnx::OnnxReranker::new(config)?)
}

/// The concatenating context builder a `concat` node configures.
///
/// `separator` is required, and may be empty. A default would make an absent
/// key and the default's own spelling two configurations of one builder with
/// two hashes — the reason ADR-C32 § 1 gives `embedder:` no default — and the
/// builder has no default of its own for this file to defer to.
fn concat(params: &Params) -> Result<ragondin_context_concat::ConcatContextBuilder> {
    Ok(ragondin_context_concat::ConcatContextBuilder::new(
        required_string(params, "separator")?,
    ))
}

/// The stub generator a `stub_generator` node configures: it serves the node's
/// `served_model`, the name the executor passes it on every call.
#[cfg(feature = "stub")]
fn stub_generator(params: &Params) -> Result<ragondin_stub::StubGenerator> {
    Ok(ragondin_stub::StubGenerator::new(required_string(
        params,
        SERVED_MODEL,
    )?))
}

/// A count a component takes as a [`NonZeroUsize`](std::num::NonZeroUsize).
///
/// Zero is refused here rather than cast, for the reason `optional_usize`
/// refuses a negative: the configuration is where a nonsensical count is still
/// attached to the key that carries it.
#[cfg(feature = "onnx")]
fn nonzero(value: usize, key: &str) -> Result<std::num::NonZeroUsize> {
    std::num::NonZeroUsize::new(value).ok_or_else(|| anyhow::anyhow!("`{key}` must not be zero"))
}

/// Refuses a pipeline holding a node this binary does not run.
///
/// Retrieval and generation have primitive nodes, which planning resolves by
/// family (ADR-C31 § 3). A judge and control flow do not, and until one does
/// it would arrive as an `Extension` node (ADR-C3), which nothing here can
/// run — so this is the whole of the check.
pub fn refuse_unsupported(pipeline: &LogicalPipeline) -> Result<()> {
    let extensions: Vec<&str> = pipeline
        .nodes()
        .iter()
        .filter_map(|node| match node {
            LogicalNode::Extension(node) => Some(node.id.as_str()),
            _ => None,
        })
        .collect();

    if !extensions.is_empty() {
        bail!(
            "extension nodes are not supported in v0: {}. \
             v0 runs retrieval and generation — no judge, no control flow",
            extensions.join(", ")
        );
    }
    Ok(())
}

/// Checks the keys of every node whose keys this composition root owns,
/// before anything is loaded (ADR-C32 § 1, and step 1 of its § 4).
///
/// A `dense` node must name its embedder with `embedder:` — `onnx`, or a name
/// in `bindings` — carry only the keys that embedder's nature reads, and never
/// an empty prefix; a `cross_encoder` node only the keys the ONNX reranker
/// reads, and a reranker node under a bound name only `top_k` and its required
/// `served_model`. A key outside those sets is refused rather than hashed as
/// inert: it would move the run's identity while changing nothing the run did.
/// Build-independent, except for one refusal: `embedder: onnx` in a build
/// without the `onnx` feature is named here, because no planner will ever look
/// an embedder up to name it.
pub fn check_nodes(pipeline: &LogicalPipeline, bindings: &Bindings) -> Result<()> {
    check_keys(pipeline, bindings).map_err(KeyRefusal::into_error)?;
    let embedder = embedder_spec(pipeline, bindings)?;
    if matches!(embedder, Some(EmbedderSpec::Onnx { .. })) && !cfg!(feature = "onnx") {
        let node = pipeline
            .nodes()
            .iter()
            .find_map(|node| match node {
                LogicalNode::Retriever(node) if node.implementation == DENSE => {
                    Some(node.id.as_str())
                }
                _ => None,
            })
            .unwrap_or_default();
        bail!(
            "node `{node}` names the `{ONNX_EMBEDDER}` embedder, which this build does not \
             carry: rebuild with the `onnx` feature"
        );
    }
    Ok(())
}

/// A key refusal of [`check_keys`]: the node at fault, when one node is, and
/// the refusal in `bench`'s words.
#[derive(Debug)]
pub struct KeyRefusal {
    /// The node whose keys are refused; `None` when the refusal is about two
    /// nodes together, which its words name.
    pub node: Option<String>,
    /// The refusal.
    pub error: anyhow::Error,
}

impl KeyRefusal {
    /// The refusal as `bench` reports it: under the context naming the node,
    /// when one node is at fault.
    pub fn into_error(self) -> anyhow::Error {
        match self.node {
            Some(node) => self.error.context(format!("node `{node}`")),
            None => self.error,
        }
    }
}

/// The keys of every node whose keys this composition root owns — the part
/// of [`check_nodes`] that holds in every build: a `dense` node's keys by the
/// nature of the embedder it names, a `cross_encoder` node's, a bound
/// reranker's, and the agreement of every `dense` node on one embedder
/// (ADR-C32 § 1). `ragondin ui` asks it of a document before storing it, so
/// what is stored is what `bench` would accept.
pub fn check_keys(pipeline: &LogicalPipeline, bindings: &Bindings) -> Result<(), KeyRefusal> {
    for node in pipeline.nodes() {
        let checked = match node {
            LogicalNode::Retriever(node) if node.implementation == DENSE => {
                embedder_of(&node.params, bindings).map(drop)
            }
            LogicalNode::Reranker(node) if node.implementation == CROSS_ENCODER => {
                reranker_of(&node.params).map(drop)
            }
            LogicalNode::Reranker(node)
                if bindings.binds(Family::Reranker, &node.implementation) =>
            {
                bound_reranker_of(&node.params, &node.implementation).map(drop)
            }
            _ => Ok(()),
        };
        checked.map_err(|error| KeyRefusal {
            node: Some(node.id().as_str().to_owned()),
            error,
        })?;
    }
    // Every node's own keys are sound; what is left is two `dense` nodes
    // disagreeing, which names both.
    embedder_spec(pipeline, bindings)
        .map(drop)
        .map_err(|error| KeyRefusal { node: None, error })
}

/// The embedder every `dense` node of `pipeline` is configured with, if it has
/// one.
///
/// **One embedder per pipeline, in v0.** The corpus is embedded once, before
/// the components are constructed, so two `dense` nodes disagreeing about the
/// model, the tokenizer or a prefix would need two indexes — and a run that
/// searched two indexes has one `index_version` naming neither. Refusing it
/// says so; embedding twice would quietly make the recorded identity false.
pub fn embedder_spec(
    pipeline: &LogicalPipeline,
    bindings: &Bindings,
) -> Result<Option<EmbedderSpec>> {
    let mut found: Option<(&str, EmbedderSpec)> = None;

    for node in pipeline.nodes() {
        let LogicalNode::Retriever(node) = node else {
            continue;
        };
        if node.implementation != DENSE {
            continue;
        }

        let spec = embedder_of(&node.params, bindings)
            .with_context(|| format!("node `{}`", node.id.as_str()))?;

        match &found {
            Some((first, seen)) if *seen != spec => bail!(
                "nodes `{first}` and `{}` configure different embedders, and v0 embeds the \
                 corpus once: evaluate them as two pipelines",
                node.id.as_str()
            ),
            Some(_) => {}
            None => found = Some((node.id.as_str(), spec)),
        }
    }

    Ok(found.map(|(_, spec)| spec))
}

/// The model identities of the run, by the role each component played
/// (`docs/system-architecture.md` §7.1; ADR-C32 § 4, step 2).
///
/// Each is read from the component itself, through its trait's
/// `model_identity`, on an instance constructed here for that purpose and
/// then dropped — registration constructs another. The key is the node's
/// **family**, never its `impl:` name: `embedder` for a `dense` node's
/// embedder, `reranker`, `context_builder` and `generator`. A node whose name
/// this build cannot construct is skipped, and planning's unknown `impl:`
/// names it. A role is recorded once, so two nodes on one role must report the
/// same identity — the rule [`embedder_spec`] states for the corpus, seen from
/// the identity side.
///
/// The names this build knows are its `Local` ones and those in `bound`. An
/// embedder or reranker is read with its node's served model — `None` over
/// the ONNX ones, the node's `served_model` over a bound one — and a
/// generator's identity with its node's `served_model` (ADR-C31 § 4), which is
/// required: its absence is refused here, before the run, rather than passed
/// on as an empty name for the component to refuse. Any refusal here ends the
/// run before anything expensive has been done — which is also what finds a
/// missing model file, an unreachable service or a model a service does not
/// serve early.
pub async fn model_hashes(
    pipeline: &LogicalPipeline,
    bound: &Bound,
) -> Result<BTreeMap<String, String>> {
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();

    for node in pipeline.nodes() {
        let id = node.id().as_str();
        let Some((role, identity)) = identity_of(node, bound)
            .await
            .with_context(|| format!("node `{id}`"))?
        else {
            continue;
        };

        match hashes.get(role) {
            Some(seen) if *seen != identity => bail!(
                "node `{id}` reports a second `{role}` identity, and a run records one per \
                 role: evaluate them as two pipelines"
            ),
            _ => {
                hashes.insert(role.to_string(), identity);
            }
        }
    }

    Ok(hashes)
}

/// The role and identity of one node's component, or `None` when this build
/// constructs no component under the node's name.
async fn identity_of(
    node: &LogicalNode,
    #[cfg_attr(
        not(any(feature = "onnx", feature = "remote")),
        expect(unused_variables)
    )]
    bound: &Bound,
) -> Result<Option<(&'static str, String)>> {
    use ragondin_contracts::ContextBuilder;

    let (role, identity) = match node {
        #[cfg(any(feature = "onnx", feature = "remote"))]
        LogicalNode::Retriever(node) if node.implementation == DENSE => {
            let spec = embedder_of(&node.params, bound.bindings())?;
            let embedder = embedder(&spec, bound)?;
            (
                EMBEDDER_ROLE,
                embedder.model_identity(spec.served_model()).await?,
            )
        }
        #[cfg(feature = "onnx")]
        LogicalNode::Reranker(node) if node.implementation == CROSS_ENCODER => {
            use ragondin_contracts::Reranker;
            // `None`: the key-set check refuses `served_model` on this node,
            // and the ONNX reranker answers only for the model it loaded.
            let reranker = onnx_reranker(&node.params)?;
            (RERANKER_ROLE, reranker.model_identity(None).await?)
        }
        #[cfg(feature = "remote")]
        LogicalNode::Reranker(node)
            if bound
                .bindings()
                .binds(Family::Reranker, &node.implementation) =>
        {
            let served_model = required_string(&node.params, SERVED_MODEL)?;
            (
                RERANKER_ROLE,
                service_identity(
                    bound,
                    Family::Reranker,
                    &node.implementation,
                    Some(&served_model),
                )
                .await?,
            )
        }
        LogicalNode::ContextBuilder(node) if node.implementation == CONCAT => (
            CONTEXT_BUILDER_ROLE,
            concat(&node.params)?.model_identity().await?,
        ),
        #[cfg(feature = "remote")]
        LogicalNode::ContextBuilder(node)
            if bound
                .bindings()
                .binds(Family::ContextBuilder, &node.implementation) =>
        {
            (
                CONTEXT_BUILDER_ROLE,
                service_identity(bound, Family::ContextBuilder, &node.implementation, None).await?,
            )
        }
        #[cfg(feature = "remote")]
        LogicalNode::Generator(node)
            if bound
                .bindings()
                .binds(Family::Generator, &node.implementation) =>
        {
            // Required, and refused here rather than sent: ADR-C31 § 4 has the
            // composition root refuse an absent `served_model` itself.
            let served_model = required_string(&node.params, SERVED_MODEL)?;
            (
                GENERATOR_ROLE,
                service_identity(
                    bound,
                    Family::Generator,
                    &node.implementation,
                    Some(&served_model),
                )
                .await?,
            )
        }
        #[cfg(feature = "stub")]
        LogicalNode::Generator(node) if node.implementation == STUB_GENERATOR => {
            use ragondin_contracts::Generator;
            let served_model = required_string(&node.params, SERVED_MODEL)?;
            let generator = stub_generator(&node.params)?;
            (
                GENERATOR_ROLE,
                generator.model_identity(&served_model).await?,
            )
        }
        _ => return Ok(None),
    };
    Ok(Some((role, identity.as_str().to_owned())))
}

/// The identity the service bound as `family`/`name` in `bound` reports,
/// read with `served_model` — the identity read of [`model_hashes`] for a
/// bound reranker, context builder or generator, and the whole of `ragondin
/// ui`'s probe (ADR-C36 § 1), which calls it with no served model.
///
/// An embedder, a reranker and a generator report an identity for a served
/// model only (ADR-C32 § 4, ADR-C31 § 4), so without one it is refused before
/// any call. A retriever's and a fusion's services have no identity rpc, and
/// are refused as such. A service that cannot be reached fails at this call —
/// the channel connects lazily — as the adapter's `Unavailable`.
#[cfg(feature = "remote")]
pub async fn service_identity(
    bound: &Bound,
    family: Family,
    name: &str,
    served_model: Option<&str>,
) -> Result<ragondin_types::ModelIdentity> {
    use ragondin_contracts::{ContextBuilder, Embedder, Generator, Reranker};

    let needed = || {
        anyhow::anyhow!(
            "`{family}/{name}`: a {family} reports an identity for a served model, and none \
             was given"
        )
    };
    let channel = bound.channel(family, name)?;
    Ok(match family {
        Family::Retriever | Family::Fusion => bail!(
            "`{family}/{name}`: a `Remote` {family} reports no identity; its service has no \
             identity rpc"
        ),
        Family::Embedder => {
            let served_model = served_model.ok_or_else(needed)?;
            // The prefixes are applied to texts, and an identity names none.
            ragondin_remote::RemoteEmbedder::new(channel, "", "")
                .model_identity(Some(served_model))
                .await?
        }
        Family::Reranker => {
            let served_model = served_model.ok_or_else(needed)?;
            ragondin_remote::RemoteReranker::new(channel)
                .model_identity(Some(served_model))
                .await?
        }
        Family::Generator => {
            let served_model = served_model.ok_or_else(needed)?;
            ragondin_remote::RemoteGenerator::new(channel)
                .model_identity(served_model)
                .await?
        }
        Family::ContextBuilder => {
            ragondin_remote::RemoteContextBuilder::new(channel)
                .model_identity()
                .await?
        }
    })
}

/// Refuses every key of `params` that is not in one of `allowed`, naming it.
///
/// `reader` says who would have read it, so the refusal says what the node
/// is: the same key is fine on one nature and inert on another (ADR-C32 § 1).
fn refuse_keys_outside(params: &Params, allowed: &[&[&str]], reader: &str) -> Result<()> {
    for key in params.keys() {
        if !allowed.iter().any(|set| set.contains(&key.as_str())) {
            bail!("`{key}` is not a key {reader} reads: refused rather than hashed as inert");
        }
    }
    Ok(())
}

/// The model, tokenizer and token budget a node configures.
fn model_of(params: &Params) -> Result<ModelSpec> {
    Ok(ModelSpec {
        model: PathBuf::from(required_string(params, "model")?),
        tokenizer: PathBuf::from(required_string(params, "tokenizer")?),
        max_sequence_length: optional_usize(params, "max_sequence_length")?,
    })
}

/// The model a `cross_encoder` node configures, once its keys are checked.
fn reranker_of(params: &Params) -> Result<ModelSpec> {
    refuse_keys_outside(params, &[&["top_k"], &ONNX_KEYS], "the ONNX cross-encoder")?;
    model_of(params)
}

/// The served model a reranker node under the bound name `name` configures,
/// once its keys are checked: `top_k` and `served_model`, which is required
/// because a service has no loaded model for `None` to name (ADR-C32 § 1).
fn bound_reranker_of(params: &Params, name: &str) -> Result<String> {
    refuse_keys_outside(
        params,
        &[&["top_k", SERVED_MODEL]],
        &format!("the bound reranker `{name}`"),
    )?;
    required_string(params, SERVED_MODEL)
}

/// The embedder a `dense` node configures, resolved from its `embedder:` name:
/// `onnx`, or a name `bindings` binds in the `embedder` family.
fn embedder_of(params: &Params, bindings: &Bindings) -> Result<EmbedderSpec> {
    let name = required_string(params, "embedder")?;
    if name.is_empty() {
        bail!("`embedder` must name an embedder, and an empty name names none");
    }
    if name == ONNX_EMBEDDER {
        refuse_keys_outside(
            params,
            &[&DENSE_KEYS, &ONNX_KEYS],
            "a dense node over the `onnx` embedder",
        )?;
        return Ok(EmbedderSpec::Onnx {
            model: model_of(params)?,
            query_prefix: prefix(params, "query_prefix")?,
            passage_prefix: prefix(params, "passage_prefix")?,
        });
    }
    if !bindings.binds(Family::Embedder, &name) {
        bail!(
            "`embedder` names `{name}`, which is neither `{ONNX_EMBEDDER}` nor bound with \
             `--remote embedder/{name}=<uri>`"
        );
    }
    refuse_keys_outside(
        params,
        &[&DENSE_KEYS, &[SERVED_MODEL]],
        &format!("a dense node over the bound embedder `{name}`"),
    )?;
    Ok(EmbedderSpec::Bound {
        served_model: required_string(params, SERVED_MODEL)?,
        query_prefix: prefix(params, "query_prefix")?,
        passage_prefix: prefix(params, "passage_prefix")?,
        name,
    })
}

/// A prefix, where absence is the only spelling of "no prefix".
///
/// `""` would build the same embedder as an absent key while hashing
/// differently, so it is refused rather than accepted (ADR-C32 § 1, INV-8).
fn prefix(params: &Params, key: &str) -> Result<String> {
    match optional_string(params, key)? {
        Some(value) if value.is_empty() => {
            bail!("`{key}` is empty: leave the key out to embed with no prefix")
        }
        value => Ok(value.unwrap_or_default()),
    }
}

/// A string parameter that must be there.
fn required_string(params: &Params, key: &str) -> Result<String> {
    match optional_string(params, key)? {
        Some(value) => Ok(value),
        None => bail!("`{key}` is required"),
    }
}

/// A string parameter, absent or present.
fn optional_string(params: &Params, key: &str) -> Result<Option<String>> {
    match params.get(key) {
        None => Ok(None),
        Some(ParamValue::String(value)) => Ok(Some(value.clone())),
        Some(other) => bail!("`{key}` must be a string, found {other:?}"),
    }
}

/// A whole-number parameter, absent or present.
///
/// Negative is refused rather than cast: every count this binary reads is a
/// `usize` on the far side, and `-1 as usize` is a very large count rather
/// than an error.
fn optional_usize(params: &Params, key: &str) -> Result<Option<usize>> {
    match params.get(key) {
        None => Ok(None),
        Some(ParamValue::Int(value)) => usize::try_from(*value)
            .map(Some)
            .map_err(|_| anyhow::anyhow!("`{key}` must not be negative, found {value}")),
        Some(other) => bail!("`{key}` must be a whole number, found {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use ragondin_pipeline::LogicalPipeline;

    use super::*;
    #[cfg(feature = "remote")]
    use crate::remote_fakes as remote;

    /// Loads a pipeline from YAML through `ragondin-config`'s
    /// `parse_document`, the load every configuration goes through: a
    /// hand-built `LogicalPipeline` would skip the validation that decides
    /// what these functions are ever handed.
    fn pipeline(yaml: &str) -> LogicalPipeline {
        ragondin_config::parse_document(yaml).expect("the fixture loads")
    }

    /// No `--remote` argument, as every test before bindings existed ran.
    fn unbound() -> Bindings {
        Bindings::default()
    }

    /// [`model_hashes`] with nothing bound.
    async fn model_hashes(pipeline: &LogicalPipeline) -> Result<BTreeMap<String, String>> {
        super::model_hashes(
            pipeline,
            &Bound::new(unbound()).expect("nothing to connect"),
        )
        .await
    }

    fn embedder_spec(pipeline: &LogicalPipeline) -> Result<Option<EmbedderSpec>> {
        super::embedder_spec(pipeline, &unbound())
    }

    fn check_nodes(pipeline: &LogicalPipeline) -> Result<()> {
        super::check_nodes(pipeline, &unbound())
    }

    /// The ONNX half of a spec, which every unbound `dense` node configures.
    fn onnx(spec: EmbedderSpec) -> (ModelSpec, String, String) {
        match spec {
            EmbedderSpec::Onnx {
                model,
                query_prefix,
                passage_prefix,
            } => (model, query_prefix, passage_prefix),
            other => panic!("an unbound dense node names the onnx embedder: {other:?}"),
        }
    }

    fn dense_node(id: &str, params: &str) -> String {
        format!(
            "    - id: {id}\n      component: retriever\n      impl: dense\n      \
             inputs: [question]\n      params: {params}\n"
        )
    }

    fn bm25_node() -> String {
        "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
         inputs: [question]\n      params: { top_k: 10 }\n"
            .to_owned()
    }

    fn wrap(nodes: &str) -> String {
        format!("pipeline:\n  inputs: [question]\n  nodes:\n{nodes}")
    }

    #[test]
    fn an_extension_node_is_refused_by_name() {
        let yaml = wrap(
            "    - id: hyde\n      component: extension\n      kind: query_transform\n      \
             impl: hyde\n      inputs: [question]\n",
        );

        let error = refuse_unsupported(&pipeline(&yaml)).expect_err("v0 runs no extension");

        assert!(error.to_string().contains("hyde"), "{error}");
        assert!(error.to_string().contains("not supported in v0"), "{error}");
    }

    #[test]
    fn a_retrieval_only_pipeline_is_supported() {
        let yaml = wrap(&bm25_node());

        refuse_unsupported(&pipeline(&yaml)).expect("bm25 alone is what v0 is for");
    }

    #[test]
    fn the_embedder_of_a_pipeline_with_no_dense_node_is_absent() {
        let yaml = wrap(&bm25_node());

        assert_eq!(
            embedder_spec(&pipeline(&yaml)).expect("no dense node"),
            None
        );
    }

    #[test]
    fn a_dense_node_carries_its_model_tokenizer_and_prefixes() {
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, embedder: onnx, model: m.onnx, tokenizer: t.json, query_prefix: 'query: ', \
             passage_prefix: 'passage: ' }",
        ));

        let (model, query_prefix, passage_prefix) = onnx(
            embedder_spec(&pipeline(&yaml))
                .expect("the node is complete")
                .expect("there is a dense node"),
        );

        assert_eq!(model.model, PathBuf::from("m.onnx"));
        assert_eq!(model.tokenizer, PathBuf::from("t.json"));
        assert_eq!(model.max_sequence_length, None);
        assert_eq!(query_prefix, "query: ");
        assert_eq!(passage_prefix, "passage: ");
    }

    #[test]
    fn a_dense_node_without_a_model_names_the_node_and_the_key() {
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, embedder: onnx, tokenizer: t.json }",
        ));

        let error = embedder_spec(&pipeline(&yaml)).expect_err("`model` is required");

        assert!(error.to_string().contains("vectors"), "{error}");
        assert!(
            format!("{error:#}").contains("`model` is required"),
            "{error:#}"
        );
    }

    #[test]
    fn two_dense_nodes_agreeing_on_the_embedder_are_one_embedder() {
        let yaml = wrap(&format!(
            "{}{}",
            dense_node(
                "left",
                "{ top_k: 10, embedder: onnx, model: m.onnx, tokenizer: t.json }"
            ),
            dense_node(
                "right",
                "{ top_k: 5, embedder: onnx, model: m.onnx, tokenizer: t.json }"
            ),
        ));

        let (model, _, _) = onnx(
            embedder_spec(&pipeline(&yaml))
                .expect("both name one embedder")
                .expect("there are dense nodes"),
        );

        assert_eq!(model.model, PathBuf::from("m.onnx"));
    }

    #[test]
    fn two_dense_nodes_disagreeing_on_a_prefix_are_refused() {
        let yaml = wrap(&format!(
            "{}{}",
            dense_node(
                "left",
                "{ top_k: 10, embedder: onnx, model: m.onnx, tokenizer: t.json }"
            ),
            dense_node(
                "right",
                "{ top_k: 5, embedder: onnx, model: m.onnx, tokenizer: t.json, \
                 passage_prefix: 'passage: ' }"
            ),
        ));

        let error = embedder_spec(&pipeline(&yaml)).expect_err("one corpus, one embedder");

        assert!(error.to_string().contains("left"), "{error}");
        assert!(error.to_string().contains("right"), "{error}");
    }

    #[test]
    fn a_negative_token_budget_is_refused_rather_than_cast() {
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, embedder: onnx, model: m.onnx, tokenizer: t.json, \
             max_sequence_length: -1 }",
        ));

        let error = embedder_spec(&pipeline(&yaml)).expect_err("-1 is not a token budget");

        assert!(
            format!("{error:#}").contains("must not be negative"),
            "{error:#}"
        );
    }

    #[tokio::test]
    async fn a_pipeline_naming_no_model_records_no_model_hash() {
        let yaml = wrap(
            "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
             inputs: [question]\n      params: { top_k: 10 }\n",
        );

        assert!(model_hashes(&pipeline(&yaml))
            .await
            .expect("bm25 reads no model")
            .is_empty());
    }

    fn reranker_node(params: &str) -> String {
        format!(
            "    - id: reranked\n      component: reranker\n      impl: cross_encoder\n      \
             inputs: [question, lexical]\n      params: {params}\n"
        )
    }

    fn concat_node(id: &str, params: &str) -> String {
        format!(
            "    - id: {id}\n      component: context_builder\n      impl: concat\n      \
             inputs: [question, lexical]\n      params: {params}\n"
        )
    }

    fn generator_node(implementation: &str, params: &str) -> String {
        format!(
            "    - id: answer\n      component: generator\n      impl: {implementation}\n      \
             inputs: [question, prompt]\n      params: {params}\n"
        )
    }

    /// The error's whole chain, as `bench` prints it.
    fn chain(error: &anyhow::Error) -> String {
        format!("{error:#}")
    }

    #[test]
    fn a_dense_node_without_an_embedder_key_is_refused_naming_the_node() {
        // ADR-C32 § 1: no default, because an absent key and `embedder: onnx`
        // would be two spellings of one configuration with two hashes.
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, model: m.onnx, tokenizer: t.json }",
        ));

        let error = check_nodes(&pipeline(&yaml)).expect_err("`embedder` is required");

        assert!(chain(&error).contains("vectors"), "{error:#}");
        assert!(
            chain(&error).contains("`embedder` is required"),
            "{error:#}"
        );
    }

    #[test]
    fn an_empty_embedder_name_is_refused() {
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, embedder: '', model: m.onnx, tokenizer: t.json }",
        ));

        let error = check_nodes(&pipeline(&yaml)).expect_err("an empty name names nothing");

        assert!(chain(&error).contains("vectors"), "{error:#}");
        assert!(chain(&error).contains("`embedder`"), "{error:#}");
    }

    #[test]
    fn an_embedder_this_composition_root_does_not_know_is_refused_by_name() {
        // An embedder is not a node family, so no planner will ever look the
        // name up: the composition root diagnoses it itself (ADR-C32 § 4).
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, embedder: bge, served_model: bge-small }",
        ));

        let error = check_nodes(&pipeline(&yaml)).expect_err("no embedder is called `bge`");

        assert!(chain(&error).contains("vectors"), "{error:#}");
        assert!(chain(&error).contains("`bge`"), "{error:#}");
    }

    #[test]
    fn served_model_on_a_dense_node_over_the_onnx_embedder_is_refused_not_hashed() {
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, embedder: onnx, model: m.onnx, tokenizer: t.json, \
             served_model: minilm }",
        ));

        let error = check_nodes(&pipeline(&yaml)).expect_err("an inert key is refused");

        assert!(chain(&error).contains("vectors"), "{error:#}");
        assert!(chain(&error).contains("`served_model`"), "{error:#}");
    }

    #[test]
    fn a_key_no_reader_reads_on_a_dense_node_is_refused() {
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, embedder: onnx, model: m.onnx, tokenizer: t.json, pooling: cls }",
        ));

        let error = check_nodes(&pipeline(&yaml)).expect_err("an inert key is refused");

        assert!(chain(&error).contains("`pooling`"), "{error:#}");
    }

    #[test]
    fn an_empty_prefix_is_refused_because_absence_is_the_only_spelling_of_none() {
        for key in ["query_prefix", "passage_prefix"] {
            let yaml = wrap(&dense_node(
                "vectors",
                &format!(
                    "{{ top_k: 10, embedder: onnx, model: m.onnx, tokenizer: t.json, {key}: '' }}"
                ),
            ));

            let error = embedder_spec(&pipeline(&yaml)).expect_err("`\"\"` is refused");

            assert!(chain(&error).contains("vectors"), "{error:#}");
            assert!(chain(&error).contains(&format!("`{key}`")), "{error:#}");
        }
    }

    #[test]
    fn served_model_on_a_cross_encoder_node_is_refused_not_hashed() {
        let yaml = wrap(&format!(
            "{}{}",
            bm25_node(),
            reranker_node(
                "{ top_k: 10, model: r.onnx, tokenizer: t.json, served_model: ms-marco }"
            )
        ));

        let error = check_nodes(&pipeline(&yaml)).expect_err("an inert key is refused");

        assert!(chain(&error).contains("reranked"), "{error:#}");
        assert!(chain(&error).contains("`served_model`"), "{error:#}");
    }

    #[test]
    fn a_complete_cross_encoder_node_passes_the_key_check() {
        let yaml = wrap(&format!(
            "{}{}",
            bm25_node(),
            reranker_node(
                "{ top_k: 10, model: r.onnx, tokenizer: t.json, max_sequence_length: 512 }"
            )
        ));

        check_nodes(&pipeline(&yaml)).expect("every key is one the reranker reads");
    }

    /// The lean build names what it lacks: an `embedder:` name it gives a
    /// `Local` embedder in another build is refused naming the feature, since
    /// no planner will ever look an embedder up (ADR-C32 § 4).
    #[cfg(not(feature = "onnx"))]
    #[test]
    fn the_onnx_embedder_in_a_build_without_it_is_refused_naming_the_feature() {
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, embedder: onnx, model: m.onnx, tokenizer: t.json }",
        ));

        let error = check_nodes(&pipeline(&yaml)).expect_err("this build has no onnx embedder");

        assert!(chain(&error).contains("vectors"), "{error:#}");
        assert!(chain(&error).contains("`onnx` feature"), "{error:#}");
    }

    #[test]
    fn a_generation_pipeline_is_supported() {
        let yaml = wrap(&format!(
            "{}{}{}",
            bm25_node(),
            concat_node("prompt", "{ budget: 100, separator: \"\\n\" }"),
            generator_node("vllm", "{ served_model: m, template: '{context}' }"),
        ));

        refuse_unsupported(&pipeline(&yaml)).expect("generation has primitive nodes now");
    }

    #[tokio::test]
    async fn a_context_builder_records_its_identity_under_its_role() {
        let yaml = wrap(&format!(
            "{}{}",
            bm25_node(),
            concat_node("prompt", "{ budget: 100, separator: \"\\n\" }"),
        ));

        let hashes = model_hashes(&pipeline(&yaml))
            .await
            .expect("concat is known");

        use ragondin_contracts::ContextBuilder;
        let expected = ragondin_context_concat::ConcatContextBuilder::new("\n")
            .model_identity()
            .await
            .expect("a concat builder always has an identity");
        assert_eq!(
            hashes.get(CONTEXT_BUILDER_ROLE).map(String::as_str),
            Some(expected.as_str())
        );
    }

    #[tokio::test]
    async fn a_concat_node_without_a_separator_is_refused() {
        // Required rather than defaulted: a default would make an absent key
        // and its value two spellings of one builder with two hashes.
        let yaml = wrap(&format!(
            "{}{}",
            bm25_node(),
            concat_node("prompt", "{ budget: 100 }")
        ));

        let error = model_hashes(&pipeline(&yaml))
            .await
            .expect_err("`separator` is required");

        assert!(chain(&error).contains("prompt"), "{error:#}");
        assert!(
            chain(&error).contains("`separator` is required"),
            "{error:#}"
        );
    }

    #[tokio::test]
    async fn two_context_builders_with_different_identities_are_refused() {
        let yaml = wrap(&format!(
            "{}{}{}",
            bm25_node(),
            concat_node("left", "{ budget: 100, separator: \"\\n\" }"),
            concat_node("right", "{ budget: 100, separator: ' ' }"),
        ));

        let error = model_hashes(&pipeline(&yaml))
            .await
            .expect_err("a run records one identity per role");

        assert!(chain(&error).contains("context_builder"), "{error:#}");
    }

    #[tokio::test]
    async fn a_generator_this_build_does_not_know_is_left_to_the_planner() {
        // No identity is read for a name this composition root does not
        // register — not even its `served_model` is checked — and planning's
        // unknown `impl:` is what names it (ADR-C32 § 4).
        let yaml = wrap(&format!(
            "{}{}{}",
            bm25_node(),
            concat_node("prompt", "{ budget: 100, separator: \"\\n\" }"),
            generator_node("vllm", "{ template: '{context}' }"),
        ));

        let hashes = model_hashes(&pipeline(&yaml))
            .await
            .expect("an unknown name is skipped, not refused");

        assert!(!hashes.contains_key(GENERATOR_ROLE), "{hashes:?}");
    }

    #[cfg(feature = "stub")]
    #[tokio::test]
    async fn the_stub_generator_s_identity_is_read_with_its_node_s_served_model() {
        let yaml = wrap(&format!(
            "{}{}{}",
            bm25_node(),
            concat_node("prompt", "{ budget: 100, separator: \"\\n\" }"),
            generator_node(
                "stub_generator",
                "{ served_model: stub-model, template: '{context}' }"
            ),
        ));

        let hashes = model_hashes(&pipeline(&yaml))
            .await
            .expect("the stub is known");

        assert_eq!(
            hashes.get(GENERATOR_ROLE).map(String::as_str),
            Some(ragondin_stub::StubGenerator::IDENTITY)
        );
    }

    #[cfg(feature = "stub")]
    #[tokio::test]
    async fn a_generator_node_without_a_served_model_is_refused_before_the_run() {
        // ADR-C31 § 4: the composition root refuses it itself, rather than
        // passing an empty name for the component to refuse.
        let yaml = wrap(&format!(
            "{}{}{}",
            bm25_node(),
            concat_node("prompt", "{ budget: 100, separator: \"\\n\" }"),
            generator_node("stub_generator", "{ template: '{context}' }"),
        ));

        let error = model_hashes(&pipeline(&yaml))
            .await
            .expect_err("`served_model` is required");

        assert!(chain(&error).contains("answer"), "{error:#}");
        assert!(
            chain(&error).contains("`served_model` is required"),
            "{error:#}"
        );
    }

    /// The ONNX components' identity is `<model>+<tokenizer>` (ADR-C32 § 4),
    /// read from a constructed instance — not the file digest this crate used
    /// to take itself, which left the tokenizer's contents out.
    #[cfg(feature = "onnx")]
    #[tokio::test]
    async fn a_dense_node_records_the_embedder_s_model_and_tokenizer_identity() {
        use sha2::{Digest, Sha256};

        use std::path::Path;

        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../components/ragondin-embedder-onnx/tests/fixtures");
        let model = fixtures.join("tiny-embedder.onnx");
        let tokenizer = fixtures.join("tokenizer.json");
        let yaml = wrap(&dense_node(
            "vectors",
            &format!(
                "{{ top_k: 10, embedder: onnx, model: '{}', tokenizer: '{}' }}",
                model.display(),
                tokenizer.display()
            ),
        ));
        let digest = |path: &Path| {
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(path).expect("a fixture"))
            )
        };

        let hashes = model_hashes(&pipeline(&yaml))
            .await
            .expect("the fixture loads");

        assert_eq!(
            hashes.get(EMBEDDER_ROLE),
            Some(&format!("{}+{}", digest(&model), digest(&tokenizer)))
        );
    }

    /// `--remote` arguments, parsed the way `bench` parses them.
    #[cfg(feature = "remote")]
    fn bound(arguments: &[String]) -> Bindings {
        Bindings::parse(arguments).expect("well-formed bindings")
    }

    #[cfg(feature = "remote")]
    fn bound_reranker_node(input: &str, params: &str) -> String {
        format!(
            "    - id: reranked\n      component: reranker\n      impl: bge-reranker\n      \
             inputs: [question, {input}]\n      params: {params}\n"
        )
    }

    #[cfg(feature = "remote")]
    #[test]
    fn a_dense_node_over_a_bound_embedder_carries_its_name_served_model_and_prefixes() {
        let bindings = bound(&["embedder/bge=http://localhost:1".to_owned()]);
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, embedder: bge, served_model: bge-small, query_prefix: 'q: ' }",
        ));

        super::check_nodes(&pipeline(&yaml), &bindings).expect("every key is one it reads");
        let spec = super::embedder_spec(&pipeline(&yaml), &bindings)
            .expect("the node is complete")
            .expect("there is a dense node");

        assert_eq!(
            spec,
            EmbedderSpec::Bound {
                name: "bge".to_owned(),
                served_model: "bge-small".to_owned(),
                query_prefix: "q: ".to_owned(),
                passage_prefix: String::new(),
            }
        );
        assert_eq!(spec.served_model(), Some("bge-small"));
    }

    #[cfg(feature = "remote")]
    #[test]
    fn a_dense_node_over_a_bound_embedder_without_a_served_model_is_refused() {
        // ADR-C32 § 1: a service has no loaded model for `None` to name.
        let bindings = bound(&["embedder/bge=http://localhost:1".to_owned()]);
        let yaml = wrap(&dense_node("vectors", "{ top_k: 10, embedder: bge }"));

        let error = super::check_nodes(&pipeline(&yaml), &bindings)
            .expect_err("`served_model` is required");

        assert!(chain(&error).contains("vectors"), "{error:#}");
        assert!(
            chain(&error).contains("`served_model` is required"),
            "{error:#}"
        );
    }

    #[cfg(feature = "remote")]
    #[test]
    fn an_onnx_key_on_a_dense_node_over_a_bound_embedder_is_refused_not_hashed() {
        let bindings = bound(&["embedder/bge=http://localhost:1".to_owned()]);
        for key in [
            "model: m.onnx",
            "tokenizer: t.json",
            "max_sequence_length: 8",
        ] {
            let yaml = wrap(&dense_node(
                "vectors",
                &format!("{{ top_k: 10, embedder: bge, served_model: bge-small, {key} }}"),
            ));

            let error = super::check_nodes(&pipeline(&yaml), &bindings)
                .expect_err("an inert key is refused");

            let name = key.split(':').next().expect("a key");
            assert!(chain(&error).contains(&format!("`{name}`")), "{error:#}");
            assert!(chain(&error).contains("vectors"), "{error:#}");
        }
    }

    #[cfg(feature = "remote")]
    #[test]
    fn a_bound_reranker_node_carries_top_k_and_a_required_served_model_and_nothing_else() {
        let bindings = bound(&["reranker/bge-reranker=http://localhost:1".to_owned()]);
        let with = |params: &str| {
            wrap(&format!(
                "{}{}",
                bm25_node(),
                bound_reranker_node("lexical", params)
            ))
        };

        super::check_nodes(
            &pipeline(&with("{ top_k: 5, served_model: ms-marco }")),
            &bindings,
        )
        .expect("both keys are read");
        let missing = super::check_nodes(&pipeline(&with("{ top_k: 5 }")), &bindings)
            .expect_err("`served_model` is required");
        let inert = super::check_nodes(
            &pipeline(&with(
                "{ top_k: 5, served_model: ms-marco, tokenizer: t.json }",
            )),
            &bindings,
        )
        .expect_err("an inert key is refused");

        assert!(chain(&missing).contains("reranked"), "{missing:#}");
        assert!(
            chain(&missing).contains("`served_model` is required"),
            "{missing:#}"
        );
        assert!(chain(&inert).contains("`tokenizer`"), "{inert:#}");
    }

    #[cfg(feature = "remote")]
    fn remote_pipeline() -> String {
        wrap(&format!(
            "{}{}",
            dense_node(
                "vectors",
                "{ top_k: 10, embedder: bge, served_model: bge-small }"
            ),
            bound_reranker_node("vectors", "{ top_k: 5, served_model: ms-marco }"),
        ))
    }

    #[cfg(feature = "remote")]
    #[tokio::test]
    async fn bound_components_record_the_identity_their_service_reports_for_the_node_s_model() {
        // The fakes answer only for the name each node gives, so an identity
        // recorded at all is an identity read with that `served_model`.
        let embedder = remote::serve_embedder(remote::FakeEmbedder::default());
        let reranker = remote::serve_reranker();
        let bindings = bound(&[
            format!("embedder/bge={}", embedder.uri),
            format!("reranker/bge-reranker={}", reranker.uri),
        ]);

        let hashes = super::model_hashes(
            &pipeline(&remote_pipeline()),
            &Bound::new(bindings).expect("lazy channels"),
        )
        .await
        .expect("both services answer");

        assert_eq!(
            hashes.get(EMBEDDER_ROLE).map(String::as_str),
            Some(remote::EMBEDDER_IDENTITY)
        );
        assert_eq!(
            hashes.get(RERANKER_ROLE).map(String::as_str),
            Some(remote::RERANKER_IDENTITY)
        );
    }

    #[cfg(feature = "remote")]
    #[tokio::test]
    async fn a_model_the_service_does_not_serve_ends_the_run_before_it_starts() {
        let embedder = remote::serve_embedder(remote::FakeEmbedder::default());
        let reranker = remote::serve_reranker();
        let bindings = bound(&[
            format!("embedder/bge={}", embedder.uri),
            format!("reranker/bge-reranker={}", reranker.uri),
        ]);
        let yaml = remote_pipeline().replace("served_model: ms-marco", "served_model: minilm");

        let error = super::model_hashes(&pipeline(&yaml), &Bound::new(bindings).expect("lazy"))
            .await
            .expect_err("the reranker does not serve `minilm`");

        assert!(chain(&error).contains("reranked"), "{error:#}");
        assert!(chain(&error).contains("minilm"), "{error:#}");
    }

    #[cfg(feature = "remote")]
    #[tokio::test]
    async fn an_unreachable_service_is_found_at_the_identity_read_as_unavailable() {
        // No connection is attempted when the channel is built (ADR-C32 § 3):
        // the identity call is the first call, and it reports the failure.
        let bindings = bound(&[format!("embedder/bge={}", remote::unreachable_uri())]);
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, embedder: bge, served_model: bge-small }",
        ));
        let bound = Bound::new(bindings).expect("building a lazy channel connects nothing");

        let error = super::model_hashes(&pipeline(&yaml), &bound)
            .await
            .expect_err("nothing listens there");

        assert!(chain(&error).contains("vectors"), "{error:#}");
        assert!(chain(&error).contains("unavailable"), "{error:#}");
    }

    #[cfg(feature = "remote")]
    #[tokio::test]
    async fn a_bound_generator_and_context_builder_record_their_services_identities() {
        // The fake generator answers only for its served model, so an
        // identity recorded at all was read with the node's `served_model`
        // (ADR-C31 § 4); the context builder's is read with no argument.
        let builder = remote::serve_context_builder();
        let generator = remote::serve_generator();
        let bindings = bound(&[
            format!("context_builder/lines={}", builder.uri),
            format!("generator/vllm={}", generator.uri),
        ]);
        let yaml = wrap(&format!(
            "{}{}{}",
            bm25_node(),
            "    - id: prompt\n      component: context_builder\n      impl: lines\n      \
             inputs: [question, lexical]\n      params: { budget: 100 }\n",
            generator_node(
                "vllm",
                &format!(
                    "{{ served_model: {}, template: '{{context}}' }}",
                    remote::GENERATOR_MODEL
                )
            ),
        ));

        let hashes = super::model_hashes(&pipeline(&yaml), &Bound::new(bindings).expect("lazy"))
            .await
            .expect("both services answer");

        assert_eq!(
            hashes.get(GENERATOR_ROLE).map(String::as_str),
            Some(remote::GENERATOR_IDENTITY)
        );
        assert_eq!(
            hashes.get(CONTEXT_BUILDER_ROLE).map(String::as_str),
            Some(remote::CONTEXT_BUILDER_IDENTITY)
        );
    }

    #[cfg(feature = "remote")]
    #[tokio::test]
    async fn a_bound_generator_node_without_a_served_model_is_refused_before_any_call() {
        let bindings = bound(&[format!("generator/vllm={}", remote::unreachable_uri())]);
        let yaml = wrap(&format!(
            "{}{}{}",
            bm25_node(),
            concat_node("prompt", "{ budget: 100, separator: \"\\n\" }"),
            generator_node("vllm", "{ template: '{context}' }"),
        ));

        let error = super::model_hashes(&pipeline(&yaml), &Bound::new(bindings).expect("lazy"))
            .await
            .expect_err("`served_model` is required");

        // Refused by the composition root, not reported as the unreachable
        // service it would otherwise have called.
        assert!(chain(&error).contains("answer"), "{error:#}");
        assert!(
            chain(&error).contains("`served_model` is required"),
            "{error:#}"
        );
    }
}
