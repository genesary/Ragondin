//! The identity of a dataset and of the chunk set derived from it, pinned to
//! literal digests.
//!
//! Every stored run names itself by `hash(pipeline_config, dataset_version,
//! index_version, model_hashes, engine_version)`, so a change to either digest
//! here changes the `run_id` of every run already recorded (INV-8). The values
//! below are the ones `ragondin-harness` computed over these fixtures before
//! the two digests and the chunk derivation moved into this crate (ADR-C36
//! § 4); only a recorded value can tell a changed encoding from an unchanged
//! one, so they are literals and never recomputed.
//!
//! The same code, run over the recorded SciFact and NFCorpus snapshots, gives
//! the `dataset_version`s `bin/ragondin/tests/calibration.rs` pins —
//! `9a07f80c…` and `8046025011…`. That material lives outside the tree, so the
//! calibration test is where those two are checked; the fixtures here are the
//! ones every `just check` can read.

use std::path::PathBuf;

use ragondin_benchmarks::identity::{dataset_version, CorpusIndex};
use ragondin_benchmarks::{BeirAdapter, Benchmark, BenchmarkAdapter, SquadAdapter};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn assert_digests(benchmark: &Benchmark, dataset: &str, index: &str) {
    assert_eq!(dataset_version(benchmark), dataset, "dataset_version moved");
    assert_eq!(
        CorpusIndex::build(benchmark.corpus()).version(),
        index,
        "index_version moved"
    );
}

#[test]
fn beir_mini_digests_to_its_recorded_versions() {
    // Qrels only: no reference-answers section in the stream.
    assert_digests(
        &BeirAdapter::new(fixture("beir-mini"))
            .load()
            .expect("the checked-in BEIR fixture loads"),
        "f2949d95523a15be0486c7d4a8495df7d59b1f724efcea12628fb5661011d403",
        "77e5d900dd7681619a8062c5c6d6d4c4db23ed08d03b0cea18dab1641fdccf0d",
    );
}

#[test]
fn beir_qa_mini_digests_to_its_recorded_versions() {
    // Qrels and reference answers: the tagged section is in the stream.
    assert_digests(
        &BeirAdapter::new(fixture("beir-qa-mini"))
            .with_reference_answers()
            .load()
            .expect("the checked-in BEIR QA fixture loads"),
        "bbd1e63fa22181b642ab858835c12951f04f73c67ae096ebe778b8e8c540a18d",
        "9c49f7275376d8bfbbe10ab9e22e0a8a086055808f74c59169b635adad8836f6",
    );
}

#[test]
fn squad_mini_digests_to_its_recorded_versions() {
    // Document metadata (the article title) enters the dataset digest.
    assert_digests(
        &SquadAdapter::new(fixture("squad-mini"))
            .load()
            .expect("the checked-in SQuAD fixture loads"),
        "abd1e681e7f11f6749ccbf778c9968e35b8c13607a1bdbca951f805bb71a08d5",
        "d493ac2719128e1b8044ab75a1d8898625181ff780c381a491166c186837c0b7",
    );
}
