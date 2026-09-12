//! The cross-encoder, its configuration, and its errors.

use std::borrow::Cow;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ort::session::{Session, SessionInputValue};
use ort::value::Tensor;
use ragondin_contracts::{ComponentError, RerankParams, Reranker};
use ragondin_types::{Query, ScoredChunk};
use thiserror::Error;
use tokenizers::tokenizer::{Tokenizer, TruncationDirection, TruncationParams, TruncationStrategy};

/// The tensors this component can build, in the order a pair encoder produces
/// them. A model that asks for anything else is rejected at construction
/// rather than fed a tensor of zeros.
const PAIR_INPUTS: [&str; 3] = ["input_ids", "attention_mask", "token_type_ids"];

/// The two a cross-encoder cannot do without. `token_type_ids` is genuinely
/// optional: a RoBERTa-based cross-encoder has no segment embedding and
/// declares no such input.
const REQUIRED_INPUTS: [&str; 2] = ["input_ids", "attention_mask"];

/// How a cross-encoder is loaded and run.
///
/// The paths have no default; the three knobs do. Every field is public, so a
/// caller overrides one by assignment rather than through a builder method per
/// knob:
///
/// ```no_run
/// # use std::num::NonZeroUsize;
/// # use ragondin_reranker_onnx::OnnxRerankerConfig;
/// let mut config = OnnxRerankerConfig::new("model.onnx", "tokenizer.json");
/// config.batch_size = NonZeroUsize::new(32).unwrap();
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnnxRerankerConfig {
    /// The ONNX cross-encoder: `(query, passage)` pairs in, one score per pair
    /// out.
    pub model_path: PathBuf,
    /// Its tokenizer, in `tokenizers`' JSON format — the `tokenizer.json` that
    /// ships beside the model.
    pub tokenizer_path: PathBuf,
    /// How many pairs are scored per forward pass.
    ///
    /// Batch *composition* is fixed by the input order rather than by arrival
    /// time, so this changes how the work is divided and not which pairs share
    /// a pass. See `ARCHITECTURE.md` on what is and is not reproducible.
    pub batch_size: NonZeroUsize,
    /// The longest encoded pair the model is given, in tokens.
    ///
    /// Longer pairs are truncated, longest segment first, which is what a
    /// cross-encoder's own preprocessing does. The default is BERT's limit;
    /// a model with a shorter positional table needs this lowered, and will
    /// otherwise fail inside ONNX Runtime rather than silently.
    pub max_sequence_length: NonZeroUsize,
    /// How many threads ONNX Runtime may use inside a single operator.
    ///
    /// One by default, and that is a reproducibility choice rather than a
    /// conservative guess — see `ARCHITECTURE.md`.
    pub intra_threads: NonZeroUsize,
}

impl OnnxRerankerConfig {
    /// The default configuration for the model and tokenizer at these paths.
    pub fn new(model_path: impl Into<PathBuf>, tokenizer_path: impl Into<PathBuf>) -> Self {
        Self {
            model_path: model_path.into(),
            tokenizer_path: tokenizer_path.into(),
            batch_size: NonZeroUsize::new(16).expect("16 is not zero"),
            max_sequence_length: NonZeroUsize::new(512).expect("512 is not zero"),
            intra_threads: NonZeroUsize::new(1).expect("1 is not zero"),
        }
    }
}

