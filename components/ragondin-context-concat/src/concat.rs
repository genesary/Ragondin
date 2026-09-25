//! Ordered concatenation under a character budget.

use async_trait::async_trait;
use ragondin_contracts::{ComponentError, ContextBuilder, ContextParams};
use ragondin_types::{Context, ContextChunk, ModelIdentity, Query, ScoredChunk};
use sha2::{Digest, Sha256};

/// The unit [`ContextParams::budget`] is counted in by this builder, as it
/// enters [`ConcatContextBuilder::model_identity`].
const BUDGET_UNIT: &str = "char";

/// Concatenates chunks in the order given, joined by a fixed separator, under a
/// budget counted in **characters** — Unicode scalar values, [`char`]s, not
/// bytes.
///
/// The chunks are taken in the order they arrive, which is rank order when the
/// producing node ranked them; the builder never reorders. Each is appended,
/// preceded by the separator from the second chunk on, **while the whole text,
/// separators included, stays within the budget**. The first chunk that would
/// take it over ends the context: that chunk and every later one are left out
/// whole — no chunk is truncated, and no smaller later chunk is taken in its
/// place. A first chunk longer than the budget therefore gives the empty
/// context, as zero chunks do.
///
/// [`Context::chunks`] names the chunks that made it in, in order, each with
/// its document and the score it carried in, untouched: this builder selects
/// and renders, it does not score. The query is not read.
///
/// A budget of zero is refused as [`ComponentError::InvalidRequest`], whatever
/// the chunks.
pub struct ConcatContextBuilder {
    separator: String,
}

impl ConcatContextBuilder {
    /// Joins chunks with `separator`, which may be empty.
    ///
    /// The separator is the builder's whole configuration; the budget is per
    /// call, in [`ContextParams`].
    pub fn new(separator: impl Into<String>) -> Self {
        Self {
            separator: separator.into(),
        }
    }
}

#[async_trait]
impl ContextBuilder for ConcatContextBuilder {
    async fn build(
        &self,
        _query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &ContextParams,
    ) -> Result<Context, ComponentError> {
        let budget = params.budget;
        if budget == 0 {
            return Err(ComponentError::InvalidRequest(
                "a context budget of zero asks for a context that cannot exist".to_owned(),
            ));
        }

        let separator_len = self.separator.chars().count();
        let mut context = Context {
            chunks: Vec::new(),
            text: String::new(),
        };
        let mut used = 0usize;
        for scored in chunks {
            let first = context.chunks.is_empty();
            let joint = if first { 0 } else { separator_len };
            // Saturating: a sum past `usize::MAX` is over any budget anyway.
            let needed = used
                .saturating_add(joint)
                .saturating_add(scored.chunk.text.chars().count());
            if needed > budget {
                break;
            }
            if !first {
                context.text.push_str(&self.separator);
            }
            context.text.push_str(&scored.chunk.text);
            context.chunks.push(ContextChunk {
                id: scored.chunk.id,
                document_id: scored.chunk.document_id,
                score: scored.score,
            });
            used = needed;
        }
        Ok(context)
    }

    /// `sha256:` and the hex SHA-256 of this builder's configuration: its
    /// name, its budget unit and its separator.
    ///
    /// Each of the three enters the digest as its UTF-8 byte length (a
    /// little-endian `u64`) followed by its bytes, so no two configurations
    /// share an encoding — a separator cannot be confused with the field that
    /// follows it. Nothing per call enters it, and nothing that varies between
    /// calls or between instances built the same way, so it is stable.
    async fn model_identity(&self) -> Result<ModelIdentity, ComponentError> {
        let mut hasher = Sha256::new();
        for field in ["ragondin-context-concat", BUDGET_UNIT, &self.separator] {
            hasher.update((field.len() as u64).to_le_bytes());
            hasher.update(field.as_bytes());
        }
        Ok(ModelIdentity::new(format!(
            "sha256:{:x}",
            hasher.finalize()
        )))
    }
}
