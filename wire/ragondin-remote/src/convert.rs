//! The conversions between the domain values and their generated messages
//! (ADR-C24).
//!
//! Two traits, one per direction, because the two directions are not alike:
//! every domain value has a message, so [`IntoProto`] is total, and the wire
//! admits messages the domain has no value for, so [`FromProto`] returns a
//! [`DecodeError`]. They are this crate's own traits rather than `From` and
//! `TryFrom` because both sides are foreign here: the domain types live in
//! `ragondin-types` and `ragondin-contracts`, the messages in
//! `ragondin-proto`, and the orphan rule allows a `From` between two foreign
//! types in neither crate.
//!
//! Beside the values, a trait method's arguments convert as one tuple to its
//! request message, and its return value to its response message, so that the
//! adapter and a Rust-hosted service convert a whole call with one call each.
//!
//! What [`FromProto`] refuses, and nothing else:
//!
//! - a message-typed field a request or value requires, left out
//!   (`types.proto`'s header states the rule);
//! - an `EmbedRole` of `EMBED_ROLE_UNSPECIFIED`, or a number the enum does not
//!   name (ADR-C17);
//! - an empty `ModelIdentity` (ADR-C31 § 1);
//! - an empty `served_model`: absent is a domain value, empty is not
//!   (ADR-C32 § 4);
//! - a count wider than `usize`, which a truncation would silently change.
//!
//! It refuses no value the domain can represent and a component refuses by
//! its own contract: a `top_k` of zero, a non-finite score, an empty
//! embedding. Those reach the component, whose refusal is the conformance
//! suite's to check.

use ragondin_contracts::{
    EmbedParams, EmbedRole, EmbeddedChunk, FusionParams, RerankParams, RetrieveParams, SearchParams,
};
use ragondin_proto::v1;
use ragondin_types::{
    Chunk, ChunkId, DocId, Embedding, ModelIdentity, Query, QueryId, ScoredChunk,
};
use thiserror::Error;

/// Converts a domain value into the message `P` that carries it. Total.
pub trait IntoProto<P> {
    /// The message carrying `self`.
    fn into_proto(self) -> P;
}

/// Converts a message `P` into the domain value it carries, refusing a
/// message the domain has no value for.
pub trait FromProto<P>: Sized {
    /// The domain value `proto` carries.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] when `proto` carries what the domain cannot represent.
    fn from_proto(proto: P) -> Result<Self, DecodeError>;
}

/// A message the domain has no value for.
///
/// Each variant names the message and the field at fault, as the `.proto`
/// spells them. Which `ComponentError` it becomes is decided by who decoded
/// it (ADR-C35): a service refuses the request with
/// [`status_from_request`](crate::status_from_request), and an adapter
/// refuses the response with [`error_from_response`](crate::error_from_response).
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeError {
    /// A message-typed field that is required was left out.
    #[error("{message}.{field} is required and was left out")]
    Missing {
        /// The message holding the field.
        message: &'static str,
        /// The field left out.
        field: &'static str,
    },
    /// An enum field holds its reserved zero, which is never valid.
    #[error("{message}.{field} is unspecified, which is never valid")]
    Unspecified {
        /// The message holding the field.
        message: &'static str,
        /// The enum field.
        field: &'static str,
    },
    /// An enum field holds a number the enum does not name.
    #[error("{message}.{field} is {value}, which the enum does not name")]
    Unknown {
        /// The message holding the field.
        message: &'static str,
        /// The enum field.
        field: &'static str,
        /// The number received.
        value: i32,
    },
    /// A string that must not be empty is.
    #[error("{message}.{field} is empty, which is not valid")]
    Empty {
        /// The message holding the field.
        message: &'static str,
        /// The string field.
        field: &'static str,
    },
    /// A count does not fit in this target's `usize`.
    #[error("{message}.{field} is {value}, which does not fit in usize")]
    TooLarge {
        /// The message holding the field.
        message: &'static str,
        /// The count field.
        field: &'static str,
        /// The number received.
        value: u64,
    },
}

fn required<T>(
    field: Option<T>,
    message: &'static str,
    name: &'static str,
) -> Result<T, DecodeError> {
    field.ok_or(DecodeError::Missing {
        message,
        field: name,
    })
}

