//! The [`Embedder`] implementation, and the configuration it is built from.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ort::session::{Session, SessionInputValue};
use ort::value::Tensor;
use ragondin_contracts::{ComponentError, EmbedParams, EmbedRole, Embedder};
use ragondin_types::Embedding;
use thiserror::Error;
use tokenizers::tokenizer::{
    PostProcessor, Tokenizer, TruncationDirection, TruncationParams, TruncationStrategy,
};

/// The token ids of a batch. The one input this component cannot do without.
const INPUT_IDS: &str = "input_ids";
/// Which positions carry a token, as opposed to padding.
const ATTENTION_MASK: &str = "attention_mask";
/// Which segment each token belongs to. Always one segment here.
const TOKEN_TYPE_IDS: &str = "token_type_ids";

/// How long a sequence is tokenized to before the model sees it, unless
/// configured otherwise. 512 is the input width of the BERT-family encoders
/// this component is built for.
const DEFAULT_MAX_SEQUENCE_LENGTH: usize = 512;

/// How many texts are pushed through the model at once, unless configured
/// otherwise.
const DEFAULT_BATCH_SIZE: usize = 32;

/// How many threads ONNX Runtime may split one operator's work across, unless
/// configured otherwise. One, so that a reduction happens in one order.
const DEFAULT_INTRA_THREADS: usize = 1;

/// What an [`OnnxEmbedder`] is built from.
///
/// Everything here is **constructor configuration** rather than a per-call
/// parameter: it is fixed for the life of the component, and the split is the
/// one `ragondin-contracts` states — implementation-specific configuration to
/// the constructor, only what varies per call in the params struct. The
/// per-role prefixes are configuration for exactly that reason (ADR-C17): the
/// text a model wants prepended is fixed when the component is built, while the
/// role it applies to is known only per call.
///
/// The two paths are read when the component is constructed and never later.
/// Nothing is downloaded at any point.
#[derive(Clone, Debug)]
pub struct OnnxEmbedderConfig {
    model: PathBuf,
    tokenizer: PathBuf,
    query_prefix: String,
    passage_prefix: String,
    max_sequence_length: NonZeroUsize,
    batch_size: NonZeroUsize,
    intra_threads: NonZeroUsize,
}

impl OnnxEmbedderConfig {
    /// Configures an embedder over the ONNX model at `model` and the
    /// HuggingFace `tokenizer.json` at `tokenizer`.
    ///
    /// The prefixes default to empty on both sides, which is the symmetric
    /// configuration: a model that draws no distinction between a query and a
    /// passage honours the role by prepending the empty string to each
    /// (ADR-C17), and is therefore not a special case.
    pub fn new(model: impl Into<PathBuf>, tokenizer: impl Into<PathBuf>) -> Self {
        Self {
            model: model.into(),
            tokenizer: tokenizer.into(),
            query_prefix: String::new(),
            passage_prefix: String::new(),
            max_sequence_length: nonzero(DEFAULT_MAX_SEQUENCE_LENGTH),
            batch_size: nonzero(DEFAULT_BATCH_SIZE),
            intra_threads: nonzero(DEFAULT_INTRA_THREADS),
        }
    }

    /// Sets the text prepended to a query and to a passage.
    ///
    /// Both at once, because an asymmetric model's two prefixes are one fact
    /// about one model: setting a query prefix and forgetting the passage one
    /// is the silent multi-point regression ADR-C17 exists to prevent, and it
    /// would raise no error anywhere.
    #[must_use]
    pub fn with_prefixes(mut self, query: impl Into<String>, passage: impl Into<String>) -> Self {
        self.query_prefix = query.into();
        self.passage_prefix = passage.into();
        self
    }

    /// Sets how many tokens of a text reach the model. Longer texts are
    /// truncated; the default is 512.
    #[must_use]
    pub fn with_max_sequence_length(mut self, tokens: NonZeroUsize) -> Self {
        self.max_sequence_length = tokens;
        self
    }

    /// Sets how many texts are embedded per forward pass. The default is 32.
    ///
    /// It is a performance knob and nothing more: batching happens inside the
    /// component, behind the trait (`docs/code-architecture.md` §11.2), and the
    /// vectors do not depend on it.
    #[must_use]
    pub fn with_batch_size(mut self, texts: NonZeroUsize) -> Self {
        self.batch_size = texts;
        self
    }

