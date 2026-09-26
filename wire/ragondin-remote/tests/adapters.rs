//! What each adapter does beyond the conformance suite: it is a trait object
//! like any `Local` component, it connects nothing until called, it refuses an
//! absent or empty `served_model` before sending, it applies the embedding
//! prefixes, it batches the two unbounded calls within its message limit, and
//! it maps every failure through ADR-C35's one conversion. The generator's
//! adapter also refuses an empty template before sending, sends each optional
//! with its presence, and sends the template unrendered.

mod support;

use std::error::Error as _;

use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use ragondin_contracts::{
    ComponentError, ContextBuilder, ContextParams, EmbedParams, EmbedRole, EmbeddedChunk, Embedder,
    Fusion, FusionParams, GenerateParams, Generator, RerankParams, Reranker, RetrieveParams,
    Retriever, SearchParams, VectorStore,
};
use ragondin_proto::v1::{
    self, context_builder_server, embedder_client::EmbedderClient, embedder_server,
    generator_server, reranker_server, retriever_server,
};
use ragondin_remote::{
    DecodeError, IntoProto, RemoteContextBuilder, RemoteEmbedder, RemoteFusion, RemoteGenerator,
    RemoteReranker, RemoteRetriever, RemoteVectorStore, EMBED_BATCH, UPSERT_BATCH,
};
use ragondin_stub::{StubContextBuilder, StubGenerator};
use ragondin_types::{
    Context, ContextChunk, Embedding, ModelIdentity, Query, QueryId, ScoredChunk,
};
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

// --- the generation adapters (ADR-C31 § 1–§ 2, § 4) ----------------------------

/// A template the stub generator places the context with, so its answer is
/// the context's first line.
const TEMPLATE: &str = "{context}\n\n{query}";

fn context() -> Context {
    Context {
        chunks: vec![ContextChunk {
            id: chunk("a").id,
            document_id: chunk("a").document_id,
            score: 0.5,
        }],
        text: "the first line\nthe second".into(),
    }
}

fn generate_params() -> GenerateParams {
    GenerateParams::new(SERVED_MODEL, TEMPLATE)
}

#[tokio::test]
async fn each_generation_adapter_is_its_family_trait_object() {
    let builder: Box<dyn ContextBuilder> = Box::new(RemoteContextBuilder::new(
        support::serve_context_builder(StubContextBuilder),
    ));
    let built = builder
        .build(
            &query(),
            vec![ScoredChunk {
                chunk: chunk("a"),
                score: 0.5,
            }],
            &ContextParams::new(4),
        )
        .await
        .unwrap();
    assert_eq!(built.chunks[0].id.as_str(), "a");
    assert_eq!(built.chunks[0].score, 0.5);
    assert_eq!(
        builder.model_identity().await.unwrap().as_str(),
        StubContextBuilder::IDENTITY
    );

    let generator: Box<dyn Generator> = Box::new(RemoteGenerator::new(support::serve_generator(
        StubGenerator::new(SERVED_MODEL),
    )));
    let answer = generator
        .generate(&query(), &context(), &generate_params())
        .await
        .unwrap();
    assert_eq!(answer.text, "the first line");
    assert_eq!(
        generator
            .model_identity(SERVED_MODEL)
            .await
            .unwrap()
            .as_str(),
        StubGenerator::IDENTITY
    );
}

#[tokio::test]
async fn an_unreachable_generation_service_is_unavailable() {
    let generator = RemoteGenerator::new(support::unreachable_channel());
    let error = generator
        .generate(&query(), &context(), &generate_params())
        .await
        .unwrap_err();
    assert!(matches!(error, ComponentError::Unavailable(_)), "{error:?}");
    let error = generator.model_identity(SERVED_MODEL).await.unwrap_err();
    assert!(matches!(error, ComponentError::Unavailable(_)), "{error:?}");

    let builder = RemoteContextBuilder::new(support::unreachable_channel());
    let error = builder
        .build(&query(), vec![], &ContextParams::new(1))
        .await
        .unwrap_err();
    assert!(matches!(error, ComponentError::Unavailable(_)), "{error:?}");
    let error = builder.model_identity().await.unwrap_err();
    assert!(matches!(error, ComponentError::Unavailable(_)), "{error:?}");
}