fn count(value: u64, message: &'static str, field: &'static str) -> Result<usize, DecodeError> {
    usize::try_from(value).map_err(|_| DecodeError::TooLarge {
        message,
        field,
        value,
    })
}

/// A `served_model` as the wire carries it: absent is `None`, empty is
/// refused.
fn served_model(
    value: Option<String>,
    message: &'static str,
) -> Result<Option<String>, DecodeError> {
    match value {
        Some(name) if name.is_empty() => Err(DecodeError::Empty {
            message,
            field: "served_model",
        }),
        other => Ok(other),
    }
}

// `usize` to `u64` is lossless on every target Rust supports.
fn wire_count(value: usize) -> u64 {
    value as u64
}

// --- values ---------------------------------------------------------------

impl IntoProto<v1::Query> for Query {
    fn into_proto(self) -> v1::Query {
        v1::Query {
            id: self.id.as_str().to_owned(),
            text: self.text,
        }
    }
}

impl FromProto<v1::Query> for Query {
    fn from_proto(proto: v1::Query) -> Result<Self, DecodeError> {
        Ok(Query {
            id: QueryId::new(proto.id),
            text: proto.text,
        })
    }
}

impl IntoProto<v1::Chunk> for Chunk {
    fn into_proto(self) -> v1::Chunk {
        v1::Chunk {
            id: self.id.as_str().to_owned(),
            text: self.text,
            document_id: self.document_id.as_str().to_owned(),
        }
    }
}

impl FromProto<v1::Chunk> for Chunk {
    fn from_proto(proto: v1::Chunk) -> Result<Self, DecodeError> {
        Ok(Chunk {
            id: ChunkId::new(proto.id),
            text: proto.text,
            document_id: DocId::new(proto.document_id),
        })
    }
}

impl IntoProto<v1::ScoredChunk> for ScoredChunk {
    fn into_proto(self) -> v1::ScoredChunk {
        v1::ScoredChunk {
            chunk: Some(self.chunk.into_proto()),
            score: self.score,
        }
    }
}

impl FromProto<v1::ScoredChunk> for ScoredChunk {
    fn from_proto(proto: v1::ScoredChunk) -> Result<Self, DecodeError> {
        Ok(ScoredChunk {
            chunk: Chunk::from_proto(required(proto.chunk, "ScoredChunk", "chunk")?)?,
            score: proto.score,
        })
    }
}

fn scored_chunks_into(chunks: Vec<ScoredChunk>) -> Vec<v1::ScoredChunk> {
    chunks.into_iter().map(IntoProto::into_proto).collect()
}

fn scored_chunks_from(chunks: Vec<v1::ScoredChunk>) -> Result<Vec<ScoredChunk>, DecodeError> {
    chunks.into_iter().map(ScoredChunk::from_proto).collect()
}

impl IntoProto<v1::ScoredChunkList> for Vec<ScoredChunk> {
    fn into_proto(self) -> v1::ScoredChunkList {
        v1::ScoredChunkList {
            chunks: scored_chunks_into(self),
        }
    }
}

impl FromProto<v1::ScoredChunkList> for Vec<ScoredChunk> {
    fn from_proto(proto: v1::ScoredChunkList) -> Result<Self, DecodeError> {
        scored_chunks_from(proto.chunks)
    }
}

impl IntoProto<v1::Embedding> for Embedding {
    fn into_proto(self) -> v1::Embedding {
        v1::Embedding {
            components: self.as_slice().to_vec(),
        }
    }
}

impl FromProto<v1::Embedding> for Embedding {
    fn from_proto(proto: v1::Embedding) -> Result<Self, DecodeError> {
        Ok(Embedding::new(proto.components))
    }
}

impl IntoProto<v1::EmbeddedChunk> for EmbeddedChunk {
    fn into_proto(self) -> v1::EmbeddedChunk {
        v1::EmbeddedChunk {
            chunk: Some(self.chunk.into_proto()),
            embedding: Some(self.embedding.into_proto()),
        }
    }
}

