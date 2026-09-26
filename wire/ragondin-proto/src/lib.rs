//! # ragondin-proto
//!
//! The wire layer: protobuf messages and gRPC service stubs. It carries two
//! things behind one crate boundary:
//!
//! - the **component services** (the [`v1`] module, protobuf package
//!   `ragondin.v1`) that a `Remote` component implements — face 2 of the
//!   two-faced contract (ADR-3), mirroring the `Retriever`, `Fusion`,
//!   `Reranker`, `Embedder` and `VectorStore` traits of `ragondin-contracts`;
//! - the **configuration-delivery service** (the [`config`] module, package
//!   `ragondin.config.v1`): a purpose-built, versioned gRPC service, reserved
//!   here with no rpc yet.
//!
//! The domain types in `ragondin-types` are the **source of truth** (ADR-C24).
//! The `.proto` files under `proto/` are hand-maintained to mirror them field
//! for field, and they are the whole contract a `Remote` author in another
//! language reads. `build.rs` compiles them with `protox` and generates the
//! Rust here with `tonic-build`, with no `protoc` (ADR-C34). The conversions
//! between the two faces are `ragondin-remote`'s, not this crate's. The wire
//! format is versioned by its package, independently of the in-memory types.
//!
//! The mirror covers the traits as they were before the generation work: the
//! `GetModelIdentity` rpcs, the `served_model` fields of the embed and rerank
//! params, and the `ContextBuilder` and `Generator` services are not here yet.
//! #257 adds them; until it does, a `Remote` embedder or reranker cannot be
//! asked for a served model or report its identity over this wire.
//!
//! See `ARCHITECTURE.md`.

/// The component services, protobuf package `ragondin.v1`.
///
/// Each service has a server trait, in `<service>_server`, which a `Remote`
/// component written in Rust implements, and a client, in `<service>_client`,
/// which `ragondin-remote` calls.
// The code is generated, and these allowances are for its shape, not ours:
// every generated rpc returns `Result<_, tonic::Status>`, and `Status` is as
// large as `tonic` makes it.
#[allow(clippy::result_large_err)]
pub mod v1 {
    tonic::include_proto!("ragondin.v1");
}

/// The configuration-delivery gRPC service (versioned, ACK/NACK).
///
/// A module rather than a separate crate: config delivery shares the wire
/// layer's `tonic`/`prost` toolchain and its versioning discipline, so it lives
/// beside the component services rather than in a crate of its own.
pub mod config {
    /// Protobuf package `ragondin.config.v1`: the service is reserved, with
    /// no rpc and no message yet.
    // Generated: a service with no rpc routes every path to the one
    // `unimplemented` arm, which clippy reads as a pointless match.
    #[allow(clippy::match_single_binding)]
    pub mod v1 {
        tonic::include_proto!("ragondin.config.v1");
    }
}
