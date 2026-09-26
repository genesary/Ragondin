//! What each adapter does beyond the conformance suite: it is a trait object
//! like any `Local` component, it connects nothing until called, it refuses an
//! absent or empty `served_model` before sending, it applies the embedding
//! prefixes, it batches the two unbounded calls within its message limit, and
//! it maps every failure through ADR-C35's one conversion.

mod support;

use std::error::Error as _;

use async_trait::async_trait;
use ragondin_contracts::{
    ComponentError, EmbedParams, EmbedRole, EmbeddedChunk, Embedder, Fusion, FusionParams,
    RerankParams, Reranker, RetrieveParams, Retriever, SearchParams, VectorStore,
};
use ragondin_proto::v1::{
    self, embedder_client::EmbedderClient, embedder_server, reranker_server, retriever_server,
};
use ragondin_remote::{
    DecodeError, IntoProto, RemoteEmbedder, RemoteFusion, RemoteReranker, RemoteRetriever,
    RemoteVectorStore, EMBED_BATCH, UPSERT_BATCH,
};
use ragondin_types::{Embedding, ModelIdentity, Query, QueryId, ScoredChunk};
use support::stubs::{chunk, StubEmbedder, StubReranker, StubRetriever, StubStore, SERVED_MODEL};
use tonic::transport::Server;
use tonic::{Code, Request, Response, Status};

fn query() -> Query {
    Query {
        id: QueryId::new("q"),
        text: "why".into(),
    }
}

fn embed_params(role: EmbedRole) -> EmbedParams {
    EmbedParams::new(role).with_served_model(SERVED_MODEL)
}

fn is_invalid_request(error: &ComponentError) -> bool {
    matches!(error, ComponentError::InvalidRequest(_))
}

// --- a trait object like any other (INV-7) ------------------------------------

#[tokio::test]
async fn each_adapter_is_its_family_trait_object() {
    let retriever: Box<dyn Retriever> = Box::new(RemoteRetriever::new(support::serve_retriever(
        StubRetriever,
    )));
    let hits = retriever
        .retrieve(&query(), &RetrieveParams::new(2))
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);

    let fusion: Box<dyn Fusion> = Box::new(RemoteFusion::new(support::serve_fusion(
        support::stubs::StubFusion,
    )));
    assert!(fusion
        .fuse(vec![], &FusionParams::new())
        .await
        .unwrap()
        .is_empty());

    let reranker: Box<dyn Reranker> =
        Box::new(RemoteReranker::new(support::serve_reranker(StubReranker)));
    assert_eq!(
        reranker
            .model_identity(Some(SERVED_MODEL))
            .await
            .unwrap()
            .as_str(),
        support::stubs::IDENTITY
    );

    let embedder: Box<dyn Embedder> = Box::new(RemoteEmbedder::new(
        support::serve_embedder(StubEmbedder::new(3)),
        "",
        "",
    ));
    let vectors = embedder
        .embed(&["a".to_string()], &embed_params(EmbedRole::Query))
        .await
        .unwrap();
    assert_eq!(vectors[0].dim(), 3);

    let store: Box<dyn VectorStore> = Box::new(RemoteVectorStore::new(
        support::serve_vector_store(StubStore::default()),
    ));
    store.upsert(vec![]).await.unwrap();
    assert!(store
        .search(&Embedding::new(vec![1.0]), &SearchParams::new(1))
        .await
        .unwrap()
        .is_empty());
}

// --- lazy connection (ADR-C32 § 3) ------------------------------------------------

#[tokio::test]
async fn an_unreachable_service_is_unavailable_at_the_first_call() {
    // Construction connects nothing, so it cannot fail.
    let retriever = RemoteRetriever::new(support::unreachable_channel());
    let error = retriever
        .retrieve(&query(), &RetrieveParams::new(1))
        .await
        .unwrap_err();
    assert!(matches!(error, ComponentError::Unavailable(_)), "{error:?}");

    let embedder = RemoteEmbedder::new(support::unreachable_channel(), "", "");
    let error = embedder
        .model_identity(Some(SERVED_MODEL))
        .await
        .unwrap_err();
    assert!(matches!(error, ComponentError::Unavailable(_)), "{error:?}");
}

