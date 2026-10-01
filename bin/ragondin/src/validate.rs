//! The `validate` subcommand: parse, validate, canonicalize, print the hash.
//!
//! The cheapest end-to-end exercise of the config→logical→hash path, and the
//! one subcommand that needs no `EngineContext`: `ConfigSource::load` stops at
//! `LogicalPipeline`, which *names* each implementation and resolves none
//! (ADR-C2), so a configuration is checked and content-addressed with no
//! registry at all and nothing is executed.
//!
//! # Why one error gets a report of its own
//!
//! ADR-C16 places the check of an edge's value kinds at `LogicalPipeline`
//! validation, and it is the check a person most needs help reading: unlike a
//! syntax error, it points at no single line of the file. So
//! [`ValidationError::KindMismatch`] is rendered as a structured report — the
//! edge, the kind that port expects, the kind that arrives — instead of going
//! out as one more `caused by:` line. The check itself lives in
//! `ragondin-pipeline`'s validation pass, and the report in `ragondin-config`'s
//! `incompatible_wiring`, which `POST /pipelines/validate` renders too; this
//! module only names the file as its subject, and the verdict is the same one
//! the configuration service will NACK on (ADR-6).
//!
//! [`ValidationError::KindMismatch`]: ragondin_pipeline::ValidationError::KindMismatch

use std::path::Path;

use anyhow::{anyhow, Result};
use ragondin_config::{incompatible_wiring, ConfigError, ConfigSource, LocalFile};

/// Loads `config` and prints its content hash, or reports why it is not a
/// pipeline.
pub async fn run(config: &Path) -> Result<()> {
    let pipeline = match LocalFile::new(config).load().await {
        Ok(pipeline) => pipeline,
        Err(ConfigError::Invalid { path, source }) => {
            match incompatible_wiring(&format!("`{}`", path.display()), &source) {
                Some(report) => return Err(anyhow!(report)),
                None => return Err(ConfigError::Invalid { path, source }.into()),
            }
        }
        // Everything else keeps the typed error's own wording, and its cause
        // chain with it: the deserializer's message carries the line and column
        // no report written here could reconstruct.
        Err(other) => return Err(other.into()),
    };

    println!("{}: valid", config.display());
    println!("content hash: {}", pipeline.content_hash());

    Ok(())
}
