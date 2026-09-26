//! The `Remote` adapters: one per M2 family, each implementing the
//! `ragondin-contracts` trait by calling the generated client.
//!
//! Each call is the same four steps: refuse what the adapter refuses before
//! sending, convert the arguments to the request, call, and convert the
//! response back. A failed call is mapped by [`error_from_status`], a
//! response that does not convert by [`error_from_response`], and an identity
//! response by [`error_from_identity_response`]: no adapter maps a status or a
//! refusal itself (ADR-C35 § 4).
//!
//! Every adapter is built over a `tonic` [`Channel`] its caller supplies. The
//! composition root builds it once per binding, connecting lazily
//! (`Endpoint::connect_lazy`, ADR-C32 § 3), so nothing is connected at
//! construction and an unreachable service is reported at the first call, as
//! `ComponentError::Unavailable`.

use async_trait::async_trait;
use ragondin_contracts::{
    ComponentError, EmbedParams, EmbedRole, EmbeddedChunk, Embedder, Fusion, FusionParams,
    RerankParams, Reranker, RetrieveParams, Retriever, SearchParams, VectorStore,
};
use ragondin_proto::v1::{
    embedder_client::EmbedderClient, fusion_client::FusionClient, reranker_client::RerankerClient,
    retriever_client::RetrieverClient, vector_store_client::VectorStoreClient, EmbedRequest,
    EmbedderModelIdentityRequest, FuseRequest, RerankRequest, RerankerModelIdentityRequest,
    RetrieveRequest, SearchRequest, UpsertRequest,
};
use ragondin_types::{Embedding, ModelIdentity, Query, ScoredChunk};
use tonic::transport::Channel;

use crate::{
    error_from_identity_response, error_from_response, error_from_status, FromProto, IntoProto,
};

/// The largest message an adapter sends or accepts, in bytes: 64 MiB.
///
/// `tonic`'s default decode limit is 4 MiB, which one Embed response of 1024
/// vectors of 1024 dimensions already reaches; over it, a call fails as
/// `OUT_OF_RANGE`, which is `Backend`, in the middle of a run. The adapters
/// set it deliberately, and in both directions, together with the batching of
/// [`EMBED_BATCH`] and [`UPSERT_BATCH`]: a batch of the widest embeddings in
/// common use stays several times below it. A `Remote` service accepts
/// requests of this size, and answers within it. See `ARCHITECTURE.md`.
pub const MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

/// At most this many texts go in one Embed rpc; a larger call is split, in
/// order, and its vectors are concatenated in the same order.
///
/// A corpus is embedded in one `embed` call, so without a batch no message
/// limit would hold. 256 vectors of 16 384 `f32` components are 16 MiB,
/// a quarter of [`MAX_MESSAGE_SIZE`].
pub const EMBED_BATCH: usize = 256;

/// At most this many entries go in one Upsert rpc; a larger call is split, in
/// order.
///
/// An upsert replaces by chunk id and its entries are independent, so a split
/// call leaves the store as one call would. It is not atomic: if a later batch
/// fails, the earlier ones stay written, as they would under a retry.
pub const UPSERT_BATCH: usize = 256;

/// `items` in slices of at most `size`, in order, and always at least one:
/// an empty call still reaches the service, which may refuse its parameters
/// exactly as a `Local` component would (ADR-C19 makes the empty call valid,
/// not exempt).
fn batches<T>(items: &[T], size: usize) -> impl Iterator<Item = &[T]> {
    let count = items.len().div_ceil(size).max(1);
    (0..count).map(move |i| &items[i * size..items.len().min((i + 1) * size)])
}

/// The served model a `Remote` embedder or reranker is asked for, which it
/// requires: a service has no loaded model that `None` could name, and the
/// adapter refuses `None` before sending, as it refuses an empty name
/// (ADR-C32 § 4).
fn served_model(served_model: Option<&str>) -> Result<&str, ComponentError> {
    match served_model {
        None => Err(ComponentError::InvalidRequest(
            "a Remote component serves its models by name, and served_model is absent".into(),
        )),
        Some("") => Err(ComponentError::InvalidRequest(
            "served_model is empty".into(),
        )),
        Some(name) => Ok(name),
    }
}

macro_rules! client {
    ($client:ident, $channel:expr) => {
        $client::new($channel)
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE)
    };
}

/// A [`Retriever`] served over gRPC.
#[derive(Clone, Debug)]
pub struct RemoteRetriever {
    client: RetrieverClient<Channel>,
}

