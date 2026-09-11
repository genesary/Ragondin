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
//! [`ValidationError::KindMismatch`] is rendered as a structured report —
//! the edge, the kind that port expects, the kind that arrives — instead of
//! going out as one more `caused by:` line. The check itself lives in
//! `ragondin-pipeline`'s validation pass and is not repeated here; this module
//! only surfaces its verdict, and it is the same verdict the configuration
//! service will NACK on (ADR-6).

use std::path::Path;

use anyhow::{anyhow, Result};
use ragondin_config::{ConfigError, ConfigSource, LocalFile};
use ragondin_pipeline::{NodeId, ValidationError, ValueKind};

/// Loads `config` and prints its content hash, or reports why it is not a
/// pipeline.
pub async fn run(config: &Path) -> Result<()> {
    let pipeline = match LocalFile::new(config).load().await {
        Ok(pipeline) => pipeline,
        Err(ConfigError::Invalid {
            path,
            source:
                ValidationError::KindMismatch {
                    consumer,
                    port,
                    producer,
                    expected,
                    found,
                },
        }) => {
            return Err(anyhow!(incompatible_wiring_report(
                &path, &consumer, port, &producer, expected, found
            )))
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

/// Renders an incompatible edge as the three things a reader has to know to
/// fix it: which edge, what the port expects, and what arrives there.
///
/// `expected` is `None` when the consumer's variant declares no port at that
/// position at all — an edge that should not exist rather than a kind that does
/// not fit — so that case is worded as the absence of a port and not as an
/// absent kind.
fn incompatible_wiring_report(
    path: &Path,
    consumer: &NodeId,
    port: usize,
    producer: &NodeId,
    expected: Option<ValueKind>,
    found: ValueKind,
) -> String {
    let expected = match expected {
        Some(kind) => kind.to_string(),
        None => format!(
            "nothing — `{}` declares no port at position {port}",
            consumer.as_str()
        ),
    };

    format!(
        "`{}` wires two nodes incompatibly\n  \
         edge: `{}` feeds `{}` at port {port}\n  \
         expected: {expected}\n  \
         found: {found}",
        path.display(),
        producer.as_str(),
        consumer.as_str(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ragondin_pipeline::{NodeId, ValueKind};

    #[test]
    fn the_wiring_report_names_the_edge_the_expected_kind_and_the_kind_found() {
        let report = incompatible_wiring_report(
            Path::new("pipeline.yaml"),
            &NodeId::new("ranked"),
            0,
            &NodeId::new("legs"),
            Some(ValueKind::Query),
            ValueKind::Chunks,
        );

        assert_eq!(
            report,
            "`pipeline.yaml` wires two nodes incompatibly\n  \
             edge: `legs` feeds `ranked` at port 0\n  \
             expected: query\n  \
             found: chunks"
        );
    }

    #[test]
    fn a_port_that_does_not_exist_is_reported_as_an_edge_that_should_not_exist() {
        // `expected: None` is not "expected nothing" — it is a consumer whose
        // variant declares no port at that position at all, so the fault is the
        // edge rather than the kind travelling along it. Rendering it as an
        // absent kind would send a reader looking for a type error.
        let report = incompatible_wiring_report(
            Path::new("pipeline.yaml"),
            &NodeId::new("retrieve"),
            2,
            &NodeId::new("legs"),
            None,
            ValueKind::Chunks,
        );

        assert!(
            report.contains("expected: nothing — `retrieve` declares no port at position 2"),
            "got:\n{report}"
        );
        assert!(report.contains("found: chunks"), "got:\n{report}");
    }
}