impl FromProto<v1::EmbeddedChunk> for EmbeddedChunk {
    fn from_proto(proto: v1::EmbeddedChunk) -> Result<Self, DecodeError> {
        Ok(EmbeddedChunk {
            chunk: Chunk::from_proto(required(proto.chunk, "EmbeddedChunk", "chunk")?)?,
            embedding: Embedding::from_proto(required(
                proto.embedding,
                "EmbeddedChunk",
                "embedding",
            )?)?,
        })
    }
}

impl IntoProto<v1::ModelIdentity> for ModelIdentity {
    fn into_proto(self) -> v1::ModelIdentity {
        v1::ModelIdentity {
            identity: self.as_str().to_owned(),
        }
    }
}

impl FromProto<v1::ModelIdentity> for ModelIdentity {
    fn from_proto(proto: v1::ModelIdentity) -> Result<Self, DecodeError> {
        if proto.identity.is_empty() {
            return Err(DecodeError::Empty {
                message: "ModelIdentity",
                field: "identity",
            });
        }
        Ok(ModelIdentity::new(proto.identity))
    }
}

impl IntoProto<v1::EmbedRole> for EmbedRole {
    fn into_proto(self) -> v1::EmbedRole {
        match self {
            EmbedRole::Query => v1::EmbedRole::Query,
            EmbedRole::Passage => v1::EmbedRole::Passage,
        }
    }
}

/// The role as a message field carries it: an `i32`, open on the wire where
/// the Rust enum is closed (ADR-C17).
fn embed_role(number: i32, message: &'static str) -> Result<EmbedRole, DecodeError> {
    match v1::EmbedRole::try_from(number) {
        Ok(v1::EmbedRole::Query) => Ok(EmbedRole::Query),
        Ok(v1::EmbedRole::Passage) => Ok(EmbedRole::Passage),
        Ok(v1::EmbedRole::Unspecified) => Err(DecodeError::Unspecified {
            message,
            field: "role",
        }),
        Err(_) => Err(DecodeError::Unknown {
            message,
            field: "role",
            value: number,
        }),
    }
}

// --- params -----------------------------------------------------------------

impl IntoProto<v1::RetrieveParams> for RetrieveParams {
    fn into_proto(self) -> v1::RetrieveParams {
        v1::RetrieveParams {
            top_k: wire_count(self.top_k),
        }
    }
}

impl FromProto<v1::RetrieveParams> for RetrieveParams {
    fn from_proto(proto: v1::RetrieveParams) -> Result<Self, DecodeError> {
        Ok(RetrieveParams::new(count(
            proto.top_k,
            "RetrieveParams",
            "top_k",
        )?))
    }
}

impl IntoProto<v1::FusionParams> for FusionParams {
    fn into_proto(self) -> v1::FusionParams {
        v1::FusionParams {}
    }
}

impl FromProto<v1::FusionParams> for FusionParams {
    fn from_proto(_proto: v1::FusionParams) -> Result<Self, DecodeError> {
        Ok(FusionParams::new())
    }
}

impl IntoProto<v1::RerankParams> for RerankParams {
    fn into_proto(self) -> v1::RerankParams {
        v1::RerankParams {
            top_k: wire_count(self.top_k),
            served_model: self.served_model,
        }
    }
}

impl FromProto<v1::RerankParams> for RerankParams {
    fn from_proto(proto: v1::RerankParams) -> Result<Self, DecodeError> {
        let params = RerankParams::new(count(proto.top_k, "RerankParams", "top_k")?);
        Ok(match served_model(proto.served_model, "RerankParams")? {
            Some(name) => params.with_served_model(name),
            None => params,
        })
    }
}

impl IntoProto<v1::EmbedParams> for EmbedParams {
    fn into_proto(self) -> v1::EmbedParams {
        v1::EmbedParams {
            role: IntoProto::<v1::EmbedRole>::into_proto(self.role) as i32,
            served_model: self.served_model,
        }
    }
}

impl FromProto<v1::EmbedParams> for EmbedParams {
    fn from_proto(proto: v1::EmbedParams) -> Result<Self, DecodeError> {
        let params = EmbedParams::new(embed_role(proto.role, "EmbedParams")?);
        Ok(match served_model(proto.served_model, "EmbedParams")? {
            Some(name) => params.with_served_model(name),
            None => params,
        })
    }
}

