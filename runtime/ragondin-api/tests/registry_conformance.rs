//! [`FsRegistry`] against the conformance suite every `Registry` backend
//! passes (`ragondin_api::conformance`).
//!
//! Built only with `--features conformance` (`required-features` in this
//! crate's manifest), which `just test-features` turns on. The obtainable
//! benchmark is served by a local HTTP server; no test touches the network.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};

use ragondin_api::conformance::{assert_registry_conformance, RegistryFixture};
use ragondin_api::fs::FsRegistry;
use ragondin_benchmarks::manifest::Format;

use support::datasets::{beir_mini_entry, benchmark_fixture, scratch, version_of, Server};

#[tokio::test]
async fn the_file_registry_is_a_conformant_registry() {
    let base = scratch("registry_conformance");
    let server = Server::beir_mini();
    let beir = benchmark_fixture("beir-mini");
    let entry = beir_mini_entry(
        "beir/mini",
        &server.url("/beir-mini"),
        &version_of(Format::Beir, &beir),
    );
    let next = AtomicUsize::new(0);

    assert_registry_conformance(|| RegistryFixture {
        registry: FsRegistry::new(
            base.join(next.fetch_add(1, Ordering::Relaxed).to_string()),
            vec![entry.clone()],
        ),
        obtainable: "beir/mini".to_owned(),
        importable: beir.clone(),
    })
    .await;
}
