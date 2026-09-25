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

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use ragondin_contracts::EmbeddedChunk;
use ragondin_engine::EngineContext;
use ragondin_pipeline::{LogicalNode, LogicalPipeline, ParamValue, Params};
use ragondin_types::Chunk;

/// The `impl:` name of the in-process BM25 retriever.
///
/// Gated, where the others are not: nothing reads a BM25 node's
/// configuration — its constructor takes the corpus and no parameter — so this
/// name is used only where the component is registered.
#[cfg(feature = "bm25")]
const BM25: &str = "bm25";
/// The `impl:` name of the dense retriever (an embedder over a vector store).
const DENSE: &str = "dense";
/// The `impl:` name of Reciprocal Rank Fusion.
const RRF: &str = "rrf";
/// The `impl:` name of the ONNX cross-encoder reranker.
const CROSS_ENCODER: &str = "cross_encoder";
/// The `impl:` name of the concatenating context builder.
const CONCAT: &str = "concat";
/// The `impl:` name of the stub generator, the one generator a build of this
/// binary can carry in-process. It exists for tests: no production `Local`
/// generator exists, a generator being `Remote` by design (ADR-C31).
#[cfg(feature = "stub")]
const STUB_GENERATOR: &str = "stub_generator";

/// The `embedder:` name of the in-process ONNX embedder (ADR-C32 § 1).
const ONNX_EMBEDDER: &str = "onnx";

/// The keys every `dense` node may carry, whatever embedder it names
/// (ADR-C32 § 1). `top_k` is the executor's; the rest are this file's.
const DENSE_KEYS: [&str; 4] = ["top_k", "embedder", "query_prefix", "passage_prefix"];
/// The keys an ONNX model-bearing node adds: the model, its tokenizer and its
/// token budget. Shared by a `dense` node over the ONNX embedder and a
/// `cross_encoder` node, which read the same three.
const ONNX_KEYS: [&str; 3] = ["model", "tokenizer", "max_sequence_length"];

/// The role an embedder's model plays in run identity
/// (`docs/system-architecture.md` §7.1). Only a build that can construct the
/// ONNX components reads an identity under this role or the next.
#[cfg(feature = "onnx")]
const EMBEDDER_ROLE: &str = "embedder";
/// The role a reranker's model plays in run identity.
#[cfg(feature = "onnx")]
const RERANKER_ROLE: &str = "reranker";
/// The role of a context builder's identity (ADR-C31 § 4).
const CONTEXT_BUILDER_ROLE: &str = "context_builder";
/// The role of a generator's identity (ADR-C31 § 4). Read only in a build
/// that carries a generator, which is the `stub` one.
#[cfg(any(feature = "stub", test))]
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

/// What a `dense` node says about the embedder it retrieves through.
///
/// The only embedder this composition root knows is the ONNX one
/// (`embedder: onnx`), so the spec is that embedder's. The prefixes travel
/// with the model because they decide the vectors: two nodes naming one model
/// under different prefixes are two embedders, and the corpus one of them
/// indexed is not the corpus the other would search (ADR-C17).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbedderSpec {
    /// The model and its tokenizer.
    pub model: ModelSpec,
    /// Prepended to a query before it is embedded; empty when the node
    /// carries no `query_prefix`, which is the only spelling of "none" — an
    /// empty value is refused when the node is read.
    pub query_prefix: String,
    /// Prepended to a passage before it is embedded; empty as `query_prefix`
    /// is.
    pub passage_prefix: String,
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
/// Registration is infallible on purpose: a constructor's failure belongs to
/// the plan that names it, where planning reports it against the node
/// (`PlanError::Construction`). Refusing here would refuse a component this
/// configuration may never mention.
#[cfg_attr(
    not(any(feature = "bm25", feature = "onnx")),
    allow(unused_variables, clippy::needless_pass_by_value)
)]
pub fn register(ctx: &mut EngineContext, chunks: &[Chunk], embedded: Option<&[EmbeddedChunk]>) {
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

    #[cfg(feature = "onnx")]
    {
        // Registered only when the corpus was embedded: a dense retriever over
        // an empty store answers every query with nothing, which is a wrong
        // number rather than an error. With no entries there is no dense node
        // to answer, and planning says so by name.
        if let Some(entries) = embedded {
            let entries = entries.to_vec();
            ctx.register_retriever(
                DENSE,
                Box::new(move |params| {
                    // The `embedder:` name is resolved here, inside the
                    // closure this composition root writes (ADR-C32 § 3):
                    // `embedder_of` knows `onnx` and refuses anything else.
                    let spec = embedder_of(params)?;
                    let store = ragondin_store_memory::MemoryVectorStore::seeded(entries.clone())?;
                    Ok(Box::new(ragondin_retriever_dense::DenseRetriever::new(
                        Box::new(onnx_embedder(&spec)?),
                        Box::new(store),
                    )))
                }),
            );
        }

        ctx.register_reranker(
            CROSS_ENCODER,
            Box::new(|params| Ok(Box::new(onnx_reranker(params)?))),
        );
    }
}

