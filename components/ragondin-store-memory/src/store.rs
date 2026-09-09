//! Brute-force exact search over vectors held in RAM.

use std::sync::RwLock;

use async_trait::async_trait;
use ragondin_contracts::{ComponentError, EmbeddedChunk, SearchParams, VectorStore};
use ragondin_types::{Embedding, ScoredChunk};

/// An in-memory [`VectorStore`] that answers a search by scanning everything it
/// holds and scoring by **cosine similarity**.
///
/// Exact, so nothing about a result depends on how an index was built, and
/// deterministic down to its ties, which are broken by chunk id ascending. It
/// is the store the reproducible bench runs against; a real backend
/// (`ragondin-store-qdrant`) is the one that proves the trait against a service.
///
/// Scanning is linear in what it holds, which is the deliberate trade: at the
/// corpus sizes M2 measures — BEIR/scifact is some five thousand documents —
/// an exact scan is fast enough, and it removes approximation as a source of
/// run-to-run difference.
#[derive(Debug, Default)]
pub struct MemoryVectorStore {
    /// `upsert` takes `&self` because a `Box<dyn VectorStore>` is shared across
    /// concurrent queries, so the interior mutability is this component's own
    /// to provide, as the contract says. The lock is never held across an
    /// await point: every critical section below is plain arithmetic.
    entries: RwLock<Vec<EmbeddedChunk>>,
}

impl MemoryVectorStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many entries the store holds.
    pub fn len(&self) -> usize {
        self.read().len()
    }

    /// Whether the store holds nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Vec<EmbeddedChunk>> {
        // A poisoned lock means another thread panicked mid-write. Nothing
        // here can leave the vector inconsistent -- the panic would have come
        // from the allocator -- so the contents are still sound to read.
        self.entries.read().unwrap_or_else(|e| e.into_inner())
    }
}

/// The cosine of the angle between two vectors of equal width, or `None` when
/// one of them has no direction to speak of.
fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    let norm = |v: &[f32]| v.iter().map(|c| c * c).sum::<f32>().sqrt();
    let (na, nb) = (norm(a), norm(b));
    if na == 0.0 || nb == 0.0 {
        return None;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    Some(dot / (na * nb))
}

#[async_trait]
impl VectorStore for MemoryVectorStore {
    /// Inserts or replaces `entries`, keyed by chunk id.
    ///
    /// Every entry must have the width the store already holds; a batch that
    /// disagrees is rejected whole, so a caller never has to guess how much of
    /// it landed. Within one batch the last entry for a chunk id wins, which
    /// is what two sequential upserts would leave.
    ///
    /// An empty batch is a no-op rather than an error. The contract leaves that
    /// open (#91) and this is a local choice, not an answer to it.
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        let mut held = self
            .entries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(width) = held.first().map(|entry| entry.embedding.dim()) {
            if let Some(odd) = entries.iter().find(|entry| entry.embedding.dim() != width) {
                return Err(ComponentError::InvalidRequest(format!(
                    "`{}` has {} components, but this store holds vectors of {width}",
                    odd.chunk.id.as_str(),
                    odd.embedding.dim()
                )));
            }
        } else if let Some(odd) = entries
            .iter()
            .find(|entry| entry.embedding.dim() != entries[0].embedding.dim())
        {
            // The store is empty, so the batch itself fixes the width -- and it
            // can only do that if it agrees with itself.
            return Err(ComponentError::InvalidRequest(format!(
                "`{}` has {} components, but this batch opens the store at {}",
                odd.chunk.id.as_str(),
                odd.embedding.dim(),
                entries[0].embedding.dim()
            )));
        }

        for entry in entries {
            match held
                .iter_mut()
                .find(|existing| existing.chunk.id == entry.chunk.id)
            {
                Some(existing) => *existing = entry,
                None => held.push(entry),
            }
        }

