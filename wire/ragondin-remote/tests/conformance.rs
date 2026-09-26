//! Each adapter passes its family's conformance suite, against an in-process
//! service hosting a `Local` stub (`support`): an in-test one for the five M2
//! families, and `ragondin-stub`'s for the generator and the context builder.
//!
//! Each stub is checked on its own first, so that a failure of the `Remote`
//! run is the adapter's or the wire's, not the stub's. The `Remote` run then
//! goes through every layer a service in another language would: the
//! adapter's conversion, the bytes on a TCP connection, the service's
//! conversion back, and ADR-C35's status mapping for every refusal the suite
//! provokes.

mod support;

use ragondin_conformance::{
    assert_context_builder_conformance, assert_embedder_conformance, assert_fusion_conformance,
    assert_generator_conformance, assert_reranker_conformance, assert_retriever_conformance,
    assert_vector_store_conformance, RolePrefixes,
};
use ragondin_remote::{
    RemoteContextBuilder, RemoteEmbedder, RemoteFusion, RemoteGenerator, RemoteReranker,
    RemoteRetriever, RemoteVectorStore,
};
use ragondin_stub::{StubContextBuilder, StubGenerator};
use support::stubs::{
    StubEmbedder, StubFusion, StubReranker, StubRetriever, StubStore, SERVED_MODEL,
};

#[tokio::test]
async fn the_remote_retriever_is_conformant() {
    assert_retriever_conformance(|| Box::new(StubRetriever)).await;

    let channel = support::serve_retriever(StubRetriever);
    assert_retriever_conformance(|| Box::new(RemoteRetriever::new(channel.clone()))).await;
}

#[tokio::test]
async fn the_remote_fusion_is_conformant() {
    assert_fusion_conformance(|| Box::new(StubFusion)).await;

    let channel = support::serve_fusion(StubFusion);
    assert_fusion_conformance(|| Box::new(RemoteFusion::new(channel.clone()))).await;
}

#[tokio::test]
async fn the_remote_reranker_is_conformant() {
    assert_reranker_conformance(|| Box::new(StubReranker), Some(SERVED_MODEL)).await;

    let channel = support::serve_reranker(StubReranker);
    assert_reranker_conformance(
        || Box::new(RemoteReranker::new(channel.clone())),
        Some(SERVED_MODEL),
    )
    .await;
}

#[tokio::test]
async fn the_remote_embedder_is_conformant() {
    // The stub applies no prefix: under ADR-C32 § 4 the adapter does, so the
    // stub alone answers both roles alike, and `Distinct` is declared only of
    // the adapter, which is configured with two different prefixes.
    assert_embedder_conformance(
        || Box::new(StubEmbedder::new(4)),
        RolePrefixes::Undeclared,
        Some(SERVED_MODEL),
    )
    .await;

    let channel = support::serve_embedder(StubEmbedder::new(4));
    assert_embedder_conformance(
        || Box::new(RemoteEmbedder::new(channel.clone(), "query: ", "passage: ")),
        RolePrefixes::Distinct,
        Some(SERVED_MODEL),
    )
    .await;
}

#[tokio::test]
async fn the_remote_vector_store_is_conformant() {
    assert_vector_store_conformance(|| Box::new(StubStore::default()), 4).await;

    // A fresh service per store: the suite needs each `make` to return a store
    // it owns and that starts empty.
    assert_vector_store_conformance(
        || {
            Box::new(RemoteVectorStore::new(support::serve_vector_store(
                StubStore::default(),
            )))
        },
        4,
    )
    .await;
}

#[tokio::test]
async fn the_remote_context_builder_is_conformant() {
    assert_context_builder_conformance(|| Box::new(StubContextBuilder)).await;

    let channel = support::serve_context_builder(StubContextBuilder);
    assert_context_builder_conformance(|| Box::new(RemoteContextBuilder::new(channel.clone())))
        .await;
}

/// The suite's own template exercises the whole grammar, and its malformed
/// ones must be refused: the adapter renders nothing, so every one of those
/// refusals is the hosted generator's, made on receipt and carried back as
/// `INVALID_ARGUMENT`. The empty `served_model` and the empty template never
/// leave the adapter.
#[tokio::test]
async fn the_remote_generator_is_conformant() {
    assert_generator_conformance(|| Box::new(StubGenerator::new(SERVED_MODEL)), SERVED_MODEL).await;

    let channel = support::serve_generator(StubGenerator::new(SERVED_MODEL));
    assert_generator_conformance(
        || Box::new(RemoteGenerator::new(channel.clone())),
        SERVED_MODEL,
    )
    .await;
}
