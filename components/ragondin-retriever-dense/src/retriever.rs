//! Dense retrieval: embed the query, then search the vector index.

use async_trait::async_trait;
use ragondin_contracts::{
    ComponentError, EmbedParams, EmbedRole, Embedder, RetrieveParams, Retriever, SearchParams,
    VectorStore,
};
use ragondin_types::{Query, ScoredChunk};

/// A [`Retriever`] that answers a query by embedding it and searching a
/// [`VectorStore`] for the nearest chunks.
///
/// # What it is built from
///
/// An [`Embedder`] and a [`VectorStore`], both injected as trait objects. This
/// component owns neither model nor index: it is the two-call composition
/// between them, and everything that decides retrieval quality — which model,
/// which store, what the corpus was chunked into — is settled by whoever
/// constructs it. Holding them as `Box<dyn _>` is also what keeps this crate a
/// leaf: a dependency on a concrete embedder or store would make one component
/// depend on another (INV-5).
///
/// # Ranking
///
/// Results honour the ranking contract on [`Retriever`] — descending score,
/// every score finite — because the store's `search` honours it: the scores are
/// the store's own and are neither rescaled nor re-sorted here. A store holding
/// fewer than `top_k` vectors returns fewer than `top_k` hits, and an empty
/// store returns an empty list rather than an error.
///
/// A query the embedder maps to a vector of the wrong width is refused by the
/// store, which is where a dimensionality disagreement is meaningful: pairing
/// an embedder with an index built by a different model is a wiring mistake,
/// and this component cannot see it — a width is all it would have to compare,
/// and two models frequently share one.
pub struct DenseRetriever {
    embedder: Box<dyn Embedder>,
    store: Box<dyn VectorStore>,
}

impl DenseRetriever {
    /// A retriever that embeds with `embedder` and searches `store`.
    ///
    /// Infallible and cheap: nothing is loaded or connected here. Whatever the
    /// two backends need — a model session, a client — they acquired before
    /// they were handed over.
    pub fn new(embedder: Box<dyn Embedder>, store: Box<dyn VectorStore>) -> Self {
        Self { embedder, store }
    }
}

#[async_trait]
impl Retriever for DenseRetriever {
    async fn retrieve(
        &self,
        query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        // Refused here rather than left to the store. Rejecting a `top_k` of
        // zero is this component's obligation under the `Retriever` contract,
        // and a store is free to answer a search of zero however it likes; if
        // the check were delegated, whether this retriever is conformant would
        // depend on which store it happens to have been built over.
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest(
                "top_k of zero: ask for at least one chunk".to_string(),
            ));
        }

        // The role is the one fact only the caller knows (ADR-C17): the text is
        // a query, and an asymmetric model prefixes it accordingly. Passing the
        // passage role here would raise no error anywhere and simply score
        // worse.
        let mut embeddings = self
            .embedder
            .embed(
                std::slice::from_ref(&query.text),
                &EmbedParams::new(EmbedRole::Query),
            )
            .await?;

        // One text in, one vector out is the `Embedder` contract. An
        // implementation that breaks it leaves nothing to search with, so it is
        // reported as the backend failure it is rather than indexed into.
        if embeddings.len() != 1 {
            return Err(ComponentError::Backend(Box::new(EmbedderCountError {
                returned: embeddings.len(),
            })));
        }
        let embedding = embeddings.remove(0);

        self.store
            .search(&embedding, &SearchParams::new(params.top_k))
            .await
    }
}

/// The embedder answered one text with something other than one vector.
///
/// Private: it travels boxed inside [`ComponentError::Backend`], and a caller
/// that matched on its type would be building on fidelity the contract only
/// offers in-process (`ragondin-contracts` says so of that variant).
#[derive(Debug, thiserror::Error)]
#[error("the embedder returned {returned} vectors for one query; the contract is one per input")]
struct EmbedderCountError {
    returned: usize,
}
