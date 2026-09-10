//! # ragondin-embedder-onnx
//!
//! In-process **embeddings via ONNX Runtime**. A sentence-embedding model is
//! loaded from a file and run inside this process, with no Python sidecar
//! (`docs/system-architecture.md` §10) — which is one of the reasons this
//! platform is written in Rust at all.
//!
//! A `Local` component and a **leaf** of the dependency graph (INV-5): it
//! depends on `ragondin-contracts` — the [`Embedder`](ragondin_contracts::Embedder)
//! trait it implements — and on `ragondin-types`, and on nothing else in the
//! workspace. Its whole surface is that trait and the constructor below: it has
//! no entry point into the engine that a crate outside this workspace could not
//! also call, which is what "no privilege for built-ins" (INV-7) asks of it.
//! Putting one on an `EngineContext` is the binary's job, as the composition
//! root (`docs/code-architecture.md` §8.1), and happens nowhere in this crate.
//!
//! # The `onnx` feature
//!
//! ONNX Runtime and the tokenizer are heavy backends, so they are optional and
//! confined here (ADR-C14). **The feature is off by default**: `cargo build
//! --workspace` compiles none of it, which is what the lean default build means
//! for a workspace member. With the feature off this crate exports nothing at
//! all — an ONNX embedder without ONNX Runtime would be a different component,
//! not a degraded one.
//!
//! ```text
//! cargo test -p ragondin-embedder-onnx --features onnx   # or: just test-features
//! ```
//!
//! # Building one
//!
//! Two paths — a model and a tokenizer — plus the per-role prefixes the model
//! wants. Both paths are configuration, read at construction; nothing is
//! fetched, here or at build time.
//!
//! ```
//! # #[cfg(feature = "onnx")] {
//! use ragondin_contracts::{EmbedParams, EmbedRole, Embedder};
//! use ragondin_embedder_onnx::{OnnxEmbedder, OnnxEmbedderConfig};
//!
//! let embedder = OnnxEmbedder::new(
//!     OnnxEmbedderConfig::new("tests/fixtures/tiny-embedder.onnx", "tests/fixtures/tokenizer.json")
//!         .with_prefixes("query: ", "passage: "),
//! )?;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let texts = vec!["the cat sat on the mat".to_string()];
//! let vectors = embedder
//!     .embed(&texts, &EmbedParams::new(EmbedRole::Passage))
//!     .await
//!     .unwrap();
//!
//! assert_eq!(vectors.len(), texts.len());
//! # });
//! # }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The [`EmbedRole`](ragondin_contracts::EmbedRole) is the caller's to pass and
//! selects which prefix is prepended (ADR-C17). Which prefix a given model
//! wants is not this crate's knowledge either: `"query: "` / `"passage: "` is
//! E5's, a symmetric model takes neither, and the configuration is where that
//! is said.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

#[cfg(feature = "onnx")]
mod embedder;

#[cfg(feature = "onnx")]
pub use embedder::{EmbedderError, OnnxEmbedder, OnnxEmbedderConfig};