impl IntoProto<v1::SearchParams> for SearchParams {
    fn into_proto(self) -> v1::SearchParams {
        v1::SearchParams {
            top_k: wire_count(self.top_k),
        }
    }
}

impl FromProto<v1::SearchParams> for SearchParams {
    fn from_proto(proto: v1::SearchParams) -> Result<Self, DecodeError> {
        Ok(SearchParams::new(count(
            proto.top_k,
            "SearchParams",
            "top_k",
        )?))
    }
}

// --- requests: a trait method's arguments, in the method's order -------------

impl IntoProto<v1::RetrieveRequest> for (Query, RetrieveParams) {
    fn into_proto(self) -> v1::RetrieveRequest {
        let (query, params) = self;
        v1::RetrieveRequest {
            query: Some(query.into_proto()),
            params: Some(params.into_proto()),
        }
    }
}

impl FromProto<v1::RetrieveRequest> for (Query, RetrieveParams) {
    fn from_proto(proto: v1::RetrieveRequest) -> Result<Self, DecodeError> {
        const M: &str = "RetrieveRequest";
        Ok((
            Query::from_proto(required(proto.query, M, "query")?)?,
            RetrieveParams::from_proto(required(proto.params, M, "params")?)?,
        ))
    }
}

impl IntoProto<v1::FuseRequest> for (Vec<Vec<ScoredChunk>>, FusionParams) {
    fn into_proto(self) -> v1::FuseRequest {
        let (inputs, params) = self;
        v1::FuseRequest {
            inputs: inputs.into_iter().map(IntoProto::into_proto).collect(),
            params: Some(params.into_proto()),
        }
    }
}

impl FromProto<v1::FuseRequest> for (Vec<Vec<ScoredChunk>>, FusionParams) {
    fn from_proto(proto: v1::FuseRequest) -> Result<Self, DecodeError> {
        Ok((
            proto
                .inputs
                .into_iter()
                .map(Vec::<ScoredChunk>::from_proto)
                .collect::<Result<_, _>>()?,
            FusionParams::from_proto(required(proto.params, "FuseRequest", "params")?)?,
        ))
    }
}

impl IntoProto<v1::RerankRequest> for (Query, Vec<ScoredChunk>, RerankParams) {
    fn into_proto(self) -> v1::RerankRequest {
        let (query, chunks, params) = self;
        v1::RerankRequest {
            query: Some(query.into_proto()),
            chunks: scored_chunks_into(chunks),
            params: Some(params.into_proto()),
        }
    }
}

impl FromProto<v1::RerankRequest> for (Query, Vec<ScoredChunk>, RerankParams) {
    fn from_proto(proto: v1::RerankRequest) -> Result<Self, DecodeError> {
        const M: &str = "RerankRequest";
        Ok((
            Query::from_proto(required(proto.query, M, "query")?)?,
            scored_chunks_from(proto.chunks)?,
            RerankParams::from_proto(required(proto.params, M, "params")?)?,
        ))
    }
}

impl IntoProto<v1::EmbedRequest> for (Vec<String>, EmbedParams) {
    fn into_proto(self) -> v1::EmbedRequest {
        let (texts, params) = self;
        v1::EmbedRequest {
            texts,
            params: Some(params.into_proto()),
        }
    }
}

impl FromProto<v1::EmbedRequest> for (Vec<String>, EmbedParams) {
    fn from_proto(proto: v1::EmbedRequest) -> Result<Self, DecodeError> {
        Ok((
            proto.texts,
            EmbedParams::from_proto(required(proto.params, "EmbedRequest", "params")?)?,
        ))
    }
}

impl IntoProto<v1::UpsertRequest> for Vec<EmbeddedChunk> {
    fn into_proto(self) -> v1::UpsertRequest {
        v1::UpsertRequest {
            entries: self.into_iter().map(IntoProto::into_proto).collect(),
        }
    }
}

impl FromProto<v1::UpsertRequest> for Vec<EmbeddedChunk> {
    fn from_proto(proto: v1::UpsertRequest) -> Result<Self, DecodeError> {
        proto
            .entries
            .into_iter()
            .map(EmbeddedChunk::from_proto)
            .collect()
    }
}

