//! # ragondin-reranker-onnx
//!
//! An in-process **cross-encoder reranker**, over ONNX Runtime. A cross-encoder
//! scores a `(query, chunk)` pair jointly rather than comparing two vectors, so
//! it reads the pair the way a reader would — and running one without a Python
//! sidecar is one of the reasons this platform is written in Rust at all
//! (`docs/system-architecture.md` §10).
//!
//! A `Local` component and a **leaf** of the dependency graph (INV-5): it
//! depends on `ragondin-contracts` — the [`Reranker`](ragondin_contracts::Reranker)
//! trait it implements — and on `ragondin-types`, and on nothing else in the
//! workspace. Its whole surface is that trait and the constructor below: it has
//! no entry point into the engine that a crate outside this workspace could not
//! also call, which is what "no privilege for built-ins" (INV-7) asks of it.
//! Putting one on an `EngineContext` is the binary's job, as the composition
//! root (`docs/code-architecture.md` §8.1), and happens nowhere in this crate.
//!
//! # The `onnx` feature
//!
//! ONNX Runtime is a heavy backend, so it is optional and confined here
//! (ADR-C14). **The feature is off by default**: `cargo build --workspace`
//! compiles none of it, which is what the lean default build means for a
//! workspace member. With the feature off this crate exports nothing at all —
//! a cross-encoder reranker without an inference runtime would be a different
//! component, not a degraded one.
//!
//! ```text
//! cargo test -p ragondin-reranker-onnx --features onnx   # or: just test-features
//! ```
//!
//! # Building one
//!
//! The model and its tokenizer are named by path and read at construction:
//!
//! ```no_run
//! # #[cfg(feature = "onnx")] {
//! use ragondin_contracts::{RerankParams, Reranker};
//! use ragondin_reranker_onnx::{OnnxReranker, OnnxRerankerConfig};
//! use ragondin_types::{Chunk, ChunkId, DocId, Query, QueryId, ScoredChunk};
//!
//! let reranker = OnnxReranker::new(OnnxRerankerConfig::new(
//!     "cross-encoder/model.onnx",
//!     "cross-encoder/tokenizer.json",
//! ))?;
//!
//! let query = Query { id: QueryId::new("q-1"), text: "where do ragondins live".to_string() };
//! let candidates = vec![ScoredChunk {
//!     chunk: Chunk {
//!         id: ChunkId::new("c-1"),
//!         text: "ragondins burrow into river banks".to_string(),
//!         document_id: DocId::new("d-1"),
//!     },
//!     score: 0.3,
//! }];
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let reranked = reranker.rerank(&query, candidates, &RerankParams::new(8)).await.unwrap();
//! # });
//! # }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Where the candidates came from is not this crate's business: a reranker
//! reorders what it is handed and has no corpus of its own.
//!
//! # It does not block the thread that called it
//!
//! A forward pass is CPU-bound, and a `Local` component moves that work off the
//! caller's thread itself (ADR-C25). This one does it with
//! `tokio::task::spawn_blocking`, so **it requires its caller to be running
//! under a `tokio` runtime** — a property of this component and not of the
//! contract, which names no runtime. `ARCHITECTURE.md` argues the choice.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

#[cfg(feature = "onnx")]
mod reranker;

#[cfg(feature = "onnx")]
pub use reranker::{ModelError, OnnxReranker, OnnxRerankerConfig};
