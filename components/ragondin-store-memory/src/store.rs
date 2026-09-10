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
        // A poisoned lock means another thread panicked mid-write. The write
        // section below inserts and replaces whole entries, so what a reader
        // then sees is some prefix of a batch rather than a torn value, and
        // refusing to read it would be a worse answer than serving it.
        self.entries.read().unwrap_or_else(|e| e.into_inner())
    }
}

/// The cosine of the angle between two vectors of equal width, or `None` when
/// one of them has no direction to speak of — zero magnitude, or a component
/// that is not a finite number.
///
/// The arithmetic is `f64` although the components are `f32`, because squaring
/// is where an ordinary embedding leaves the range: `1e30` is a perfectly good
/// `f32` and `1e30 * 1e30` is not, so an `f32` accumulator turns a vector's
/// similarity to *itself* into `inf / inf`, which is `NaN`. Every `f32` is
/// exactly representable in `f64` and no sum of squares of `f32`s can leave
/// its range, so the only way a norm here is not finite is that an input
/// component was not — which is the case the guard below answers.
fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    let norm = |v: &[f32]| {
        v.iter()
            .map(|c| f64::from(*c) * f64::from(*c))
            .sum::<f64>()
            .sqrt()
    };
    let directed = |n: f64| n.is_finite() && n != 0.0;
    let (na, nb) = (norm(a), norm(b));
    if !directed(na) || !directed(nb) {
        return None;
    }
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum();
    Some((dot / (na * nb)) as f32)
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
    ///
    /// An entry whose *embedding* is empty is refused. That is also a local
    /// choice about this store and not an answer to #90, which asks what an
    /// `Embedder` may legitimately return.
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        // Checked before the held width, because a width-zero opening batch is
        // otherwise accepted and fixes the store at a width no query and no
        // later vector can match: every search over it is then an
        // `InvalidRequest` and the store can never be corrected.
        if let Some(odd) = entries.iter().find(|entry| entry.embedding.dim() == 0) {
            return Err(ComponentError::InvalidRequest(format!(
                "`{}` has no components, and a store of vectors of width zero can answer no search",
                odd.chunk.id.as_str()
            )));
        }

        let mut held = self
            .entries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Read once, before the branch. Indexing `entries[0]` inside the
        // predicate below was safe only because `find` never calls it on an
        // empty iterator -- and an empty batch is a supported input, documented
        // as a no-op just above. A refactor that resolved the reference width
        // eagerly would have panicked on `upsert(vec![])`, so the safety is
        // taken out of the reader's hands rather than left to that detail.
        let opening = entries.first().map(|entry| entry.embedding.dim());

        if let Some(width) = held.first().map(|entry| entry.embedding.dim()) {
            if let Some(odd) = entries.iter().find(|entry| entry.embedding.dim() != width) {
                return Err(ComponentError::InvalidRequest(format!(
                    "`{}` has {} components, but this store holds vectors of {width}",
                    odd.chunk.id.as_str(),
                    odd.embedding.dim()
                )));
            }
        } else if let Some(width) = opening {
            // The store is empty, so the batch itself fixes the width -- and it
            // can only do that if it agrees with itself.
            if let Some(odd) = entries.iter().find(|entry| entry.embedding.dim() != width) {
                return Err(ComponentError::InvalidRequest(format!(
                    "`{}` has {} components, but this batch opens the store at {width}",
                    odd.chunk.id.as_str(),
                    odd.embedding.dim()
                )));
            }
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

        // Before the empty-store return below, because nothing about this
        // check depends on what is held: a query with no direction has none
        // whether the store holds a million vectors or none at all. Ordering it
        // after gave the same call two verdicts — `Ok([])` on a fresh store,
        // `InvalidRequest` once one vector was in — so a caller smoke-testing
        // its embedder saw no error for a vector the store would later refuse.
        if cosine(embedding.as_slice(), embedding.as_slice()).is_none() {
            return Err(ComponentError::InvalidRequest(
                "a query vector of zero magnitude, or with a component that is not a finite \
                 number, has no direction to search along"
                    .into(),
            ));
        }

        let held = self.read();
        let Some(width) = held.first().map(|entry| entry.embedding.dim()) else {
            // Nothing is held, so nothing can disagree with the query about its
            // width -- that check, unlike the one above, genuinely needs a
            // stored vector to compare against. An empty store answers rather
            // than failing.
            return Ok(Vec::new());
        };
        if embedding.dim() != width {
            return Err(ComponentError::InvalidRequest(format!(
                "the query has {} components, but this store holds vectors of {width}",
                embedding.dim()
            )));
        }

        // Scored by position first, and only the survivors are cloned. Building
        // a `ScoredChunk` per entry up front means allocating a `String` for
        // every chunk in the corpus and dropping all but `top_k` of them a few
        // lines later: at BEIR chunk sizes that copying is the larger half of a
        // search, and it buys nothing the scan needs.
        let mut ranked: Vec<(usize, f32)> = held
            .iter()
            .enumerate()
            .map(|(position, entry)| {
                // A stored vector with no direction -- zero magnitude, or a
                // component that is not finite -- is scored rather than
                // rejected: it is already indexed, and NaN would break the
                // ranking contract for every other hit in the list.
                //
                // `+ 0.0` normalizes a negative zero away. `total_cmp` is a
                // total order over *bit patterns* and separates `-0.0` from
                // `+0.0`, which `==` calls equal -- so without this, two chunks
                // that scored identically were ordered by the sign of a zero
                // rather than by the chunk id this crate promises to break ties
                // with.
                let score =
                    cosine(embedding.as_slice(), entry.embedding.as_slice()).unwrap_or(0.0) + 0.0;
                (position, score)
            })
            .collect();

        ranked.sort_by(|(a_position, a_score), (b_position, b_score)| {
            b_score.total_cmp(a_score).then_with(|| {
                held[*a_position]
                    .chunk
                    .id
                    .as_str()
                    .cmp(held[*b_position].chunk.id.as_str())
            })
        });
        ranked.truncate(params.top_k);

        Ok(ranked
            .into_iter()
            .map(|(position, score)| ScoredChunk {
                chunk: held[position].chunk.clone(),
                score,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

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

    /// Two scores that compare equal must be ordered by chunk id, whatever
    /// their bit pattern.
    ///
    /// `total_cmp` is a *total* order over bit patterns, and it separates
    /// `-0.0` from `+0.0` — which `==` calls equal. So two chunks that scored
    /// identically were ordered by the sign of a zero rather than by their id,
    /// silently, in the one component whose whole justification is that a run
    /// is reproducible.
    ///
    /// `-0.0` is not contrived here: it is what cosine returns for a stored
    /// vector pointing exactly away from a query with a signed zero in it.
    #[tokio::test]
    async fn a_negative_zero_score_does_not_outrank_a_positive_zero() {
        let store = store_of(vec![
            entry("zzz", vec![1.0, 1.0, 0.0]),
            entry("aaa", vec![-1.0, -1.0, -0.0]),
        ])
        .await;

        let hits = store
            .search(&Embedding::new(vec![0.0, 0.0, 1.0]), &SearchParams::new(2))
            .await
            .expect("the query has a direction");

        assert_eq!(hits[0].score, hits[1].score, "the two scores compare equal");
        assert_eq!(
            ids(&hits),
            vec!["aaa", "zzz"],
            "equal scores order by chunk id, not by the sign of a zero"
        );
    }

    /// A query with no direction is rejected whether or not anything is held.
    ///
    /// The empty-store early return used to come first, so the same call got
    /// two verdicts depending on unrelated state: `Ok([])` against a fresh
    /// store, `InvalidRequest` once one vector was in. A caller smoke-testing
    /// its embedder against an empty store saw no error for a vector the store
    /// would later refuse. The width check legitimately depends on what is
    /// held; this one depends on nothing.
    #[tokio::test]
    async fn a_query_with_no_direction_is_refused_by_an_empty_store_too() {
        let store = MemoryVectorStore::new();

        let error = store
            .search(&Embedding::new(vec![f32::NAN, 1.0]), &SearchParams::new(1))
            .await
            .expect_err("a query with no direction has nothing to search along");

        assert!(matches!(error, ComponentError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn a_stored_vector_with_a_component_that_is_not_a_number_scores_zero() {
        // The same answer as a stored vector of zero magnitude, for the same
        // reason: it is already indexed, and a NaN score would break the
        // ranking contract for every other hit in the list.
        let store = store_of(vec![
            entry("not-a-number", vec![f32::NAN, 0.0]),
            entry("somewhere", vec![1.0, 0.0]),
        ])
        .await;

        let hits = store
            .search(&Embedding::new(vec![1.0, 0.0]), &SearchParams::new(2))
            .await
            .expect("a well-formed search succeeds");

        assert!(hits.iter().all(|hit| hit.score.is_finite()));
        assert_eq!(ids(&hits), ["somewhere", "not-a-number"]);
        assert_eq!(hits[1].score, 0.0);
    }

    #[tokio::test]
    async fn a_query_with_a_component_that_is_not_a_number_is_an_invalid_request() {
        // The query side of the case above, answered the way the zero vector is
        // answered: the caller is holding this vector right now and can fix it.
        let store = store_of(vec![entry("c1", vec![1.0, 0.0])]).await;

        let error = store
            .search(&Embedding::new(vec![f32::NAN, 1.0]), &SearchParams::new(1))
            .await
            .expect_err("a vector that is not a number points nowhere");
        assert!(matches!(error, ComponentError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn a_vector_whose_squared_norm_overflows_f32_still_scores_finite() {
        // 1e30 is an ordinary f32 and its square is not: accumulated in f32 the
        // norm is infinite, and `inf / inf` is the NaN the ranking contract
        // forbids -- on the one search whose answer is least in doubt, a vector
        // against itself.
        let store = store_of(vec![entry("huge", vec![1e30, 0.0])]).await;

        let hits = store
            .search(&Embedding::new(vec![1e30, 0.0]), &SearchParams::new(1))
            .await
            .expect("a well-formed search succeeds");

        assert!(hits[0].score.is_finite(), "scored {}", hits[0].score);
        assert!(
            (hits[0].score - 1.0).abs() < 1e-6,
            "scored {}",
            hits[0].score
        );
    }

    #[tokio::test]
    async fn a_batch_that_opens_the_store_must_agree_with_itself() {
        // The empty-store path: there is no held width to check against, so the
        // batch itself has to fix one, which it can only do if it agrees.
        let store = MemoryVectorStore::new();

        let error = store
            .upsert(vec![
                entry("a", vec![1.0, 0.0]),
                entry("b", vec![1.0, 0.0, 0.0]),
            ])
            .await
            .expect_err("a batch cannot open the store at two widths");

        assert!(matches!(error, ComponentError::InvalidRequest(_)));
        assert_eq!(store.len(), 0, "a batch that disagrees is rejected whole");
    }

    #[tokio::test]
    async fn an_embedding_of_no_width_cannot_be_stored() {
        // An empty embedding is representable, so without this a width-zero
        // batch opens the store at a width against which every later vector --
        // and every query -- is a disagreement.
        let store = MemoryVectorStore::new();

        let error = store
            .upsert(vec![entry("nothing", Vec::new())])
            .await
            .expect_err("a store of width zero could answer no search");

        assert!(matches!(error, ComponentError::InvalidRequest(_)));
        assert_eq!(store.len(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_upserts_and_searches_leave_one_consistent_store() {
        // `upsert` takes `&self`, so the contract lets one caller write while
        // others read. What that has to leave is checkable: every write lands
        // exactly once, and no reader sees a list breaking the ranking contract.
        const WRITERS: usize = 4;
        const PER_WRITER: usize = 25;

        let store = Arc::new(MemoryVectorStore::new());
        let mut tasks = Vec::new();

        for writer in 0..WRITERS {
            let store = Arc::clone(&store);
            tasks.push(tokio::spawn(async move {
                for i in 0..PER_WRITER {
                    // Disjoint id ranges, so the final count is arithmetic
                    // rather than a race the assertion has to tolerate.
                    let id = format!("w{writer}-{i}");
                    store
                        .upsert(vec![entry(&id, vec![1.0, i as f32 / 100.0])])
                        .await
                        .expect("a well-formed upsert");
                }
            }));
        }

        for _ in 0..WRITERS {
            let store = Arc::clone(&store);
            tasks.push(tokio::spawn(async move {
                for _ in 0..PER_WRITER {
                    let hits = store
                        .search(&Embedding::new(vec![1.0, 0.0]), &SearchParams::new(5))
                        .await
                        .expect("a well-formed search succeeds");
                    assert!(
                        hits.iter().all(|hit| hit.score.is_finite()),
                        "a search interleaved with a write returned a non-finite score"
                    );
                    assert!(
                        hits.windows(2).all(|pair| pair[0].score >= pair[1].score),
                        "a search interleaved with a write returned an unsorted list"
                    );
                }
            }));
        }

        for task in tasks {
            task.await.expect("no task panicked");
        }

        assert_eq!(store.len(), WRITERS * PER_WRITER);
    }
}
