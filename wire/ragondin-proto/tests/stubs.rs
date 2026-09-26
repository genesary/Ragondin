//! The gRPC stubs are generated: one server trait and one client per service.
//!
//! The server half is proved by implementing every generated trait. Each
//! method must name its request and response types, so an rpc whose messages
//! do not pair the way the Rust trait's arguments and return do stops this
//! file compiling. The client half is proved by naming each generated client's
//! constructor over a `tonic` channel.

use ragondin_proto::config;
use ragondin_proto::v1;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

/// A service that implements every rpc and answers none of them.
struct Unimplemented;

#[tonic::async_trait]
impl v1::retriever_server::Retriever for Unimplemented {
    async fn retrieve(
        &self,
        _: Request<v1::RetrieveRequest>,
    ) -> Result<Response<v1::RetrieveResponse>, Status> {
        Err(Status::unimplemented("retrieve"))
    }
}

#[tonic::async_trait]
impl v1::fusion_server::Fusion for Unimplemented {
    async fn fuse(
        &self,
        _: Request<v1::FuseRequest>,
    ) -> Result<Response<v1::FuseResponse>, Status> {
        Err(Status::unimplemented("fuse"))
    }
}

#[tonic::async_trait]
impl v1::reranker_server::Reranker for Unimplemented {
    async fn rerank(
        &self,
        _: Request<v1::RerankRequest>,
    ) -> Result<Response<v1::RerankResponse>, Status> {
        Err(Status::unimplemented("rerank"))
    }

    async fn get_model_identity(
        &self,
        _: Request<v1::RerankerModelIdentityRequest>,
    ) -> Result<Response<v1::RerankerModelIdentityResponse>, Status> {
        Err(Status::unimplemented("get_model_identity"))
    }
}

#[tonic::async_trait]
impl v1::embedder_server::Embedder for Unimplemented {
    async fn embed(
        &self,
        _: Request<v1::EmbedRequest>,
    ) -> Result<Response<v1::EmbedResponse>, Status> {
        Err(Status::unimplemented("embed"))
    }

    async fn get_model_identity(
        &self,
        _: Request<v1::EmbedderModelIdentityRequest>,
    ) -> Result<Response<v1::EmbedderModelIdentityResponse>, Status> {
        Err(Status::unimplemented("get_model_identity"))
    }
}

#[tonic::async_trait]
impl v1::vector_store_server::VectorStore for Unimplemented {
    async fn upsert(
        &self,
        _: Request<v1::UpsertRequest>,
    ) -> Result<Response<v1::UpsertResponse>, Status> {
        Err(Status::unimplemented("upsert"))
    }

    async fn search(
        &self,
        _: Request<v1::SearchRequest>,
    ) -> Result<Response<v1::SearchResponse>, Status> {
        Err(Status::unimplemented("search"))
    }
}

#[tonic::async_trait]
impl v1::context_builder_server::ContextBuilder for Unimplemented {
    async fn build(
        &self,
        _: Request<v1::BuildRequest>,
    ) -> Result<Response<v1::BuildResponse>, Status> {
        Err(Status::unimplemented("build"))
    }

    async fn get_model_identity(
        &self,
        _: Request<v1::ContextBuilderModelIdentityRequest>,
    ) -> Result<Response<v1::ContextBuilderModelIdentityResponse>, Status> {
        Err(Status::unimplemented("get_model_identity"))
    }
}

#[tonic::async_trait]
impl v1::generator_server::Generator for Unimplemented {
    async fn generate(
        &self,
        _: Request<v1::GenerateRequest>,
    ) -> Result<Response<v1::GenerateResponse>, Status> {
        Err(Status::unimplemented("generate"))
    }

    async fn get_model_identity(
        &self,
        _: Request<v1::GeneratorModelIdentityRequest>,
    ) -> Result<Response<v1::GeneratorModelIdentityResponse>, Status> {
        Err(Status::unimplemented("get_model_identity"))
    }
}

/// The configuration-delivery service is a reserved stub with no rpc yet, so
/// its trait has nothing to implement.
#[tonic::async_trait]
impl config::v1::config_delivery_server::ConfigDelivery for Unimplemented {}

#[test]
fn every_service_has_a_generated_server() {
    let _ = v1::retriever_server::RetrieverServer::new(Unimplemented);
    let _ = v1::fusion_server::FusionServer::new(Unimplemented);
    let _ = v1::reranker_server::RerankerServer::new(Unimplemented);
    let _ = v1::embedder_server::EmbedderServer::new(Unimplemented);
    let _ = v1::vector_store_server::VectorStoreServer::new(Unimplemented);
    let _ = v1::context_builder_server::ContextBuilderServer::new(Unimplemented);
    let _ = v1::generator_server::GeneratorServer::new(Unimplemented);
    let _ = config::v1::config_delivery_server::ConfigDeliveryServer::new(Unimplemented);
}

#[test]
fn every_service_has_a_generated_client() {
    let _: fn(Channel) -> v1::retriever_client::RetrieverClient<Channel> =
        v1::retriever_client::RetrieverClient::new;
    let _: fn(Channel) -> v1::fusion_client::FusionClient<Channel> =
        v1::fusion_client::FusionClient::new;
    let _: fn(Channel) -> v1::reranker_client::RerankerClient<Channel> =
        v1::reranker_client::RerankerClient::new;
    let _: fn(Channel) -> v1::embedder_client::EmbedderClient<Channel> =
        v1::embedder_client::EmbedderClient::new;
    let _: fn(Channel) -> v1::vector_store_client::VectorStoreClient<Channel> =
        v1::vector_store_client::VectorStoreClient::new;
    let _: fn(Channel) -> v1::context_builder_client::ContextBuilderClient<Channel> =
        v1::context_builder_client::ContextBuilderClient::new;
    let _: fn(Channel) -> v1::generator_client::GeneratorClient<Channel> =
        v1::generator_client::GeneratorClient::new;
    let _: fn(Channel) -> config::v1::config_delivery_client::ConfigDeliveryClient<Channel> =
        config::v1::config_delivery_client::ConfigDeliveryClient::new;
}
