//! [`StubGenerator`]: answers with the first line of the context its prompt
//! carries.

use async_trait::async_trait;
use ragondin_contracts::{ComponentError, GenerateParams, Generator};
use ragondin_types::{Answer, Context, ModelIdentity, Query};

/// A [`Generator`] with no model, whose answer is a fixed function of what it
/// is asked.
///
/// **The answer is the first line of the context's `text`, trimmed of
/// surrounding whitespace, when the template places `{context}` in the
/// prompt, and the empty answer otherwise.** Over
/// [`StubContextBuilder`](crate::StubContextBuilder) that is the first line of
/// the first chunk's text, so a fixture whose right passage opens with the
/// reference answer on a line of its own is answered correctly exactly when
/// that passage is ranked first. The query, the template's other text and the
/// optional settings — `temperature`, `seed`, `max_tokens` — change nothing:
/// there is no sampling to steer and no tokenizer to count with.
///
/// The template is parsed under the whole grammar stated on [`Generator`] —
/// both placeholders, both escapes, one pass left to right — and a malformed
/// or empty one is refused as [`ComponentError::InvalidRequest`]. What the
/// stub needs of the rendered prompt is only whether it carries the context,
/// because a context the prompt does not carry is one a model is never shown;
/// so it computes that and assembles no prompt text it would not read. An
/// empty context is a valid call (ADR-C19), answered with the empty answer.
///
/// **It serves one name**, its constructor configuration (ADR-C31 § 2): any
/// other `served_model`, and the empty one whatever it was constructed with,
/// is refused as [`ComponentError::InvalidRequest`] by both
/// [`generate`](Generator::generate) and
/// [`model_identity`](Generator::model_identity). For its name it reports the
/// constant [`IDENTITY`](Self::IDENTITY): the name decides whether a call is
/// served, never what it is answered, so no configuration decides the output
/// and a constant is conformant (ADR-C31 § 4).
#[derive(Clone, Debug)]
pub struct StubGenerator {
    served_model: String,
}

impl StubGenerator {
    /// The identity [`model_identity`](Generator::model_identity) reports for
    /// the name this generator serves. It names the answer function, the one
    /// thing that decides the answer and is not in the node's params.
    pub const IDENTITY: &'static str = "ragondin-stub/generator:first-line-of-context:v1";

    /// A generator that serves `served_model` and no other name.
    pub fn new(served_model: impl Into<String>) -> Self {
        Self {
            served_model: served_model.into(),
        }
    }

    /// Refuses a `served_model` this generator does not serve.
    fn check_served(&self, served_model: &str) -> Result<(), ComponentError> {
        if served_model.is_empty() {
            return Err(ComponentError::InvalidRequest(
                "an empty served_model names no model".to_string(),
            ));
        }
        if served_model != self.served_model {
            return Err(ComponentError::InvalidRequest(format!(
                "model {served_model:?} is not served here; this stub serves {:?}",
                self.served_model
            )));
        }
        Ok(())
    }
}

/// Parses `template` under the grammar stated on [`Generator`], and reports
/// whether the prompt it renders carries the context.
///
/// One pass, left to right: at each position `{{` or `}}` is an escape before
/// a placeholder is looked for, so `{{context}}` is the literal text
/// `{context}` and places nothing. Any other `{` or `}` is malformed.
fn places_context(template: &str) -> Result<bool, ComponentError> {
    let mut placed = false;
    let mut rest = template;
    while let Some(next) = rest.chars().next() {
        let taken = if rest.starts_with("{{") || rest.starts_with("}}") {
            2
        } else if rest.starts_with("{query}") {
            "{query}".len()
        } else if rest.starts_with("{context}") {
            placed = true;
            "{context}".len()
        } else if next == '{' {
            return Err(ComponentError::InvalidRequest(match rest.find('}') {
                Some(close) => format!(
                    "the template names {:?}, which is neither {{query}} nor {{context}}",
                    &rest[..=close]
                ),
                None => "the template opens a `{` that no `}` closes".to_string(),
            }));
        } else if next == '}' {
            return Err(ComponentError::InvalidRequest(
                "the template holds a lone `}`; a literal one is written `}}`".to_string(),
            ));
        } else {
            next.len_utf8()
        };
        rest = &rest[taken..];
    }
    Ok(placed)
}