/// ADR-C31 § 2: the adapter refuses an empty `served_model` or template
/// before it sends. Over a channel to nothing, a call that was sent is
/// `Unavailable`, so `InvalidRequest` proves nothing was.
#[tokio::test]
async fn an_empty_served_model_or_template_is_refused_before_sending() {
    let generator = RemoteGenerator::new(support::unreachable_channel());
    for params in [
        GenerateParams::new("", TEMPLATE),
        GenerateParams::new(SERVED_MODEL, ""),
    ] {
        let error = generator
            .generate(&query(), &context(), &params)
            .await
            .unwrap_err();
        assert!(is_invalid_request(&error), "{params:?}: {error:?}");
    }
    let error = generator.model_identity("").await.unwrap_err();
    assert!(is_invalid_request(&error), "{error:?}");
}

/// A generator service that answers every call with the status its test
/// scripts, or, when none is scripted, with an OK answer; it records every
/// request it receives, as received.
#[derive(Clone, Default)]
struct Scripted {
    status: Option<(Code, &'static str)>,
    received: Arc<Mutex<Vec<v1::GenerateRequest>>>,
}

impl Scripted {
    fn failing(code: Code) -> Self {
        Self {
            status: Some((code, "scripted")),
            ..Self::default()
        }
    }

    #[allow(clippy::result_large_err)]
    fn outcome<T>(&self, ok: T) -> Result<Response<T>, Status> {
        match self.status {
            Some((code, message)) => Err(Status::new(code, message)),
            None => Ok(Response::new(ok)),
        }
    }
}

#[tonic::async_trait]
impl generator_server::Generator for Scripted {
    async fn generate(
        &self,
        request: Request<v1::GenerateRequest>,
    ) -> Result<Response<v1::GenerateResponse>, Status> {
        self.received.lock().unwrap().push(request.into_inner());
        self.outcome(v1::GenerateResponse {
            answer: Some(v1::Answer {
                text: "scripted".into(),
            }),
        })
    }

    async fn get_model_identity(
        &self,
        _: Request<v1::GeneratorModelIdentityRequest>,
    ) -> Result<Response<v1::GeneratorModelIdentityResponse>, Status> {
        self.outcome(ModelIdentity::new("scripted@rev").into_proto())
    }
}

#[tonic::async_trait]
impl context_builder_server::ContextBuilder for Scripted {
    async fn build(
        &self,
        _: Request<v1::BuildRequest>,
    ) -> Result<Response<v1::BuildResponse>, Status> {
        self.outcome(
            Context {
                chunks: vec![],
                text: String::new(),
            }
            .into_proto(),
        )
    }

