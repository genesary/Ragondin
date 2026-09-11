//! The corpus preparation a run needs before it can retrieve anything —
//! **ad hoc, and deliberately not a pipeline**.
//!
//! Question 5 of `docs/OPEN_QUESTIONS.md` — *does indexing share the IR
//! formalism?* — is **deliberately unresolved**, and nothing here resolves it:
//! the corpus is prepared by this module, directly, and no indexing graph, node
//! kind or `ValueKind` is invented to express it. When that question is
//! settled, this module is what moves.
//!
//! # What it prepares, and what it does not
//!
//! It turns the benchmark's documents into the **chunk set** a run retrieves
//! over, and content-addresses it as the `index_version` of the identity tuple
//! (§7.1). It builds **no backend index**: a BM25 index or a populated vector
//! store is a *component's* state, and a component reaches the harness only as
//! a constructor on an `EngineContext` — `Bm25Retriever::new` takes the chunks
//! it will search and builds its index there, and a `VectorStore` a run would
//! upsert into is not reachable from a context at all. So the backend index is
//! built where the components are constructed, which is the composition root,
//! from exactly [`CorpusIndex::chunks`] — the set [`CorpusIndex::version`]
//! names.
//!
//! # One chunk per document
//!
//! Chunking is the `Chunker` family's job and no chunker exists yet, so a
//! document becomes one chunk carrying its whole text. This is the M2
//! placement: BEIR is a document-ranking benchmark, its documents are short,
//! and its published figures are computed over whole documents — so a splitting
//! strategy invented here would move the figures the M2 leaderboard milestone
//! has to reproduce. A real chunker arrives as a component, and it arrives in
//! the pipeline, not here.

use ragondin_types::{Chunk, ChunkId, Document};

use crate::identity::index_version;

/// The chunk set a run retrieves over, and the version that names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusIndex {
    chunks: Vec<Chunk>,
    version: String,
}

impl CorpusIndex {
    /// Prepares `corpus` for retrieval: one chunk per document, in corpus
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
    pub fn version(&self) -> &str {
        &self.version
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ragondin_types::DocId;

    use super::*;

    fn document(id: &str, text: &str) -> Document {
        Document {
            id: DocId::new(id),
            text: text.to_string(),
            metadata: BTreeMap::from([("title".to_string(), "a title".to_string())]),
        }
    }

    #[test]
    fn a_document_becomes_one_chunk_carrying_its_whole_text() {
        let index = CorpusIndex::build(&[document("d-1", "the cat sat"), document("d-2", "a dog")]);

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
        let corpus = [document("d-1", "the cat sat")];
        let index = CorpusIndex::build(&corpus);

        assert_eq!(
            index.version(),
            index_version(index.chunks()),
            "the version names these chunks and nothing else"
        );
        assert_eq!(index.version(), CorpusIndex::build(&corpus).version());
        assert_ne!(
            index.version(),
            CorpusIndex::build(&[document("d-1", "the dog barked")]).version(),
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
