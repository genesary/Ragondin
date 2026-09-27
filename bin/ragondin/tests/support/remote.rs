//! Fake `Remote` services for the tests that bind one with `--remote`: a
//! `tonic` server on an ephemeral loopback port, hosting an in-test component
//! through `ragondin-remote`'s conversions — the request decoded by
//! `FromProto`, the component's error mapped by `status_from_error`, its
//! answer encoded by `IntoProto`. That is what a Rust-hosted `Remote` service
//! does (ADR-C35 § 3), so the composition root is tested against the face a
//! service in any language presents, and against no real inference server.
//!
//! Each service runs on a runtime of its own, so it keeps answering while a
//! test blocks on the spawned binary, and it stops when its [`Service`] is
//! dropped. Shared by `src/wiring.rs`'s tests and `tests/bench.rs`.

#![allow(dead_code)] // each includer uses the part it needs

use std::net::TcpListener as StdListener;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ragondin_contracts::{
    ComponentError, ContextBuilder, ContextParams, EmbedParams, Embedder, GenerateParams,
    Generator, RerankParams, Reranker,
};
use ragondin_proto::v1::{
    self, context_builder_server, embedder_server, generator_server, reranker_server,
};
use ragondin_remote::{status_from_error, status_from_request, FromProto, IntoProto};
use ragondin_stub::{StubContextBuilder, StubGenerator};
use ragondin_types::{Answer, Context, Embedding, ModelIdentity, Query, ScoredChunk};
use tonic::transport::server::{Router, TcpIncoming};
use tonic::transport::Server;
use tonic::{Request, Response, Status};

/// The name the fake embedder serves its model under.
pub const EMBEDDER_MODEL: &str = "bge-small";
/// What the fake embedder reports for [`EMBEDDER_MODEL`].
pub const EMBEDDER_IDENTITY: &str = "bge-small@rev1";
/// The name the fake reranker serves its model under.
pub const RERANKER_MODEL: &str = "ms-marco";
/// What the fake reranker reports for [`RERANKER_MODEL`].
pub const RERANKER_IDENTITY: &str = "ms-marco@rev7";

/// The name the fake generator serves its model under.
pub const GENERATOR_MODEL: &str = "qwen2.5-7b-instruct";
/// What the fake generator reports for [`GENERATOR_MODEL`]: the stub's own.
pub const GENERATOR_IDENTITY: &str = StubGenerator::IDENTITY;
/// What the fake context builder reports: the stub's own.
pub const CONTEXT_BUILDER_IDENTITY: &str = StubContextBuilder::IDENTITY;

/// A running fake service, stopped when dropped.
pub struct Service {
    /// The address to bind, `http://127.0.0.1:<port>`.
    pub uri: String,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Drop for Service {
    fn drop(&mut self) {
        // Without blocking: a `#[tokio::test]` drops its services inside a
        // runtime, where waiting on another one would panic.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

fn serve(router: Router) -> Service {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("a runtime for the fake service");
    let std_listener = StdListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    std_listener.set_nonblocking(true).expect("non-blocking");
    let address = std_listener.local_addr().expect("local address");
    let incoming = {
        let _entered = runtime.enter();
        let listener = tokio::net::TcpListener::from_std(std_listener).expect("tokio listener");
        TcpIncoming::from_listener(listener, true, None).expect("incoming")
    };
    runtime.spawn(router.serve_with_incoming(incoming));
    Service {
        uri: format!("http://{address}"),
        runtime: Some(runtime),
    }
}

/// An address nothing listens on: port 1 of the loopback interface, which is
/// reserved and which no test binds. Fixed rather than a port bound and
/// dropped, which another process could take in between.
pub fn unreachable_uri() -> String {
    "http://127.0.0.1:1".to_owned()
}

// `Status` is as large as `tonic` makes it, and every generated rpc returns it.
#[allow(clippy::result_large_err)]
fn decode<D: FromProto<P>, P>(request: Request<P>) -> Result<D, Status> {
    D::from_proto(request.into_inner()).map_err(status_from_request)
}

/// Refuses every served model but `served`, as a `Remote` service refuses a
/// name it does not serve, and `None` with it (ADR-C32 § 4).
fn serves(served: &str, asked: Option<&str>) -> Result<(), ComponentError> {
    if asked == Some(served) {
        Ok(())
    } else {
        Err(ComponentError::InvalidRequest(format!(
            "model {asked:?} is not served here"
        )))
    }
}

/// An embedder serving [`EMBEDDER_MODEL`], whose vectors are three counts of
/// the text — enough for a store to rank by, and deterministic. It records
/// every text it was sent, so a test can see what the adapter prefixed.
#[derive(Clone, Default)]
pub struct FakeEmbedder {
    texts: Arc<Mutex<Vec<String>>>,
}

impl FakeEmbedder {
    /// Every text embedded so far, in the order it arrived.
    pub fn texts(&self) -> Vec<String> {
        self.texts.lock().expect("the fake's lock").clone()
    }
}

#[async_trait]
impl Embedder for FakeEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        serves(EMBEDDER_MODEL, params.served_model.as_deref())?;
        self.texts
            .lock()
            .expect("the fake's lock")
            .extend(texts.iter().cloned());
        Ok(texts
            .iter()
            .map(|text| {
                let count =
                    |keep: fn(&char) -> bool| 1.0 + text.chars().filter(keep).count() as f32;
                Embedding::new(vec![
                    count(|c| "aeiou".contains(*c)),
                    count(|c| c.is_whitespace()),
                    count(|c| "st".contains(*c)),
                ])
            })
            .collect())
    }

    async fn model_identity(
        &self,
        served_model: Option<&str>,
    ) -> Result<ModelIdentity, ComponentError> {
        serves(EMBEDDER_MODEL, served_model)?;
        Ok(ModelIdentity::new(EMBEDDER_IDENTITY))
    }
}

/// A reranker serving [`RERANKER_MODEL`] that keeps the order it was handed
/// and rescores it descending, as the ranking contract requires.
#[derive(Clone, Default)]
pub struct FakeReranker;

#[async_trait]
impl Reranker for FakeReranker {
    async fn rerank(
        &self,
        _query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        serves(RERANKER_MODEL, params.served_model.as_deref())?;
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest("a top_k of zero".into()));
        }
        Ok(chunks
            .into_iter()
            .take(params.top_k)
            .enumerate()
            .map(|(rank, hit)| ScoredChunk {
                chunk: hit.chunk,
                score: 1.0 / (1.0 + rank as f32),
            })
            .collect())
    }

    async fn model_identity(
        &self,
        served_model: Option<&str>,
    ) -> Result<ModelIdentity, ComponentError> {
        serves(RERANKER_MODEL, served_model)?;
        Ok(ModelIdentity::new(RERANKER_IDENTITY))
    }
}