impl IntoProto<v1::SearchRequest> for (Embedding, SearchParams) {
    fn into_proto(self) -> v1::SearchRequest {
        let (embedding, params) = self;
        v1::SearchRequest {
            embedding: Some(embedding.into_proto()),
            params: Some(params.into_proto()),
        }
    }
}

impl FromProto<v1::SearchRequest> for (Embedding, SearchParams) {
    fn from_proto(proto: v1::SearchRequest) -> Result<Self, DecodeError> {
        const M: &str = "SearchRequest";
        Ok((
            Embedding::from_proto(required(proto.embedding, M, "embedding")?)?,
            SearchParams::from_proto(required(proto.params, M, "params")?)?,
        ))
    }
}

impl IntoProto<v1::RerankerModelIdentityRequest> for Option<String> {
    fn into_proto(self) -> v1::RerankerModelIdentityRequest {
        v1::RerankerModelIdentityRequest { served_model: self }
    }
}

impl FromProto<v1::RerankerModelIdentityRequest> for Option<String> {
    fn from_proto(proto: v1::RerankerModelIdentityRequest) -> Result<Self, DecodeError> {
        served_model(proto.served_model, "RerankerModelIdentityRequest")
    }
}

impl IntoProto<v1::EmbedderModelIdentityRequest> for Option<String> {
    fn into_proto(self) -> v1::EmbedderModelIdentityRequest {
        v1::EmbedderModelIdentityRequest { served_model: self }
    }
}

impl FromProto<v1::EmbedderModelIdentityRequest> for Option<String> {
    fn from_proto(proto: v1::EmbedderModelIdentityRequest) -> Result<Self, DecodeError> {
        served_model(proto.served_model, "EmbedderModelIdentityRequest")
    }
}

// --- responses: a trait method's return value --------------------------------

/// The four responses that are one ranked list.
macro_rules! ranked_response {
    ($($message:ident),+) => {$(
        impl IntoProto<v1::$message> for Vec<ScoredChunk> {
            fn into_proto(self) -> v1::$message {
                v1::$message {
                    chunks: scored_chunks_into(self),
                }
            }
        }

        impl FromProto<v1::$message> for Vec<ScoredChunk> {
            fn from_proto(proto: v1::$message) -> Result<Self, DecodeError> {
                scored_chunks_from(proto.chunks)
            }
        }
    )+};
}

ranked_response!(
    RetrieveResponse,
    FuseResponse,
    RerankResponse,
    SearchResponse
);

impl IntoProto<v1::EmbedResponse> for Vec<Embedding> {
    fn into_proto(self) -> v1::EmbedResponse {
        v1::EmbedResponse {
            embeddings: self.into_iter().map(IntoProto::into_proto).collect(),
        }
    }
}

impl FromProto<v1::EmbedResponse> for Vec<Embedding> {
    fn from_proto(proto: v1::EmbedResponse) -> Result<Self, DecodeError> {
        proto
            .embeddings
            .into_iter()
            .map(Embedding::from_proto)
            .collect()
    }
}

impl IntoProto<v1::UpsertResponse> for () {
    fn into_proto(self) -> v1::UpsertResponse {
        v1::UpsertResponse {}
    }
}

impl FromProto<v1::UpsertResponse> for () {
    fn from_proto(_proto: v1::UpsertResponse) -> Result<Self, DecodeError> {
        Ok(())
    }
}

/// The two identity responses, each a required `ModelIdentity`.
macro_rules! identity_response {
    ($($message:ident),+) => {$(
        impl IntoProto<v1::$message> for ModelIdentity {
            fn into_proto(self) -> v1::$message {
                v1::$message {
                    identity: Some(self.into_proto()),
                }
            }
        }

        impl FromProto<v1::$message> for ModelIdentity {
            fn from_proto(proto: v1::$message) -> Result<Self, DecodeError> {
                ModelIdentity::from_proto(required(
                    proto.identity,
                    stringify!($message),
                    "identity",
                )?)
            }
        }
    )+};
}

identity_response!(RerankerModelIdentityResponse, EmbedderModelIdentityResponse);
