//! # ragondin-proto
//!
//! The wire layer: protobuf messages and gRPC service stubs. It carries two
//! things behind one crate boundary:
//!
//! - the **component services** (`Reranker`, `Retriever`, …) that a `Remote`
//!   component implements — the protobuf mirror of the `ragondin-contracts` traits;
//! - the **configuration-delivery service** (the [`config`] module): a
//!   purpose-built, versioned gRPC service with ACK/NACK.
//!
//! The domain types in `ragondin-types` are the **source of truth** (INV-9); the
//! protobuf here is generated to mirror them, and `ragondin-remote` supplies the
//! conversions. The wire format is deliberately **separate** from the in-memory
//! representation and versioned independently.
//!
//! Message and service definitions land in a later issue; this is the compiling
//! skeleton.

/// The configuration-delivery gRPC service (versioned, ACK/NACK).
///
/// A module rather than a separate crate: config delivery shares the wire
/// layer's `tonic`/`prost` toolchain and its versioning discipline, so it lives
/// beside the component services rather than in a crate of its own.
pub mod config {
    // Configuration-delivery service stubs land in a later issue.
}

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(
            name.starts_with("ragondin-"),
            "unexpected crate name: {name}"
        );
    }
}
