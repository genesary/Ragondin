//! Run identity: the content-addressed tuple that names a run (P4).
//!
//! ```text
//! run_id = hash( pipeline_config, dataset_version, index_version,
//!                model_hashes, engine_version )
//! ```
//!
//! `docs/system-architecture.md` §7.1 states the tuple; `ragondin-experiments`
//! defines the record and says in as many words that the digest is assembled
//! here, because this is the only place that holds all five pieces at once.
//!
//! Two of the five are not defined here. `dataset_version` and
//! `index_version` — and the one-chunk-per-document derivation the second is
//! taken over — live in `ragondin_benchmarks::identity`, because a stored run
//! is verified against them by a reader that may not reach the engine, and
//! ADR-C36 § 4 allows one definition of each. This module digests the tuple
//! through the same `Encoder`, under its own domain, so the three digests
//! cannot drift apart in how a string or a count is written.

use ragondin_benchmarks::identity::Encoder;
use ragondin_experiments::{RunId, RunInputs};

/// The domain separator of the run identity digest.
const RUN_DOMAIN: &str = "ragondin/run-id/v1";

/// The content address of a run: the digest of its whole identity tuple.
pub(crate) fn run_id(inputs: &RunInputs) -> RunId {
    let mut encoder = Encoder::new(RUN_DOMAIN);

    encoder.field(inputs.pipeline.as_bytes());
    encoder.field(inputs.dataset_version.as_bytes());
    encoder.field(inputs.index_version.as_bytes());
    encoder.count(inputs.model_hashes.len());
    for (role, digest) in &inputs.model_hashes {
        encoder.field(role.as_bytes());
        encoder.field(digest.as_bytes());
    }
    encoder.field(inputs.engine_version.as_bytes());

    RunId::from_digest(encoder.finish())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ragondin_pipeline::PipelineHash;

    use super::*;

    fn inputs() -> RunInputs {
        RunInputs {
            pipeline: PipelineHash::from_digest([7; 32]),
            dataset_version: "dataset".to_string(),
            index_version: "index".to_string(),
            model_hashes: BTreeMap::new(),
            engine_version: "0.0.0".to_string(),
        }
    }

    /// The id [`run_id`] gave [`inputs`] before the encoder moved to
    /// `ragondin-benchmarks`. Pinned rather than recomputed: the encoder now
    /// lives in another crate, and only a recorded value proves that every
    /// stored run keeps its id.
    const RECORDED_RUN_ID: &str =
        "5f2bd9ab3cd30b0cd4634dad0be9794caae31e262d086e0e20fb145675b05cf3";

    #[test]
    fn the_run_id_of_fixed_inputs_is_the_recorded_one() {
        assert_eq!(run_id(&inputs()).to_string(), RECORDED_RUN_ID);
    }

    #[test]
    fn every_component_of_the_tuple_changes_the_run_id() {
        let base = run_id(&inputs());

        assert_eq!(
            base,
            run_id(&inputs()),
            "identical inputs, identical id (P4)"
        );

        let mut pipeline = inputs();
        pipeline.pipeline = PipelineHash::from_digest([8; 32]);
        assert_ne!(base, run_id(&pipeline));

        let mut dataset = inputs();
        dataset.dataset_version = "other".to_string();
        assert_ne!(base, run_id(&dataset));

        let mut index = inputs();
        index.index_version = "other".to_string();
        assert_ne!(base, run_id(&index));

        let mut models = inputs();
        models
            .model_hashes
            .insert("embedder".to_string(), "sha".to_string());
        assert_ne!(base, run_id(&models));

        let mut engine = inputs();
        engine.engine_version = "0.1.0".to_string();
        assert_ne!(base, run_id(&engine));
    }

    #[test]
    fn a_model_hash_cannot_be_smuggled_across_its_field_boundary() {
        let mut split = inputs();
        split
            .model_hashes
            .insert("embed".to_string(), "der:sha".to_string());
        let mut joined = inputs();
        joined
            .model_hashes
            .insert("embedder".to_string(), "sha".to_string());

        assert_ne!(run_id(&split), run_id(&joined));
    }
}