/// What the model or its tokenizer can fail at.
///
/// Typed, so a `Local` caller can walk [`std::error::Error::source`] to the
/// cause (ADR-C13). It is returned directly by [`OnnxReranker::new`] and
/// boxed into [`ComponentError::Backend`] by [`Reranker::rerank`], because the
/// contract admits one error type for both faces and a cross-encoder failing
/// mid-run is a backend failure however it failed.
#[derive(Debug, Error)]
pub enum ModelError {
    /// ONNX Runtime refused to load the model, or to run it.
    #[error("ONNX Runtime: {0}")]
    Runtime(#[from] ort::Error),

    /// The tokenizer could not be loaded, or could not encode a pair.
    #[error("tokenizer: {0}")]
    Tokenizer(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// The model declares an input this component does not know how to build.
    #[error("the model declares an input this component cannot supply: {0}")]
    UnsupportedInput(String),

    /// The model declares none of the inputs a cross-encoder needs.
    #[error("the model declares no `{0}` input")]
    MissingInput(&'static str),

    /// The model has no output, so there is no score to read.
    #[error("the model declares no output")]
    MissingOutput,

    /// The model's output is not one score per pair.
    ///
    /// A cross-encoder emits a single relevance logit per `(query, passage)`
    /// pair. Anything else — a two-way classification head, a token-level
    /// output — is a different model, and guessing which column means
    /// *relevant* would be guessing at the caller's expense.
    #[error("the model returned {returned} scores for {pairs} pairs")]
    OutputShape {
        /// How many scores the model emitted.
        returned: usize,
        /// How many pairs were scored.
        pairs: usize,
    },

    /// The model produced a score that is not finite.
    ///
    /// The ranking contract requires finite scores, and a `NaN` would make the
    /// comparison that sorts them meaningless rather than merely wrong.
    #[error("the model produced a score that is not finite")]
    NonFiniteScore,

    /// A previous call panicked while holding the session.
    ///
    /// The session's state after a panic inside ONNX Runtime is not this
    /// crate's to vouch for, so the component reports it rather than carrying
    /// on over it.
    #[error("the model session is poisoned by an earlier panic")]
    PoisonedSession,
}

/// An in-process cross-encoder [`Reranker`] over ONNX Runtime.
///
/// See the crate documentation for what it is and `ARCHITECTURE.md` for why it
/// is built this way.
pub struct OnnxReranker {
    /// Shared with the blocking task that runs the model, which is why this is
    /// an `Arc` rather than a field.
    inner: Arc<CrossEncoder>,
    batch_size: NonZeroUsize,
}

struct CrossEncoder {
    /// One session, serialized. ONNX Runtime's `Run` takes the session
    /// mutably here, and a pool of sessions is a separate decision from the
    /// off-thread hop (ADR-C25) — recorded as such in `ARCHITECTURE.md`.
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    /// The pair tensors this model actually declares, in its own order.
    inputs: Vec<String>,
    output: String,
}

impl OnnxReranker {
    /// Loads the cross-encoder and the tokenizer `config` names.
    ///
    /// Both are read from disk here, at construction — which is exactly what a
    /// `ComponentCtor` does at physical planning, and what ADR-C25 leaves
    /// outside the rule that a call does not block its caller.
    pub fn new(config: OnnxRerankerConfig) -> Result<Self, ModelError> {
        let mut tokenizer =
            Tokenizer::from_file(&config.tokenizer_path).map_err(ModelError::Tokenizer)?;
        // Imposed rather than inherited from the file: the tensors are padded
        // to the longest pair in a batch, so a pair the file would let run long
        // sets the width of every pair beside it.
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: config.max_sequence_length.get(),
                strategy: TruncationStrategy::LongestFirst,
                stride: 0,
                direction: TruncationDirection::Right,
            }))
            .map_err(ModelError::Tokenizer)?;

        let mut builder = Session::builder()?
            .with_intra_threads(config.intra_threads.get())
            .map_err(drop_builder)?;
        let session = builder.commit_from_file(&config.model_path)?;

        let inputs: Vec<String> = session
            .inputs()
            .iter()
            .map(|input| input.name().to_string())
            .collect();
        if let Some(unsupported) = inputs
            .iter()
            .find(|name| !PAIR_INPUTS.contains(&name.as_str()))
        {
            return Err(ModelError::UnsupportedInput(unsupported.clone()));
        }
        if let Some(missing) = REQUIRED_INPUTS
            .iter()
            .find(|required| !inputs.iter().any(|name| name == *required))
        {
            return Err(ModelError::MissingInput(missing));
        }

        let output = session
            .outputs()
            .first()
            .ok_or(ModelError::MissingOutput)?
            .name()
            .to_string();

        Ok(Self {
            inner: Arc::new(CrossEncoder {
                session: Mutex::new(session),
                tokenizer,
                inputs,
                output,
            }),
            batch_size: config.batch_size,
        })
    }
}

/// A failed session option hands the builder back inside the error, so that a
/// caller can retry with the option dropped. There is nothing to retry here —
/// the option is the reproducibility choice — so the builder is discarded and
/// the status kept.
fn drop_builder(error: ort::Error<ort::session::builder::SessionBuilder>) -> ort::Error {
    ort::Error::new_with_code(error.code(), error.message())
}

