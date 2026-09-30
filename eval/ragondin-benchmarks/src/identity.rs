//! The identity of a dataset and of the chunk set derived from it: the
//! `dataset_version` and `index_version` components of run identity (§7.1),
//! and the one-chunk-per-document derivation the second is taken over.
//!
//! ```text
//! run_id = hash( pipeline_config, dataset_version, index_version,
//!                model_hashes, engine_version )
//! ```
//!
//! # Why they live here
//!
//! A stored run is replayed with its passages' text only when the dataset on
//! disk digests to the run's `dataset_version` and the chunk set derived from
//! it digests to the run's `index_version` (ADR-C36 § 4). The writer of those
//! two values is `ragondin-harness`; the reader is the experiment plane's API,
//! which may reach no engine (ADR-C36 § 3) and therefore not the harness. The
//! ADR allows **one definition** of each digest and of the chunk derivation,
//! used by both, so it sits in the crate both can reach: this one, which
//! already holds the [`Benchmark`] the dataset digest is taken over and reaches
//! no engine and no component. `run_id` itself stays in the harness, which is
//! the only place that holds all five pieces; it digests through the same
//! [`Encoder`], exported for that.
//!
//! # The encoding
//!
//! Every digest is taken over a **tagged, length-prefixed byte stream**, in the
//! shape `core/ragondin-pipeline/src/hash.rs` established for the pipeline's
//! own content hash, and for the same reason: a digest is only as canonical as
//! the bytes fed to it.
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
//! qrels in the key order of the `BTreeMap` that holds them, reference answers
//! in the order of the query set.
//!
//! **Changing any byte of this encoding changes the id of every stored run**
//! (INV-8). `tests/identity_golden.rs` pins the digests of this crate's
//! fixtures as literals for that reason.

use ragondin_types::{Chunk, ChunkId, Document, QueryId};
use sha2::{Digest, Sha256};

use crate::{Benchmark, CarriedPieces};

/// The domain separator of the dataset digest.
const DATASET_DOMAIN: &str = "ragondin/dataset-version/v1";
/// The tag opening the reference-answers section of the dataset digest.
const REFERENCES_TAG: &str = "references";
/// The domain separator of the index digest.
const INDEX_DOMAIN: &str = "ragondin/index-version/v1";

/// A SHA-256 hasher fed a domain-separated, length-prefixed stream — the
/// encoding every identity digest is taken over.
///
/// Public because `ragondin-harness` takes `run_id` over the same encoding,
/// under its own domain; one encoder keeps the three digests from drifting
/// apart in how a string or a count is written.
pub struct Encoder {
    hasher: Sha256,
}

impl Encoder {
    /// An encoder whose stream opens with `domain`.
    pub fn new(domain: &str) -> Self {
        let mut encoder = Self {
            hasher: Sha256::new(),
        };
        encoder.field(domain.as_bytes());
        encoder
    }

    /// Writes a byte string, preceded by its length.
    pub fn field(&mut self, bytes: &[u8]) {
        self.count(bytes.len());
        self.hasher.update(bytes);
    }

    /// Writes a count — of elements, or of bytes — as a little-endian `u64`.
    ///
    /// `u64` and not `usize`: the width of the encoding must not depend on the
    /// machine that computed it, or the same run would have two ids on two
    /// architectures.
    pub fn count(&mut self, count: usize) {
        self.hasher.update((count as u64).to_le_bytes());
    }

    /// The digest of everything written so far.
    pub fn finish(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }
}

/// Renders a digest as 64 lowercase hex digits: the two versions are plain
/// strings in a run's inputs.
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
/// metric can depend on — the corpus, the query set, every judgment, and the
/// reference answers when the benchmark carries any.
///
/// The reference answers follow the qrels as a section of their own, opened
/// by the tag `references` and present only when the benchmark carries them
/// (ADR-C30 § 5): the query count, then, for each query in benchmark order
/// that has a reference, its id, its reference count and its references in
/// the order the benchmark holds them. A benchmark that carries none digests
/// exactly as it did before reference answers existed.
///
/// Over the *loaded* benchmark rather than over the files it came from: two
/// snapshots that parse to the same corpus, queries and qrels are the same
/// dataset whatever their bytes or their path, and a version taken over the
/// files would make a re-download a different dataset.
pub fn dataset_version(benchmark: &Benchmark) -> String {
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

    // Conditional, so tagged (ADR-C30 § 5): a benchmark that carries no
    // reference answer ends its stream here, byte for byte as it did before
    // reference answers existed, and every digest already recorded for one
    // stays valid. The tag is the only in-stream section marker in this
    // encoding, because this is the only section that may be absent.
    if matches!(
        benchmark.carries(),
        CarriedPieces::ReferenceAnswersOnly | CarriedPieces::QrelsAndReferenceAnswers
    ) {
        let answered: Vec<(&QueryId, &[String])> = benchmark
            .queries()
            .iter()
            .filter_map(|query| {
                benchmark
                    .reference_answers()
                    .for_query(&query.id)
                    .map(|references| (&query.id, references))
            })
            .collect();
        encoder.field(REFERENCES_TAG.as_bytes());
        encoder.count(answered.len());
        for (query, references) in answered {
            encoder.field(query.as_str().as_bytes());
            encoder.count(references.len());
            for reference in references {
                encoder.field(reference.as_bytes());
            }
        }
    }

    hex(encoder.finish())
}

