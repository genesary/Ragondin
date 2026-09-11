//! # ragondin-stub
//!
//! **Deterministic stub components**: a [`Retriever`](ragondin_contracts::Retriever)
//! whose ranked list is fabricated from its configuration, and a
//! [`Fusion`](ragondin_contracts::Fusion) that interleaves the lists it is
//! given. Neither reads a corpus, an index or a model, so a pipeline built out
//! of them runs anywhere, in the same time, with the same answer.
//!
//! # What it is for
//!
//! It is the **test fixture** the end-to-end paths are wired with: the vertical
//! slice in `bin/ragondin/tests/`, and the evaluation harness (#29) and
//! `ragondin bench` (#31) that build on it. Its value is that it makes the
//! *wiring* observable — a configuration file on disk, a registration on an
//! `EngineContext`, a plan, an execution trace — with nothing in the components
//! themselves that could vary between two runs.
//!
//! **Nothing here says anything about retrieval.** A stub retriever answers
//! every query identically and its scores encode rank and nothing else, so no
//! measurement taken over these components means anything about quality. They
//! are conformant, which is a floor and not an endorsement.
//!
//! # Two things this crate is not evidence for
//!
//! - **Crate granularity.** It holds two components of two different families,
//!   and `docs/OPEN_QUESTIONS.md` #4 — one crate per family, or one per
//!   implementation — is **deliberately unresolved**. One crate holding several
//!   trivial stubs is a fixture kept in one place, not a position on how
//!   production components should be split. Do not cite it as a precedent.
//! - **Registry ergonomics.** `docs/OPEN_QUESTIONS.md` #1 is also open, and the
//!   composition root registers these stubs explicitly and verbosely, which is
//!   the valid interim. This crate reaches no registry of its own.
//!
//! It is a **leaf** of the dependency graph (INV-5): it compiles against
//! `ragondin-contracts` and `ragondin-types` and nothing else in the workspace,
//! exactly as a third-party component would (INV-7). The binary is what
//! constructs it and registers it on an `EngineContext`.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

#[cfg(feature = "stub")]
mod fusion;
#[cfg(feature = "stub")]
mod retriever;

#[cfg(feature = "stub")]
pub use fusion::StubFusion;
#[cfg(feature = "stub")]
pub use retriever::StubRetriever;
