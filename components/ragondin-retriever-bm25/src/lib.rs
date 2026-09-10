//! # ragondin-retriever-bm25
//!
//! In-process **BM25 sparse retrieval**, over [`tantivy`]. Sparse retrieval
//! without an Elasticsearch sidecar is one of the reasons this platform is
//! written in Rust at all (`docs/system-architecture.md` §10).
//!
//! A `Local` component and a **leaf** of the dependency graph (INV-5): it
//! depends on `ragondin-contracts` — the [`Retriever`](ragondin_contracts::Retriever)
//! trait it implements — and on `ragondin-types`, and on nothing else in the
//! workspace. Its whole surface is that trait and the constructor below: it has
//! no entry point into the engine that a crate outside this workspace could not
//! also call, which is what "no privilege for built-ins" (INV-7) asks of it.
//! Putting one on an `EngineContext` is the binary's job, as the composition
//! root (`docs/code-architecture.md` §8.1), and happens nowhere in this crate.
//!
//! # The `bm25` feature
//!
//! `tantivy` is a heavy backend, so it is optional and confined here
//! (ADR-C14). **The feature is off by default**: `cargo build --workspace`
//! compiles none of it, which is what the lean default build means for a
//! workspace member. With the feature off this crate exports nothing at all —
//! a BM25 retriever without tantivy would be a different component, not a
//! degraded one.
//!
//! ```text
//! cargo test -p ragondin-retriever-bm25 --features bm25   # or: just test-features
//! ```
//!
//! # Building one
//!
//! The retriever is constructed over the chunks it will search, and the index
//! is built once, in memory, at construction:
//!
//! ```
//! # #[cfg(feature = "bm25")] {
//! use ragondin_contracts::{RetrieveParams, Retriever};
//! use ragondin_retriever_bm25::Bm25Retriever;
//! use ragondin_types::{Chunk, ChunkId, DocId, Query, QueryId};
//!
//! let retriever = Bm25Retriever::new(vec![Chunk {
//!     id: ChunkId::new("c-1"),
//!     text: "the cat sat on the mat".to_string(),
//!     document_id: DocId::new("d-1"),
//! }])?;
//!
//! let query = Query { id: QueryId::new("q-1"), text: "cat".to_string() };
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let hits = retriever.retrieve(&query, &RetrieveParams::new(5)).await.unwrap();
//! assert_eq!(hits[0].chunk.id.as_str(), "c-1");
//! # });
//! # }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Where the corpus itself comes from is not this crate's business: fetching
//! and chunking a corpus belongs to the harness and to the `Chunker` family.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

#[cfg(feature = "bm25")]
mod retriever;

#[cfg(feature = "bm25")]
pub use retriever::{Bm25Retriever, IndexError};
