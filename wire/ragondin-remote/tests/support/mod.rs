//! An in-process `Remote` service for the adapter tests: a `tonic` server on
//! an ephemeral port, hosting a `Local` component through the inverse
//! conversions — the request decoded by `FromProto`, the component's error
//! mapped by `status_from_error`, its answer encoded by `IntoProto`. This is
//! what a Rust-hosted `Remote` service does (ADR-C35 § 3), so the adapters
//! are tested against the same face a service in another language presents.
//!
//! Each `serve_*` binds its own port and returns a lazily connecting channel
//! to it, as the composition root builds one (ADR-C32 § 3).

#![allow(dead_code)] // each test file uses the part it needs

pub mod stubs;

use std::net::TcpListener as StdListener;

use ragondin_contracts::{
    EmbedParams, EmbeddedChunk, Embedder, Fusion, FusionParams, RerankParams, Reranker,
    RetrieveParams, Retriever, SearchParams, VectorStore,
};
use ragondin_proto::v1::{
    self, embedder_server, fusion_server, reranker_server, retriever_server, vector_store_server,
};
use ragondin_remote::{
    status_from_error, status_from_request, FromProto, IntoProto, MAX_MESSAGE_SIZE,
};
use ragondin_types::{Embedding, ModelIdentity, Query, ScoredChunk};
use tonic::transport::server::{Router, TcpIncoming};
use tonic::transport::{Channel, Endpoint, Server};
use tonic::{Request, Response, Status};

/// Serves `router` on a fresh ephemeral port of the loopback interface and
/// returns a lazy channel to it. Must be called inside a Tokio runtime.
pub fn serve(router: Router) -> Channel {
    let std_listener = StdListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    std_listener.set_nonblocking(true).expect("non-blocking");
    let address = std_listener.local_addr().expect("local address");
    let listener = tokio::net::TcpListener::from_std(std_listener).expect("tokio listener");
    let incoming = TcpIncoming::from_listener(listener, true, None).expect("incoming");
    tokio::spawn(router.serve_with_incoming(incoming));
    lazy_channel(&format!("http://{address}"))
}

/// A lazily connecting channel, as the composition root builds one per
/// binding (ADR-C32 § 3): nothing is connected until the first call.
pub fn lazy_channel(uri: &str) -> Channel {
    Endpoint::from_shared(uri.to_owned())
        .expect("a valid uri")
        .connect_lazy()
}

/// A channel to a port nothing listens on: the port of a listener bound and
/// dropped at once.
pub fn unreachable_channel() -> Channel {
    let address = StdListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
        .local_addr()
        .expect("local address");
    lazy_channel(&format!("http://{address}"))
}

// `Status` is as large as `tonic` makes it, and every generated rpc returns it.
#[allow(clippy::result_large_err)]
fn decode<D: FromProto<P>, P>(request: Request<P>) -> Result<D, Status> {
    D::from_proto(request.into_inner()).map_err(status_from_request)
}

// --- the five services, each hosting a `Local` component --------------------

pub struct RetrieverService(pub Box<dyn Retriever>);

#[tonic::async_trait]
impl retriever_server::Retriever for RetrieverService {
    async fn retrieve(
        &self,
        request: Request<v1::RetrieveRequest>,
    ) -> Result<Response<v1::RetrieveResponse>, Status> {
        let (query, params): (Query, RetrieveParams) = decode(request)?;
        let hits = self
            .0
            .retrieve(&query, &params)
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(hits.into_proto()))
    }
}

pub fn serve_retriever(component: impl Retriever + 'static) -> Channel {
    serve(
        Server::builder().add_service(
            retriever_server::RetrieverServer::new(RetrieverService(Box::new(component)))
                .max_decoding_message_size(MAX_MESSAGE_SIZE)
                .max_encoding_message_size(MAX_MESSAGE_SIZE),
        ),
    )
}

pub struct FusionService(pub Box<dyn Fusion>);

#[tonic::async_trait]
impl fusion_server::Fusion for FusionService {
    async fn fuse(
        &self,
        request: Request<v1::FuseRequest>,
    ) -> Result<Response<v1::FuseResponse>, Status> {
        let (inputs, params): (Vec<Vec<ScoredChunk>>, FusionParams) = decode(request)?;
        let fused = self
            .0
            .fuse(inputs, &params)
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(fused.into_proto()))
    }
}

pub fn serve_fusion(component: impl Fusion + 'static) -> Channel {
    serve(
        Server::builder().add_service(
            fusion_server::FusionServer::new(FusionService(Box::new(component)))
                .max_decoding_message_size(MAX_MESSAGE_SIZE)
                .max_encoding_message_size(MAX_MESSAGE_SIZE),
        ),
    )
}

pub struct RerankerService(pub Box<dyn Reranker>);

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
        let identity: ModelIdentity = self
            .0
            .model_identity(served_model.as_deref())
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(identity.into_proto()))
    }
}

pub fn serve_reranker(component: impl Reranker + 'static) -> Channel {
    serve(
        Server::builder().add_service(
            reranker_server::RerankerServer::new(RerankerService(Box::new(component)))
                .max_decoding_message_size(MAX_MESSAGE_SIZE)
                .max_encoding_message_size(MAX_MESSAGE_SIZE),
        ),
    )
}

pub struct EmbedderService(pub Box<dyn Embedder>);

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
        let identity: ModelIdentity = self
            .0
            .model_identity(served_model.as_deref())
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(identity.into_proto()))
    }
}

pub fn serve_embedder(component: impl Embedder + 'static) -> Channel {
    serve(
        Server::builder().add_service(
            embedder_server::EmbedderServer::new(EmbedderService(Box::new(component)))
                .max_decoding_message_size(MAX_MESSAGE_SIZE)
                .max_encoding_message_size(MAX_MESSAGE_SIZE),
        ),
    )
}

pub struct VectorStoreService(pub Box<dyn VectorStore>);

#[tonic::async_trait]
impl vector_store_server::VectorStore for VectorStoreService {
    async fn upsert(
        &self,
        request: Request<v1::UpsertRequest>,
    ) -> Result<Response<v1::UpsertResponse>, Status> {
        let entries: Vec<EmbeddedChunk> = decode(request)?;
        self.0
            .upsert(entries)
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(().into_proto()))
    }

    async fn search(
        &self,
        request: Request<v1::SearchRequest>,
    ) -> Result<Response<v1::SearchResponse>, Status> {
        let (embedding, params): (Embedding, SearchParams) = decode(request)?;
        let hits = self
            .0
            .search(&embedding, &params)
            .await
            .map_err(|e| status_from_error(&e))?;
        Ok(Response::new(hits.into_proto()))
    }
}

pub fn serve_vector_store(component: impl VectorStore + 'static) -> Channel {
    serve(
        Server::builder().add_service(
            vector_store_server::VectorStoreServer::new(VectorStoreService(Box::new(component)))
                .max_decoding_message_size(MAX_MESSAGE_SIZE)
                .max_encoding_message_size(MAX_MESSAGE_SIZE),
        ),
    )
}