    /// Sets how many threads ONNX Runtime may split one operator's work
    /// across. The default is one.
    ///
    /// The default is not a performance choice: a session built with a
    /// different intra-op thread count can reduce in a different order, so the
    /// last bits of a vector move with it. Pinning it to one removes that
    /// variable at a real cost in throughput, and raising it is a trade a
    /// caller embedding a corpus can make deliberately.
    #[must_use]
    pub fn with_intra_threads(mut self, threads: NonZeroUsize) -> Self {
        self.intra_threads = threads;
        self
    }
}

/// A [`NonZeroUsize`] from a literal this crate wrote itself.
fn nonzero(n: usize) -> NonZeroUsize {
    match NonZeroUsize::new(n) {
        Some(n) => n,
        // Unreachable for the two constants above, and an `expect` would put a
        // panic path in a library for a value no caller supplies.
        None => NonZeroUsize::MIN,
    }
}

/// What can go wrong loading or running an ONNX embedder.
///
/// Typed, and this crate's own: a library never imposes `anyhow` on its
/// consumers (ADR-C13). The first five variants arise when the component is
/// **built** — they report a configuration or wiring mistake, and reporting
/// them at construction is what keeps them out of the query path. The rest
/// arise per call, and reach a caller boxed inside
/// [`ComponentError::Backend`], whose in-process fidelity is what lets them be
/// walked back to here.
#[derive(Debug, Error)]
pub enum EmbedderError {
    /// The ONNX model could not be loaded.
    #[error("the ONNX model at {path} could not be loaded")]
    ModelLoad {
        /// The path that was tried.
        path: PathBuf,
        /// What ONNX Runtime said about it.
        #[source]
        source: ort::Error,
    },

