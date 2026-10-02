//! Which workspace pipeline a stored run is a run of.
//!
//! A run records no pipeline name: its identity is the canonical hash of
//! what it ran. ADR-C39 decides run → pipeline identity; until the code
//! follows it, a run's pipeline is found by content — **interim behaviour**:
//! the one document under `pipelines/` whose canonical hash is the run's
//! (INV-8: the canonical form, never the text). No document, or several,
//! names no pipeline; a document edited since a run no longer names it. `POST /compare` reads it to find the pairing of two runs' pipelines,
//! and the pipeline matrix needs the same index.

use std::collections::BTreeMap;

use ragondin_experiments::Run;

use crate::backends::PipelineSource;
use crate::error::ApiError;
use crate::validation;

/// Every pipeline document that validates, by its canonical hash, as
/// `validation::check` renders it: a hash maps to several names when several
/// documents are one canonical form.
pub(crate) async fn pipelines_by_hash(
    source: &dyn PipelineSource,
) -> Result<BTreeMap<String, Vec<String>>, ApiError> {
    let mut by_hash: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in source.list().await? {
        // A document that does not validate has no canonical form, so no
        // run can be a run of it.
        if let Ok(hash) = validation::check(&file.document) {
            by_hash.entry(hash).or_default().push(file.name);
        }
    }
    Ok(by_hash)
}

/// The pipeline `run` is a run of, by `index`: the one name its hash maps
/// to, `None` for none or several.
pub(crate) fn pipeline_of(index: &BTreeMap<String, Vec<String>>, run: &Run) -> Option<String> {
    match index.get(&run.inputs.pipeline.to_string()) {
        Some(names) if names.len() == 1 => Some(names[0].clone()),
        _ => None,
    }
}
