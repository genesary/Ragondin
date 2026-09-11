//! Run identity: the content-addressed tuple that names a run (P4).
//!
//! ```text
//! run_id = hash( pipeline_config, dataset_version, index_version,
//!                model_hashes, engine_version )
//! ```
//!
//! `docs/system-architecture.md` §7.1 states the tuple; `ragondin-experiments`
//! defines the record and says in as many words that the digest is assembled
//! here, because this is the only place that holds all five pieces at once.
//!
//! # The encoding
//!
//! Every digest below is taken over a **tagged, length-prefixed byte stream**,
//! in the shape `core/ragondin-pipeline/src/hash.rs` established for the
//! pipeline's own content hash, and for the same reason: a digest is only as
//! canonical as the bytes fed to it.
//!
//! - Each digest opens with a **domain separator**, so a dataset digest and an
//!   index digest over the same bytes cannot collide.
//! - Every string is written with its byte length first, as a little-endian
//!   `u64`, and every sequence with its element count. Length prefixes are what
//!   make the encoding **injective**: without them a corpus holding one
//!   document `ab` would feed the hasher the same bytes as one holding `a`
//!   followed by `b`.
//! - Nothing is serialized through `serde`. A general-purpose serializer
//!   canonicalizes nothing it was not asked to, and its output is documented as
//!   readable rather than as stable — which is not a property to hang run
//!   identity on.
//!
//! Iteration order is fixed everywhere it is read: a corpus and a query set in
//! the order the adapter loaded them (`Benchmark::new` preserves file order),
//! qrels and model hashes in the key order of the `BTreeMap` that holds them.

use ragondin_benchmarks::Benchmark;
use ragondin_experiments::{RunId, RunInputs};
use ragondin_types::Chunk;
use sha2::{Digest, Sha256};

/// The domain separator of the dataset digest.
const DATASET_DOMAIN: &str = "ragondin/dataset-version/v1";
/// The domain separator of the index digest.
const INDEX_DOMAIN: &str = "ragondin/index-version/v1";
/// The domain separator of the run identity digest.
const RUN_DOMAIN: &str = "ragondin/run-id/v1";

/// A SHA-256 hasher fed a length-prefixed stream.
struct Encoder {
    hasher: Sha256,
}

impl Encoder {
    /// An encoder whose stream opens with `domain`.
    fn new(domain: &str) -> Self {
        let mut encoder = Self {
            hasher: Sha256::new(),
        };
        encoder.field(domain.as_bytes());
        encoder
    }

    /// Writes a byte string, preceded by its length.
    fn field(&mut self, bytes: &[u8]) {
        self.count(bytes.len());
        self.hasher.update(bytes);
    }

    /// Writes a count — of elements, or of bytes — as a little-endian `u64`.
    ///
    /// `u64` and not `usize`: the width of the encoding must not depend on the
    /// machine that computed it, or the same run would have two ids on two
    /// architectures.
    fn count(&mut self, count: usize) {
        self.hasher.update((count as u64).to_le_bytes());
    }

    /// The digest of everything written so far.
    fn finish(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }
}

