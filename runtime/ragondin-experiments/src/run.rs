//! What a run *is*: [`RunId`], [`RunInputs`], [`Metrics`], [`ConfigDocument`],
//! [`TraceDocument`] and the [`Run`] record that holds them.
//!
//! A run is one execution of a pipeline over a benchmark, together with its
//! metrics and its traces, and it is named by the content-addressed tuple of
//! its inputs (`docs/system-architecture.md` §7.1):
//!
//! ```text
//! run_id = hash( pipeline_config, dataset_version, index_version,
//!                model_hashes, engine_version )
//! ```
//!
//! **The hash is not assembled here.** The harness holds the pipeline's
//! content hash and the dataset, index, model and engine versions, folds them
//! into one digest, and hands the result in as a [`RunId`]. This crate defines
//! the record, stores it by that id, and compares two of them; a store that
//! also decided identity would be two things at once, and the pieces of the
//! tuple reach the harness long before they reach a store.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use ragondin_pipeline::PipelineHash;
use ragondin_types::QueryId;
use serde::{Deserialize, Serialize};

/// The content address of a run: a 32-byte digest, rendered as 64 lowercase
/// hex digits.
///
/// It is a digest and not a string for two reasons. It is what a run is
/// *named* by, so accepting an arbitrary string would let two spellings of one
/// run exist side by side — the trap content addressing exists to close. And
/// the store writes one directory per id: a value that can only be 64 hex
/// digits cannot name a path outside the store's root, so path safety falls
/// out of the type rather than out of a check someone must remember.
///
/// `Copy`, because a digest is 32 bytes and a run id is passed around a great
/// deal. No `Ord`, following [`PipelineHash`]: digests have no meaningful
/// order, and sorting runs by one would be sorting by noise.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RunId([u8; 32]);

impl RunId {
    /// Wraps the digest the harness computed over the identity tuple.
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for RunId {
    /// Hand-written, because the derive would print 32 decimal numbers — and
    /// `Debug` is what a failing `assert_eq!` between two ids reports.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RunId({self})")
    }
}

impl FromStr for RunId {
    type Err = RunIdParseError;

    /// Reads back exactly what [`Display`](fmt::Display) writes, and nothing
    /// else. This is how a run id crosses into the process from outside — the
    /// argument to `ragondin compare`, a directory name under a store root —
    /// and it is where "only a digest is a run id" is actually enforced.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = text.as_bytes();
        if bytes.len() != 64 {
            // Reported in bytes because bytes is what was measured: `é` is one
            // character and two bytes.
            return Err(RunIdParseError::Length { bytes: bytes.len() });
        }

        let mut digest = [0u8; 32];
        for (byte, pair) in digest.iter_mut().zip(bytes.chunks_exact(2)) {
            let hex = std::str::from_utf8(pair).map_err(|_| RunIdParseError::NotHex)?;
            if hex.chars().any(|c| c.is_ascii_uppercase()) {
                // Uppercase is refused rather than folded: accepting two
                // spellings of one digest is what the type exists to prevent.
                return Err(RunIdParseError::NotHex);
            }
            *byte = u8::from_str_radix(hex, 16).map_err(|_| RunIdParseError::NotHex)?;
        }

        Ok(Self(digest))
    }
}

/// Why a string is not a [`RunId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RunIdParseError {
    /// The string is not 64 bytes long.
    #[error("a run id is 64 lowercase hex digits, found {bytes} bytes")]
    Length {
        /// How long the string actually was, in bytes.
        bytes: usize,
    },
    /// The string is the right length but is not lowercase hexadecimal.
    #[error("a run id is 64 lowercase hex digits")]
    NotHex,
}

/// The components of the identity tuple, kept beside the run they identify.
///
/// The digest alone says two runs differ; these say *how*, which is what a
/// comparison view needs and what makes a run reproducible from the store.
///
/// This is the run store's own record, and it is not the pipeline's wire
/// format: the configuration itself is kept verbatim as a [`ConfigDocument`]
/// rather than re-serialized out of an in-memory pipeline type, so that what
/// the store holds is the text whose canonical logical form hashes to the
/// [`pipeline`](Self::pipeline) digest, and not a second spelling of it. The
/// digest is taken over that canonical form and never over the text (INV-8);
/// the text is kept because it is what a person reads and re-runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunInputs {
    /// The content hash of the canonical logical pipeline that was run.
    pub pipeline: PipelineHash,
    /// The version of the benchmark dataset the run was evaluated over.
    pub dataset_version: String,
    /// The version of the index the run retrieved from. An index is a derived,
    /// immutable, pinned artifact (§7.1), so it is part of identity.
    pub index_version: String,
    /// The model hashes, by the role the model played — the judge on exactly
    /// the same footing as the generator and the embedder (§7.1), which is
    /// what makes self-preference detectable mechanically later.
    pub model_hashes: BTreeMap<String, String>,
    /// The version of the engine that executed the run.
    pub engine_version: String,
}

