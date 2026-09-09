//! The suite every implementation of a contract must pass (ADR-C6), plus the
//! behaviour that is this component's own.
//!
//! Behind `bm25` because the component is: with the feature off the crate
//! exports nothing to test, so the whole file compiles away rather than
//! failing to build.
#![cfg(feature = "bm25")]

use ragondin_conformance::assert_retriever_conformance;
use ragondin_contracts::{ComponentError, RetrieveParams, Retriever};
use ragondin_retriever_bm25::Bm25Retriever;
use ragondin_types::{Chunk, ChunkId, DocId, Query, QueryId};

/// A corpus small enough that the expected ranking can be read off it.
///
/// `cats-1` says "cat" twice and is the shortest document, so BM25 must rank it
/// above `cats-2`, which says it once among more words; `dogs-1` shares no term
/// with a query about cats and must not appear at all.
fn corpus() -> Vec<Chunk> {
    vec![
        chunk("cats-1", "felines", "the cat sat on the cat mat"),
        chunk(
            "cats-2",
            "felines",
            "a cat wandered slowly through the long empty hallway at dusk",
        ),
        chunk("dogs-1", "canines", "the dog barked at the postman"),
    ]
}

fn chunk(id: &str, document: &str, text: &str) -> Chunk {
    Chunk {
        id: ChunkId::new(id),
        text: text.to_string(),
        document_id: DocId::new(document),
    }
}

fn query(text: &str) -> Query {
    Query {
        id: QueryId::new("q-1"),
        text: text.to_string(),
    }
}

fn retriever() -> Bm25Retriever {
    Bm25Retriever::new(corpus()).expect("the corpus must index")
}

fn ids(hits: &[ragondin_types::ScoredChunk]) -> Vec<&str> {
    hits.iter().map(|hit| hit.chunk.id.as_str()).collect()
}

#[tokio::test]
async fn honours_the_retriever_contract() {
    assert_retriever_conformance(|| Box::new(retriever())).await;
}

#[tokio::test]
async fn ranks_the_denser_match_first_and_omits_the_unmatched() {
    let hits = retriever()
        .retrieve(&query("cat"), &RetrieveParams::new(10))
        .await
        .expect("a well-formed query must succeed");

    // `dogs-1` shares no term with the query: BM25 scores only what matches, so
    // a corpus-sized `top_k` still returns two hits, not three.
    assert_eq!(ids(&hits), vec!["cats-1", "cats-2"]);
}

/// The same index, queried twice, ranks identically — including the ties that
/// tantivy's own ordering leaves to the document address.
#[tokio::test]
async fn ranking_is_deterministic() {
    let corpus_query = query("the cat sat");
    let params = RetrieveParams::new(3);

    let first = retriever().retrieve(&corpus_query, &params).await.unwrap();
    for _ in 0..4 {
        let again = retriever().retrieve(&corpus_query, &params).await.unwrap();
        assert_eq!(first, again, "two runs over one corpus must rank alike");
    }
}

/// Two chunks with identical text score identically. Nothing in tantivy orders
/// such a tie, so the component does — by chunk id — or the ranking is a coin
/// flip that only shows up as a flaky benchmark number.
#[tokio::test]
async fn ties_are_broken_by_chunk_id() {
    let tied = vec![
        chunk("b", "doc", "identical text"),
        chunk("c", "doc", "identical text"),
        chunk("a", "doc", "identical text"),
    ];
    let retriever = Bm25Retriever::new(tied).expect("the corpus must index");

    let hits = retriever
        .retrieve(&query("identical text"), &RetrieveParams::new(3))
        .await
        .unwrap();

    assert_eq!(ids(&hits), vec!["a", "b", "c"]);
}

/// The chunk comes back whole — text and document id included — because a
/// downstream cross-encoder scores text pairs and the contract offers no
/// corpus lookup (`ragondin-types`: `ScoredChunk`).
#[tokio::test]
async fn returns_the_whole_chunk() {
    let hits = retriever()
        .retrieve(&query("postman"), &RetrieveParams::new(1))
        .await
        .unwrap();

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].chunk, corpus()[2]);
}

/// A query whose every term is unknown to the corpus retrieves nothing, and
/// that is an answer rather than a failure.
#[tokio::test]
async fn a_query_matching_nothing_yields_an_empty_list() {
    let hits = retriever()
        .retrieve(&query("aardvark xylophone"), &RetrieveParams::new(5))
        .await
        .unwrap();

    assert!(hits.is_empty(), "expected no hits, got {:?}", ids(&hits));
}

/// A retriever over no chunks is well-formed. The harness builds one before it
/// has indexed anything, and an error here would report an empty corpus as a
/// broken component.
#[tokio::test]
async fn an_empty_corpus_is_conformant() {
    assert_retriever_conformance(|| Box::new(Bm25Retriever::new(Vec::new()).unwrap())).await;
}

/// Punctuation is not query syntax. A retriever is handed natural-language
/// questions, and `?`, `:` and quotes must tokenize rather than parse.
#[tokio::test]
async fn punctuation_in_a_query_is_not_syntax() {
    let hits = retriever()
        .retrieve(
            &query("where is the \"cat\": (mat)?"),
            &RetrieveParams::new(2),
        )
        .await
        .expect("punctuation must not be read as query syntax");

    assert_eq!(ids(&hits), vec!["cats-1", "cats-2"]);
}