// --- served_model refused before sending (ADR-C32 § 4) ------------------------

/// Over a channel to nothing, a call that was sent is `Unavailable`; one
/// refused before sending is `InvalidRequest`.
#[tokio::test]
async fn an_absent_or_empty_served_model_is_refused_before_sending() {
    let embedder = RemoteEmbedder::new(support::unreachable_channel(), "", "");
    let reranker = RemoteReranker::new(support::unreachable_channel());
    let texts = ["a".to_string()];

    for served_model in [None, Some("")] {
        let params = match served_model {
            None => EmbedParams::new(EmbedRole::Query),
            Some(name) => EmbedParams::new(EmbedRole::Query).with_served_model(name),
        };
        let error = embedder.embed(&texts, &params).await.unwrap_err();
        assert!(
            is_invalid_request(&error),
            "embed {served_model:?}: {error:?}"
        );

        let error = embedder.model_identity(served_model).await.unwrap_err();
        assert!(
            is_invalid_request(&error),
            "embedder identity {served_model:?}: {error:?}"
        );

        let params = match served_model {
            None => RerankParams::new(1),
            Some(name) => RerankParams::new(1).with_served_model(name),
        };
        let error = reranker
            .rerank(&query(), vec![], &params)
            .await
            .unwrap_err();
        assert!(
            is_invalid_request(&error),
            "rerank {served_model:?}: {error:?}"
        );

        let error = reranker.model_identity(served_model).await.unwrap_err();
        assert!(
            is_invalid_request(&error),
            "reranker identity {served_model:?}: {error:?}"
        );
    }
}

#[tokio::test]
async fn a_name_the_service_does_not_serve_is_its_refusal() {
    let reranker = RemoteReranker::new(support::serve_reranker(StubReranker));
    let error = reranker.model_identity(Some("other")).await.unwrap_err();
    assert!(is_invalid_request(&error), "{error:?}");
}

// --- the adapter applies the prefixes (ADR-C32 § 4) ---------------------------

#[tokio::test]
async fn the_embedder_adapter_prefixes_each_text_by_its_role() {
    let stub = StubEmbedder::new(2);
    let received = stub.received.clone();
    let embedder = RemoteEmbedder::new(support::serve_embedder(stub), "query: ", "passage: ");
    let texts = ["one".to_string(), "two".to_string()];

    embedder
        .embed(&texts, &embed_params(EmbedRole::Query))
        .await
        .unwrap();
    embedder
        .embed(&texts, &embed_params(EmbedRole::Passage))
        .await
        .unwrap();

    assert_eq!(
        *received.lock().unwrap(),
        [
            vec!["query: one".to_string(), "query: two".to_string()],
            vec!["passage: one".to_string(), "passage: two".to_string()],
        ]
    );
}

#[tokio::test]
async fn empty_prefixes_send_the_text_unchanged() {
    let stub = StubEmbedder::new(2);
    let received = stub.received.clone();
    let embedder = RemoteEmbedder::new(support::serve_embedder(stub), "", "");
    embedder
        .embed(&["é 🦝".to_string()], &embed_params(EmbedRole::Passage))
        .await
        .unwrap();
    assert_eq!(*received.lock().unwrap(), [vec!["é 🦝".to_string()]]);
}

// --- batching, and the message limit ------------------------------------------

#[tokio::test]
async fn a_large_embed_is_sent_in_batches_and_answered_in_order() {
    let stub = StubEmbedder::new(2);
    let received = stub.received.clone();
    let local = stub.clone();
    let embedder = RemoteEmbedder::new(support::serve_embedder(stub), "", "");
    let texts: Vec<String> = (0..EMBED_BATCH * 2 + 7).map(|i| format!("t{i}")).collect();
    let params = embed_params(EmbedRole::Passage);

    let vectors = embedder.embed(&texts, &params).await.unwrap();

    let sizes: Vec<usize> = received.lock().unwrap().iter().map(Vec::len).collect();
    assert_eq!(sizes, [EMBED_BATCH, EMBED_BATCH, 7]);
    let expected = local.embed(&texts, &params).await.unwrap();
    assert_eq!(
        vectors, expected,
        "the vectors come back in the texts' order"
    );
}

