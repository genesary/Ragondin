//! # ragondin-context-concat
//!
//! **Ordered concatenation under a character budget** — a `Local`
//! implementation of the [`ContextBuilder`](ragondin_contracts::ContextBuilder)
//! contract, and the baseline every other prompt-construction strategy is
//! measured against.
//!
//! It is a **leaf** of the dependency graph (INV-5): it compiles against
//! `ragondin-contracts` and `ragondin-types` and nothing else in the workspace,
//! exactly as a third-party component would (INV-7). The binary is what
//! constructs it and registers it on an `EngineContext`; this crate reaches no
//! registry of its own.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

#[cfg(feature = "concat")]
mod concat;

#[cfg(feature = "concat")]
pub use concat::ConcatContextBuilder;
