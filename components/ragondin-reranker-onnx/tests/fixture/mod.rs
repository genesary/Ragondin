//! A cross-encoder fixture: a model and a tokenizer, built here.
//!
//! Neither is downloaded and neither is committed. The tests need a
//! cross-encoder whose scores they can predict exactly, which no real model
//! offers, and a fixture nobody can read is a fixture nobody can debug — so
//! both are generated from the source in this directory, into
//! `CARGO_TARGET_TMPDIR`, once per test binary.

mod onnx;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Where the generated models and tokenizer landed.
pub struct Fixture {
    /// The ONNX cross-encoder.
    pub model: PathBuf,
    /// The same model with a two-column head: not a cross-encoder, and there to
    /// be refused.
    pub two_headed_model: PathBuf,
    /// The tokenizer both share, in `tokenizers`' own JSON format.
    pub tokenizer: PathBuf,
}

/// The words the fixture tokenizer knows.
///
/// A WordPiece vocabulary is closed: anything outside it becomes `[UNK]`, and
/// every `[UNK]` is the same token id, so out-of-vocabulary text would overlap
/// with itself and blur the scores the tests assert on. Every word the ranking
/// tests use is therefore here — and the conformance suite's synthetic text
/// deliberately is not, because conformance asserts contract behaviour and
/// never quality.
const WORDS: &[&str] = &[
    "how",
    "do",
    "ragondins",
    "build",
    "their",
    "burrows",
    "in",
    "river",
    "banks",
    "the",
    "price",
    "of",
    "tin",
    "fell",
    "sharply",
    "last",
    "quarter",
    "a",
    "sonnet",
    "has",
    "fourteen",
    "lines",
    "and",
    "fixed",
    "rhyme",
];

/// `[PAD]`, `[UNK]`, `[CLS]`, `[SEP]`: ids 0 to 3, and the post-processor below
/// wraps a pair as `[CLS] query [SEP] passage [SEP]` with those ids.
const SPECIALS: &[&str] = &["[PAD]", "[UNK]", "[CLS]", "[SEP]"];

/// The fixture, built on first use and shared by every test in the binary.
pub fn cross_encoder() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(build)
}

fn build() -> Fixture {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cross-encoder-fixture");
    std::fs::create_dir_all(&directory).expect("the fixture directory is writable");

    let model = directory.join("model.onnx");
    std::fs::write(&model, onnx::cross_encoder()).expect("the fixture model is writable");

    let two_headed_model = directory.join("two-headed-model.onnx");
    std::fs::write(&two_headed_model, onnx::two_headed_cross_encoder())
        .expect("the fixture model is writable");

    let tokenizer = directory.join("tokenizer.json");
    std::fs::write(&tokenizer, tokenizer_json()).expect("the fixture tokenizer is writable");

    Fixture {
        model,
        two_headed_model,
        tokenizer,
    }
}

/// The tokenizer, as `tokenizers` serializes one.
///
/// A BERT-style WordPiece tokenizer, which is what an off-the-shelf
/// cross-encoder ships with — the same pair encoding, the same special tokens,
/// the same `token_type_ids` the fixture model reads to tell query from
/// passage. Only the vocabulary is small.
fn tokenizer_json() -> String {
    let vocab: Vec<String> = SPECIALS
        .iter()
        .chain(WORDS)
        .enumerate()
        .map(|(id, word)| format!("    \"{word}\": {id}"))
        .collect();

    format!(
        r###"{{
  "version": "1.0",
  "truncation": null,
  "padding": null,
  "added_tokens": [],
  "normalizer": {{
    "type": "BertNormalizer",
    "clean_text": true,
    "handle_chinese_chars": true,
    "strip_accents": null,
    "lowercase": true
  }},
  "pre_tokenizer": {{ "type": "BertPreTokenizer" }},
  "post_processor": {{
    "type": "BertProcessing",
    "sep": ["[SEP]", 3],
    "cls": ["[CLS]", 2]
  }},
  "decoder": null,
  "model": {{
    "type": "WordPiece",
    "unk_token": "[UNK]",
    "continuing_subword_prefix": "##",
    "max_input_chars_per_word": 100,
    "vocab": {{
{}
    }}
  }}
}}
"###,
        vocab.join(",\n")
    )
}