/// `top_k` bounds the answer from above.
#[tokio::test]
async fn top_k_truncates() {
    let hits = retriever()
        .retrieve(&query("cat"), &RetrieveParams::new(1))
        .await
        .unwrap();

    assert_eq!(ids(&hits), vec!["cats-1"]);
}

/// A `top_k` of zero is an unmet precondition, not a request for nothing.
#[tokio::test]
async fn zero_top_k_is_an_invalid_request() {
    let error = retriever()
        .retrieve(&query("cat"), &RetrieveParams::new(0))
        .await
        .expect_err("a top_k of zero must be rejected");

    assert!(matches!(error, ComponentError::InvalidRequest(_)));
}

/// The tie-break must happen *before* `top_k` truncates, or it decides only the
/// order of a selection tantivy already made by document address.
///
/// Three equally scoring chunks, `top_k` of two: whichever two survive is the
/// answer, so the survivors have to be chosen by the component's own rule. The
/// same corpus is indexed in three insertion orders — the one thing a document
/// address is a function of — and must answer identically each time.
#[tokio::test]
async fn ties_are_broken_before_top_k_truncates() {
    for order in [["a", "b", "c"], ["b", "c", "a"], ["c", "b", "a"]] {
        let tied = order
            .iter()
            .map(|id| chunk(id, "doc", "identical text"))
            .collect();
        let hits = Bm25Retriever::new(tied)
            .expect("the corpus must index")
            .retrieve(&query("identical text"), &RetrieveParams::new(2))
            .await
            .unwrap();

        assert_eq!(
            ids(&hits),
            vec!["a", "b"],
            "insertion order {order:?} must not change which two chunks survive"
        );
    }
}

/// A `top_k` larger than the corpus is a bound, not an allocation. The largest
/// one there is must return every match and nothing else.
#[tokio::test]
async fn an_unbounded_top_k_returns_every_match_and_does_not_panic() {
    let hits = retriever()
        .retrieve(&query("cat"), &RetrieveParams::new(usize::MAX))
        .await
        .expect("an enormous top_k is a bound, not a failure");

    assert_eq!(ids(&hits), vec!["cats-1", "cats-2"]);
}

/// No stemming and no stopword removal — `default-features = false` on tantivy
/// drops both features, and the `"default"` analyzer applies neither in any
/// case. The measurable consequence is pinned here rather than described: `cat`
/// does not match `cats`, and a stopword is a term like any other. Restoring
/// either feature, or naming an analyzer that carries one, then shows up as a
/// failing test instead of as a ranking that quietly changed.
#[tokio::test]
async fn a_plural_is_a_different_term_and_a_stopword_is_a_term() {
    let unstemmed = retriever()
        .retrieve(&query("cats"), &RetrieveParams::new(5))
        .await
        .unwrap();
    assert!(
        unstemmed.is_empty(),
        "`cats` must not reach `cat`, got {:?}",
        ids(&unstemmed)
    );

    let stopword = retriever()
        .retrieve(&query("the"), &RetrieveParams::new(5))
        .await
        .unwrap();
    assert_eq!(
        stopword.len(),
        3,
        "`the` is indexed and scored like any other term, not discarded"
    );
}

/// The text field names tantivy's `"default"` analyzer: SimpleTokenizer, then
/// `RemoveLongFilter(40)`, then `LowerCaser`. Case folds, so a query need not
/// match the corpus's capitalization; accents do not, so a token keeps the
/// letters it was written with.
#[tokio::test]
async fn the_analyzer_folds_case_but_not_accents() {
    let retriever = Bm25Retriever::new(vec![chunk("s-1", "doc", "Die Straße war leer")])
        .expect("the corpus must index");

    let cased = retriever
        .retrieve(&query("STRAßE"), &RetrieveParams::new(5))
        .await
        .unwrap();
    assert_eq!(ids(&cased), vec!["s-1"], "case must fold");

    let transliterated = retriever
        .retrieve(&query("strasse"), &RetrieveParams::new(5))
        .await
        .unwrap();
    assert!(
        transliterated.is_empty(),
        "`ß` is a letter, not two: got {:?}",
        ids(&transliterated)
    );
}

/// `RemoveLongFilter(40)` drops any token of 40 bytes or more, on both sides of
/// the call: such a term is neither indexed nor searchable, so a corpus of
/// base64 blobs or long identifiers retrieves nothing. The limit is in bytes of
/// UTF-8, not in characters.
#[tokio::test]
async fn a_token_of_forty_bytes_or_more_is_dropped() {
    let too_long = "a".repeat(40);
    let just_short_enough = "b".repeat(39);
    let retriever = Bm25Retriever::new(vec![chunk(
        "t-1",
        "doc",
        &format!("{too_long} {just_short_enough}"),
    )])
    .expect("the corpus must index");

    let dropped = retriever
        .retrieve(&query(&too_long), &RetrieveParams::new(5))
        .await
        .unwrap();
    assert!(
        dropped.is_empty(),
        "a 40-byte token is filtered out of both the corpus and the query"
    );

    let kept = retriever
        .retrieve(&query(&just_short_enough), &RetrieveParams::new(5))
        .await
        .unwrap();
    assert_eq!(ids(&kept), vec!["t-1"], "39 bytes is under the limit");
}