struct EmbedderService(FakeEmbedder);

#[tonic::async_trait]
impl embedder_server::Embedder for EmbedderService {
    async fn embed(
        &self,
        request: Request<v1::EmbedRequest>,
    ) -> Result<Response<v1::EmbedResponse>, Status> {
        let (texts, params): (Vec<String>, EmbedParams) = decode(request)?;
        let vectors = self
            .0
            .embed(&texts, &params)
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(vectors.into_proto()))
    }

    async fn get_model_identity(
        &self,
        request: Request<v1::EmbedderModelIdentityRequest>,
    ) -> Result<Response<v1::EmbedderModelIdentityResponse>, Status> {
        let served_model: Option<String> = decode(request)?;
        let identity = self
            .0
            .model_identity(served_model.as_deref())
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(identity.into_proto()))
    }
}

struct RerankerService(FakeReranker);

#[tonic::async_trait]
impl reranker_server::Reranker for RerankerService {
    async fn rerank(
        &self,
        request: Request<v1::RerankRequest>,
    ) -> Result<Response<v1::RerankResponse>, Status> {
        let (query, chunks, params): (Query, Vec<ScoredChunk>, RerankParams) = decode(request)?;
        let reranked = self
            .0
            .rerank(&query, chunks, &params)
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(reranked.into_proto()))
    }

    async fn get_model_identity(
        &self,
        request: Request<v1::RerankerModelIdentityRequest>,
    ) -> Result<Response<v1::RerankerModelIdentityResponse>, Status> {
        let served_model: Option<String> = decode(request)?;
        let identity = self
            .0
            .model_identity(served_model.as_deref())
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(identity.into_proto()))
    }
}

/// Serves `embedder`, which the caller keeps a handle on.
pub fn serve_embedder(embedder: FakeEmbedder) -> Service {
    serve(
        Server::builder().add_service(embedder_server::EmbedderServer::new(EmbedderService(
            embedder,
        ))),
    )
}

/// Serves a [`FakeReranker`].
pub fn serve_reranker() -> Service {
    serve(
        Server::builder().add_service(reranker_server::RerankerServer::new(RerankerService(
            FakeReranker,
        ))),
    )
}

/// Hosts `ragondin-stub`'s context builder. It renders nothing of its own
/// beyond what the stub does, as a Rust-hosted service would.
struct ContextBuilderService(StubContextBuilder);

#[tonic::async_trait]
impl context_builder_server::ContextBuilder for ContextBuilderService {
    async fn build(
        &self,
        request: Request<v1::BuildRequest>,
    ) -> Result<Response<v1::BuildResponse>, Status> {
        let (query, chunks, params): (Query, Vec<ScoredChunk>, ContextParams) = decode(request)?;
        let context: Context = self
            .0
            .build(&query, chunks, &params)
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(context.into_proto()))
    }

    async fn get_model_identity(
        &self,
        request: Request<v1::ContextBuilderModelIdentityRequest>,
    ) -> Result<Response<v1::ContextBuilderModelIdentityResponse>, Status> {
        decode::<(), _>(request)?;
        let identity = self
            .0
            .model_identity()
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(identity.into_proto()))
    }
}

/// Hosts `ragondin-stub`'s generator, serving [`GENERATOR_MODEL`]: it answers
/// with the first line of the context the template places, and refuses any
/// other served model (ADR-C31 § 4).
struct GeneratorService(StubGenerator);

#[tonic::async_trait]
impl generator_server::Generator for GeneratorService {
    async fn generate(
        &self,
        request: Request<v1::GenerateRequest>,
    ) -> Result<Response<v1::GenerateResponse>, Status> {
        let (query, context, params): (Query, Context, GenerateParams) = decode(request)?;
        let answer: Answer = self
            .0
            .generate(&query, &context, &params)
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(answer.into_proto()))
    }

    async fn get_model_identity(
        &self,
        request: Request<v1::GeneratorModelIdentityRequest>,
    ) -> Result<Response<v1::GeneratorModelIdentityResponse>, Status> {
        let served_model: String = decode(request)?;
        let identity = self
            .0
            .model_identity(&served_model)
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(identity.into_proto()))
    }
}

/// Serves `ragondin-stub`'s context builder.
pub fn serve_context_builder() -> Service {
    serve(
        Server::builder().add_service(context_builder_server::ContextBuilderServer::new(
            ContextBuilderService(StubContextBuilder),
        )),
    )
}

/// Serves `ragondin-stub`'s generator under [`GENERATOR_MODEL`].
pub fn serve_generator() -> Service {
    serve(
        Server::builder().add_service(generator_server::GeneratorServer::new(GeneratorService(
            StubGenerator::new(GENERATOR_MODEL),
        ))),
    )
}
