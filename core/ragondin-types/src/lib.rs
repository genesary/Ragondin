//! # ragondin-types
//!
//! The platform's core **value types**: `Document`, `Chunk`, `Query`,
//! `Embedding`, `ScoredChunk`, and the identifier newtypes that name them; and
//! the generation-side values `Context`, `ContextChunk`, `Answer` and
//! `ModelIdentity` (ADR-C31 § 1).
//!
//! This crate is a **stable API boundary** (INV-1) and holds **value types
//! only** (INV-3): no global context, no interner, no I/O. A value is fully
//! determined by its content. It carries **no heavy dependency** (INV-4) —
//! `serde` at most.
//!
//! See `ARCHITECTURE.md`.

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
/// Components are expected to emit finite components: JSON encodes `NaN` and
/// the infinities as `null`, which then fails to decode back into an `f32`, so
/// a non-finite value serializes without error and cannot be read back.
///
/// An empty embedding is *representable* and reports a dimensionality of zero.
/// Rejecting it here would require a fallible constructor and an error type in
/// a crate that deliberately has none.
///
/// Representable is not the same as valid. An `Embedder` that *returns* an
/// empty embedding breaks its contract (ADR-C20); `ragondin-contracts` states
/// that on the trait, which is where a component's obligations live. What this
/// type stays silent on is a dimensionality *disagreement* between an embedder
/// and a store, which is caught where it is meaningful — by the vector store
/// being searched.
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

/// One chunk of a [`Context`], named by identifier.
///
/// A context carries its provenance by identifier, never the chunk text: the
/// text is already rendered into [`Context::text`] (ADR-C31 § 1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextChunk {
    /// The chunk placed in the context.
    pub id: ChunkId,
    /// The document that chunk was derived from.
    pub document_id: DocId,
    /// The score the chunk carried in, on the scale of the node that scored
    /// it — not a score of the context's own (ADR-C31 § 1).
    pub score: f32,
}

/// A rendered context, and the chunks it was rendered from.
///
/// It does not hold the query (ADR-C31 § 1): a value that carried one could not
/// say which query it speaks for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Context {
    /// The chunks rendered into [`text`](Self::text), in the order the builder
    /// placed them — which makes them a ranking rather than a set.
    pub chunks: Vec<ContextChunk>,
    /// The rendered context.
    pub text: String,
}

/// A generated answer: text, and nothing else.
///
/// Deliberately so (ADR-C31 § 1). Because no type here is
/// `#[non_exhaustive]`, adding a field later — token usage, say — is a
/// versioned INV-1 break rather than an additive change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    /// The answer text.
    pub text: String,
}

/// The identity of the model behind a component, as the component reports it.
///
/// Opaque: nothing here parses it (ADR-C31 § 1, § 4).
///
/// An empty identity is *representable* and not valid (ADR-C31 § 1) — the
/// shape ADR-C20 gave [`Embedding`]. Rejecting it here would require a
/// fallible constructor and an error type in a crate that deliberately has
/// none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelIdentity(String);

impl ModelIdentity {
    /// Wraps an identity.
    pub fn new(identity: impl Into<String>) -> Self {
        Self(identity.into())
    }

    /// Borrows the underlying identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use std::fmt::Debug;

    /// Every value type must survive a serde round trip unchanged.
    ///
    /// This proves symmetry only. It cannot see the *shape* of the wire form —
    /// a renamed field or a lost `#[serde(transparent)]` round trips happily.
    /// The shape is pinned separately, by the tests that deserialize from a
    /// literal JSON document.
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
        let mut metadata = BTreeMap::new();
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
    fn identifiers_expose_their_inner_value() {
        assert_eq!(DocId::new("doc-1").as_str(), "doc-1");
        assert_eq!(ChunkId::new("chunk-1").as_str(), "chunk-1");
        assert_eq!(QueryId::new("query-1").as_str(), "query-1");
    }

    #[test]
    fn embedding_exposes_its_components() {
        assert_eq!(
            Embedding::new(vec![0.1, 0.2, 0.3]).as_slice(),
            [0.1, 0.2, 0.3]
        );
    }