    /// The tokenizer could not be loaded, or could not be configured to
    /// truncate.
    #[error("the tokenizer at {path} could not be prepared")]
    TokenizerLoad {
        /// The path that was tried.
        path: PathBuf,
        /// What the tokenizer said about it.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The model demands an input this component has no value for.
    ///
    /// It knows three: `input_ids`, `attention_mask` and `token_type_ids`. A
    /// model wanting anything else — a position id it does not derive itself, a
    /// temperature, a cached key — is not a sentence encoder this component can
    /// drive.
    #[error(
        "the model demands an input this component cannot supply: `{name}` \
         (it supplies `input_ids`, `attention_mask` and `token_type_ids`)"
    )]
    UnknownModelInput {
        /// The input the model declared.
        name: String,
    },

    /// The model takes no `input_ids`, so no text reaches it.
    #[error("the model declares no `input_ids` input, so it encodes no text")]
    NoTokenInput,

    /// The configured `max_sequence_length` leaves no room for any text.
    ///
    /// A tokenizer's post-processor wraps a sequence in special tokens, and
    /// `tokenizers` subtracts their count from the truncation limit with
    /// unchecked `usize` arithmetic — in `with_truncation`, and again per call
    /// in `post_process`. *At* the count the effective limit is zero, so every
    /// text is truncated away and every vector is the special tokens' own.
    /// *Below* it the subtraction underflows: a debug build panics and a
    /// release build wraps to `usize::MAX`, which switches truncation off — the
    /// opposite of what lowering the knob asks for, and then every batch is
    /// padded to an unbounded width. Both are refused here, the same way in
    /// both profiles.
    #[error(
        "a max_sequence_length of {max_sequence_length} leaves no room for text: \
         the tokenizer's post-processor adds {added_tokens} tokens to every sequence"
    )]
    MaxSequenceLengthTooSmall {
        /// The limit that was configured.
        max_sequence_length: usize,
        /// What this tokenizer's post-processor adds to one sequence.
        added_tokens: usize,
    },

    /// A batch could not be tokenized.
    #[error("a batch could not be tokenized")]
    Tokenize(
        /// What the tokenizer said about it.
        #[source]
        Box<dyn std::error::Error + Send + Sync>,
    ),

    /// A tensor could not be built, or the model failed to run.
    ///
    /// Not the variant for a model whose output this component cannot pool:
    /// every variant below says what is wrong with such an output instead,
    /// because "the model failed to run" is false of a model that ran and
    /// answered.
    #[error("the model failed to run")]
    Inference(
        /// What ONNX Runtime said about it.
        #[source]
        ort::Error,
    ),

    /// The model produced no output at all.
    #[error("the model returned no output to pool")]
    NoOutput,

    /// The model's first output is not float32.
    ///
    /// A quantized export shipping float16 hidden states is the ordinary way
    /// to arrive here, and it is a different fact from the model failing: it
    /// loaded, it ran, and what it returned is not what this component pools.
    #[error("the model's first output is not float32, which is what mask pooling reads")]
    OutputNotFloat32(
        /// What ONNX Runtime said when the extraction was attempted.
        #[source]
        ort::Error,
    ),

    /// The model's first output is not `[batch, sequence, hidden]`.
    ///
    /// Pooling is over the sequence axis with the attention mask, so a model
    /// that pools for itself — returning `[batch, hidden]` — cannot be driven
    /// by this component even though it loads.
    #[error(
        "the model's first output is {shape:?}, where mask pooling needs \
         [batch, sequence, hidden]"
    )]
    UnexpectedOutput {
        /// The shape that came back.
        shape: Vec<i64>,
    },

    /// The model answered a batch of one size with a batch of another.
    #[error("the model answered {rows} texts with {returned} rows")]
    BatchMismatch {
        /// How many texts went in.
        rows: usize,
        /// How many rows came back.
        returned: usize,
    },

    /// The model answered a padded batch of one width with a sequence axis of
    /// another.
    ///
    /// Pooling reads position `n` of the output against position `n` of the
    /// mask that was fed, so the two axes have to be the same length. A graph
    /// that strips a `[CLS]` for itself is the ordinary way they differ, and
    /// pooling one against the other reads either out of bounds or into the
    /// next row.
    #[error("the model answered a batch {width} wide with a sequence axis of {returned}")]
    SequenceMismatch {
        /// How wide the padded batch that was fed is.
        width: usize,
        /// How long the sequence axis that came back is.
        returned: usize,
    },

    /// A pooled vector has a component that is not finite.
    ///
    /// Mean pooling over a hidden state carrying `NaN` or an infinity produces
    /// one, and L2 normalization propagates it rather than removing it. The
    /// contract requires finite components, and `ragondin-types` says such a
    /// vector cannot be read back once serialized — so it is reported here,
    /// where the model that produced it can still be named, rather than
    /// returned for a `VectorStore` to score.
    #[error("the model's hidden states pool to a vector with a component that is not finite")]
    NonFiniteVector,

    /// A previous call panicked while holding the model.
    ///
    /// The session is behind a lock, so a panic inside one call would otherwise
    /// leave every later call to panic on the poisoned lock instead of
    /// returning.
    #[error("the model is unusable: a previous call panicked while holding it")]
    Poisoned,
}

/// Embeddings from an ONNX sentence-embedding model, computed in this process.
///
/// # What it does to a text
///
/// The role's prefix is prepended, the tokenizer truncates to the configured
/// length, a batch is padded to its own longest member, and the model's
/// per-token output is **mean-pooled over the attention mask** and then
/// **L2-normalized**. That is the sentence-transformers convention, and it is
/// what makes a dot product against these vectors a cosine similarity.
///
/// # Why the model is behind a lock
///
/// [`Embedder`] takes `&self` and is `Send + Sync`, so one embedder answers
/// concurrent calls; an ONNX Runtime session is not safe to run concurrently
/// and takes `&mut` to say so. The session therefore sits behind a
/// [`Mutex`] — the interior mutability the contract asks of an implementation
/// that holds mutable state — and calls queue rather than racing.
///
/// # It does not block the thread that called it
///
/// Tokenization, the forward pass and the lock held across both run inside
/// `tokio::task::spawn_blocking`, which is how this component meets ADR-C25.
/// That makes an ambient `tokio` runtime a requirement of *this* component —
/// see the crate documentation and `ARCHITECTURE.md`.
pub struct OnnxEmbedder {
    /// Shared with the blocking task that runs the model, which is why this is
    /// an `Arc` rather than a set of fields.
    inner: Arc<Encoder>,
    query_prefix: String,
    passage_prefix: String,
    batch_size: NonZeroUsize,
}