        Ok(())
    }

    /// Returns the `top_k` nearest chunks by cosine similarity, sorted by
    /// descending score, ties broken by chunk id ascending.
    ///
    /// Nothing is filtered: a chunk orthogonal to the query comes back scored
    /// zero rather than dropped, because "few results" and "poor results" are
    /// different facts and only the caller knows which one it wants.
    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest(
                "a search for the top 0 chunks returns nothing by construction".into(),
            ));
        }

        let held = self.read();
        let Some(width) = held.first().map(|entry| entry.embedding.dim()) else {
            // Nothing is held, so nothing can disagree with the query -- not
            // even about its width. An empty store answers rather than failing.
            return Ok(Vec::new());
        };
        if embedding.dim() != width {
            return Err(ComponentError::InvalidRequest(format!(
                "the query has {} components, but this store holds vectors of {width}",
                embedding.dim()
            )));
        }
        if cosine(embedding.as_slice(), embedding.as_slice()).is_none() {
            return Err(ComponentError::InvalidRequest(
                "a query vector of zero magnitude has no direction to search along".into(),
            ));
        }

        let mut hits: Vec<ScoredChunk> = held
            .iter()
            .map(|entry| ScoredChunk {
                chunk: entry.chunk.clone(),
                // A stored vector of zero magnitude is scored rather than
                // rejected: it is already indexed, and NaN would break the
                // ranking contract for every other hit in the list.
                score: cosine(embedding.as_slice(), entry.embedding.as_slice()).unwrap_or(0.0),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ragondin_types::{Chunk, ChunkId, DocId};

    fn entry(id: &str, components: Vec<f32>) -> EmbeddedChunk {
        EmbeddedChunk {
            chunk: Chunk {
                id: ChunkId::new(id),
                text: format!("the text of {id}"),
                document_id: DocId::new("doc"),
            },
            embedding: Embedding::new(components),
        }
    }

    fn ids(hits: &[ScoredChunk]) -> Vec<&str> {
        hits.iter().map(|hit| hit.chunk.id.as_str()).collect()
    }

    async fn store_of(entries: Vec<EmbeddedChunk>) -> MemoryVectorStore {
        let store = MemoryVectorStore::new();
        store.upsert(entries).await.expect("well-formed entries");
        store
    }

    #[tokio::test]
    async fn an_empty_store_answers_with_no_results() {
        let store = MemoryVectorStore::new();
        let hits = store
            .search(&Embedding::new(vec![1.0, 0.0]), &SearchParams::new(3))
            .await
            .expect("an empty store answers rather than failing");
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn the_vector_nearest_the_query_comes_first() {
        let store = store_of(vec![
            entry("far", vec![0.0, 1.0]),
            entry("near", vec![1.0, 0.1]),
            entry("middling", vec![1.0, 1.0]),
        ])
        .await;

        let hits = store
            .search(&Embedding::new(vec![1.0, 0.0]), &SearchParams::new(3))
            .await
            .expect("a well-formed search succeeds");

        assert_eq!(ids(&hits), ["near", "middling", "far"]);
    }

    #[tokio::test]
    async fn magnitude_does_not_outrank_direction() {
        // The failure a dot-product store has and a cosine store does not: a
        // long vector pointing elsewhere beating a short one pointing at the
        // query. A BEIR corpus has both.
        let store = store_of(vec![
            entry("long-but-orthogonal", vec![0.0, 900.0]),
            entry("short-and-aligned", vec![0.01, 0.0]),
        ])
        .await;

        let hits = store
            .search(&Embedding::new(vec![1.0, 0.0]), &SearchParams::new(1))
            .await
            .expect("a well-formed search succeeds");

        assert_eq!(ids(&hits), ["short-and-aligned"]);
    }

    #[tokio::test]
    async fn top_k_caps_the_result() {
        let store = store_of(vec![
            entry("a", vec![1.0, 0.0]),
            entry("b", vec![0.9, 0.1]),
            entry("c", vec![0.8, 0.2]),
        ])
        .await;

        let hits = store
            .search(&Embedding::new(vec![1.0, 0.0]), &SearchParams::new(2))
            .await
            .expect("a well-formed search succeeds");

        assert_eq!(hits.len(), 2);
    }

    #[tokio::test]
    async fn equal_scores_are_ordered_by_chunk_id_and_not_by_insertion() {
        // Ties are the common case here, not a rarity: every vector orthogonal
        // to the query scores exactly zero. A run that reorders them between
        // executions is not reproducible, which is what M2 is for.
        let aligned = vec![1.0, 0.0];
        let tied = |id: &str| entry(id, vec![0.0, 1.0]);

        let one = store_of(vec![
            tied("c"),
            tied("a"),
            entry("hit", aligned.clone()),
            tied("b"),
        ])
        .await;
        let other = store_of(vec![
            tied("b"),
            tied("c"),
            tied("a"),
            entry("hit", aligned.clone()),
        ])
        .await;

        let query = Embedding::new(aligned);
        let hits_one = one
            .search(&query, &SearchParams::new(4))
            .await
            .expect("a well-formed search succeeds");
        let hits_other = other
            .search(&query, &SearchParams::new(4))
            .await
            .expect("a well-formed search succeeds");

        assert_eq!(ids(&hits_one), ["hit", "a", "b", "c"]);
        assert_eq!(ids(&hits_one), ids(&hits_other));
    }

    #[tokio::test]
    async fn the_same_search_repeated_returns_the_same_thing() {
        let store = store_of(vec![
            entry("a", vec![1.0, 0.0]),
            entry("b", vec![0.0, 1.0]),
            entry("c", vec![0.5, 0.5]),
        ])
        .await;

        let query = Embedding::new(vec![1.0, 0.0]);
        let first = store
            .search(&query, &SearchParams::new(3))
            .await
            .expect("a well-formed search succeeds");
        let second = store
            .search(&query, &SearchParams::new(3))
            .await
            .expect("a well-formed search succeeds");

        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn upsert_replaces_an_entry_of_the_same_chunk_id() {
        let store = store_of(vec![entry("c1", vec![1.0, 0.0])]).await;
        let revised = EmbeddedChunk {
            chunk: Chunk {
                text: "the same chunk, re-indexed".to_string(),
                ..entry("c1", vec![0.0, 1.0]).chunk
            },
            embedding: Embedding::new(vec![0.0, 1.0]),
        };
        store.upsert(vec![revised]).await.expect("a valid upsert");

        assert_eq!(store.len(), 1, "a re-indexed chunk is not a second entry");
        let hits = store
            .search(&Embedding::new(vec![0.0, 1.0]), &SearchParams::new(2))
            .await
            .expect("a well-formed search succeeds");
        assert_eq!(ids(&hits), ["c1"]);
        assert_eq!(hits[0].chunk.text, "the same chunk, re-indexed");
    }

    #[tokio::test]
    async fn the_last_entry_of_a_batch_wins_within_that_batch() {
        // One batch may carry a chunk id twice; the store must still hold one
        // entry, and it must be the later one, exactly as two sequential
        // upserts would leave it.
        let store = store_of(vec![
            entry("c1", vec![1.0, 0.0]),
            entry("c1", vec![0.0, 1.0]),
        ])
        .await;

        assert_eq!(store.len(), 1);
        let hits = store
            .search(&Embedding::new(vec![0.0, 1.0]), &SearchParams::new(1))
            .await
            .expect("a well-formed search succeeds");
        assert_eq!(hits[0].score, 1.0);
    }

    #[tokio::test]
    async fn a_vector_of_another_width_is_an_invalid_request() {
        let store = store_of(vec![entry("c1", vec![1.0, 0.0])]).await;

        let upsert = store
            .upsert(vec![entry("c2", vec![1.0, 0.0, 0.0])])
            .await
            .expect_err("a store holds one width");
        assert!(matches!(upsert, ComponentError::InvalidRequest(_)));

        let search = store
            .search(&Embedding::new(vec![1.0]), &SearchParams::new(1))
            .await
            .expect_err("a query of another width cannot be compared");
        assert!(matches!(search, ComponentError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn a_query_with_no_direction_is_an_invalid_request() {
        // Cosine is undefined against the zero vector, and answering it with a
        // NaN score would break the ranking contract silently.
        let store = store_of(vec![entry("c1", vec![1.0, 0.0])]).await;

        let error = store
            .search(&Embedding::new(vec![0.0, 0.0]), &SearchParams::new(1))
            .await
            .expect_err("the zero vector points nowhere");
        assert!(matches!(error, ComponentError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn a_stored_vector_with_no_direction_scores_zero_rather_than_nan() {
        // The mirror of the case above, answered the other way: a query the
        // caller controls is rejected, but a corpus vector is already indexed,
        // and failing every later search over it would be worse than scoring
        // it last.
        let store = store_of(vec![
            entry("nowhere", vec![0.0, 0.0]),
            entry("somewhere", vec![1.0, 0.0]),
        ])
        .await;

        let hits = store
            .search(&Embedding::new(vec![1.0, 0.0]), &SearchParams::new(2))
            .await
            .expect("a well-formed search succeeds");

        assert_eq!(ids(&hits), ["somewhere", "nowhere"]);
        assert!(hits.iter().all(|hit| hit.score.is_finite()));
        assert_eq!(hits[1].score, 0.0);
    }

    #[tokio::test]
    async fn a_zero_top_k_is_an_invalid_request() {
        let store = store_of(vec![entry("c1", vec![1.0, 0.0])]).await;

        let error = store
            .search(&Embedding::new(vec![1.0, 0.0]), &SearchParams::new(0))
            .await
            .expect_err("a search for nothing is a mistake, not an empty answer");
        assert!(matches!(error, ComponentError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn an_empty_upsert_leaves_the_store_alone() {
        let store = store_of(vec![entry("c1", vec![1.0, 0.0])]).await;
        store.upsert(Vec::new()).await.expect("a no-op upsert");
        assert_eq!(store.len(), 1);
    }
}