/// What a run scored, by metric name.
///
/// A value is one number for the whole run — the mean over the query set for a
/// retrieval metric, a percentile for a latency one — because that is the
/// figure a comparison puts side by side. Computing them belongs to
/// `ragondin-metrics` and to the harness that averages over queries; this type
/// only records the result, so it fixes no catalogue of names: quality, cost
/// and latency all land here (§6.5).
///
/// Ordered, not hashed: a `BTreeMap` makes `metrics.json` come out in a stable
/// order, so two runs of the same shape produce files that diff cleanly.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Metrics {
    values: BTreeMap<String, f64>,
}

impl Metrics {
    /// Records `value` under `name`, returning the value it replaced.
    ///
    /// A non-finite value is accepted here and refused by the store, which is
    /// the boundary that cannot represent it
    /// ([`NotFinite`](crate::RunStoreError::NotFinite)).
    /// This type is a plain record, and a metric that is `NaN` on the way to a
    /// comparison is a fact about the computation, not an error to raise at
    /// the point it is written down.
    pub fn insert(&mut self, name: impl Into<String>, value: f64) -> Option<f64> {
        self.values.insert(name.into(), value)
    }

    /// The value recorded under `name`, if any.
    pub fn get(&self, name: &str) -> Option<f64> {
        self.values.get(name).copied()
    }

    /// Every metric, by name, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, f64)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), *value))
    }

    /// How many metrics are recorded.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether no metric is recorded.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl<K: Into<String>> FromIterator<(K, f64)> for Metrics {
    fn from_iter<I: IntoIterator<Item = (K, f64)>>(entries: I) -> Self {
        Self {
            values: entries
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }
}

/// The configuration document that produced the run, kept verbatim.
///
/// Verbatim is the point: this is the text whose canonical logical form hashes
/// to [`RunInputs::pipeline`], and re-serializing it from an in-memory pipeline
/// would put a second, drifting spelling of the configuration in the store.
/// The store never parses it — it does not need to, and a store that parsed
/// configurations would have to be upgraded whenever the schema moves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConfigDocument {
    text: String,
}

impl ConfigDocument {
    /// Wraps the configuration text as it was written.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// The configuration text.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

/// One query's execution trace, as the harness rendered it.
///
/// The trace itself is `ragondin-engine`'s `ExecutionTrace` — the executor's
/// structured per-node return value (INV-10). This crate holds the *rendering*
/// of one rather than the type: the experiment plane's store does not depend
/// on the engine, and the harness, which depends on both, is where a trace and
/// a run record meet. Keeping it a JSON document also keeps the store out of
/// the way of the trace's shape, which belongs to the engine and moves with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TraceDocument {
    value: serde_json::Value,
}

impl TraceDocument {
    /// Wraps a rendered trace.
    pub fn new(value: serde_json::Value) -> Self {
        Self { value }
    }

    /// The rendered trace.
    ///
    /// Nothing in this crate calls it — the store moves a trace through serde
    /// rather than through this accessor — but a wrapper whose content cannot
    /// be read back is not a record of anything, and the reader is downstream:
    /// the comparison view renders a stored trace, and an export adapter would
    /// translate one.
    pub fn as_value(&self) -> &serde_json::Value {
        &self.value
    }
}

/// One execution of a pipeline over a benchmark: what identified it, what it
/// scored, and what it did.
///
/// The unit of recomputation in this system: a run whose id is already in the
/// store need not be executed again.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// The content address of the identity tuple, computed by the harness.
    pub id: RunId,
    /// The components that digest was computed over.
    pub inputs: RunInputs,
    /// What the run scored.
    pub metrics: Metrics,
    /// The configuration document that produced it.
    pub config: ConfigDocument,
    /// The per-query execution traces, by query.
    pub traces: BTreeMap<QueryId, TraceDocument>,
}
