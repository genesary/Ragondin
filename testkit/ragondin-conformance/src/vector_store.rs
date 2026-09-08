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
/// # What `make` must return
///
/// A store the suite **exclusively owns and that starts empty**, with each call
/// returning **independent** state — a fresh collection, not another handle on
/// a shared one. The suite writes before it reads, and it cannot assert that an
/// inserted vector is its own nearest neighbour if something closer may already
/// be there: under an unnormalised metric any pre-existing vector of larger
/// magnitude outranks an inserted unit vector, and no probe the suite could
/// build is guaranteed to win. `make` is called four times and must be cheap,
/// infallible and synchronous.
///
/// `dim` is the dimensionality the store was built for: a vector of the wrong
/// width is an invalid request rather than a conformance failure.
///
/// # What is checked
///
/// - **An empty store answers**, with an empty list rather than an error.
/// - **An inserted vector is its own nearest neighbour.** The one property
///   that makes a vector store a vector store; a store failing it returns
///   plausible neighbours forever without erroring once.
/// - **At most `top_k`** results, and the **ranking contract** — descending,
///   finite scores — checked on a search that returns *several* hits, since
///   ordering means nothing on one. This is what catches a store handing back
///   raw L2 **distances**, where lower is better: the shape Qdrant, FAISS and
///   pgvector return natively.
/// - **`upsert` replaces by chunk id**, as its documentation says: re-inserting
///   an id leaves one entry, and that entry carries the *new* content. A store
///   that appends double-counts a re-indexed corpus; one that ignores the
///   second write serves stale vectors forever. Both fail here.
/// - A **`top_k` of zero is rejected** as an invalid request.
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
    let probe = one_hot(dim, 0);

    let store = make();
    let context = "search before anything is inserted";
    let hits = store
        .search(&probe, &SearchParams::new(5))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    if !hits.is_empty() {
        return Err(ConformanceFailure::new(
            COMPONENT,
            "empty store yields no results",
            format!(
                "{context}: returned {} results — `make` must return a store the suite owns and that starts empty",
                hits.len()
            ),
        ));
    }

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

    // The ranked search: `top_k = 1` above can never order anything, so the
    // ranking contract is checked here, over every vector the suite inserted.
    // (With `dim == 1` there is only one basis vector and the list is again
    // too short to order — the degenerate case the check cannot cover.)
    let context = "search returning every inserted vector";
    let hits = store
        .search(&probe, &SearchParams::new(entries.len()))
        .await
        .map_err(|error| ConformanceFailure::from_call(COMPONENT, context, &error))?;
    check_top_k(COMPONENT, context, &hits, entries.len())?;
    check_ranking(COMPONENT, context, &hits)?;
    if let Some(hit) = hits.first() {
        if hit.chunk.id != entries[0].chunk.id {
            return Err(ConformanceFailure::new(
                COMPONENT,
                "nearest neighbour is itself",
                format!(
                    "{context}: `{}` outranks the vector searched for, `{}`",
                    hit.chunk.id.as_str(),
                    entries[0].chunk.id.as_str()
                ),
            ));
        }
    }

    let store = make();
    let original = entries[0].clone();
    let revised = EmbeddedChunk {
        chunk: ragondin_types::Chunk {
            text: "the same chunk, re-indexed with new text".to_string(),
            ..original.chunk.clone()
        },
        // A different vector too: a store keying on the embedding rather than
        // on the chunk id would otherwise look like it replaced correctly.
        embedding: halved(&original.embedding),
    };
    for entry in [original.clone(), revised.clone()] {
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
    let kept: Vec<_> = hits
        .iter()
        .filter(|hit| hit.chunk.id == original.chunk.id)
        .collect();
    match kept.as_slice() {
        [only] if only.chunk.text != revised.chunk.text => {
            return Err(ConformanceFailure::new(
                COMPONENT,
                "upsert replaces by id",
                format!(
                    "{context}: `{}` still carries the text it was first inserted with",
                    original.chunk.id.as_str()
                ),
            ))
        }
        // Zero is not judged: a store may filter a distant hit, and the
        // scenario above already proved an inserted vector is findable.
        [] | [_] => {}
        several => {
            return Err(ConformanceFailure::new(
                COMPONENT,
                "upsert replaces by id",
                format!(
                    "{context}: `{}` came back {} times after being inserted twice",
                    original.chunk.id.as_str(),
                    several.len()
                ),
            ))
        }
    }

    let store = make();
    check_zero_top_k_rejected(
        COMPONENT,
        "search with top_k=0",
        store.search(&probe, &SearchParams::new(0)).await,
    )
}

/// A vector with a single non-zero component, so that each entry is its own
/// nearest neighbour under any metric a store might use.
fn one_hot(dim: usize, i: usize) -> Embedding {
    let mut components = vec![0.0; dim];
    components[i] = 1.0;
    Embedding::new(components)
}

fn halved(embedding: &Embedding) -> Embedding {
    Embedding::new(embedding.as_slice().iter().map(|c| c / 2.0).collect())
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
