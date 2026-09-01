//! # rag-types
//!
//! The platform's core **value types**: `Document`, `Chunk`, `Query`,
//! `Embedding`, `ScoredChunk`, and the identifier newtypes that name them.
//!
//! This crate is a **stable API boundary** (INV-1) and holds **value types
//! only** (INV-3): no global context, no interner, no I/O. A value is fully
//! determined by its content. It carries **no heavy dependency** (INV-4) —
//! `serde` at most.
//!
//! The generation-side types (`Context`, `Generation`) arrive with the
//! milestone that uses them, not before. See `ARCHITECTURE.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Identifies a [`Document`] within a corpus.
///
/// A newtype rather than a bare `String`: it makes every downstream signature
/// say which kind of identifier it takes, and a `DocId` can never be passed
/// where a [`ChunkId`] is meant.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocId(String);

impl DocId {
    /// Wraps an identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrows the underlying identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identifies a [`Chunk`] within a corpus. See [`DocId`] for why it is a newtype.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChunkId(String);

impl ChunkId {
    /// Wraps an identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrows the underlying identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identifies a [`Query`] within a benchmark. See [`DocId`] for why it is a newtype.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QueryId(String);

impl QueryId {
    /// Wraps an identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrows the underlying identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A source document in a corpus, before any chunking.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Identifies this document within its corpus.
    pub id: DocId,
    /// The document's full text.
    pub text: String,
    /// Free-form corpus metadata (title, year, source…).
    ///
    /// A `BTreeMap` rather than a `HashMap` so serialization is ordered and
    /// therefore reproducible; absent metadata deserializes as empty, so a
    /// corpus that carries none needs no field.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// A passage of a [`Document`] — the unit that is retrieved, ranked and scored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// Identifies this chunk within its corpus.
    pub id: ChunkId,
    /// The chunk's text.
    pub text: String,
    /// The document this chunk was derived from.
    pub document_id: DocId,
}

/// A question posed to a pipeline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    /// Identifies this query within its benchmark.
    pub id: QueryId,
    /// The query text.
    pub text: String,
}

/// A dense vector representation of a text.
///
/// An empty embedding is *representable* and reports a dimensionality of zero.
/// Rejecting it would require a fallible constructor and an error type in a
/// crate that deliberately has none; a dimensionality disagreement is caught
/// where it is meaningful — by the vector store being searched.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Embedding(Vec<f32>);

impl Embedding {
    /// Wraps a vector of components.
    pub fn new(components: Vec<f32>) -> Self {
        Self(components)
    }

    /// The number of components — the vector's dimensionality.
    pub fn dim(&self) -> usize {
        self.0.len()
    }

    /// Whether the embedding carries no components at all.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Borrows the components.
    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }
}

/// A [`Chunk`] with the relevance score a retriever, fusion or reranker gave it.
///
/// It carries the whole chunk rather than an identifier because a cross-encoder
/// reranker scores query/passage *text* pairs: an id alone would force a corpus
/// lookup that the component contract does not provide.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScoredChunk {
    /// The chunk that was scored.
    pub chunk: Chunk,
    /// Its relevance score. Higher is more relevant; the scale is the scoring
    /// component's own and is comparable only within one ranked list.
    pub score: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use std::fmt::Debug;

    /// Every value type must survive a serde round trip unchanged: the wire
    /// form is how these cross a process boundary, and a lossy one would make
    /// a `Remote` component silently disagree with a `Local` one.
    fn assert_round_trips<T>(value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let json = serde_json::to_string(value).expect("serialization must succeed");
        let back: T = serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(value, &back, "value must survive a serde_json round trip");
    }

    fn a_chunk() -> Chunk {
        Chunk {
            id: ChunkId::new("chunk-1"),
            text: "the cat sat on the mat".to_string(),
            document_id: DocId::new("doc-1"),
        }
    }

    #[test]
    fn identifiers_round_trip() {
        assert_round_trips(&DocId::new("doc-1"));
        assert_round_trips(&ChunkId::new("chunk-1"));
        assert_round_trips(&QueryId::new("query-1"));
    }

    #[test]
    fn document_round_trips() {
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("title".to_string(), "On Cats".to_string());
        metadata.insert("year".to_string(), "1998".to_string());

        assert_round_trips(&Document {
            id: DocId::new("doc-1"),
            text: "a treatise on cats".to_string(),
            metadata,
        });
    }

    #[test]
    fn document_without_metadata_deserializes() {
        let doc: Document = serde_json::from_str(r#"{"id":"doc-1","text":"a treatise on cats"}"#)
            .expect("metadata is optional");
        assert!(
            doc.metadata.is_empty(),
            "absent metadata must read as empty"
        );
    }

    #[test]
    fn chunk_round_trips() {
        assert_round_trips(&a_chunk());
    }

    #[test]
    fn query_round_trips() {
        assert_round_trips(&Query {
            id: QueryId::new("query-1"),
            text: "where did the cat sit?".to_string(),
        });
    }

    #[test]
    fn embedding_round_trips() {
        assert_round_trips(&Embedding::new(vec![0.1, -0.2, 0.3]));
    }

    #[test]
    fn scored_chunk_round_trips() {
        assert_round_trips(&ScoredChunk {
            chunk: a_chunk(),
            score: 0.87,
        });
    }

    #[test]
    fn embedding_reports_its_dimensionality() {
        assert_eq!(Embedding::new(vec![0.1, 0.2, 0.3, 0.4]).dim(), 4);
    }

    #[test]
    fn empty_embedding_is_representable_with_zero_dimensionality() {
        let empty = Embedding::new(Vec::new());
        assert_eq!(empty.dim(), 0);
        assert!(empty.is_empty());
        assert_round_trips(&empty);
    }
}
