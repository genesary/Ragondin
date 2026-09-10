//! The suite every implementation of a contract must pass (ADR-C6), plus the
//! behaviour that is this component's own.
//!
//! The embedder and the store are **fakes defined here**. That is the point of
//! the component's shape: it is built from two trait objects, so testing it
//! needs no ONNX model and no store process, and the fakes make the vector
//! geometry of a case readable off the test rather than off a model.
//!
//! Behind `dense` because the component is: with the feature off the crate
//! exports nothing to test.
#![cfg(feature = "dense")]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ragondin_conformance::assert_retriever_conformance;
use ragondin_contracts::{
    ComponentError, EmbedParams, EmbedRole, EmbeddedChunk, Embedder, RetrieveParams, Retriever,
    SearchParams, VectorStore,
};
use ragondin_retriever_dense::DenseRetriever;
use ragondin_types::{Chunk, ChunkId, DocId, Embedding, Query, QueryId, ScoredChunk};

/// An [`Embedder`] whose whole model is a lookup table, so a test states the
/// query's vector instead of inferring it.
///
/// It records the [`EmbedRole`] of every call: the role is the one fact only
/// the caller knows (ADR-C17), and nothing else in a result would reveal that
/// the retriever passed the wrong one.
#[derive(Default)]
struct FakeEmbedder {
    known: Vec<(String, Vec<f32>)>,
    roles: Mutex<Vec<EmbedRole>>,
}

impl FakeEmbedder {
    fn new(known: &[(&str, [f32; 3])]) -> Self {
        Self {
            known: known
                .iter()
                .map(|(text, vector)| ((*text).to_string(), vector.to_vec()))
                .collect(),
            roles: Mutex::new(Vec::new()),
        }
    }

    fn roles(&self) -> Vec<EmbedRole> {
        self.roles.lock().expect("the fake's lock").clone()
    }
}

/// A handle onto one [`FakeEmbedder`], so a test can read back what the
/// retriever asked of it after handing it over as a `Box<dyn Embedder>`.
#[derive(Clone, Default)]
struct SharedEmbedder(Arc<FakeEmbedder>);

impl SharedEmbedder {
    fn new(known: &[(&str, [f32; 3])]) -> Self {
        Self(Arc::new(FakeEmbedder::new(known)))
    }

    fn roles(&self) -> Vec<EmbedRole> {
        self.0.roles()
    }
}

#[async_trait]
impl Embedder for SharedEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        self.0.embed(texts, params).await
    }
}

#[async_trait]
impl Embedder for FakeEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        self.roles
            .lock()
            .expect("the fake's lock")
            .push(params.role);
        Ok(texts
            .iter()
            .map(|text| {
                let components = self
                    .known
                    .iter()
                    .find(|(known, _)| known == text)
                    // An unknown text still embeds: the conformance suite asks
                    // its own query, and a fake that failed on it would be
                    // testing the fake. The components are distinct so that a
                    // ranking over them is not a tie.
                    .map_or_else(|| vec![3.0, 2.0, 1.0], |(_, vector)| vector.clone());
                Embedding::new(components)
            })
            .collect())
    }
}

/// A [`VectorStore`] that scans what it holds and scores by dot product.
///
/// Exact and deterministic, which is all this test needs of it; how a real
/// store scores is `ragondin-store-memory`'s business and is tested there.
#[derive(Default)]
struct FakeStore {
    entries: Mutex<Vec<EmbeddedChunk>>,
}

#[async_trait]
impl VectorStore for FakeStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        self.entries
            .lock()
            .expect("the fake's lock")
            .extend(entries);
        Ok(())
    }

    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("top_k of zero".into()));
        }
        let mut hits: Vec<ScoredChunk> = self
            .entries
            .lock()
            .expect("the fake's lock")
            .iter()
            .map(|entry| ScoredChunk {
                chunk: entry.chunk.clone(),
                score: dot(embedding, &entry.embedding),
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.chunk.id.as_str().cmp(b.chunk.id.as_str()))
        });
        hits.truncate(params.top_k);
        Ok(hits)
    }
}

fn dot(left: &Embedding, right: &Embedding) -> f32 {
    left.as_slice()
        .iter()
        .zip(right.as_slice())
        .map(|(l, r)| l * r)
        .sum()
}

/// One chunk per axis, so the query's vector names the expected winner.
fn corpus() -> Vec<EmbeddedChunk> {
    vec![
        embedded("space", [1.0, 0.0, 0.0]),
        embedded("ocean", [0.0, 1.0, 0.0]),
        embedded("forest", [0.0, 0.0, 1.0]),
    ]
}

fn embedded(id: &str, vector: [f32; 3]) -> EmbeddedChunk {
    EmbeddedChunk {
        chunk: Chunk {
            id: ChunkId::new(id),
            text: format!("a chunk about the {id}"),
            document_id: DocId::new(id),
        },
        embedding: Embedding::new(vector.to_vec()),
    }
}

