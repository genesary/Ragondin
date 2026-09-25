//! The behaviour the suite deliberately does not check, because it depends on
//! this builder's own budget unit: what fits, in what order, joined how.

#![cfg(feature = "concat")]

use ragondin_context_concat::ConcatContextBuilder;
use ragondin_contracts::{ComponentError, ContextBuilder, ContextParams};
use ragondin_types::{Chunk, ChunkId, Context, ContextChunk, DocId, Query, QueryId, ScoredChunk};

fn query() -> Query {
    Query {
        id: QueryId::new("q"),
        text: "what is in the context?".to_owned(),
    }
}

fn chunk(id: &str, text: &str, score: f32) -> ScoredChunk {
    ScoredChunk {
        chunk: Chunk {
            id: ChunkId::new(id),
            text: text.to_owned(),
            document_id: DocId::new(format!("doc-{id}")),
        },
        score,
    }
}

fn placed(context: &Context) -> Vec<&str> {
    context.chunks.iter().map(|c| c.id.as_str()).collect()
}

async fn build(separator: &str, chunks: Vec<ScoredChunk>, budget: usize) -> Context {
    ConcatContextBuilder::new(separator)
        .build(&query(), chunks, &ContextParams::new(budget))
        .await
        .expect("a well-formed call succeeds")
}

/// Chunks of 4, 3 and 5 characters with a 2-character separator: the first
/// two take `4 + 2 + 3 = 9`, and the third would take `9 + 2 + 5 = 16`.
fn three() -> Vec<ScoredChunk> {
    vec![
        chunk("a", "aaaa", 0.9),
        chunk("b", "bbb", 0.5),
        chunk("c", "ccccc", 0.1),
    ]
}

#[tokio::test]
async fn exactly_the_prefix_that_fits_is_included_in_order() {
    for budget in 9..16 {
        let context = build("--", three(), budget).await;
        assert_eq!(placed(&context), ["a", "b"], "budget {budget}");
        assert_eq!(context.text, "aaaa--bbb", "budget {budget}");
    }
    let context = build("--", three(), 16).await;
    assert_eq!(placed(&context), ["a", "b", "c"]);
    assert_eq!(context.text, "aaaa--bbb--ccccc");
}

#[tokio::test]
async fn the_separator_counts_toward_the_budget() {
    // `aaaa` and `bbb` are 7 characters without the separator, 9 with it.
    let context = build("--", three(), 8).await;
    assert_eq!(placed(&context), ["a"]);
    assert_eq!(context.text, "aaaa");
}

#[tokio::test]
async fn the_first_chunk_that_overflows_ends_the_context() {
    // `c` would fit on its own after `a` (4 + 2 + 1 = 7), but `b` comes first
    // and does not: the builder never skips ahead to a smaller later chunk.
    let chunks = vec![
        chunk("a", "aaaa", 0.9),
        chunk("b", "bbbbbbbbbb", 0.5),
        chunk("c", "c", 0.1),
    ];
    let context = build("--", chunks, 7).await;
    assert_eq!(placed(&context), ["a"]);
    assert_eq!(context.text, "aaaa");
}

#[tokio::test]
async fn a_first_chunk_over_the_budget_gives_the_empty_context() {
    // No truncation inside a chunk: a chunk is in or out.
    let context = build("--", three(), 3).await;
    assert!(context.chunks.is_empty());
    assert_eq!(context.text, "");
}

#[tokio::test]
async fn the_separator_is_placed_between_chunks_and_not_after_the_last() {
    let context = build(" | ", three(), 1_000).await;
    assert_eq!(context.text, "aaaa | bbb | ccccc");

    let context = build(" | ", vec![chunk("a", "aaaa", 0.9)], 1_000).await;
    assert_eq!(context.text, "aaaa");
}

#[tokio::test]
async fn the_budget_counts_chars_not_bytes() {
    // `é` is one `char` and two UTF-8 bytes: four of them are 4 characters and
    // 8 bytes, so a byte count would refuse this chunk under a budget of 4.
    let context = build("--", vec![chunk("a", "éééé", 0.9)], 4).await;
    assert_eq!(placed(&context), ["a"]);
    assert_eq!(context.text, "éééé");
}

#[tokio::test]
async fn provenance_carries_each_placed_chunk_untouched() {
    let context = build("--", three(), 9).await;
    assert_eq!(
        context.chunks,
        [
            ContextChunk {
                id: ChunkId::new("a"),
                document_id: DocId::new("doc-a"),
                score: 0.9,
            },
            ContextChunk {
                id: ChunkId::new("b"),
                document_id: DocId::new("doc-b"),
                score: 0.5,
            },
        ]
    );
}

#[tokio::test]
async fn empty_input_is_the_empty_context() {
    let context = build("--", Vec::new(), 100).await;
    assert!(context.chunks.is_empty());
    assert_eq!(context.text, "");
}

#[tokio::test]
async fn a_zero_budget_is_an_invalid_request() {
    for chunks in [three(), Vec::new()] {
        let result = ConcatContextBuilder::new("--")
            .build(&query(), chunks, &ContextParams::new(0))
            .await;
        assert!(
            matches!(result, Err(ComponentError::InvalidRequest(_))),
            "{result:?}"
        );
    }
}

#[tokio::test]
async fn the_identity_is_a_digest_of_the_separator() {
    let identity = |separator: &'static str| async move {
        ConcatContextBuilder::new(separator)
            .model_identity()
            .await
            .expect("identity")
    };
    let newline = identity("\n").await;
    assert_eq!(newline, identity("\n").await);
    assert_ne!(newline, identity("\n\n").await);
    assert_ne!(newline, identity("").await);
    // Pinned, so that a change to the scheme is a deliberate edit here rather
    // than a silent change of every recorded run's identity. The value is the
    // SHA-256 of `ragondin-context-concat`, `char` and `\n`, each preceded by
    // its byte length as a little-endian u64 -- computed outside this crate.
    assert_eq!(
        newline.as_str(),
        "sha256:2483887ed2fe9664ba2a2618f196184b7500f0cf0f0018be46ea1ce99a505112"
    );
}