impl CrossEncoder {
    /// Scores every `(query, chunk)` pair, in the order the chunks arrive.
    ///
    /// Runs on a blocking thread: it holds the session lock across a forward
    /// pass, which is the work ADR-C25 keeps off the caller's thread.
    fn score(
        &self,
        query: &str,
        chunks: &[ScoredChunk],
        batch_size: usize,
    ) -> Result<Vec<f32>, ModelError> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| ModelError::PoisonedSession)?;

        let mut scores = Vec::with_capacity(chunks.len());
        for batch in chunks.chunks(batch_size) {
            self.score_batch(&mut session, query, batch, &mut scores)?;
        }
        Ok(scores)
    }

    fn score_batch(
        &self,
        session: &mut Session,
        query: &str,
        batch: &[ScoredChunk],
        scores: &mut Vec<f32>,
    ) -> Result<(), ModelError> {
        let encodings = batch
            .iter()
            .map(|candidate| {
                self.tokenizer
                    .encode((query, candidate.chunk.text.as_str()), true)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(ModelError::Tokenizer)?;

        let rows = encodings.len();
        let width = encodings
            .iter()
            .map(|encoding| encoding.len())
            .max()
            .expect("a batch of chunks is never empty");

        // Padding is zero in all three tensors. `attention_mask` is zero at
        // those positions, so what `input_ids` and `token_type_ids` hold there
        // is not read by a model that honours its own mask.
        let mut input_ids = vec![0i64; rows * width];
        let mut attention_mask = vec![0i64; rows * width];
        let mut token_type_ids = vec![0i64; rows * width];
        for (row, encoding) in encodings.iter().enumerate() {
            let start = row * width;
            for (column, id) in encoding.get_ids().iter().enumerate() {
                input_ids[start + column] = i64::from(*id);
            }
            for (column, mask) in encoding.get_attention_mask().iter().enumerate() {
                attention_mask[start + column] = i64::from(*mask);
            }
            for (column, segment) in encoding.get_type_ids().iter().enumerate() {
                token_type_ids[start + column] = i64::from(*segment);
            }
        }

        let shape = vec![rows as i64, width as i64];
        let mut values: Vec<(Cow<'_, str>, SessionInputValue<'_>)> =
            Vec::with_capacity(self.inputs.len());
        for name in &self.inputs {
            let data = match name.as_str() {
                "input_ids" => &input_ids,
                "attention_mask" => &attention_mask,
                // The only name left: `new` rejected any input outside
                // `PAIR_INPUTS`, so nothing else can reach here.
                _ => &token_type_ids,
            };
            let tensor = Tensor::from_array((shape.clone(), data.clone()))?;
            values.push((Cow::from(name.clone()), SessionInputValue::from(tensor)));
        }

        let outputs = session.run(values)?;
        let (_, logits) = outputs[self.output.as_str()].try_extract_tensor::<f32>()?;
        if logits.len() != rows {
            return Err(ModelError::OutputShape {
                returned: logits.len(),
                pairs: rows,
            });
        }
        if logits.iter().any(|score| !score.is_finite()) {
            return Err(ModelError::NonFiniteScore);
        }
        scores.extend_from_slice(logits);
        Ok(())
    }
}

#[async_trait]
impl Reranker for OnnxReranker {
    async fn rerank(
        &self,
        query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest(
                "a top_k of zero asks for a result that cannot exist".to_string(),
            ));
        }
        // ADR-C19: an empty collection is a valid call and nothing is done with
        // it. No forward pass, and no thread hop to run one on.
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        let encoder = Arc::clone(&self.inner);
        let batch_size = self.batch_size.get();
        let query = query.text.clone();

        // ADR-C25: the forward pass, the tokenization that feeds it and the
        // lock held across both are CPU-bound, so they run off the thread that
        // called this. The chunks travel with them and come back, rather than
        // being cloned to be scored.
        let scored = tokio::task::spawn_blocking(move || {
            let scores = encoder.score(&query, &chunks, batch_size)?;
            Ok::<_, ModelError>((chunks, scores))
        })
        .await
        .map_err(|panicked| ComponentError::Backend(Box::new(panicked)))?
        .map_err(|failed| ComponentError::Backend(Box::new(failed)))?;

        let (chunks, scores) = scored;
        let mut reordered: Vec<ScoredChunk> = chunks
            .into_iter()
            .zip(scores)
            .map(|(mut candidate, score)| {
                candidate.score = score;
                candidate
            })
            .collect();
        // Descending, and ties broken by chunk id so that one candidate set
        // ranks one way whatever order it arrived in. Every score is finite by
        // the time it gets here, which is what makes `total_cmp` a total order
        // over them rather than a panic waiting for a `NaN`.
        reordered.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.chunk.id.as_str().cmp(b.chunk.id.as_str()))
        });
        reordered.truncate(params.top_k);
        Ok(reordered)
    }
}