/// Renders a digest as 64 lowercase hex digits.
///
/// `RunId` renders itself; these two versions are plain strings in
/// [`RunInputs`], so the rendering is here.
fn hex(digest: [u8; 32]) -> String {
    let mut text = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        // Infallible: writing to a `String` cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// The `dataset_version` of a loaded benchmark: a digest over everything a
/// metric can depend on — the corpus, the query set, and every judgment.
///
/// Over the *loaded* benchmark rather than over the files it came from: two
/// snapshots that parse to the same corpus, queries and qrels are the same
/// dataset whatever their bytes or their path, and a version taken over the
/// files would make a re-download a different dataset.
pub(crate) fn dataset_version(benchmark: &Benchmark) -> String {
    let mut encoder = Encoder::new(DATASET_DOMAIN);

    encoder.count(benchmark.corpus().len());
    for document in benchmark.corpus() {
        encoder.field(document.id.as_str().as_bytes());
        encoder.field(document.text.as_bytes());
        encoder.count(document.metadata.len());
        for (key, value) in &document.metadata {
            encoder.field(key.as_bytes());
            encoder.field(value.as_bytes());
        }
    }

    encoder.count(benchmark.queries().len());
    for query in benchmark.queries() {
        encoder.field(query.id.as_str().as_bytes());
        encoder.field(query.text.as_bytes());
    }

    let qrels = benchmark.qrels();
    encoder.count(qrels.judged_query_count());
    for (query, judgments) in qrels.iter() {
        encoder.field(query.as_str().as_bytes());
        encoder.count(judgments.len());
        for (document, grade) in judgments {
            encoder.field(document.as_str().as_bytes());
            // A grade is one byte and writing it raw needs no length.
            encoder.hasher.update([*grade]);
        }
    }

    hex(encoder.finish())
}

/// The `index_version` of what the harness prepared from a corpus.
///
/// Over the chunk set rather than over a backend's on-disk artifact: the chunks
/// are what any index is built from, so two runs whose chunk sets agree
/// retrieve from indexes that agree, whichever backend built them — and an
/// index file's bytes vary with a library version that changed nothing about
/// what is indexed.
pub(crate) fn index_version(chunks: &[Chunk]) -> String {
    let mut encoder = Encoder::new(INDEX_DOMAIN);

    encoder.count(chunks.len());
    for chunk in chunks {
        encoder.field(chunk.id.as_str().as_bytes());
        encoder.field(chunk.text.as_bytes());
        encoder.field(chunk.document_id.as_str().as_bytes());
    }

    hex(encoder.finish())
}

/// The content address of a run: the digest of its whole identity tuple.
pub(crate) fn run_id(inputs: &RunInputs) -> RunId {
    let mut encoder = Encoder::new(RUN_DOMAIN);

    encoder.field(inputs.pipeline.as_bytes());
    encoder.field(inputs.dataset_version.as_bytes());
    encoder.field(inputs.index_version.as_bytes());
    encoder.count(inputs.model_hashes.len());
    for (role, digest) in &inputs.model_hashes {
        encoder.field(role.as_bytes());
        encoder.field(digest.as_bytes());
    }
    encoder.field(inputs.engine_version.as_bytes());

    RunId::from_digest(encoder.finish())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ragondin_benchmarks::Qrels;
    use ragondin_pipeline::PipelineHash;
    use ragondin_types::{ChunkId, DocId, Document, Query, QueryId};

    use super::*;

    fn document(id: &str, text: &str) -> Document {
        Document {
            id: DocId::new(id),
            text: text.to_string(),
            metadata: BTreeMap::new(),
        }
    }

    fn benchmark(corpus: Vec<Document>, grade: u8) -> Benchmark {
        let mut qrels = Qrels::new();
        qrels.insert(QueryId::new("q-1"), DocId::new("d-1"), grade);
        Benchmark::new(
            corpus,
            vec![Query {
                id: QueryId::new("q-1"),
                text: "a question".to_string(),
            }],
            qrels,
        )
    }

    fn inputs() -> RunInputs {
        RunInputs {
            pipeline: PipelineHash::from_digest([7; 32]),
            dataset_version: "dataset".to_string(),
            index_version: "index".to_string(),
            model_hashes: BTreeMap::new(),
            engine_version: "0.0.0".to_string(),
        }
    }

    #[test]
    fn a_dataset_digest_is_64_lowercase_hex_digits() {
        let version = dataset_version(&benchmark(vec![document("d-1", "text")], 1));

        assert_eq!(version.len(), 64);
        assert!(version.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!version.chars().any(|c| c.is_ascii_uppercase()));
    }

    #[test]
    fn the_dataset_digest_follows_the_corpus_the_queries_and_the_judgments() {
        let base = benchmark(vec![document("d-1", "text")], 1);

        assert_eq!(
            dataset_version(&base),
            dataset_version(&benchmark(vec![document("d-1", "text")], 1)),
            "the same dataset, twice, is one version"
        );
        assert_ne!(
            dataset_version(&base),
            dataset_version(&benchmark(vec![document("d-1", "other text")], 1)),
            "a corpus edit is a different dataset"
        );
        assert_ne!(
            dataset_version(&base),
            dataset_version(&benchmark(vec![document("d-1", "text")], 2)),
            "a regraded judgment is a different dataset"
        );
    }

    #[test]
    fn the_dataset_digest_separates_a_split_id_from_the_text_that_follows_it() {
        // What the length prefixes are for: without them these two corpora
        // would feed the hasher the same bytes.
        let left = benchmark(vec![document("d-1", "ab")], 1);
        let right = benchmark(vec![document("d-1a", "b")], 1);

        assert_ne!(dataset_version(&left), dataset_version(&right));
    }

    fn chunk(id: &str, text: &str) -> Chunk {
        Chunk {
            id: ChunkId::new(id),
            text: text.to_string(),
            document_id: DocId::new("d-1"),
        }
    }

    #[test]
    fn the_index_digest_follows_the_chunk_set_and_its_order() {
        let chunks = vec![chunk("c-1", "one"), chunk("c-2", "two")];

        assert_eq!(index_version(&chunks), index_version(&chunks.clone()));
        assert_ne!(
            index_version(&chunks),
            index_version(&[chunk("c-2", "two"), chunk("c-1", "one")]),
            "order is part of the index: a retriever ranks ties by it"
        );
        assert_ne!(
            index_version(&chunks),
            index_version(&chunks[..1]),
            "an index over fewer chunks is a different index"
        );
    }

    #[test]
    fn the_index_digest_and_the_dataset_digest_do_not_share_a_domain() {
        // The domain separators, stated as a test: an empty corpus and an empty
        // chunk set encode to the same length-prefixed nothing otherwise.
        let empty = Benchmark::new(vec![], vec![], Qrels::new());

        assert_ne!(dataset_version(&empty), index_version(&[]));
    }

    #[test]
    fn every_component_of_the_tuple_changes_the_run_id() {
        let base = run_id(&inputs());

        assert_eq!(
            base,
            run_id(&inputs()),
            "identical inputs, identical id (P4)"
        );

        let mut pipeline = inputs();
        pipeline.pipeline = PipelineHash::from_digest([8; 32]);
        assert_ne!(base, run_id(&pipeline));

        let mut dataset = inputs();
        dataset.dataset_version = "other".to_string();
        assert_ne!(base, run_id(&dataset));

        let mut index = inputs();
        index.index_version = "other".to_string();
        assert_ne!(base, run_id(&index));

        let mut models = inputs();
        models
            .model_hashes
            .insert("embedder".to_string(), "sha".to_string());
        assert_ne!(base, run_id(&models));

        let mut engine = inputs();
        engine.engine_version = "0.1.0".to_string();
        assert_ne!(base, run_id(&engine));
    }

    #[test]
    fn a_model_hash_cannot_be_smuggled_across_its_field_boundary() {
        let mut split = inputs();
        split
            .model_hashes
            .insert("embed".to_string(), "der:sha".to_string());
        let mut joined = inputs();
        joined
            .model_hashes
            .insert("embedder".to_string(), "sha".to_string());

        assert_ne!(run_id(&split), run_id(&joined));
    }
}
