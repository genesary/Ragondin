//! [`ConfigSource`], its [`LocalFile`] implementation, and [`ConfigError`].
//!
//! `docs/system-architecture.md` §8.2 gives the abstraction its whole point:
//! **the data plane does not know who configures it.** A binary holds a
//! `ConfigSource` and asks it for a pipeline; whether that pipeline came from a
//! file on disk or from the controller over gRPC is not its concern.
//!
//! Only `LocalFile` exists here. `Stream` — the configuration pushed over the
//! purpose-built gRPC service — is M6, and building it now would mean designing
//! a protocol whose ACK/NACK shape §8.1 has settled only in outline.
//!
//! # The load path
//!
//! ```text
//! bytes on disk → RawPipeline (serde) → validate → LogicalPipeline
//! ```
//!
//! This crate owns the first arrow and nothing else. The wire schema is
//! `ragondin-pipeline`'s [`RawPipeline`], hand-maintained and independently
//! versioned; the pass is that crate's [`validate()`]. Neither is re-implemented
//! or paraphrased here, which is what INV-9 asks for: a file lands in the wire
//! schema and reaches the in-memory model only through the pass, never by a
//! deserializer pointed at an internal type.
//!
//! The version is read before the document is. That is not an optimization —
//! it is the one fault a reader cannot fix by editing the file, and
//! `SchemaVersion`'s own `Deserialize` refuses an unsupported version through
//! `serde::de::Error::custom`, which keeps the wording and erases the type. A
//! plain parse would therefore report *this build is too old* as a syntax
//! error. [`peek_schema_version`] exists for this caller, and reading the
//! version through it is what lets [`ConfigError`] keep the two apart.

use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use ragondin_pipeline::{
    peek_schema_version, validate, LogicalPipeline, RawPipeline, SchemaVersionPeekError,
    UnsupportedSchemaVersion, ValidationError,
};

/// Where a data plane's configuration comes from.
///
/// `docs/system-architecture.md` §8.2 names two implementations: [`LocalFile`],
/// here, and `Stream` — pushed from the controller over the configuration
/// service — which is M6 and does not exist yet.
///
/// The method is **async**, and it is the trait — not [`LocalFile`] — that
/// the signature is for. `Stream` reads a pushed configuration off a gRPC
/// stream, which is async in a way no amount of local cleverness makes
/// synchronous; a trait both implementations satisfy therefore has to be async
/// from the start, or `Stream` arrives as a breaking change to every caller
/// rather than as a second implementation.
///
/// Declared with `async_trait`, which is frozen (`AGENTS.md` § Frozen
/// decisions): not RPITIT, not a native `async fn` in a trait. That is also
/// what keeps this dyn-compatible, and dyn-compatibility is the point — §8.2's
/// rule is that the data plane does not know who configures it, so a binary
/// holds a `Box<dyn ConfigSource>` and never a concrete source.
#[async_trait]
pub trait ConfigSource {
    /// Produces the validated, canonical pipeline this source describes.
    ///
    /// Stops at [`LogicalPipeline`]. Resolving implementations to components
    /// is physical planning, which needs an `EngineContext` and belongs to the
    /// engine — which is exactly why `ragondin validate` can check and
    /// content-address a configuration with no registry at all (ADR-C2).
    async fn load(&self) -> Result<LogicalPipeline, ConfigError>;
}

/// A configuration read from a YAML file on disk — standalone mode (P2).
///
/// §8.3: the file is not a lesser sibling of the Kubernetes custom resource,
/// it **is** the custom resource, modulo the wire format. That is what makes
/// "develop locally, deploy unchanged" true rather than aspirational.
#[derive(Clone, Debug)]
pub struct LocalFile {
    path: PathBuf,
}

impl LocalFile {
    /// Names the file to read. Nothing is opened until [`ConfigSource::load`]
    /// is called, so constructing one cannot fail.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The path this source reads.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl ConfigSource for LocalFile {
    /// # The read is synchronous inside an `async fn`
    ///
    /// `std::fs`, not `tokio::fs`: every library crate in this workspace takes
    /// `async-trait` as a dependency and `tokio` only as a dev-dependency, so
    /// reaching for `tokio::fs` here would make this the one library that
    /// picks the runtime — which `docs/code-architecture.md` §11.2 puts at the
    /// binary level, keeping libraries runtime-agnostic as far as is
    /// practical.
    ///
    /// Whether that is the right general answer is **not settled here**. It is
    /// the open question in #198 — may a `Local` component block the calling
    /// thread inside an `async fn` of a contract, and if not, whose job is the
    /// hop. That issue's subject is the *component* contract and not this one,
    /// so nothing below presupposes an answer to it; if it lands on "the
    /// callee hops", this method hops too, and the change is one line. What
    /// makes the gap tolerable meanwhile is narrow and worth stating rather
    /// than assuming: a configuration is read once, at startup, off a local
    /// file — never per request on a serving path.
    async fn load(&self) -> Result<LogicalPipeline, ConfigError> {
        let text = fs::read_to_string(&self.path).map_err(|source| ConfigError::Unreadable {
            path: self.path.clone(),
            source,
        })?;

        // The version first, for the reason this module documents. A peek that
        // comes back `Unreadable` is *not* reported: the deserializer walked a
        // document it could not make sense of, and `ragondin-pipeline` says
        // what to do about that — fall through to the full parse below, which
        // fails too and with the better-located message.
        if let Err(SchemaVersionPeekError::Unsupported(source)) =
            peek_schema_version(serde_yaml::Deserializer::from_str(&text))
        {
            return Err(ConfigError::UnsupportedSchemaVersion {
                path: self.path.clone(),
                source,
            });
        }

        // Into the hand-maintained wire schema, never into an internal type
        // (INV-9).
        let raw: RawPipeline =
            serde_yaml::from_str(&text).map_err(|source| ConfigError::Malformed {
                path: self.path.clone(),
                source,
            })?;

        validate(raw).map_err(|source| ConfigError::Invalid {
            path: self.path.clone(),
            source,
        })
    }
}