/// The embedder `spec` describes, with its model session loaded.
#[cfg(feature = "onnx")]
pub fn onnx_embedder(spec: &EmbedderSpec) -> Result<ragondin_embedder_onnx::OnnxEmbedder> {
    let mut config =
        ragondin_embedder_onnx::OnnxEmbedderConfig::new(&spec.model.model, &spec.model.tokenizer)
            .with_prefixes(spec.query_prefix.as_str(), spec.passage_prefix.as_str());
    if let Some(tokens) = spec.model.max_sequence_length {
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
        "served_model",
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
/// A `dense` node must name its embedder with `embedder:`, carry only the keys
/// that embedder's nature reads, and never an empty prefix; a `cross_encoder`
/// node only the keys the ONNX reranker reads. A key outside those sets is
/// refused rather than hashed as inert: it would move the run's identity while
/// changing nothing the run did. Build-independent, except for one refusal:
/// `embedder: onnx` in a build without the `onnx` feature is named here,
/// because no planner will ever look an embedder up to name it.
pub fn check_nodes(pipeline: &LogicalPipeline) -> Result<()> {
    let embedder = embedder_spec(pipeline)?;
    if embedder.is_some() && !cfg!(feature = "onnx") {
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

    for node in pipeline.nodes() {
        if let LogicalNode::Reranker(node) = node {
            if node.implementation == CROSS_ENCODER {
                reranker_of(&node.params)
                    .with_context(|| format!("node `{}`", node.id.as_str()))?;
            }
        }
    }
    Ok(())
}

/// The embedder every `dense` node of `pipeline` is configured with, if it has
/// one.
///
/// **One embedder per pipeline, in v0.** The corpus is embedded once, before
/// the components are constructed, so two `dense` nodes disagreeing about the
/// model, the tokenizer or a prefix would need two indexes — and a run that
/// searched two indexes has one `index_version` naming neither. Refusing it
/// says so; embedding twice would quietly make the recorded identity false.
pub fn embedder_spec(pipeline: &LogicalPipeline) -> Result<Option<EmbedderSpec>> {
    let mut found: Option<(&str, EmbedderSpec)> = None;

    for node in pipeline.nodes() {
        let LogicalNode::Retriever(node) = node else {
            continue;
        };
        if node.implementation != DENSE {
            continue;
        }

        let spec =
            embedder_of(&node.params).with_context(|| format!("node `{}`", node.id.as_str()))?;

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
/// A generator's identity is read with its node's `served_model` (ADR-C31
/// § 4), which is required: its absence is refused here, before the run,
/// rather than passed on as an empty name for the component to refuse. Any
/// refusal here ends the run before anything expensive has been done — which
/// is also what finds a missing model file early.
pub async fn model_hashes(pipeline: &LogicalPipeline) -> Result<BTreeMap<String, String>> {
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();

    for node in pipeline.nodes() {
        let id = node.id().as_str();
        let Some((role, identity)) = identity_of(node)
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
async fn identity_of(node: &LogicalNode) -> Result<Option<(&'static str, String)>> {
    use ragondin_contracts::ContextBuilder;

    let identity = match node {
        #[cfg(feature = "onnx")]
        LogicalNode::Retriever(node) if node.implementation == DENSE => {
            use ragondin_contracts::Embedder;
            let embedder = onnx_embedder(&embedder_of(&node.params)?)?;
            (EMBEDDER_ROLE, embedder.model_identity(None).await?)
        }
        #[cfg(feature = "onnx")]
        LogicalNode::Reranker(node) if node.implementation == CROSS_ENCODER => {
            use ragondin_contracts::Reranker;
            // `None`: the key-set check refuses `served_model` on this node,
            // and the ONNX reranker answers only for the model it loaded.
            let reranker = onnx_reranker(&node.params)?;
            (RERANKER_ROLE, reranker.model_identity(None).await?)
        }
        LogicalNode::ContextBuilder(node) if node.implementation == CONCAT => (
            CONTEXT_BUILDER_ROLE,
            concat(&node.params)?.model_identity().await?,
        ),
        #[cfg(feature = "stub")]
        LogicalNode::Generator(node) if node.implementation == STUB_GENERATOR => {
            use ragondin_contracts::Generator;
            let served_model = required_string(&node.params, "served_model")?;
            let generator = stub_generator(&node.params)?;
            (
                GENERATOR_ROLE,
                generator.model_identity(&served_model).await?,
            )
        }
        _ => return Ok(None),
    };
    let (role, identity) = identity;
    Ok(Some((role, identity.as_str().to_owned())))
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

/// The embedder a `dense` node configures, resolved from its `embedder:` name.
fn embedder_of(params: &Params) -> Result<EmbedderSpec> {
    let name = required_string(params, "embedder")?;
    if name.is_empty() {
        bail!("`embedder` must name an embedder, and an empty name names none");
    }
    if name != ONNX_EMBEDDER {
        bail!(
            "`embedder` names `{name}`, and the only embedder this composition root knows is \
             `{ONNX_EMBEDDER}`"
        );
    }
    refuse_keys_outside(
        params,
        &[&DENSE_KEYS, &ONNX_KEYS],
        "a dense node over the `onnx` embedder",
    )?;
    Ok(EmbedderSpec {
        model: model_of(params)?,
        query_prefix: prefix(params, "query_prefix")?,
        passage_prefix: prefix(params, "passage_prefix")?,
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
    use ragondin_pipeline::{validate, LogicalPipeline, RawPipeline};

    use super::*;

    /// Loads a pipeline from YAML, through the same lowering `ragondin-config`
    /// runs: a hand-built `LogicalPipeline` would skip the validation that
    /// decides what these functions are ever handed.
    fn pipeline(yaml: &str) -> LogicalPipeline {
        let raw: RawPipeline = serde_yaml::from_str(yaml).expect("the fixture parses");
        validate(raw).expect("the fixture validates")
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

        let spec = embedder_spec(&pipeline(&yaml))
            .expect("the node is complete")
            .expect("there is a dense node");

        assert_eq!(spec.model.model, PathBuf::from("m.onnx"));
        assert_eq!(spec.model.tokenizer, PathBuf::from("t.json"));
        assert_eq!(spec.model.max_sequence_length, None);
        assert_eq!(spec.query_prefix, "query: ");
        assert_eq!(spec.passage_prefix, "passage: ");
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

        let spec = embedder_spec(&pipeline(&yaml))
            .expect("both name one embedder")
            .expect("there are dense nodes");

        assert_eq!(spec.model.model, PathBuf::from("m.onnx"));
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
}
