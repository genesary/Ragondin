//! # rag-server
//!
//! The serving driver: `ingress → [Tower stack] → Engine::execute → response`.
//!
//! **Tower governs the network envelope only** (INV-11): timeouts, retries,
//! concurrency limits, load shedding, backpressure, instrumentation. The domain
//! components are **not** `tower::Service`s — a `Retriever` and a `Generator`
//! share neither input nor output, so forcing them into Tower's uniform
//! `Service<Request>` would gain nothing and destroy the legibility of the
//! contracts. Each abstraction stays at its own layer.
//!
//! The Tower stack and the ingress land in a later issue; this is the compiling
//! skeleton. See `ARCHITECTURE.md`.

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("rag-"), "unexpected crate name: {name}");
    }
}
