//! The `Registry` file backend over a manifest and a datasets directory: what
//! it lists, and how a refused download or import reaches the API's codes.
//!
//! No test touches the network: a download is served by a local HTTP server.

mod support;

use std::fs;
use std::sync::Arc;

use ragondin_api::fs::FsRegistry;
use ragondin_api::{ApiError, BenchmarkEntry, BenchmarkState, GroundTruth, Registry};
use ragondin_benchmarks::manifest::Format;

use support::datasets::{
    beir_mini_entry, benchmark_fixture, copy_dir, scratch, version_of, Server,
};

#[tokio::test]
async fn the_registry_file_backend_lists_ready_available_local_and_differing_entries() {
    let datasets = scratch("registry_lists");
    let beir = benchmark_fixture("beir-mini");
    let version = version_of(Format::Beir, &beir);
    let other = "0".repeat(64);
    copy_dir(&beir, &datasets.join("ready"));
    copy_dir(&beir, &datasets.join("differs"));
    copy_dir(&beir, &datasets.join("broken"));
    fs::remove_file(datasets.join("broken/corpus.jsonl")).unwrap();
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

    let qa_version = version_of(Format::BeirQa, &benchmark_fixture("beir-qa-mini"));
    let local = BenchmarkEntry {
        name: "beir-qa/mine".to_owned(),
        format: "beir-qa".to_owned(),
        state: BenchmarkState::Local {
            dataset_version: qa_version,
        },
        ground_truth: Some(GroundTruth::Both),
    };
    assert_eq!(imported, local);
    let [ready, avail, differs, broken, mine] = listed.as_slice() else {
        panic!("five entries: {listed:#?}");
    };
    assert_eq!(
        ready,
        &BenchmarkEntry {
            name: "beir/ready".to_owned(),
            format: "beir".to_owned(),
            state: BenchmarkState::Ready {
                dataset_version: version.clone(),
            },
            ground_truth: Some(GroundTruth::Qrels),
        }
    );
    assert_eq!(
        avail,
        &BenchmarkEntry {
            name: "beir/available".to_owned(),
            format: "beir".to_owned(),
            state: BenchmarkState::Available {
                size_bytes: available.size_bytes(),
                licence: "CC-BY-4.0".to_owned(),
                licence_url: "https://example.invalid/licence".to_owned(),
            },
            ground_truth: None,
        }
    );
    assert_eq!(
        differs,
        &BenchmarkEntry {
            name: "beir/differs".to_owned(),
            format: "beir".to_owned(),
            state: BenchmarkState::Differs {
                expected: other,
                found: version,
            },
            ground_truth: Some(GroundTruth::Qrels),
        }
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
    assert_eq!(mine, &local);
}

#[tokio::test]
async fn a_download_whose_digest_differs_is_download_failed_with_both_digests_and_leaves_nothing() {
    let datasets = scratch("registry_download_differs");
    let beir = benchmark_fixture("beir-mini");
    let server = Server::serve(
        support::datasets::BEIR_FILES
            .iter()
            .map(|path| {
                let mut bytes = fs::read(beir.join(path)).unwrap();
                if *path == "queries.jsonl" {
                    bytes[0] ^= 1;
                }
                (format!("/beir-mini/{path}"), bytes)
            })
            .collect(),
    );
    let entry = beir_mini_entry(
        "beir/mini",
        &server.url("/beir-mini"),
        &version_of(Format::Beir, &beir),
    );
    let expected = entry.files[1].sha256.clone();
    let registry = FsRegistry::new(datasets.clone(), vec![entry]);

    let error = registry
        .download("beir/mini", Arc::new(|_| {}))
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

#[tokio::test]
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