impl RemoteRetriever {
    /// An adapter over `channel`. Connects nothing.
    pub fn new(channel: Channel) -> Self {
        Self {
            client: client!(RetrieverClient, channel),
        }
    }
}

#[async_trait]
impl Retriever for RemoteRetriever {
    async fn retrieve(
        &self,
        query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let request: RetrieveRequest = (query.clone(), params.clone()).into_proto();
        let response = self
            .client
            .clone()
            .retrieve(request)
            .await
            .map_err(error_from_status)?;
        Vec::<ScoredChunk>::from_proto(response.into_inner()).map_err(error_from_response)
    }
}

/// A [`Fusion`] served over gRPC.
#[derive(Clone, Debug)]
pub struct RemoteFusion {
    client: FusionClient<Channel>,
}

impl RemoteFusion {
    /// An adapter over `channel`. Connects nothing.
    pub fn new(channel: Channel) -> Self {
        Self {
            client: client!(FusionClient, channel),
        }
    }
}

#[async_trait]
impl Fusion for RemoteFusion {
    async fn fuse(
        &self,
        inputs: Vec<Vec<ScoredChunk>>,
        params: &FusionParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let request: FuseRequest = (inputs, params.clone()).into_proto();
        let response = self
            .client
            .clone()
            .fuse(request)
            .await
            .map_err(error_from_status)?;
        Vec::<ScoredChunk>::from_proto(response.into_inner()).map_err(error_from_response)
    }
}

/// A [`Reranker`] served over gRPC.
///
/// Every call must name its served model: the adapter refuses `None` and the
/// empty name as an invalid request before sending (ADR-C32 § 4).
#[derive(Clone, Debug)]
pub struct RemoteReranker {
    client: RerankerClient<Channel>,
}

impl RemoteReranker {
    /// An adapter over `channel`. Connects nothing.
    pub fn new(channel: Channel) -> Self {
        Self {
            client: client!(RerankerClient, channel),
        }
    }
}

#[async_trait]
impl Reranker for RemoteReranker {
    async fn rerank(
        &self,
        query: &Query,
        chunks: Vec<ScoredChunk>,
        params: &RerankParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        served_model(params.served_model.as_deref())?;
        let request: RerankRequest = (query.clone(), chunks, params.clone()).into_proto();
        let response = self
            .client
            .clone()
            .rerank(request)
            .await
            .map_err(error_from_status)?;
        Vec::<ScoredChunk>::from_proto(response.into_inner()).map_err(error_from_response)
    }

    /// What the service reports for `served_model`. An empty identity is
    /// refused as `InvalidRequest` (ADR-C31 § 1), through
    /// [`error_from_identity_response`].
    async fn model_identity(
        &self,
        served_model: Option<&str>,
    ) -> Result<ModelIdentity, ComponentError> {
        let name = self::served_model(served_model)?;
        let request: RerankerModelIdentityRequest = Some(name.to_owned()).into_proto();
        let response = self
            .client
            .clone()
            .get_model_identity(request)
            .await
            .map_err(error_from_status)?;
        ModelIdentity::from_proto(response.into_inner()).map_err(error_from_identity_response)
    }
}

/// An [`Embedder`] served over gRPC.
///
/// **The adapter applies the prefixes** (ADR-C32 § 4): `query_prefix` before
/// each text of an [`EmbedRole::Query`] call, `passage_prefix` before each
/// text of an [`EmbedRole::Passage`] call. The text on the Embed rpc is final,
/// and the service must not prefix it again. The role is still sent, for what
/// the service may use it for that is not text.
///
/// Every call must name its served model: the adapter refuses `None` and the
/// empty name as an invalid request before sending (ADR-C32 § 4). A call of
/// more than [`EMBED_BATCH`] texts is sent as several rpcs, and each batch
/// must come back with one vector per text, or the call fails as `Backend`.
#[derive(Clone, Debug)]
pub struct RemoteEmbedder {
    client: EmbedderClient<Channel>,
    query_prefix: String,
    passage_prefix: String,
}

impl RemoteEmbedder {
    /// An adapter over `channel`, prefixing each text by its role. The
    /// prefixes are the `dense` node's `query_prefix` and `passage_prefix`,
    /// which the composition root reads. A node spells "no prefix" only by
    /// leaving the key out, and an empty one is refused (ADR-C32 § 1); the
    /// composition root maps that absence to the empty string here, which
    /// prefixes nothing. Connects nothing.
    pub fn new(
        channel: Channel,
        query_prefix: impl Into<String>,
        passage_prefix: impl Into<String>,
    ) -> Self {
        Self {
            client: client!(EmbedderClient, channel),
            query_prefix: query_prefix.into(),
            passage_prefix: passage_prefix.into(),
        }
    }
}

