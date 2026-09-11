//! [`StubRetriever`]: a ranked list fabricated from a label and a rank.

use async_trait::async_trait;
use ragondin_contracts::{ComponentError, RetrieveParams, Retriever};
use ragondin_types::{Chunk, ChunkId, DocId, Query, ScoredChunk};

/// A [`Retriever`] with no corpus, whose answer is a function of its label and
/// the requested `top_k` alone.
///
/// It returns exactly `top_k` chunks, with ids `<label>-0`, `<label>-1`, … and
/// scores `1 / (rank + 1)`. Having no corpus is what lets it always satisfy
/// `top_k`, and is also why its answer carries no information: the query text
/// reaches the chunk text and changes nothing about which chunks come back or
/// in what order.
///
/// The **label** is what makes two legs of one pipeline distinguishable — the
/// vertical slice registers one constructor and configures two nodes with it —
/// and it is constructor configuration, fixed for the component's lifetime,
/// where `top_k` varies per call (`docs/code-architecture.md` §6.3).
#[derive(Clone, Debug)]
pub struct StubRetriever {
    label: String,
}

impl StubRetriever {
    /// A retriever whose chunk ids and document id are named after `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

/// The score at `rank`: `1 / (rank + 1)`.
///
/// Descending and finite, which is the ranking contract, and a function of the
/// position and nothing else — a stub has no relevance to express, and a score
/// that looked like one would invite being read as a measurement.
fn reciprocal_rank(rank: usize) -> f32 {
    1.0 / (rank + 1) as f32
}

#[async_trait]
impl Retriever for StubRetriever {
    async fn retrieve(
        &self,
        query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest(
                "top_k of zero asks for a result that cannot exist".to_string(),
            ));
        }

        Ok((0..params.top_k)
            .map(|rank| ScoredChunk {
                chunk: Chunk {
                    id: ChunkId::new(format!("{}-{rank}", self.label)),
                    text: format!("stub chunk {rank} of `{}` for `{}`", self.label, query.text),
                    document_id: DocId::new(self.label.clone()),
                },
                score: reciprocal_rank(rank),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ragondin_types::QueryId;

    fn query() -> Query {
        Query {
            id: QueryId::new("q-1"),
            text: "a question".to_string(),
        }
    }

    #[tokio::test]
    async fn a_fixed_input_yields_a_fixed_output() {
        let hits = StubRetriever::new("leg")
            .retrieve(&query(), &RetrieveParams::new(3))
            .await
            .expect("a positive top_k is a well-formed call");

        let ids: Vec<&str> = hits.iter().map(|hit| hit.chunk.id.as_str()).collect();
        assert_eq!(ids, ["leg-0", "leg-1", "leg-2"]);
        assert_eq!(
            hits.iter().map(|hit| hit.score).collect::<Vec<_>>(),
            [1.0, 0.5, 1.0 / 3.0]
        );
        assert!(
            hits.iter()
                .all(|hit| hit.chunk.document_id.as_str() == "leg"),
            "every chunk is attributed to the leg that fabricated it"
        );
        assert_eq!(
            hits.iter()
                .map(|hit| hit.chunk.text.as_str())
                .collect::<Vec<_>>(),
            [
                "stub chunk 0 of `leg` for `a question`",
                "stub chunk 1 of `leg` for `a question`",
                "stub chunk 2 of `leg` for `a question`",
            ],
            "the query reaches the chunk text, which is the only place it reaches"
        );
    }

    #[tokio::test]
    async fn the_query_does_not_change_which_chunks_come_back() {
        // The stub's whole point: two different questions, one answer. Stated
        // as a test so that a later "make it a bit more realistic" change has
        // to say so out loud.
        let retriever = StubRetriever::new("leg");
        let other = Query {
            id: QueryId::new("q-2"),
            text: "an entirely different question".to_string(),
        };

        let first = retriever
            .retrieve(&query(), &RetrieveParams::new(2))
            .await
            .expect("well-formed");
        let second = retriever
            .retrieve(&other, &RetrieveParams::new(2))
            .await
            .expect("well-formed");

        assert_eq!(
            first
                .iter()
                .map(|hit| (hit.chunk.id.clone(), hit.score))
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|hit| (hit.chunk.id.clone(), hit.score))
                .collect::<Vec<_>>()
        );
        assert_ne!(
            first[0].chunk.text, second[0].chunk.text,
            "the other half of the claim: the query does reach the text, and only the text"
        );
    }

    #[tokio::test]
    async fn a_zero_top_k_is_an_invalid_request() {
        let refused = StubRetriever::new("leg")
            .retrieve(&query(), &RetrieveParams::new(0))
            .await;

        assert!(
            matches!(refused, Err(ComponentError::InvalidRequest(_))),
            "a top_k of zero asks for a result that cannot exist"
        );
    }
}