    /// The newtypes are `transparent`: they encode as bare scalars, not as
    /// wrapper objects. Downstream persists runs as JSON on disk (#28) and the
    /// protobuf mirror (#12) assumes this shape, so it is pinned explicitly —
    /// a symmetric round trip would not notice it changing.
    #[test]
    fn newtypes_encode_as_bare_scalars() {
        let doc_id = serde_json::to_string(&DocId::new("doc-1")).expect("serializes");
        assert_eq!(doc_id, r#""doc-1""#);

        let embedding = serde_json::to_string(&Embedding::new(vec![0.5])).expect("serializes");
        assert_eq!(embedding, "[0.5]");
    }

    /// Field names are part of the wire form: renaming one is a silent breaking
    /// change on a stable boundary (INV-1) that a round trip cannot detect.
    #[test]
    fn chunk_reads_from_its_documented_shape() {
        let json = r#"{"id":"chunk-1","text":"the cat sat on the mat","document_id":"doc-1"}"#;
        let chunk: Chunk = serde_json::from_str(json).expect("field names are the wire form");
        assert_eq!(chunk, a_chunk());
    }

    /// Same, for the nested case the retrieval path actually serializes.
    #[test]
    fn scored_chunk_reads_from_its_documented_shape() {
        let json = r#"{"chunk":{"id":"chunk-1","text":"the cat sat on the mat","document_id":"doc-1"},"score":0.87}"#;
        let scored: ScoredChunk =
            serde_json::from_str(json).expect("field names are the wire form");
        assert_eq!(
            scored,
            ScoredChunk {
                chunk: a_chunk(),
                score: 0.87,
            }
        );
    }

    fn a_context() -> Context {
        Context {
            chunks: vec![
                ContextChunk {
                    id: ChunkId::new("chunk-2"),
                    document_id: DocId::new("doc-1"),
                    score: 0.91,
                },
                ContextChunk {
                    id: ChunkId::new("chunk-1"),
                    document_id: DocId::new("doc-1"),
                    score: 0.87,
                },
            ],
            text: "the cat sat on the mat\n\nthe mat was red".to_string(),
        }
    }

    #[test]
    fn context_chunk_round_trips() {
        assert_round_trips(&ContextChunk {
            id: ChunkId::new("chunk-1"),
            document_id: DocId::new("doc-1"),
            score: 0.87,
        });
    }

    #[test]
    fn context_round_trips() {
        assert_round_trips(&a_context());
    }

    /// `chunks` is positional: the builder's order is what makes it a ranking
    /// rather than a set (ADR-C31 § 1), so the round trip must keep it.
    #[test]
    fn context_keeps_the_order_of_its_chunks() {
        let json = serde_json::to_string(&a_context()).expect("serializes");
        let back: Context = serde_json::from_str(&json).expect("deserializes");
        let ids: Vec<&str> = back.chunks.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["chunk-2", "chunk-1"]);
    }

    #[test]
    fn answer_round_trips() {
        assert_round_trips(&Answer {
            text: "on the mat".to_string(),
        });
    }

    #[test]
    fn model_identity_round_trips() {
        assert_round_trips(&ModelIdentity::new("qwen2.5-7b-instruct@sha256:abc"));
    }

    #[test]
    fn model_identity_exposes_its_inner_value() {
        assert_eq!(ModelIdentity::new("m@rev").as_str(), "m@rev");
    }

    /// Field names are part of the wire form, and ADR-C24 makes the domain
    /// types the source of truth a `.proto` mirrors field for field, so they
    /// are pinned: a renamed one would still round trip.
    #[test]
    fn context_reads_from_its_documented_shape() {
        let json = r#"{"chunks":[{"id":"chunk-2","document_id":"doc-1","score":0.91},{"id":"chunk-1","document_id":"doc-1","score":0.87}],"text":"the cat sat on the mat\n\nthe mat was red"}"#;
        let context: Context = serde_json::from_str(json).expect("field names are the wire form");
        assert_eq!(context, a_context());
    }

    #[test]
    fn answer_reads_from_its_documented_shape() {
        let answer: Answer = serde_json::from_str(r#"{"text":"on the mat"}"#)
            .expect("field names are the wire form");
        assert_eq!(
            answer,
            Answer {
                text: "on the mat".to_string()
            }
        );
    }

    /// `ModelIdentity` is a newtype like the identifiers, and encodes the same
    /// way: as a bare string, not a wrapper object.
    #[test]
    fn model_identity_encodes_as_a_bare_string() {
        let json = serde_json::to_string(&ModelIdentity::new("m@rev")).expect("serializes");
        assert_eq!(json, r#""m@rev""#);
    }

    /// The empty identity is representable and not valid (ADR-C31 § 1): the
    /// crate has no error type, so construction stays infallible.
    #[test]
    fn empty_model_identity_is_representable() {
        let empty = ModelIdentity::new("");
        assert_eq!(empty.as_str(), "");
        assert_round_trips(&empty);
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