/// Everything the blocking task needs: the model, the tokenizer that feeds it,
/// and which of the three inputs it declared.
struct Encoder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    /// The inputs the model declared, of the three this component knows. Fed by
    /// name and only when declared: a model that does not ask for a mask must
    /// not be handed one.
    feeds_attention_mask: bool,
    feeds_token_type_ids: bool,
}

impl OnnxEmbedder {
    /// Loads the model and the tokenizer named by `config`.
    ///
    /// Everything checkable about the pair is checked here rather than on the
    /// first query: that the files load, and that the model's inputs are ones
    /// this component can supply. What is left for the call is what only a real
    /// tensor settles — the shape the model returns, whose sequence axis is
    /// dynamic until there is a batch.
    pub fn new(config: OnnxEmbedderConfig) -> Result<Self, EmbedderError> {
        let session = load_session(&config.model, config.intra_threads)?;

        let mut feeds_attention_mask = false;
        let mut feeds_token_type_ids = false;
        let mut feeds_input_ids = false;
        for input in session.inputs() {
            match input.name() {
                INPUT_IDS => feeds_input_ids = true,
                ATTENTION_MASK => feeds_attention_mask = true,
                TOKEN_TYPE_IDS => feeds_token_type_ids = true,
                other => {
                    return Err(EmbedderError::UnknownModelInput {
                        name: other.to_string(),
                    })
                }
            }
        }
        if !feeds_input_ids {
            return Err(EmbedderError::NoTokenInput);
        }

        Ok(Self {
            inner: Arc::new(Encoder {
                session: Mutex::new(session),
                tokenizer: load_tokenizer(&config.tokenizer, config.max_sequence_length)?,
                feeds_attention_mask,
                feeds_token_type_ids,
            }),
            query_prefix: config.query_prefix,
            passage_prefix: config.passage_prefix,
            batch_size: config.batch_size,
        })
    }

    /// The text this role prepends, per ADR-C17.
    fn prefix(&self, role: EmbedRole) -> &str {
        match role {
            EmbedRole::Query => &self.query_prefix,
            EmbedRole::Passage => &self.passage_prefix,
        }
    }
}

impl Encoder {
    /// Embeds every text, one forward pass per batch, in order.
    ///
    /// Runs on a blocking thread: it tokenizes, builds tensors and holds the
    /// session lock across every forward pass, which is the work ADR-C25 keeps
    /// off the caller's thread. The texts arrive already prefixed.
    fn embed_all(
        &self,
        texts: &[String],
        batch_size: usize,
    ) -> Result<Vec<Embedding>, EmbedderError> {
        let mut session = self.session.lock().map_err(|_| EmbedderError::Poisoned)?;

        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(batch_size) {
            self.embed_batch(&mut session, batch, &mut vectors)?;
        }
        Ok(vectors)
    }