/// The `index_version` of a chunk set.
///
/// Over the chunk set rather than over a backend's on-disk artifact: the chunks
/// are what any index is built from, so two runs whose chunk sets agree
/// retrieve from indexes that agree, whichever backend built them — and an
/// index file's bytes vary with a library version that changed nothing about
/// what is indexed.
pub fn index_version(chunks: &[Chunk]) -> String {
    let mut encoder = Encoder::new(INDEX_DOMAIN);

    encoder.count(chunks.len());
    for chunk in chunks {
        encoder.field(chunk.id.as_str().as_bytes());
        encoder.field(chunk.text.as_bytes());
        encoder.field(chunk.document_id.as_str().as_bytes());
    }

    hex(encoder.finish())
}

/// The chunk set a run retrieves over, derived from a corpus, and the
/// `index_version` that names it.
///
/// **Ad hoc, and deliberately not a pipeline.** Question 5 of
/// `docs/OPEN_QUESTIONS.md` — *does indexing share the IR formalism?* — is
/// deliberately unresolved, and nothing here resolves it: no indexing graph,
/// node kind or `ValueKind` is invented to express the derivation. When that
/// question is settled, this type is what moves.
///
/// # One chunk per document
///
/// Chunking is the `Chunker` family's job and no chunker exists yet, so a
/// document becomes one chunk carrying its whole text, under the document's
/// own id. This is the M2 placement: BEIR is a document-ranking benchmark, its
/// documents are short, and its published figures are computed over whole
/// documents — so a splitting strategy invented here would move the figures
/// the M2 leaderboard milestone has to reproduce. A real chunker arrives as a
/// component, and it arrives in the pipeline, not here — and on that day a
/// reader can no longer derive a chunk's text without executing it (ADR-C36
/// § 4).
///
/// # Who builds it
///
/// Two callers, one derivation (ADR-C36 § 4). The composition root builds it
/// before a run, constructs every component from [`chunks`](Self::chunks)
/// and hands the same value to the harness, which records its
/// [`version`](Self::version) (ADR-C26: corpus ingestion is the composition
/// root's job). A reader replaying a stored run rebuilds it from the dataset
/// on disk and compares versions before it shows any passage's text. It builds
/// **no backend index**: a BM25 index or a populated vector store is a
/// component's state, built where components are constructed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusIndex {
    chunks: Vec<Chunk>,
    version: String,
}

impl CorpusIndex {
    /// Derives the chunk set of `corpus`: one chunk per document, in corpus
    /// order.
    ///
    /// Order is preserved because identity depends on it — `Benchmark` hands
    /// the corpus out in file order for exactly this reason.
    pub fn build(corpus: &[Document]) -> Self {
        let chunks: Vec<Chunk> = corpus
            .iter()
            .map(|document| Chunk {
                id: ChunkId::new(document.id.as_str()),
                text: document.text.clone(),
                document_id: document.id.clone(),
            })
            .collect();
        let version = index_version(&chunks);

        Self { chunks, version }
    }