#[async_trait]
impl Embedder for RemoteEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        params: &EmbedParams,
    ) -> Result<Vec<Embedding>, ComponentError> {
        served_model(params.served_model.as_deref())?;
        // Matched exhaustively: `EmbedRole` is closed so that a role added
        // later stops this compiling rather than taking a default prefix.
        let prefix = match params.role {
            EmbedRole::Query => &self.query_prefix,
            EmbedRole::Passage => &self.passage_prefix,
        };
        let mut client = self.client.clone();
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in batches(texts, EMBED_BATCH) {
            let prefixed = batch.iter().map(|text| format!("{prefix}{text}")).collect();
            let request: EmbedRequest = (prefixed, params.clone()).into_proto();
            let response = client.embed(request).await.map_err(error_from_status)?;
            let answered =
                Vec::<Embedding>::from_proto(response.into_inner()).map_err(error_from_response)?;
            // Checked per batch: a short batch would otherwise shift every
            // later vector onto the wrong text, with no error anywhere. The
            // service broke the family's contract, so `Backend` (ADR-C35 § 2).
            if answered.len() != batch.len() {
                return Err(ComponentError::Backend(
                    format!(
                        "the embedder answered {} texts with {} vectors",
                        batch.len(),
                        answered.len()
                    )
                    .into(),
                ));
            }
            vectors.extend(answered);
        }
        Ok(vectors)
    }

    /// What the service reports for `served_model`. An empty identity is
    /// refused as `InvalidRequest` (ADR-C31 § 1), through
    /// [`error_from_identity_response`].
    async fn model_identity(
        &self,
        served_model: Option<&str>,
    ) -> Result<ModelIdentity, ComponentError> {
        let name = self::served_model(served_model)?;
        let request: EmbedderModelIdentityRequest = Some(name.to_owned()).into_proto();
        let response = self
            .client
            .clone()
            .get_model_identity(request)
            .await
            .map_err(error_from_status)?;
        ModelIdentity::from_proto(response.into_inner()).map_err(error_from_identity_response)
    }
}

/// A [`VectorStore`] served over gRPC.
///
/// Written, and not yet bound by anything: ADR-C32 § 5 defers a `Remote`
/// vector store until the contract can scope a store's content to a run. A
/// call of more than [`UPSERT_BATCH`] entries is sent as several rpcs.
#[derive(Clone, Debug)]
pub struct RemoteVectorStore {
    client: VectorStoreClient<Channel>,
}

impl RemoteVectorStore {
    /// An adapter over `channel`. Connects nothing.
    pub fn new(channel: Channel) -> Self {
        Self {
            client: client!(VectorStoreClient, channel),
        }
    }
}

#[async_trait]
impl VectorStore for RemoteVectorStore {
    async fn upsert(&self, entries: Vec<EmbeddedChunk>) -> Result<(), ComponentError> {
        let mut client = self.client.clone();
        let mut entries = entries.into_iter().peekable();
        // At least one rpc, as `batches` gives the embedder: an empty upsert
        // still reaches the service. The entries are moved, never copied.
        let mut first = true;
        while first || entries.peek().is_some() {
            first = false;
            let batch: Vec<EmbeddedChunk> = entries.by_ref().take(UPSERT_BATCH).collect();
            let request: UpsertRequest = batch.into_proto();
            let response = client.upsert(request).await.map_err(error_from_status)?;
            <()>::from_proto(response.into_inner()).map_err(error_from_response)?;
        }
        Ok(())
    }

    async fn search(
        &self,
        embedding: &Embedding,
        params: &SearchParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let request: SearchRequest = (embedding.clone(), params.clone()).into_proto();
        let response = self
            .client
            .clone()
            .search(request)
            .await
            .map_err(error_from_status)?;
        Vec::<ScoredChunk>::from_proto(response.into_inner()).map_err(error_from_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lengths(items: &[u8], size: usize) -> Vec<usize> {
        batches(items, size).map(<[u8]>::len).collect()
    }

    #[test]
    fn an_empty_call_is_one_empty_batch() {
        assert_eq!(lengths(&[], 3), [0]);
    }

    #[test]
    fn batches_cover_the_items_in_order() {
        assert_eq!(lengths(&[0; 3], 3), [3]);
        assert_eq!(lengths(&[0; 7], 3), [3, 3, 1]);
        let items: Vec<u8> = (0..7).collect();
        let joined: Vec<u8> = batches(&items, 3).flatten().copied().collect();
        assert_eq!(joined, items);
    }
}