    /// One forward pass over at most `batch_size` already-prefixed texts.
    fn embed_batch(
        &self,
        session: &mut Session,
        texts: &[String],
        vectors: &mut Vec<Embedding>,
    ) -> Result<(), EmbedderError> {
        let prefixed: Vec<&str> = texts.iter().map(String::as_str).collect();
        let encodings = self
            .tokenizer
            .encode_batch(prefixed, true)
            .map_err(EmbedderError::Tokenize)?;

        let rows = encodings.len();
        // The batch is padded to its own longest member rather than to the
        // configured maximum: a batch of short texts is then a small tensor.
        // Which member is longest cannot change a vector, because pooling reads
        // the mask.
        let width = encodings
            .iter()
            .map(|encoding| encoding.get_ids().len())
            .max()
            .unwrap_or(0);

        let mut ids = vec![0i64; rows * width];
        let mut mask = vec![0i64; rows * width];
        for (row, encoding) in encodings.iter().enumerate() {
            for (column, id) in encoding.get_ids().iter().enumerate() {
                ids[row * width + column] = i64::from(*id);
                // Padding keeps the id 0 it was initialized with, and its mask
                // stays 0. The id itself is immaterial twice over: the model's
                // own attention masks it, and the pooling below skips it.
                mask[row * width + column] = 1;
            }
        }

        let shape = vec![rows as i64, width as i64];
        let mut inputs: Vec<(&str, SessionInputValue<'_>)> = Vec::with_capacity(3);
        inputs.push((INPUT_IDS, tensor(shape.clone(), ids)?));
        if self.feeds_attention_mask {
            inputs.push((ATTENTION_MASK, tensor(shape.clone(), mask.clone())?));
        }
        if self.feeds_token_type_ids {
            // One segment. A sentence encoder embedding a single text has no
            // second one, and a model that wants segment ids still wants them
            // present.
            inputs.push((TOKEN_TYPE_IDS, tensor(shape, vec![0i64; rows * width])?));
        }

        let outputs = session.run(inputs).map_err(EmbedderError::Inference)?;
        // The first output, by position rather than by name: `last_hidden_state`
        // is the convention, not a rule, and a model free to name it otherwise
        // is not free to reorder it.
        let mut values = outputs.values();
        let first = values.next().ok_or(EmbedderError::NoOutput)?;
        let (dims, hidden_states) = first
            .try_extract_tensor::<f32>()
            .map_err(EmbedderError::OutputNotFloat32)?;

        // Every axis pooling indexes by is checked here, and none is taken on
        // trust. The two mismatches below are what a shape *declaration* cannot
        // settle: both axes are dynamic in the graph, so they are knowable only
        // from the tensor in hand.
        let [batch, sequence, hidden] = dims[..] else {
            return Err(EmbedderError::UnexpectedOutput {
                shape: dims.to_vec(),
            });
        };
        let returned = batch.max(0) as usize;
        if returned != rows {
            return Err(EmbedderError::BatchMismatch { rows, returned });
        }
        let returned = sequence.max(0) as usize;
        if returned != width {
            return Err(EmbedderError::SequenceMismatch { width, returned });
        }
        let dim = hidden.max(0) as usize;

        vectors.reserve(rows);
        for row in 0..rows {
            vectors.push(pool(
                &hidden_states[row * width * dim..(row + 1) * width * dim],
                &mask[row * width..(row + 1) * width],
                dim,
            )?);
        }
        Ok(())
    }
}

/// One row's vector: the mean of the positions the mask keeps, normalized.
///
/// Both slices are exactly one row: `hidden_states` is the row's `width * dim`
/// components and `mask` its `width` flags.
fn pool(hidden_states: &[f32], mask: &[i64], dim: usize) -> Result<Embedding, EmbedderError> {
    let mut summed = vec![0f32; dim];
    let mut kept = 0f32;
    for (position, _) in mask.iter().enumerate().filter(|(_, keep)| **keep == 1) {
        kept += 1.0;
        for (component, sum) in summed.iter_mut().enumerate() {
            *sum += hidden_states[position * dim + component];
        }
    }

    if kept == 0.0 {
        // A text that tokenizes to no tokens at all — the empty string, under
        // a tokenizer that adds no `[CLS]` — has no mean. It embeds to the zero
        // vector rather than to `NaN`, which the contract forbids and which
        // `ragondin-types` says cannot be read back once serialized. The width
        // is still the model's, so the batch stays aligned with its input.
        return Ok(Embedding::new(summed));
    }

    for sum in &mut summed {
        *sum /= kept;
    }

    // Before normalization, because normalization propagates a `NaN` instead
    // of removing it and the norm of a vector holding one is itself `NaN`: the
    // guard below would then be taken as though the vector were zero, and a
    // batch of `NaN` would be returned looking well-formed. What the model
    // computed is not this crate's to repair, so it is reported.
    if summed.iter().any(|component| !component.is_finite()) {
        return Err(EmbedderError::NonFiniteVector);
    }

    let norm = summed.iter().map(|c| c * c).sum::<f32>().sqrt();
    // A zero vector has no direction to preserve, and dividing by its norm
    // would replace a well-formed answer with `NaN`.
    if norm > 0.0 {
        for sum in &mut summed {
            *sum /= norm;
        }
    }
    Ok(Embedding::new(summed))
}

/// A `[rows, width]` tensor of token ids, ready to hand to the session.
fn tensor(shape: Vec<i64>, data: Vec<i64>) -> Result<SessionInputValue<'static>, EmbedderError> {
    Tensor::from_array((shape, data))
        .map(SessionInputValue::from)
        .map_err(EmbedderError::Inference)
}

