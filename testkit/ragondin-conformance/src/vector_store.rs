//! [`VectorStore`] conformance.

use ragondin_contracts::{EmbeddedChunk, SearchParams, VectorStore};
use ragondin_types::Embedding;

use crate::{
    checks::{check_ranking, check_top_k, check_zero_top_k_rejected},
    failure::ConformanceFailure,
    fixtures::chunk,
};

const COMPONENT: &str = "VectorStore";

/// Checks that `make`'s stores honour the [`VectorStore`] contract.
///
/// `dim` is the dimensionality the store was built for: unlike the other
/// families, this suite has to *write* before it can read, and a vector of the
/// wrong width is an invalid request rather than a conformance failure. The
/// store may hold anything already — nothing here assumes a fresh one is
/// empty.
///
/// - **An inserted vector is its own nearest neighbour.** The one property
///   that makes a vector store a vector store; a store failing it returns
///   plausible neighbours forever without erroring once.
/// - **At most `top_k`** results, and the **ranking contract** — descending,
///   finite scores.
/// - **`upsert` replaces by chunk id**, as its documentation says: re-inserting
///   an id leaves one entry, not two. A store that appends silently
///   double-counts a re-indexed corpus.
/// - A **`top_k` of zero is rejected** as an invalid request.
///
/// `make` is called several times: each scenario gets its own store, so one
/// scenario's writes cannot decide another's outcome.
pub async fn check_vector_store_conformance(
    make: impl Fn() -> Box<dyn VectorStore>,
    dim: usize,
) -> Result<(), ConformanceFailure> {
    if dim == 0 {
        return Err(ConformanceFailure::new(
            COMPONENT,
            "dimensionality",
            "the suite must build a vector to insert and search with, and no vector has zero components",
        ));
    }
    let entries = entries(dim);

    // Reading a store the suite has not written to must work, whatever it
    // holds. This is where "an empty index answers rather than errors" lands.
    let store = make();
    let context = "search before the suite inserts anything";
    let hits = store
        .search(&one_hot(dim, 0), &SearchParams::new(5))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_top_k(COMPONENT, context, &hits, 5)?;
    check_ranking(COMPONENT, context, &hits)?;

    let store = make();
    store
        .upsert(entries.clone())
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, "upsert", &error))?;
    for entry in &entries {
        let id = entry.chunk.id.as_str();
        let context = format!("search for the vector inserted as `{id}`");
        let hits = store
            .search(&entry.embedding, &SearchParams::new(1))
            .await
            .map_err(|error| ConformanceFailure::from_call(COMPONENT, &context, &error))?;
        check_top_k(COMPONENT, &context, &hits, 1)?;
        check_ranking(COMPONENT, &context, &hits)?;
        match hits.first() {
            Some(hit) if hit.chunk.id == entry.chunk.id => {}
            Some(hit) => {
                return Err(ConformanceFailure::new(
                    COMPONENT,
                    "nearest neighbour is itself",
                    format!("{context}: nearest is `{}`", hit.chunk.id.as_str()),
                ))
            }
            None => {
                return Err(ConformanceFailure::new(
                    COMPONENT,
                    "nearest neighbour is itself",
                    format!("{context}: returned nothing"),
                ))
            }
        }
    }

    let store = make();
    let original = entries[0].clone();
    let mut revised = original.clone();
    revised.chunk.text = "the same chunk, re-indexed with new text".to_string();
    for entry in [original.clone(), revised] {
        store
            .upsert(vec![entry])
            .await
            .map_err(|error| ConformanceFailure::from_call(COMPONENT, "upsert", &error))?;
    }
    let context = "search after re-inserting one chunk id";
    let hits = store
        .search(&original.embedding, &SearchParams::new(entries.len() + 2))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    let kept = hits
        .iter()
        .filter(|hit| hit.chunk.id == original.chunk.id)
        .count();
    if kept > 1 {
        return Err(ConformanceFailure::new(
            COMPONENT,
            "upsert replaces by id",
            format!(
                "{context}: `{}` came back {kept} times after being inserted twice",
                original.chunk.id.as_str()
            ),
        ));
    }

    let store = make();
    check_zero_top_k_rejected(
        COMPONENT,
        "search with top_k=0",
        store.search(&one_hot(dim, 0), &SearchParams::new(0)).await,
    )
}

/// A vector with a single non-zero component, so that each entry is its own
/// nearest neighbour under any metric a store might use.
fn one_hot(dim: usize, i: usize) -> Embedding {
    let mut components = vec![0.0; dim];
    components[i] = 1.0;
    Embedding::new(components)
}

/// One entry per available dimension, capped: three mutually distinguishable
/// vectors are enough to tell a store from a list.
fn entries(dim: usize) -> Vec<EmbeddedChunk> {
    (0..dim.min(3))
        .map(|i| EmbeddedChunk {
            chunk: chunk(&format!("conformance-{i}")),
            embedding: one_hot(dim, i),
        })
        .collect()
}

/// [`check_vector_store_conformance`], panicking on the first broken check.
pub async fn assert_vector_store_conformance(make: impl Fn() -> Box<dyn VectorStore>, dim: usize) {
    if let Err(failure) = check_vector_store_conformance(make, dim).await {
        panic!("{failure}");
    }
}