/// Why a configuration could not be turned into a [`LogicalPipeline`].
///
/// Typed rather than a string, per ADR-C13: `ragondin-config` is a library and
/// never imposes `anyhow` on its consumers. Every variant carries the path,
/// because a binary that was handed one may be reading several.
///
/// The four variants are four different things for the person who wrote the
/// file to do, which is the only reason to have four:
///
/// - [`Unreadable`](ConfigError::Unreadable) — check the path or the
///   permissions. Nothing was parsed.
/// - [`UnsupportedSchemaVersion`](ConfigError::UnsupportedSchemaVersion) —
///   upgrade the binary. The file may be perfectly correct, and editing it
///   will not help.
/// - [`Malformed`](ConfigError::Malformed) — the text or the shape is wrong:
///   a syntax error, a missing required key, or a parameter outside the flat
///   grammar ADR-C22 fixes. This is the wire schema's verdict, and it is
///   reached before the graph is looked at.
/// - [`Invalid`](ConfigError::Invalid) — the file is a well-formed
///   configuration describing a graph the validation pass refuses: an unknown
///   component family, a dangling input, a cycle, a kind mismatch.
///
/// Splitting the last two is the distinction with teeth. They are the two
/// halves of the load path, and conflating them sends a reader hunting for a
/// graph fault in a file that is not YAML, or for a typo in a file whose only
/// problem is that it wires two nodes the wrong way round.
///
/// Not `PartialEq`: neither `std::io::Error` nor `serde_yaml::Error` is, and
/// on a type whose whole job is to carry them a derive that forced their
/// removal would be the tail wagging the dog. Match on the variant.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read at all.
    #[error("could not read configuration `{}`", path.display())]
    Unreadable {
        /// The path that was given.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The configuration states a schema version this build cannot read.
    ///
    /// Separate from [`Malformed`](ConfigError::Malformed) deliberately: the
    /// file is not necessarily wrong, this build is merely too old to read it,
    /// and that is the one fault no edit to the file will fix.
    #[error("configuration `{}` is written in a schema version this build cannot read", path.display())]
    UnsupportedSchemaVersion {
        /// The path that was read.
        path: PathBuf,
        /// Which version was found, and which this build supports.
        #[source]
        source: UnsupportedSchemaVersion,
    },
    /// The text could not be read into the wire schema.
    #[error("could not parse configuration `{}`", path.display())]
    Malformed {
        /// The path that was read.
        path: PathBuf,
        /// The deserializer's own error, with the location it carries.
        #[source]
        source: serde_yaml::Error,
    },
    /// The wire schema parsed, and the pipeline it describes is not valid.
    #[error("configuration `{}` is not a valid pipeline", path.display())]
    Invalid {
        /// The path that was read.
        path: PathBuf,
        /// The validation pass's own verdict, intact.
        #[source]
        source: ValidationError,
    },
}

impl ConfigError {
    /// The configuration this error is about.
    pub fn path(&self) -> &Path {
        match self {
            Self::Unreadable { path, .. }
            | Self::UnsupportedSchemaVersion { path, .. }
            | Self::Malformed { path, .. }
            | Self::Invalid { path, .. } => path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_file_does_not_touch_the_disk_until_it_is_loaded() {
        // Constructing one is infallible, which is what lets a binary parse
        // its arguments and report every fault from one place.
        let source = LocalFile::new("/definitely/not/here.yaml");
        assert_eq!(source.path(), Path::new("/definitely/not/here.yaml"));
    }

    #[test]
    fn every_error_variant_reports_the_path_it_is_about() {
        // A binary reading several configurations has nothing else to go on.
        let path = PathBuf::from("/tmp/a.yaml");
        let unreadable = ConfigError::Unreadable {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        };
        let invalid = ConfigError::Invalid {
            path: path.clone(),
            source: ValidationError::InputArity { declared: 0 },
        };
        for error in [&unreadable, &invalid] {
            assert_eq!(error.path(), path);
            assert!(
                error.to_string().contains("a.yaml"),
                "the message must name the file: {error}"
            );
        }
    }
}
