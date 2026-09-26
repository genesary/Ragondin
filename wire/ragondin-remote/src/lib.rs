//! # ragondin-remote
//!
//! Face 2 of the two-faced contract (ADR-3), seen from the engine's side: a
//! `Remote` component is a gRPC service in any language, and this crate makes
//! it indistinguishable from a `Local` one by implementing the
//! `ragondin-contracts` trait and delegating over the wire:
//!
//! ```text
//! domain → protobuf → gRPC → protobuf → domain
//! ```
//!
//! The engine only ever sees `Box<dyn Trait>`, and never knows whether the
//! work runs in-process or across the network.
//!
//! Three pieces:
//!
//! - **The conversions** between each domain value and its generated message,
//!   [`IntoProto`] (total) and [`FromProto`] (refusing, with a
//!   [`DecodeError`], what the domain cannot represent). The domain types are
//!   the source of truth (ADR-C24), and round-trip tests keep the two faces in
//!   step.
//! - **The status conversion** of ADR-C35, in both directions:
//!   [`status_from_error`] and [`status_from_request`] for a service written
//!   in Rust, [`error_from_status`], [`error_from_response`] and
//!   [`error_from_identity_response`] for an adapter. Every adapter maps a
//!   failure through these and nothing else.
//! - **The adapters** for the five M2 families — [`RemoteRetriever`],
//!   [`RemoteFusion`], [`RemoteReranker`], [`RemoteEmbedder`] and
//!   [`RemoteVectorStore`] — and for the two generation families,
//!   [`RemoteContextBuilder`] and [`RemoteGenerator`], each over a `tonic`
//!   channel its caller builds.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

mod adapters;
mod convert;
mod status;

pub use adapters::{
    RemoteContextBuilder, RemoteEmbedder, RemoteFusion, RemoteGenerator, RemoteReranker,
    RemoteRetriever, RemoteVectorStore, EMBED_BATCH, MAX_MESSAGE_SIZE, UPSERT_BATCH,
};
pub use convert::{DecodeError, FromProto, IntoProto};
pub use status::{
    error_from_identity_response, error_from_response, error_from_status, status_from_error,
    status_from_request,
};
