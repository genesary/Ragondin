//! [`StubFusion`]: the legs, interleaved rank by rank.

use std::collections::HashSet;

use async_trait::async_trait;
use ragondin_contracts::{ComponentError, Fusion, FusionParams};
use ragondin_types::{ChunkId, ScoredChunk};

/// A [`Fusion`] that takes one chunk from each leg in turn.
///
/// Rank 0 of every leg, then rank 1 of every leg, and so on, in the order the
/// pipeline wires the legs — `inputs` is positional (ADR-C16) — with a chunk
/// already taken skipped wherever it appears again. Legs of unequal length are
/// not padded: a leg that has run out is simply passed over.
///
/// Carries no configuration, so there is nothing to construct it from.
#[derive(Clone, Copy, Debug, Default)]
pub struct StubFusion;

#[async_trait]
impl Fusion for StubFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        _params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let depth = inputs.iter().map(Vec::len).max().unwrap_or(0);
        let mut taken: HashSet<ChunkId> = HashSet::new();
        let mut fused: Vec<ScoredChunk> = Vec::new();

        for rank in 0..depth {
            for leg in &inputs {
                if let Some(hit) = leg.get(rank) {
                    if taken.insert(hit.chunk.id.clone()) {
                        fused.push(hit.clone());
                    }
                }
            }
        }

        // The incoming scores are re-written rather than kept, for the reason
        // RRF re-scores: two legs score on scales that have no common unit, so
        // a merged list ordered by the incoming numbers would be ordered by
        // whichever leg happens to score higher. Position is the only thing
        // this fusion decides, so position is the only thing it reports — and
        // `1 / (position + 1)` is descending and finite, which is the ranking
        // contract the merge order has already fixed.
        for (position, hit) in fused.iter_mut().enumerate() {
            hit.score = 1.0 / (position + 1) as f32;
        }

        Ok(fused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ragondin_types::{Chunk, DocId};

    fn scored(id: &str, score: f32) -> ScoredChunk {
        ScoredChunk {
            chunk: Chunk {
                id: ChunkId::new(id),
                text: format!("text of {id}"),
                document_id: DocId::new("doc"),
            },
            score,
        }
    }

    fn ids(hits: &[ScoredChunk]) -> Vec<&str> {
        hits.iter().map(|hit| hit.chunk.id.as_str()).collect()
    }

    #[tokio::test]
    async fn a_fixed_input_yields_a_fixed_output() {
        let left = vec![scored("l-0", 0.9), scored("l-1", 0.6), scored("l-2", 0.3)];
        let right = vec![scored("r-0", 0.8), scored("r-1", 0.4)];

        let fused = StubFusion
            .fuse(vec![left, right], &FusionParams::new())
            .await
            .expect("fusing two ranked lists is a well-formed call");

        assert_eq!(ids(&fused), ["l-0", "r-0", "l-1", "r-1", "l-2"]);
        assert_eq!(
            fused.iter().map(|hit| hit.score).collect::<Vec<_>>(),
            [1.0, 0.5, 1.0 / 3.0, 0.25, 0.2]
        );
    }

    #[tokio::test]
    async fn a_chunk_in_two_legs_is_taken_once() {
        let left = vec![scored("shared", 0.9), scored("l-1", 0.6)];
        let right = vec![scored("shared", 0.8), scored("r-1", 0.4)];

        let fused = StubFusion
            .fuse(vec![left, right], &FusionParams::new())
            .await
            .expect("well-formed");

        assert_eq!(
            ids(&fused),
            ["shared", "l-1", "r-1"],
            "merging is a fusion's job; concatenating would return `shared` twice"
        );
    }

    #[tokio::test]
    async fn legs_of_unequal_length_are_not_padded() {
        let long = vec![scored("l-0", 0.9), scored("l-1", 0.6), scored("l-2", 0.3)];
        let short = vec![scored("s-0", 0.8)];

        let fused = StubFusion
            .fuse(vec![long, short], &FusionParams::new())
            .await
            .expect("well-formed");

        assert_eq!(ids(&fused), ["l-0", "s-0", "l-1", "l-2"]);
    }

    #[tokio::test]
    async fn fusing_nothing_yields_nothing() {
        let fused = StubFusion
            .fuse(Vec::new(), &FusionParams::new())
            .await
            .expect("an empty collection is a valid argument (ADR-C19)");

        assert!(fused.is_empty());
    }
}
