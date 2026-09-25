//! [`StubContextBuilder`]: the chunks it is handed, one per line.

use async_trait::async_trait;
use ragondin_contracts::{ComponentError, ContextBuilder, ContextParams};
use ragondin_types::{Context, ContextChunk, ModelIdentity, Query, ScoredChunk};

/// A [`ContextBuilder`] that keeps the first `budget` chunks it is handed, in
/// the order it is handed them, and renders their texts one per line.
///
/// - **The budget counts chunks.** The contract leaves the unit to the
///   implementation (ADR-C31 § 2); a count of chunks is the one unit that
///   needs neither a tokenizer nor a rule for cutting a passage in half.
/// - **`text` is each kept chunk's text, verbatim, joined by `\n`**, with no
///   header, no separator of its own and no trailing newline. A chunk whose
///   text holds a newline spans several lines; the first line of `text` is
///   always the first line of the first chunk — the line
///   [`StubGenerator`](crate::StubGenerator) answers with.
/// - **Each kept chunk carries its incoming score** untouched: a builder
///   selects, orders and renders, and never scores (ADR-C31 § 1).
/// - **Zero chunks build the empty context** — no chunk and an empty `text` —
///   which is a valid call (ADR-C19). A zero budget is refused as
///   [`ComponentError::InvalidRequest`], as a zero `top_k` is.
///
/// The query is not read: what this builder keeps and renders depends on the
/// chunks and the budget alone.
///
/// Carries no configuration, so its identity is the constant
/// [`IDENTITY`](Self::IDENTITY): a constant is conformant where no
/// configuration decides the output (ADR-C31 § 4).
#[derive(Clone, Copy, Debug, Default)]
pub struct StubContextBuilder;

impl StubContextBuilder {
    /// The identity [`model_identity`](ContextBuilder::model_identity)
    /// reports. It names the rendering and the budget's unit, the two things
    /// that decide this builder's output and are not in its node's params.
    pub const IDENTITY: &'static str = "ragondin-stub/context-builder:chunks-one-per-line:v1";
}

#[async_trait]
impl ContextBuilder for StubContextBuilder {
    async fn build(
        &self,
        _query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &ContextParams,
    ) -> Result<Context, ComponentError> {
        if params.budget == 0 {
            return Err(ComponentError::InvalidRequest(
                "a budget of zero asks for a context that cannot exist".to_string(),
            ));
        }

        let kept: Vec<ScoredChunk> = chunks.into_iter().take(params.budget).collect();
        let text = kept
            .iter()
            .map(|hit| hit.chunk.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = kept
            .into_iter()
            .map(|hit| ContextChunk {
                id: hit.chunk.id,
                document_id: hit.chunk.document_id,
                score: hit.score,
            })
            .collect();

        Ok(Context { chunks, text })
    }

    async fn model_identity(&self) -> Result<ModelIdentity, ComponentError> {
        Ok(ModelIdentity::new(Self::IDENTITY))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ragondin_types::{Chunk, ChunkId, DocId, QueryId};

    fn query() -> Query {
        Query {
            id: QueryId::new("q-1"),
            text: "a question".to_string(),
        }
    }

    fn hit(id: &str, text: &str, score: f32) -> ScoredChunk {
        ScoredChunk {
            chunk: Chunk {
                id: ChunkId::new(id),
                text: text.to_string(),
                document_id: DocId::new(format!("doc-{id}")),
            },
            score,
        }
    }

    fn hits() -> Vec<ScoredChunk> {
        vec![
            hit("a", "Paris", 0.9),
            hit("b", "Lyon\nsecond line", 0.4),
            hit("c", "Marseille", 0.1),
        ]
    }

    #[tokio::test]
    async fn it_renders_each_chunk_on_its_own_line_in_the_order_given() {
        let context = StubContextBuilder
            .build(&query(), hits(), &ContextParams::new(10))
            .await
            .expect("a positive budget is a well-formed call");

        assert_eq!(context.text, "Paris\nLyon\nsecond line\nMarseille");
        assert_eq!(
            context
                .chunks
                .iter()
                .map(|placed| (placed.id.as_str(), placed.document_id.as_str()))
                .collect::<Vec<_>>(),
            [("a", "doc-a"), ("b", "doc-b"), ("c", "doc-c")]
        );
    }

    #[tokio::test]
    async fn each_chunk_keeps_the_score_it_carried_in() {
        // ADR-C31 § 1: a builder never assigns a score of its own.
        let context = StubContextBuilder
            .build(&query(), hits(), &ContextParams::new(10))
            .await
            .expect("well-formed");

        assert_eq!(
            context
                .chunks
                .iter()
                .map(|placed| placed.score)
                .collect::<Vec<_>>(),
            [0.9, 0.4, 0.1]
        );
    }

    #[tokio::test]
    async fn the_budget_counts_chunks_and_keeps_the_first_ones() {
        let context = StubContextBuilder
            .build(&query(), hits(), &ContextParams::new(2))
            .await
            .expect("well-formed");

        assert_eq!(context.text, "Paris\nLyon\nsecond line");
        assert_eq!(
            context
                .chunks
                .iter()
                .map(|placed| placed.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[tokio::test]
    async fn zero_chunks_build_the_empty_context() {
        // ADR-C19: a valid call, answered with the empty context.
        let context = StubContextBuilder
            .build(&query(), Vec::new(), &ContextParams::new(10))
            .await
            .expect("zero chunks is a valid call");

        assert_eq!(
            context,
            Context {
                chunks: Vec::new(),
                text: String::new(),
            }
        );
    }

    #[tokio::test]
    async fn a_zero_budget_is_an_invalid_request() {
        let refused = StubContextBuilder
            .build(&query(), hits(), &ContextParams::new(0))
            .await;

        assert!(matches!(refused, Err(ComponentError::InvalidRequest(_))));
    }

    #[tokio::test]
    async fn its_identity_is_the_documented_constant() {
        let identity = StubContextBuilder
            .model_identity()
            .await
            .expect("the stub always reports its identity");

        assert_eq!(identity, ModelIdentity::new(StubContextBuilder::IDENTITY));
        assert!(!StubContextBuilder::IDENTITY.is_empty());
    }
}