    /// The chunks, in corpus order.
    ///
    /// Public because the composition root builds the backing index from them:
    /// a retriever constructed over any other set would be answering from an
    /// index this run's `index_version` does not name.
    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    /// The `index_version` component of run identity: the content address of
    /// [`chunks`](Self::chunks).
    ///
    /// **It addresses *a* chunk set, not provably the one that was retrieved
    /// from.** The components a run executes are constructed before the harness
    /// sees them, over a set only the composition root knows, and no contract
    /// lets a driver ask a retriever what it indexed — so a caller that
    /// registers a retriever over one corpus and evaluates against another
    /// records a version that names neither. Reading this field as evidence of
    /// what was searched is therefore only as sound as the composition root
    /// that built both: it must construct its components from these chunks.
    ///
    /// That obligation is where it stays: **corpus ingestion is the
    /// composition root's job** (ADR-C26), which is the one place that holds
    /// both the engine and the concrete components. A driver was deliberately
    /// not given a way to reach a constructed component to close the gap
    /// itself.
    pub fn version(&self) -> &str {
        &self.version
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ragondin_types::{DocId, Query};

    use super::*;
    use crate::{Qrels, ReferenceAnswers};

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

    /// The digest [`dataset_version`] gave `benchmark(vec![document("d-1",
    /// "text")], 1)` before reference answers existed. Pinned rather than
    /// recomputed: ADR-C30 § 5 requires a benchmark that carries no reference
    /// answer to digest byte for byte as it did, and only a recorded value can
    /// tell a changed encoding from an unchanged one.
    const REFERENCE_FREE_DIGEST: &str =
        "7e3efcd5b197ef8f8abeeea1902efc82a438a0f0362c88092a973bec1a3be6e3";

    fn with_references(base: Benchmark, answers: &[(&str, &[&str])]) -> Benchmark {
        let mut references = ReferenceAnswers::new();
        for (query, strings) in answers {
            references.insert(
                QueryId::new(*query),
                strings.iter().map(|s| (*s).to_string()).collect(),
            );
        }
        base.with_reference_answers(references)
    }

    #[test]
    fn a_benchmark_that_carries_no_reference_answer_digests_as_it_always_did() {
        let base = benchmark(vec![document("d-1", "text")], 1);

        assert_eq!(dataset_version(&base), REFERENCE_FREE_DIGEST);
        assert_eq!(
            dataset_version(&with_references(base.clone(), &[])),
            REFERENCE_FREE_DIGEST,
            "an empty reference-answers value carries nothing, so adds nothing"
        );
        assert_eq!(
            dataset_version(&with_references(base, &[("q-unknown", &["yes"])])),
            REFERENCE_FREE_DIGEST,
            "a reference to a query the benchmark does not hold is not carried"
        );
    }

    #[test]
    fn reference_answers_the_benchmark_carries_enter_the_digest() {
        let base = benchmark(vec![document("d-1", "text")], 1);
        let answered = with_references(base.clone(), &[("q-1", &["yes", "no"])]);

        assert_ne!(
            dataset_version(&answered),
            dataset_version(&base),
            "two benchmarks differing only in their references are two datasets"
        );
        assert_eq!(
            dataset_version(&answered),
            dataset_version(&with_references(base.clone(), &[("q-1", &["yes", "no"])])),
        );
        assert_ne!(
            dataset_version(&answered),
            dataset_version(&with_references(base.clone(), &[("q-1", &["no", "yes"])])),
            "references are digested in the order the benchmark holds them"
        );
        assert_ne!(
            dataset_version(&with_references(base.clone(), &[("q-1", &["ab"])])),
            dataset_version(&with_references(base, &[("q-1", &["a", "b"])])),
            "each reference is length-prefixed, so a split is not a join"
        );
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

    fn titled(id: &str, text: &str) -> Document {
        Document {
            id: DocId::new(id),
            text: text.to_string(),
            metadata: BTreeMap::from([("title".to_string(), "a title".to_string())]),
        }
    }

    #[test]
    fn a_document_becomes_one_chunk_carrying_its_whole_text() {
        let index = CorpusIndex::build(&[titled("d-1", "the cat sat"), titled("d-2", "a dog")]);

        assert_eq!(
            index
                .chunks()
                .iter()
                .map(|chunk| (
                    chunk.id.as_str(),
                    chunk.text.as_str(),
                    chunk.document_id.as_str()
                ))
                .collect::<Vec<_>>(),
            [("d-1", "the cat sat", "d-1"), ("d-2", "a dog", "d-2")],
            "one chunk per document, in corpus order, attributed to its document"
        );
    }

    #[test]
    fn the_version_addresses_the_chunk_set_it_prepared() {
        let corpus = [titled("d-1", "the cat sat")];
        let index = CorpusIndex::build(&corpus);

        assert_eq!(
            index.version(),
            index_version(index.chunks()),
            "the version names these chunks and nothing else"
        );
        assert_eq!(index.version(), CorpusIndex::build(&corpus).version());
        assert_ne!(
            index.version(),
            CorpusIndex::build(&[titled("d-1", "the dog barked")]).version(),
            "a corpus edit is a different index"
        );
    }

    #[test]
    fn an_empty_corpus_prepares_an_empty_index_rather_than_failing() {
        // A corpus is not required to be non-empty here: a benchmark whose
        // pipeline fabricates its answers has nothing to index, and refusing it
        // would make the stub path a special case.
        let index = CorpusIndex::build(&[]);

        assert!(index.chunks().is_empty());
        assert_eq!(index.version().len(), 64);
    }
}
