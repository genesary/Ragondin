//! # rag-remote
//!
//! The generic `Remote<T>` adapters. A `Remote` component is a gRPC service in
//! any language; this crate makes it indistinguishable from a `Local` one to
//! the engine by implementing the `rag-contracts` trait and delegating over the
//! wire:
//!
//! ```text
//! domain → protobuf → gRPC → protobuf → domain
//! ```
//!
//! The engine only ever sees `Box<dyn Trait>` — it never knows whether the work
//! runs in-process or across the network. This is the foundation of the
//! `Local`/`Remote` equivalence enforced by the conformance suite.
//!
//! The adapters and the domain ⇄ protobuf conversions land in a later issue;
//! this is the compiling skeleton.

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("rag-"), "unexpected crate name: {name}");
    }
}
