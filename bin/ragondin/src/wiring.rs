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
//! configuration with a diagnosis rather than a mystery — the unknown `impl:`
//! comes back from planning, naming the family and the name it looked up.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ragondin_contracts::EmbeddedChunk;
use ragondin_engine::EngineContext;
use ragondin_pipeline::{LogicalNode, LogicalPipeline, ParamValue, Params};
use ragondin_types::Chunk;

/// The `impl:` name of the in-process BM25 retriever.
///
/// Gated, where the other three are not: nothing reads a BM25 node's
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

/// The role an embedder's model plays in run identity
/// (`docs/system-architecture.md` §7.1).
const EMBEDDER_ROLE: &str = "embedder";
/// The role a reranker's model plays in run identity
/// (`docs/system-architecture.md` §7.1).
const RERANKER_ROLE: &str = "reranker";

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
    /// the component's, free to disagree (`docs/code-architecture.md` §6.4).
    pub max_sequence_length: Option<usize>,
}

/// What a `dense` node says about the embedder it retrieves through.
///
/// The prefixes travel with the model because they decide the vectors: two
/// nodes naming one model under different prefixes are two embedders, and the
/// corpus one of them indexed is not the corpus the other would search
/// (ADR-C17).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbedderSpec {
    /// The model and its tokenizer.
    pub model: ModelSpec,
    /// Prepended to a query before it is embedded.
    pub query_prefix: String,
    /// Prepended to a passage before it is embedded.
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
            Box::new(|params| {
                let spec = model_of(params)?;
                let mut config =
                    ragondin_reranker_onnx::OnnxRerankerConfig::new(&spec.model, &spec.tokenizer);
                if let Some(tokens) = spec.max_sequence_length {
                    config.max_sequence_length = nonzero(tokens, "max_sequence_length")?;
                }
                Ok(Box::new(ragondin_reranker_onnx::OnnxReranker::new(config)?))
            }),
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

/// A count a component takes as a [`NonZeroUsize`](std::num::NonZeroUsize).
///
/// Zero is refused here rather than cast, for the reason `optional_usize`
/// refuses a negative: the configuration is where a nonsensical count is still
/// attached to the key that carries it.
#[cfg(feature = "onnx")]
fn nonzero(value: usize, key: &str) -> Result<std::num::NonZeroUsize> {
    std::num::NonZeroUsize::new(value).ok_or_else(|| anyhow::anyhow!("`{key}` must not be zero"))
}

/// Refuses a pipeline holding anything M2 does not run.
///
/// The milestone carries no judge, no generation, no serving and no control
/// flow, and today none of those has a primitive node of its own — an
/// `Extension` node is how they would arrive (ADR-C3). So this is the whole of
/// the check, and it will grow a case the day a primitive does.
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
             v0 evaluates retrieval only — no judge, no generation, no control flow",
            extensions.join(", ")
        );
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

/// The model hashes of the run, by the role each model played
/// (`docs/system-architecture.md` §7.1).
///
/// Taken here rather than inside a component: a run is identified by what it
/// read, and only the composition root sees every node's configuration at
/// once. A role is recorded once, so two nodes on one role must name the same
/// model — the same v0 rule [`embedder_spec`] states, seen from the identity
/// side.
pub fn model_hashes(pipeline: &LogicalPipeline) -> Result<BTreeMap<String, String>> {
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();

    for node in pipeline.nodes() {
        let (role, params, id) = match node {
            LogicalNode::Retriever(node) if node.implementation == DENSE => {
                (EMBEDDER_ROLE, &node.params, node.id.as_str())
            }
            LogicalNode::Reranker(node) if node.implementation == CROSS_ENCODER => {
                (RERANKER_ROLE, &node.params, node.id.as_str())
            }
            _ => continue,
        };

        let spec = model_of(params).with_context(|| format!("node `{id}`"))?;
        let digest = file_digest(&spec.model)
            .with_context(|| format!("node `{id}` names a model that cannot be hashed"))?;

        match hashes.get(role) {
            Some(seen) if *seen != digest => bail!(
                "node `{id}` names a second `{role}` model, and a run records one model per \
                 role: evaluate them as two pipelines"
            ),
            _ => {
                hashes.insert(role.to_string(), digest);
            }
        }
    }

    Ok(hashes)
}

/// The SHA-256 of a file's bytes, hex-encoded.
///
/// A model is an opaque artifact, so its identity is its content and nothing
/// else: not its path, which moves between machines, and not its modification
/// time, which a checkout resets.
fn file_digest(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};

    let bytes =
        std::fs::read(path).with_context(|| format!("reading the model at {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// The model, tokenizer and token budget a node configures.
fn model_of(params: &Params) -> Result<ModelSpec> {
    Ok(ModelSpec {
        model: PathBuf::from(required_string(params, "model")?),
        tokenizer: PathBuf::from(required_string(params, "tokenizer")?),
        max_sequence_length: optional_usize(params, "max_sequence_length")?,
    })
}

/// The embedder a `dense` node configures.
fn embedder_of(params: &Params) -> Result<EmbedderSpec> {
    Ok(EmbedderSpec {
        model: model_of(params)?,
        query_prefix: optional_string(params, "query_prefix")?.unwrap_or_default(),
        passage_prefix: optional_string(params, "passage_prefix")?.unwrap_or_default(),
    })
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
        let yaml = wrap(
            "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
             inputs: [question]\n      params: { top_k: 10 }\n",
        );

        refuse_unsupported(&pipeline(&yaml)).expect("bm25 alone is what v0 is for");
    }

    #[test]
    fn the_embedder_of_a_pipeline_with_no_dense_node_is_absent() {
        let yaml = wrap(
            "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
             inputs: [question]\n      params: { top_k: 10 }\n",
        );

        assert_eq!(
            embedder_spec(&pipeline(&yaml)).expect("no dense node"),
            None
        );
    }

    #[test]
    fn a_dense_node_carries_its_model_tokenizer_and_prefixes() {
        let yaml = wrap(&dense_node(
            "vectors",
            "{ top_k: 10, model: m.onnx, tokenizer: t.json, query_prefix: 'query: ', \
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
        let yaml = wrap(&dense_node("vectors", "{ top_k: 10, tokenizer: t.json }"));

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
            dense_node("left", "{ top_k: 10, model: m.onnx, tokenizer: t.json }"),
            dense_node("right", "{ top_k: 5, model: m.onnx, tokenizer: t.json }"),
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
            dense_node("left", "{ top_k: 10, model: m.onnx, tokenizer: t.json }"),
            dense_node(
                "right",
                "{ top_k: 5, model: m.onnx, tokenizer: t.json, passage_prefix: 'passage: ' }"
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
            "{ top_k: 10, model: m.onnx, tokenizer: t.json, max_sequence_length: -1 }",
        ));

        let error = embedder_spec(&pipeline(&yaml)).expect_err("-1 is not a token budget");

        assert!(
            format!("{error:#}").contains("must not be negative"),
            "{error:#}"
        );
    }

    #[test]
    fn a_pipeline_naming_no_model_records_no_model_hash() {
        let yaml = wrap(
            "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
             inputs: [question]\n      params: { top_k: 10 }\n",
        );

        assert!(model_hashes(&pipeline(&yaml))
            .expect("bm25 reads no model")
            .is_empty());
    }
}