fn query(text: &str) -> Query {
    Query {
        id: QueryId::new("q-1"),
        text: text.to_string(),
    }
}

/// A retriever over `corpus()`, whose embedder maps `known` and nothing else.
async fn retriever_over(known: &[(&str, [f32; 3])]) -> DenseRetriever {
    let store = FakeStore::default();
    store.upsert(corpus()).await.expect("the fake must upsert");
    DenseRetriever::new(Box::new(SharedEmbedder::new(known)), Box::new(store))
}

/// A store holding `corpus()` already, for the cases that build one
/// synchronously.
fn full_store() -> FakeStore {
    FakeStore {
        entries: Mutex::new(corpus()),
    }
}

fn ids(hits: &[ScoredChunk]) -> Vec<&str> {
    hits.iter().map(|hit| hit.chunk.id.as_str()).collect()
}

#[tokio::test]
async fn honours_the_retriever_contract() {
    // `assert_retriever_conformance` builds a retriever more than once, and the
    // constructor it takes is synchronous, so the corpus is loaded through a
    // store constructed already full rather than through `upsert`.
    assert_retriever_conformance(|| {
        Box::new(DenseRetriever::new(
            Box::new(FakeEmbedder::default()),
            Box::new(full_store()),
        ))
    })
    .await;
}

#[tokio::test]
async fn ranks_the_vector_nearest_chunk_first() {
    let retriever = retriever_over(&[("anything about the sea", [0.1, 0.9, 0.0])]).await;

    let hits = retriever
        .retrieve(&query("anything about the sea"), &RetrieveParams::new(2))
        .await
        .expect("a well-formed query retrieves");

    // The query vector leans on the `ocean` axis, so `ocean` outranks `space`
    // and the orthogonal `forest` is cut by top_k.
    assert_eq!(ids(&hits), ["ocean", "space"]);
}

#[tokio::test]
async fn embeds_the_query_under_the_query_role() {
    let embedder = SharedEmbedder::default();
    let retriever = DenseRetriever::new(Box::new(embedder.clone()), Box::new(full_store()));

    retriever
        .retrieve(&query("who asks this"), &RetrieveParams::new(1))
        .await
        .expect("a well-formed query retrieves");

    // An asymmetric model prefixes a query differently from a passage
    // (ADR-C17), and passing the wrong role costs points without failing
    // anything — so the role is asserted rather than the result it produced.
    assert_eq!(embedder.roles(), [EmbedRole::Query]);
}

#[tokio::test]
async fn rejects_a_top_k_of_zero_before_embedding() {
    let embedder = SharedEmbedder::default();
    let retriever = DenseRetriever::new(Box::new(embedder.clone()), Box::new(full_store()));

    let error = retriever
        .retrieve(&query("who asks this"), &RetrieveParams::new(0))
        .await
        .expect_err("a top_k of zero is an invalid request");

    assert!(matches!(error, ComponentError::InvalidRequest(_)));
    // The rejection is this component's own, not one inherited from whichever
    // store it was built over: nothing downstream was reached at all.
    assert!(embedder.roles().is_empty());
}

/// An [`Embedder`] that answers one text with `count` vectors, which the
/// contract forbids.
struct MiscountingEmbedder {
    count: usize,
}

#[async_trait]
impl Embedder for MiscountingEmbedder {
    async fn embed(
        &self,
        _texts: &[String],
        _params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        Ok((0..self.count)
            .map(|_| Embedding::new(vec![1.0, 0.0, 0.0]))
            .collect())
    }
}

#[tokio::test]
async fn reports_an_embedder_that_returns_no_vector_as_a_backend_failure() {
    let retriever = DenseRetriever::new(
        Box::new(MiscountingEmbedder { count: 0 }),
        Box::new(full_store()),
    );

    let error = retriever
        .retrieve(&query("who asks this"), &RetrieveParams::new(1))
        .await
        .expect_err("no vector leaves nothing to search with");

    // `Backend`, not `InvalidRequest`: the caller's request was well-formed and
    // the component this one was built over is what broke.
    assert!(matches!(error, ComponentError::Backend(_)), "{error:?}");
    assert_eq!(
        error.to_string(),
        "backend failure: the embedder returned 0 vectors for one query; \
         the contract is one per input"
    );
}

#[tokio::test]
async fn reports_an_embedder_that_returns_several_vectors_as_a_backend_failure() {
    let retriever = DenseRetriever::new(
        Box::new(MiscountingEmbedder { count: 2 }),
        Box::new(full_store()),
    );

    let error = retriever
        .retrieve(&query("who asks this"), &RetrieveParams::new(1))
        .await
        .expect_err("two vectors for one query is not one of them being right");

    // Refused rather than indexed into: which of the two is the query's vector
    // is exactly what a broken `Embedder` has stopped saying.
    assert!(matches!(error, ComponentError::Backend(_)), "{error:?}");
    assert_eq!(
        error.to_string(),
        "backend failure: the embedder returned 2 vectors for one query; \
         the contract is one per input"
    );
}