#[tokio::test]
async fn an_empty_embed_still_reaches_the_service() {
    let stub = StubEmbedder::new(2);
    let received = stub.received.clone();
    let embedder = RemoteEmbedder::new(support::serve_embedder(stub), "", "");
    let vectors = embedder
        .embed(&[], &embed_params(EmbedRole::Query))
        .await
        .unwrap();
    assert!(vectors.is_empty());
    assert_eq!(*received.lock().unwrap(), [Vec::<String>::new()]);
}

#[tokio::test]
async fn a_large_upsert_is_sent_in_batches() {
    let stub = StubStore::default();
    let upserts = stub.upserts.clone();
    let store = RemoteVectorStore::new(support::serve_vector_store(stub));
    let entries: Vec<EmbeddedChunk> = (0..UPSERT_BATCH + 3)
        .map(|i| EmbeddedChunk {
            chunk: chunk(&format!("c{i}")),
            embedding: Embedding::new(vec![i as f32]),
        })
        .collect();

    store.upsert(entries).await.unwrap();

    assert_eq!(*upserts.lock().unwrap(), [UPSERT_BATCH, 3]);
    let hits = store
        .search(&Embedding::new(vec![1.0]), &SearchParams::new(1))
        .await
        .unwrap();
    assert_eq!(hits[0].chunk.id.as_str(), format!("c{}", UPSERT_BATCH + 2));
}

/// One batch of wide vectors is 8 MiB: over `tonic`'s default 4 MiB decode
/// limit, which a client left at its defaults hits, and within the adapter's.
#[tokio::test]
async fn a_batch_over_tonics_default_limit_is_received() {
    const DIM: usize = 8192;
    let channel = support::serve_embedder(StubEmbedder::new(DIM));
    let texts: Vec<String> = (0..EMBED_BATCH).map(|i| format!("t{i}")).collect();
    let params = embed_params(EmbedRole::Passage);

    let request: v1::EmbedRequest = (texts.clone(), params.clone()).into_proto();
    let status = EmbedderClient::new(channel.clone())
        .embed(request)
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::OutOfRange, "{status:?}");

    let vectors = RemoteEmbedder::new(channel, "", "")
        .embed(&texts, &params)
        .await
        .unwrap();
    assert_eq!(vectors.len(), EMBED_BATCH);
    assert!(vectors.iter().all(|v| v.dim() == DIM));
}

// --- ADR-C35 over the wire ------------------------------------------------------

/// A retriever that fails every call with the error `make` builds.
struct Failing(fn() -> ComponentError);

#[async_trait]
impl Retriever for Failing {
    async fn retrieve(
        &self,
        _query: &Query,
        _params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        Err((self.0)())
    }
}

/// ADR-C35 § 3: a `Local` component's error, converted to a status by the
/// service and back by the adapter, is the variant it started as.
#[tokio::test]
async fn a_local_error_keeps_its_variant_across_the_wire() {
    type Make = fn() -> ComponentError;
    type Kept = fn(&ComponentError) -> bool;
    let cases: [(Make, Kept); 3] = [
        (
            || ComponentError::InvalidRequest("top_k of zero".into()),
            |e| matches!(e, ComponentError::InvalidRequest(m) if m.contains("top_k of zero")),
        ),
        (
            || ComponentError::Unavailable("index not loaded".into()),
            |e| matches!(e, ComponentError::Unavailable(m) if m.contains("index not loaded")),
        ),
        (
            || ComponentError::Backend("index corrupt".into()),
            |e| {
                let status = e
                    .source()
                    .and_then(|s| s.downcast_ref::<Status>())
                    .expect("Backend's source is the Status");
                matches!(e, ComponentError::Backend(_))
                    && status.code() == Code::Internal
                    && status.message().contains("index corrupt")
            },
        ),
    ];
    for (make, kept) in cases {
        let retriever = RemoteRetriever::new(support::serve_retriever(Failing(make)));
        let error = retriever
            .retrieve(&query(), &RetrieveParams::new(1))
            .await
            .unwrap_err();
        assert!(kept(&error), "{:?} came back as {error:?}", make());
    }
}

