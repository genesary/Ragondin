//! The `Registry` file backend over a manifest and a datasets directory: what
//! it lists, and how a refused download or import reaches the API's codes.
//!
//! No test touches the network: a download is served by a local HTTP server.

mod support;

use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use ragondin_api::fs::FsRegistry;
use ragondin_api::{ApiError, BenchmarkEntry, BenchmarkState, GroundTruth, Registry};
use ragondin_benchmarks::manifest::Format;

use support::datasets::{
    beir_mini_entry, benchmark_fixture, copy_dir, scratch, sha256, version_of, Server, BEIR_FILES,
};

const LICENCE_URL: &str = "https://example.invalid/licence";

fn not_cancelled() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_registry_file_backend_lists_ready_available_local_and_differing_entries() {
    let datasets = scratch("registry_lists");
    let beir = benchmark_fixture("beir-mini");
    let version = version_of(Format::Beir, &beir);
    let other = "0".repeat(64);
    copy_dir(&beir, &datasets.join("ready"));
    copy_dir(&beir, &datasets.join("differs"));
    copy_dir(&beir, &datasets.join("broken"));
    fs::remove_file(datasets.join("broken/corpus.jsonl")).unwrap();
    // A directory whose import marker this build cannot read.
    fs::create_dir_all(datasets.join("bad")).unwrap();
    fs::write(datasets.join("bad/ragondin-local.json"), "{ not json").unwrap();
    let nowhere = "https://example.invalid";
    let available = beir_mini_entry("beir/available", nowhere, &version);
    let registry = FsRegistry::new(
        datasets.clone(),
        vec![
            beir_mini_entry("beir/ready", nowhere, &version),
            available.clone(),
            beir_mini_entry("beir/differs", nowhere, &other),
            beir_mini_entry("beir/broken", nowhere, &version),
        ],
    );
    let imported = registry
        .import("mine", &benchmark_fixture("beir-qa-mini"))
        .await
        .expect("the fixture imports");

    let listed = registry.benchmarks().await.expect("the registry lists");

    let manifest_entry = |name: &str, state, ground_truth| BenchmarkEntry {
        name: name.to_owned(),
        format: "beir".to_owned(),
        state,
        ground_truth,
        licence: Some("CC-BY-4.0".to_owned()),
        licence_url: Some(LICENCE_URL.to_owned()),
    };
    let qa_version = version_of(Format::BeirQa, &benchmark_fixture("beir-qa-mini"));
    let local = BenchmarkEntry {
        name: "beir-qa/mine".to_owned(),
        format: "beir-qa".to_owned(),
        state: BenchmarkState::Local {
            dataset_version: qa_version,
        },
        ground_truth: Some(GroundTruth::Both),
        licence: None,
        licence_url: None,
    };
    assert_eq!(imported, local);
    let [ready, avail, differs, broken, bad, mine] = listed.as_slice() else {
        panic!("six entries: {listed:#?}");
    };
    assert_eq!(
        ready,
        &manifest_entry(
            "beir/ready",
            BenchmarkState::Ready {
                dataset_version: version.clone(),
            },
            Some(GroundTruth::Qrels),
        ),
        "a ready entry keeps its licence"
    );
    assert_eq!(
        avail,
        &manifest_entry(
            "beir/available",
            BenchmarkState::Available {
                size_bytes: available.size_bytes(),
            },
            Some(GroundTruth::Qrels),
        ),
        "an entry not yet downloaded shows the ground truth its manifest entry declares"
    );
    assert_eq!(
        differs,
        &manifest_entry(
            "beir/differs",
            BenchmarkState::Differs {
                expected: other,
                found: version,
            },
            Some(GroundTruth::Qrels),
        )
    );
    assert_eq!(broken.name, "beir/broken");
    assert_eq!(broken.ground_truth, None);
    match &broken.state {
        BenchmarkState::Unreadable { error } => {
            assert!(
                error.contains("corpus.jsonl"),
                "the adapter's error: {error}"
            )
        }
        other => panic!("expected unreadable, got {other:?}"),
    }
    assert_eq!(bad.name, "bad", "a bad marker is its directory's problem");
    match &bad.state {
        BenchmarkState::Unreadable { error } => {
            assert!(error.contains("ragondin-local.json"), "{error}")
        }
        other => panic!("expected unreadable, got {other:?}"),
    }
    assert_eq!(mine, &local);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_whose_digest_differs_is_download_failed_with_both_digests_and_leaves_nothing() {
    let datasets = scratch("registry_download_differs");
    let server = Server::beir_mini();
    let beir = benchmark_fixture("beir-mini");
    let entry = beir_mini_entry(
        "beir/mini",
        &server.url("/beir-mini-corrupt"),
        &version_of(Format::Beir, &beir),
    );
    let expected = entry.files[1].sha256.clone();
    let registry = FsRegistry::new(datasets.clone(), vec![entry]);

    let error = registry
        .download("beir/mini", Arc::new(|_| {}), not_cancelled())
        .await
        .expect_err("the digest differs");

    match &error {
        ApiError::DownloadFailed { name, reason } => {
            assert_eq!(name, "beir/mini");
            assert!(reason.contains(&expected), "{reason}");
        }
        other => panic!("expected download_failed, got {other:?}"),
    }
    assert_eq!(error.code(), "download_failed");
    assert_eq!(
        fs::read_dir(&datasets).unwrap().count(),
        0,
        "nothing is left"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pinned_snapshot_that_does_not_load_is_a_defect_of_the_build_not_of_the_source() {
    let datasets = scratch("registry_download_load");
    let beir = benchmark_fixture("beir-mini");
    let broken = b"{ not json\n".to_vec();
    let server = Server::serve(
        BEIR_FILES
            .iter()
            .map(|path| {
                let bytes = if *path == "corpus.jsonl" {
                    broken.clone()
                } else {
                    fs::read(beir.join(path)).unwrap()
                };
                (format!("/broken/{path}"), bytes)
            })
            .collect(),
    );
    // The manifest pins the broken bytes: every digest matches, and the
    // snapshot still does not load.
    let mut entry = beir_mini_entry(
        "beir/broken",
        &server.url("/broken"),
        &version_of(Format::Beir, &beir),
    );
    entry.files[0].sha256 = sha256(&broken);
    entry.files[0].size_bytes = broken.len() as u64;
    let registry = FsRegistry::new(datasets.clone(), vec![entry]);

    let error = registry
        .download("beir/broken", Arc::new(|_| {}), not_cancelled())
        .await
        .expect_err("it does not load");

    assert_eq!(error.code(), "backend_failed", "{error:?}");
    assert_eq!(
        fs::read_dir(&datasets).unwrap().count(),
        0,
        "nothing is left"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_import_the_adapter_refuses_is_import_refused_with_the_adapter_error() {
    let datasets = scratch("registry_import_refused");
    let source = scratch("registry_import_refused_source");
    copy_dir(&benchmark_fixture("beir-mini"), &source);
    fs::write(source.join("queries.jsonl"), "{ not json\n").unwrap();
    let registry = FsRegistry::new(datasets.clone(), Vec::new());

    let error = registry
        .import("broken", &source)
        .await
        .expect_err("the adapter refuses it");

    match &error {
        ApiError::ImportRefused { name, reason } => {
            assert_eq!(name, "broken");
            assert!(reason.contains("queries.jsonl"), "{reason}");
        }
        other => panic!("expected import_refused, got {other:?}"),
    }
    assert!(registry.benchmarks().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_names_an_import_may_not_take_are_those_of_the_registry_s_own_manifest() {
    let datasets = scratch("registry_import_reserved");
    let beir = benchmark_fixture("beir-mini");
    let version = version_of(Format::Beir, &beir);
    let registry = FsRegistry::new(
        datasets.clone(),
        vec![beir_mini_entry(
            "beir/reserved",
            "https://example.invalid",
            &version,
        )],
    );

    let refused = registry.import("reserved", &beir).await;
    assert!(
        matches!(refused, Err(ApiError::BenchmarkExists { .. })),
        "{refused:?}"
    );
    // Not in this registry's manifest, whatever the build's manifest holds.
    registry
        .import("scifact", &beir)
        .await
        .expect("scifact is not reserved here");
}

/// Verifying one import reads that import alone: `POST /runs` up to a node
/// verifies the chosen benchmark, and a workspace holding many imports must
/// not load them all for it. Another import's corpus is a named pipe no one
/// writes, so reading it would block; the verify answers regardless.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn verifying_one_import_reads_no_other_import() {
    let datasets = scratch("registry_verify_one");
    let registry = FsRegistry::new(datasets.clone(), Vec::new());
    for name in ["first", "second"] {
        registry
            .import(name, &benchmark_fixture("beir-mini"))
            .await
            .expect("the fixture imports");
    }
    let corpus = datasets.join("first/corpus.jsonl");
    fs::remove_file(&corpus).unwrap();
    let made = std::process::Command::new("mkfifo")
        .arg(&corpus)
        .status()
        .unwrap();
    assert!(made.success());

    let answered = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        registry.verify("beir/second"),
    )
    .await;

    // Release any reader before asserting: a verify that wrongly read `first`
    // is blocked in `open` on the pipe, on a blocking thread the runtime
    // waits for when it drops — so the test would hang rather than fail.
    // Opening the pipe for writing unblocks that `open` (read and write, so
    // this side never blocks itself, whether or not a reader is there), and
    // closing it gives the reader end of file.
    drop(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&corpus)
            .unwrap(),
    );
    fs::remove_file(&corpus).unwrap();

    let verified = answered
        .expect("verifying `second` does not read `first`")
        .expect("`second` is known");
    assert_eq!(verified.name, "beir/second");
    assert_eq!(verified.ground_truth, Some(GroundTruth::Qrels));
}