    async fn get_model_identity(
        &self,
        _: Request<v1::ContextBuilderModelIdentityRequest>,
    ) -> Result<Response<v1::ContextBuilderModelIdentityResponse>, Status> {
        self.outcome(ModelIdentity::new("scripted@rev").into_proto())
    }
}

fn scripted_generator(service: Scripted) -> RemoteGenerator {
    RemoteGenerator::new(support::serve(
        Server::builder().add_service(generator_server::GeneratorServer::new(service)),
    ))
}

fn scripted_builder(service: Scripted) -> RemoteContextBuilder {
    RemoteContextBuilder::new(support::serve(
        Server::builder().add_service(context_builder_server::ContextBuilderServer::new(service)),
    ))
}

/// ADR-C35 § 2, one status class at a time, on every call of both generation
/// adapters: `INVALID_ARGUMENT` is `InvalidRequest`; `UNAVAILABLE`,
/// `DEADLINE_EXCEEDED` and `CANCELLED` are `Unavailable`; `INTERNAL` and any
/// other code are `Backend`, keeping the `Status` as the source.
#[tokio::test]
async fn each_status_class_maps_to_its_variant_on_the_generation_adapters() {
    type Kept = fn(&ComponentError, Code) -> bool;
    let classes: [(&[Code], Kept); 3] = [
        (
            &[Code::InvalidArgument],
            |e, _| matches!(e, ComponentError::InvalidRequest(m) if m.contains("scripted")),
        ),
        (
            &[Code::Unavailable, Code::DeadlineExceeded, Code::Cancelled],
            |e, _| matches!(e, ComponentError::Unavailable(m) if m.contains("scripted")),
        ),
        (
            &[Code::Internal, Code::NotFound, Code::ResourceExhausted],
            |e, code| {
                matches!(e, ComponentError::Backend(_))
                    && e.source()
                        .and_then(|s| s.downcast_ref::<Status>())
                        .is_some_and(|s| s.code() == code && s.message() == "scripted")
            },
        ),
    ];
    for (codes, kept) in classes {
        for &code in codes {
            let generator = scripted_generator(Scripted::failing(code));
            let errors = [
                generator
                    .generate(&query(), &context(), &generate_params())
                    .await
                    .unwrap_err(),
                generator.model_identity(SERVED_MODEL).await.unwrap_err(),
            ];
            let builder = scripted_builder(Scripted::failing(code));
            let errors = errors.into_iter().chain([
                builder
                    .build(&query(), vec![], &ContextParams::new(1))
                    .await
                    .unwrap_err(),
                builder.model_identity().await.unwrap_err(),
            ]);
            for error in errors {
                assert!(kept(&error, code), "{code:?} came back as {error:?}");
            }
        }
    }
}

/// ADR-C31 § 2: an optional left `None` is omitted from the request, never
/// sent as a zero, and one set to zero is sent as zero. The template goes
/// out exactly as the params hold it, and the query and the context
/// separately: the service renders, the adapter does not.
#[tokio::test]
async fn the_generator_adapter_sends_presence_and_the_template_unrendered() {
    let service = Scripted::default();
    let received = service.received.clone();
    let generator = scripted_generator(service);

    let absent = GenerateParams::new(SERVED_MODEL, "{{literal}} {query} {context}");
    let zeros = absent
        .clone()
        .with_temperature(0.0)
        .with_seed(0)
        .with_max_tokens(0);
    let set = absent
        .clone()
        .with_temperature(0.7)
        .with_seed(42)
        .with_max_tokens(256);
    for params in [&absent, &zeros, &set] {
        let answer = generator
            .generate(&query(), &context(), params)
            .await
            .unwrap();
        assert_eq!(answer.text, "scripted");
    }

    let received = received.lock().unwrap();
    let sent: Vec<_> = received
        .iter()
        .map(|r| {
            let p = r.params.as_ref().unwrap();
            (p.temperature, p.seed, p.max_tokens)
        })
        .collect();
    assert_eq!(
        sent,
        [
            (None, None, None),
            (Some(0.0), Some(0), Some(0)),
            (Some(0.7), Some(42), Some(256)),
        ]
    );
    for request in received.iter() {
        let params = request.params.as_ref().unwrap();
        assert_eq!(params.template, "{{literal}} {query} {context}");
        assert_eq!(params.served_model, SERVED_MODEL);
        assert_eq!(request.query.as_ref().unwrap().text, query().text);
        assert_eq!(request.context.as_ref().unwrap().text, context().text);
    }
}

/// A malformed template is the service's refusal, since only the service
/// renders: it arrives as `INVALID_ARGUMENT` and comes back `InvalidRequest`.
/// So does a served model the service does not serve, from either call.
#[tokio::test]
async fn the_services_refusals_arrive_as_invalid_request() {
    let generator =
        RemoteGenerator::new(support::serve_generator(StubGenerator::new(SERVED_MODEL)));
    let error = generator
        .generate(
            &query(),
            &context(),
            &GenerateParams::new(SERVED_MODEL, "{unknown}"),
        )
        .await
        .unwrap_err();
    assert!(is_invalid_request(&error), "{error:?}");

    let error = generator
        .generate(
            &query(),
            &context(),
            &GenerateParams::new("other", TEMPLATE),
        )
        .await
        .unwrap_err();
    assert!(is_invalid_request(&error), "{error:?}");
    let error = generator.model_identity("other").await.unwrap_err();
    assert!(is_invalid_request(&error), "{error:?}");

    let builder = RemoteContextBuilder::new(support::serve_context_builder(StubContextBuilder));
    let error = builder
        .build(&query(), vec![], &ContextParams::new(0))
        .await
        .unwrap_err();
    assert!(is_invalid_request(&error), "{error:?}");
}

/// A generation service that answers OK with what the domain cannot
/// represent: no answer, no context, an empty identity.
struct MalformedGeneration;

#[tonic::async_trait]
impl generator_server::Generator for MalformedGeneration {
    async fn generate(
        &self,
        _: Request<v1::GenerateRequest>,
    ) -> Result<Response<v1::GenerateResponse>, Status> {
        Ok(Response::new(v1::GenerateResponse { answer: None }))
    }

