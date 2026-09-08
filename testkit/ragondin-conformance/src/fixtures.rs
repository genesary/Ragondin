//! The inputs the suite applies.
//!
//! They are the suite's own, not the caller's: a conformance function knows
//! nothing about the corpus an implementation was built over, so it may only
//! assert properties that hold whatever that corpus contains.

use ragondin_types::{Chunk, ChunkId, DocId, Query, QueryId, ScoredChunk};

/// The query every family that takes one is exercised with.
pub(crate) fn query() -> Query {
    Query {
        id: QueryId::new("conformance-query"),
        text: "what does this component do with a query".to_string(),
    }
}

pub(crate) fn chunk(id: &str) -> Chunk {
    Chunk {
        id: ChunkId::new(id),
        text: format!("the conformance suite's text for {id}"),
        document_id: DocId::new("conformance-doc"),
    }
}

pub(crate) fn scored(id: &str, score: f32) -> ScoredChunk {
    ScoredChunk {
        chunk: chunk(id),
        score,
    }
}

/// A ranked list of `n` chunks: descending, finite, distinct ids.
pub(crate) fn ranked(prefix: &str, n: usize) -> Vec<ScoredChunk> {
    (0..n)
        .map(|i| scored(&format!("{prefix}-{i}"), 1.0 - i as f32 / 10.0))
        .collect()
}

pub(crate) fn ids(hits: &[ScoredChunk]) -> Vec<&str> {
    hits.iter().map(|hit| hit.chunk.id.as_str()).collect()
}
