//! # ragondin-fusion-rrf
//!
//! **Reciprocal Rank Fusion** — a `Local` implementation of the
//! [`Fusion`](ragondin_contracts::Fusion) contract, merging the ranked lists of
//! several retrieval legs into one.
//!
//! It is a **leaf** of the dependency graph (INV-5): it compiles against
//! `ragondin-contracts` and `ragondin-types` and nothing else in the workspace,
//! exactly as a third-party component would (INV-7). The binary is what
//! constructs it and registers it on an `EngineContext`; this crate reaches no
//! registry of its own.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

#[cfg(feature = "rrf")]
mod rrf;

#[cfg(feature = "rrf")]
pub use rrf::{ReciprocalRankFusion, DEFAULT_K};