    async fn get_model_identity(
        &self,
        _: Request<v1::GeneratorModelIdentityRequest>,
    ) -> Result<Response<v1::GeneratorModelIdentityResponse>, Status> {
        Ok(Response::new(ModelIdentity::new("").into_proto()))
    }
}

#[tonic::async_trait]
impl context_builder_server::ContextBuilder for MalformedGeneration {
    async fn build(
        &self,
        _: Request<v1::BuildRequest>,
    ) -> Result<Response<v1::BuildResponse>, Status> {
        Ok(Response::new(v1::BuildResponse { context: None }))
    }

    async fn get_model_identity(
        &self,
        _: Request<v1::ContextBuilderModelIdentityRequest>,
    ) -> Result<Response<v1::ContextBuilderModelIdentityResponse>, Status> {
        Ok(Response::new(ModelIdentity::new("").into_proto()))
    }
}

/// ADR-C35 § 2: an OK response that does not convert is `Backend`, with the
/// `DecodeError` as its source; ADR-C31 § 1: an empty identity is
/// `InvalidRequest`, through the same shared function as the M2 adapters.
#[tokio::test]
async fn a_malformed_generation_response_is_backend_and_an_empty_identity_invalid_request() {
    let generator = RemoteGenerator::new(support::serve(
        Server::builder().add_service(generator_server::GeneratorServer::new(MalformedGeneration)),
    ));
    let error = generator
        .generate(&query(), &context(), &generate_params())
        .await
        .unwrap_err();
    assert!(
        matches!(
            decode_error(&error),
            Some(DecodeError::Missing {
                message: "GenerateResponse",
                field: "answer"
            })
        ),
        "{error:?}"
    );
    let error = generator.model_identity(SERVED_MODEL).await.unwrap_err();
    assert!(is_invalid_request(&error), "{error:?}");

    let builder = RemoteContextBuilder::new(support::serve(Server::builder().add_service(
        context_builder_server::ContextBuilderServer::new(MalformedGeneration),
    )));
    let error = builder
        .build(&query(), vec![], &ContextParams::new(1))
        .await
        .unwrap_err();
    assert!(
        matches!(
            decode_error(&error),
            Some(DecodeError::Missing {
                message: "BuildResponse",
                field: "context"
            })
        ),
        "{error:?}"
    );
    let error = builder.model_identity().await.unwrap_err();
    assert!(is_invalid_request(&error), "{error:?}");
}
