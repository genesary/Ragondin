//! # ragondin-config
//!
//! Where the data plane's configuration comes from, and how a configuration
//! becomes a pipeline.
//!
//! One module: [`source`], holding the [`ConfigSource`] abstraction
//! (`docs/system-architecture.md` §8.2), its [`LocalFile`] implementation, and
//! the typed [`ConfigError`] a bad configuration produces. §8.2's rule is the
//! reason the abstraction exists at all: **the data plane does not know who
//! configures it.**
//!
//! The load path is `bytes → RawPipeline → validate → LogicalPipeline`, and
//! this crate owns only the first arrow. The wire schema and the validation
//! pass both live in `ragondin-pipeline`; neither is re-implemented here.
//! That is INV-9 in practice — the wire format is **separate** from the
//! in-memory representation and **versioned independently**, so a file lands
//! in the hand-maintained `RawPipeline` and reaches `LogicalPipeline` only
//! through the pass, never through a deserializer pointed at an internal type.
//!
//! `serde_yaml` is used here and nowhere upstream: `ragondin-pipeline` carries
//! no format implementation (INV-4) and does no I/O (INV-3), which is why the
//! crate's `peek_schema_version` is generic over the deserializer and why this
//! crate is the one that supplies it.
//!
//! Not here: the `Stream` configuration source, the controller, and the
//! custom-resource watch. All three are M6, and `docs/OPEN_QUESTIONS.md` #2 —
//! the controller's language — is deliberately unresolved.

#![warn(missing_docs)]

pub mod source;

pub use source::{ConfigError, ConfigSource, LocalFile};