/// A service that answers every call OK, with a message the domain cannot
/// represent or that breaks its family's contract: a chunk left out, no
/// vectors for any texts, an empty identity.
struct Malformed;

#[tonic::async_trait]
impl retriever_server::Retriever for Malformed {
    async fn retrieve(
        &self,
        _: Request<v1::RetrieveRequest>,
    ) -> Result<Response<v1::RetrieveResponse>, Status> {
        Ok(Response::new(v1::RetrieveResponse {
            chunks: vec![v1::ScoredChunk {
                chunk: None,
                score: 1.0,
            }],
        }))
    }
}

#[tonic::async_trait]
impl embedder_server::Embedder for Malformed {
    async fn embed(
        &self,
        _: Request<v1::EmbedRequest>,
    ) -> Result<Response<v1::EmbedResponse>, Status> {
        Ok(Response::new(v1::EmbedResponse::default()))
    }

    async fn get_model_identity(
        &self,
        _: Request<v1::EmbedderModelIdentityRequest>,
    ) -> Result<Response<v1::EmbedderModelIdentityResponse>, Status> {
        Ok(Response::new(ModelIdentity::new("").into_proto()))
    }
}

#[tonic::async_trait]
impl reranker_server::Reranker for Malformed {
    async fn rerank(
        &self,
        _: Request<v1::RerankRequest>,
    ) -> Result<Response<v1::RerankResponse>, Status> {
        Ok(Response::new(v1::RerankResponse::default()))
    }

    async fn get_model_identity(
        &self,
        _: Request<v1::RerankerModelIdentityRequest>,
    ) -> Result<Response<v1::RerankerModelIdentityResponse>, Status> {
        Ok(Response::new(ModelIdentity::new("").into_proto()))
    }
}

fn decode_error(error: &ComponentError) -> Option<&DecodeError> {
    match error {
        ComponentError::Backend(source) => source.downcast_ref::<DecodeError>(),
        _ => None,
    }
}

#[tokio::test]
async fn an_ok_response_the_domain_cannot_represent_is_backend() {
    let channel = support::serve(
        Server::builder().add_service(retriever_server::RetrieverServer::new(Malformed)),
    );
    let error = RemoteRetriever::new(channel)
        .retrieve(&query(), &RetrieveParams::new(1))
        .await
        .unwrap_err();
    assert!(
        matches!(
            decode_error(&error),
            Some(DecodeError::Missing {
                message: "ScoredChunk",
                field: "chunk"
            })
        ),
        "{error:?}"
    );
}

/// ADR-C31 § 1: an empty identity is not valid, and the adapter refuses it as
/// an `InvalidRequest`-class failure — its specific rule, which ADR-C35 § 2's
/// general row for a contract-breaking response does not displace. Checked on
/// both model-bearing adapters, which share `error_from_identity_response`.
#[tokio::test]
async fn an_empty_identity_from_the_service_is_refused_as_invalid_request() {
    let channel = support::serve(
        Server::builder().add_service(embedder_server::EmbedderServer::new(Malformed)),
    );
    let error = RemoteEmbedder::new(channel, "", "")
        .model_identity(Some(SERVED_MODEL))
        .await
        .unwrap_err();
    assert!(is_invalid_request(&error), "{error:?}");

    let channel = support::serve(
        Server::builder().add_service(reranker_server::RerankerServer::new(Malformed)),
    );
    let error = RemoteReranker::new(channel)
        .model_identity(Some(SERVED_MODEL))
        .await
        .unwrap_err();
    assert!(is_invalid_request(&error), "{error:?}");
}

/// An embedder answering a batch with the wrong number of vectors breaks its
/// contract: `Backend` (ADR-C35 § 2), before a later batch's vectors could be
/// read against the wrong texts.
#[tokio::test]
async fn a_batch_answered_with_the_wrong_count_is_backend() {
    let channel = support::serve(
        Server::builder().add_service(embedder_server::EmbedderServer::new(Malformed)),
    );
    let error = RemoteEmbedder::new(channel, "", "")
        .embed(&["a".to_string()], &embed_params(EmbedRole::Query))
        .await
        .unwrap_err();
    assert!(matches!(error, ComponentError::Backend(_)), "{error:?}");
}