/// A failed session option hands the builder back inside the error, so that a
/// caller can retry with the option dropped. There is nothing to retry here —
/// the option is the reproducibility choice — so the builder is discarded and
/// the status kept.
fn drop_builder(error: ort::Error<ort::session::builder::SessionBuilder>) -> ort::Error {
    ort::Error::new_with_code(error.code(), error.message())
}

fn load_session(path: &Path, intra_threads: NonZeroUsize) -> Result<Session, EmbedderError> {
    let load = |source| EmbedderError::ModelLoad {
        path: path.to_path_buf(),
        source,
    };
    let mut builder = Session::builder()
        .map_err(load)?
        .with_intra_threads(intra_threads.get())
        .map_err(drop_builder)
        .map_err(load)?;
    builder.commit_from_file(path).map_err(load)
}

fn load_tokenizer(
    path: &Path,
    max_sequence_length: NonZeroUsize,
) -> Result<Tokenizer, EmbedderError> {
    let load = |source| EmbedderError::TokenizerLoad {
        path: path.to_path_buf(),
        source,
    };

    let mut tokenizer = Tokenizer::from_file(path).map_err(load)?;

    // Checked here because `tokenizers` does not check it. It subtracts the
    // post-processor's special-token count from `max_length` with unchecked
    // `usize` arithmetic, in `with_truncation` and again per call in
    // `post_process`. A limit equal to that count leaves zero tokens for the
    // text; one below it underflows, which panics in a debug build and wraps
    // to `usize::MAX` in a release one — switching truncation off, the
    // opposite of what lowering the knob asks for. Both are refused here.
    //
    // `added_tokens(false)`: this component encodes single sequences, never
    // pairs, so the count that applies is the single-sequence one.
    let added_tokens = tokenizer
        .get_post_processor()
        .map_or(0, |processor| processor.added_tokens(false));
    if max_sequence_length.get() <= added_tokens {
        return Err(EmbedderError::MaxSequenceLengthTooSmall {
            max_sequence_length: max_sequence_length.get(),
            added_tokens,
        });
    }

    // Truncation is the tokenizer's rather than this crate's, so that it
    // happens in token units and keeps whatever the model's post-processor
    // appends: cutting the id sequence here would drop a trailing `[SEP]` and
    // hand the model a sentence it was not trained to see the end of.
    tokenizer
        .with_truncation(Some(TruncationParams {
            max_length: max_sequence_length.get(),
            strategy: TruncationStrategy::LongestFirst,
            direction: TruncationDirection::Right,
            stride: 0,
        }))
        .map_err(load)?;
    Ok(tokenizer)
}

#[async_trait]
impl Embedder for OnnxEmbedder {
    /// Embeds `texts` under `params.role`, one vector per input, in order.
    ///
    /// Every failure arrives as [`ComponentError::Backend`]: nothing about a
    /// call is a precondition this component can find unmet — a batch of any
    /// size, including none, is a valid request — so what is left is the model
    /// and the tokenizer failing, which is what that variant is for.
    ///
    /// The work does not run on the thread that called this (ADR-C25).
    /// Tokenization, tensor building, the session lock and the forward pass
    /// all happen inside `tokio::task::spawn_blocking`, so the future this
    /// returns yields like any other and a serving driver's concurrency limit
    /// means what it says. The means is this component's choice and has a
    /// consequence the caller inherits: `spawn_blocking` needs an ambient
    /// `tokio` runtime and panics without one. `ARCHITECTURE.md` argues both.
    ///
    /// The role's prefix is prepended before the hop, because that is a string
    /// copy rather than the CPU-bound work the rule is about, and the prefixed
    /// texts then travel into the task rather than being borrowed across it.
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        // An empty batch embeds to no vectors, and does it here: no model is
        // locked, and no thread is borrowed to run nothing on.
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let prefix = self.prefix(params.role);
        let prefixed: Vec<String> = texts.iter().map(|text| format!("{prefix}{text}")).collect();
        let encoder = Arc::clone(&self.inner);
        let batch_size = self.batch_size.get();

        tokio::task::spawn_blocking(move || encoder.embed_all(&prefixed, batch_size))
            .await
            .map_err(|panicked| ComponentError::Backend(Box::new(panicked)))?
            .map_err(|failed| ComponentError::Backend(Box::new(failed)))
    }
}
