//! # ragondin-store-memory
//!
//! An **in-memory, brute-force** implementation of the
//! [`VectorStore`](ragondin_contracts::VectorStore) contract: it holds vectors
//! in RAM and answers a search by scanning all of them. Exact, so there is no
//! approximation to make a run irreproducible, and it needs no external
//! service — which is what the M2 bench requires of a store that has to run in
//! plain CI.
//!
//! It is a **leaf** of the dependency graph (INV-5): it compiles against
//! `ragondin-contracts` and `ragondin-types` and nothing else in the workspace,
//! exactly as a third-party component would (INV-7). The binary is what
//! constructs it and registers it on an `EngineContext`; this crate reaches no
//! registry of its own.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

#[cfg(feature = "memory")]
mod store;

#[cfg(feature = "memory")]
pub use store::MemoryVectorStore;