#[async_trait]
impl Generator for StubGenerator {
    async fn generate(
        &self,
        _query: &Query,
        context: &Context,
        params: &GenerateParams,
    ) -> Result<Answer, ComponentError> {
        self.check_served(&params.served_model)?;
        if params.template.is_empty() {
            return Err(ComponentError::InvalidRequest(
                "an empty template renders no prompt".to_string(),
            ));
        }

        let text = if places_context(&params.template)? {
            context.text.lines().next().unwrap_or("").trim().to_string()
        } else {
            String::new()
        };
        Ok(Answer { text })
    }

    async fn model_identity(&self, served_model: &str) -> Result<ModelIdentity, ComponentError> {
        self.check_served(served_model)?;
        Ok(ModelIdentity::new(Self::IDENTITY))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StubContextBuilder;
    use ragondin_contracts::{ContextBuilder, ContextParams};
    use ragondin_types::{Chunk, ChunkId, ContextChunk, DocId, QueryId, ScoredChunk};

    const SERVED: &str = "stub-model";

    fn query(text: &str) -> Query {
        Query {
            id: QueryId::new("q-1"),
            text: text.to_string(),
        }
    }

    fn context(text: &str) -> Context {
        Context {
            chunks: vec![ContextChunk {
                id: ChunkId::new("a"),
                document_id: DocId::new("doc-a"),
                score: 1.0,
            }],
            text: text.to_string(),
        }
    }

    async fn answer(
        template: &str,
        query_text: &str,
        context_text: &str,
    ) -> Result<Answer, ComponentError> {
        StubGenerator::new(SERVED)
            .generate(
                &query(query_text),
                &context(context_text),
                &GenerateParams::new(SERVED, template),
            )
            .await
    }

    fn text(answer: Result<Answer, ComponentError>) -> String {
        answer.expect("a well-formed call").text
    }

    #[tokio::test]
    async fn it_answers_with_the_first_line_of_the_context_trimmed() {
        assert_eq!(
            text(
                answer(
                    "Q: {query}\nC: {context}",
                    "capital?",
                    "  Paris  \nLyon\nMarseille"
                )
                .await
            ),
            "Paris"
        );
    }

    #[tokio::test]
    async fn over_the_stub_builder_the_answer_is_the_first_chunks_first_line_every_time() {
        // The fixture contract the exit criterion builds on: the right passage
        // ranked first, opening with the reference answer on its own line.
        let passage = |id: &str, text: &str, score: f32| ScoredChunk {
            chunk: Chunk {
                id: ChunkId::new(id),
                text: text.to_string(),
                document_id: DocId::new("doc"),
            },
            score,
        };
        let context = StubContextBuilder
            .build(
                &query("q"),
                vec![
                    passage("right", "Paris\nThe capital of France.", 0.9),
                    passage("wrong", "Lyon", 0.1),
                ],
                &ContextParams::new(2),
            )
            .await
            .expect("well-formed");
        let generator = StubGenerator::new(SERVED);
        let params = GenerateParams::new(SERVED, "Answer from:\n{context}\nQuestion: {query}")
            .with_temperature(1.0)
            .with_seed(3);

        let first = generator
            .generate(&query("q"), &context, &params)
            .await
            .expect("well-formed");
        let second = generator
            .generate(&query("q"), &context, &params)
            .await
            .expect("well-formed");

        assert_eq!(first.text.as_bytes(), second.text.as_bytes());
        assert_eq!(first.text, "Paris");
    }

    #[tokio::test]
    async fn the_query_does_not_change_the_answer() {
        assert_eq!(
            text(answer("{query} {context}", "one question", "Paris").await),
            text(answer("{query} {context}", "another question entirely", "Paris").await),
        );
    }

    #[tokio::test]
    async fn an_empty_context_answers_the_empty_answer() {
        // ADR-C19: a valid call; the empty answer stands for "I do not know".
        assert_eq!(text(answer("{context}", "q", "").await), "");
    }

    #[tokio::test]
    async fn a_template_that_does_not_place_the_context_gets_the_empty_answer() {
        // The rendered prompt is the whole of what the stub's model is asked:
        // a context the prompt does not carry is one it never saw.
        assert_eq!(text(answer("{query}", "q", "Paris").await), "");
        assert_eq!(
            text(answer("no placeholder at all", "q", "Paris").await),
            ""
        );
    }

    #[tokio::test]
    async fn an_escaped_placeholder_is_literal_text_and_places_nothing() {
        assert_eq!(text(answer("{{context}}", "q", "Paris").await), "");
        assert_eq!(text(answer("{{{context}}}", "q", "Paris").await), "Paris");
    }

    #[tokio::test]
    async fn a_placeholder_may_appear_many_times() {
        assert_eq!(
            text(answer("{context}{query}{context}{query}", "q", "Paris").await),
            "Paris"
        );
    }

    #[tokio::test]
    async fn a_malformed_template_is_an_invalid_request() {
        for malformed in [
            "{unknown}",
            "{context} {",
            "{query} }",
            "{",
            "}",
            "{Query}",
            "{ context }",
            "{query",
            "}{context}",
            "{{query}",
            "{{{query}}",
        ] {
            assert!(
                matches!(
                    answer(malformed, "q", "Paris").await,
                    Err(ComponentError::InvalidRequest(_))
                ),
                "{malformed:?} is malformed"
            );
        }
    }

    #[tokio::test]
    async fn an_empty_template_is_an_invalid_request() {
        assert!(matches!(
            answer("", "q", "Paris").await,
            Err(ComponentError::InvalidRequest(_))
        ));
    }

    #[tokio::test]
    async fn a_served_model_other_than_its_own_is_an_invalid_request() {
        let generator = StubGenerator::new(SERVED);
        for name in ["", "another-model", "stub-model "] {
            let refused = generator
                .generate(
                    &query("q"),
                    &context("Paris"),
                    &GenerateParams::new(name, "{context}"),
                )
                .await;
            assert!(
                matches!(refused, Err(ComponentError::InvalidRequest(_))),
                "{name:?} is not served"
            );
            assert!(
                matches!(
                    generator.model_identity(name).await,
                    Err(ComponentError::InvalidRequest(_))
                ),
                "{name:?} has no identity here"
            );
        }
    }

    #[tokio::test]
    async fn an_empty_name_is_refused_even_when_it_was_configured() {
        let generator = StubGenerator::new("");
        assert!(matches!(
            generator
                .generate(
                    &query("q"),
                    &context("Paris"),
                    &GenerateParams::new("", "{context}")
                )
                .await,
            Err(ComponentError::InvalidRequest(_))
        ));
        assert!(matches!(
            generator.model_identity("").await,
            Err(ComponentError::InvalidRequest(_))
        ));
    }

    #[tokio::test]
    async fn its_identity_is_the_documented_constant_for_its_own_name() {
        let identity = StubGenerator::new(SERVED)
            .model_identity(SERVED)
            .await
            .expect("its own name is served");

        assert_eq!(identity, ModelIdentity::new(StubGenerator::IDENTITY));
        assert_eq!(
            StubGenerator::new("other-name")
                .model_identity("other-name")
                .await
                .expect("its own name is served"),
            identity,
            "the name decides what is served, never what is answered"
        );
    }
}
