//! # ragondin-retriever-dense
//!
//! **Dense retrieval through a [`VectorStore`](ragondin_contracts::VectorStore)**:
//! it embeds the query and returns the nearest chunks the store holds. It is
//! the dense arm of hybrid retrieval, and on its own it is the dense-only
//! baseline the M2 exit criterion compares against (`docs/code-architecture.md` §4).
//!
//! It is a **leaf** of the dependency graph (INV-5): it compiles against
//! `ragondin-contracts` and `ragondin-types` and nothing else in the workspace,
//! exactly as a third-party component would (INV-7). In particular it depends
//! on neither an embedder implementation nor a store implementation — it holds
//! both as trait objects, and the binary, as the composition root
//! (`docs/code-architecture.md` §8.1), decides which ones. That is what lets one
//! retriever run against an in-memory store in CI and a real backend in
//! production without a line changing here.
//!
//! It embeds a query and nothing else. Embedding the corpus is the caller's:
//! it happens at bench time, driven by the harness and the binary, and
//! deliberately not as a pipeline — whether indexing is expressible as a
//! pipeline is `docs/OPEN_QUESTIONS.md` question 5, which is open.
//!
//! # The `dense` feature
//!
//! It gates nothing heavy — this component's backends arrive as trait objects —
//! so it is on by default. It exists so that every component crate is entered
//! the same way; ADR-C14 is the rule it follows, and it is followed here where
//! it buys no compile time because uniformity is what makes the pattern
//! guessable.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

#[cfg(feature = "dense")]
mod retriever;

#[cfg(feature = "dense")]
pub use retriever::DenseRetriever;
