//! [`FsRegistry`] against the conformance suite every `Registry` backend
//! passes (`ragondin_api::conformance`).
//!
//! Built only with `--features conformance` (`required-features` in this
//! crate's manifest), which `just test-features` turns on. Both benchmarks the
//! manifest names are served by a local HTTP server — one faithfully, one with
//! a byte changed; no test touches the network.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};

use ragondin_api::conformance::{assert_registry_conformance, Alteration, RegistryFixture};
use ragondin_api::fs::FsRegistry;
use ragondin_benchmarks::manifest::Format;

use support::datasets::{beir_mini_entry, benchmark_fixture, scratch, version_of, Server};

#[tokio::test(flavor = "multi_thread")]
async fn the_file_registry_is_a_conformant_registry() {
    let base = scratch("registry_conformance");
    let server = Server::beir_mini();
    let beir = benchmark_fixture("beir-mini");
    let version = version_of(Format::Beir, &beir);
    let manifest = vec![
        beir_mini_entry("beir/mini", &server.url("/beir-mini"), &version),
        beir_mini_entry("beir/corrupt", &server.url("/beir-mini-corrupt"), &version),
    ];
    let next = AtomicUsize::new(0);

    assert_registry_conformance(|| RegistryFixture {
        registry: FsRegistry::new(
            base.join(next.fetch_add(1, Ordering::Relaxed).to_string()),
            manifest.clone(),
        ),
        obtainable: "beir/mini".to_owned(),
        corrupt: "beir/corrupt".to_owned(),
        importable: beir.clone(),
        // A benchmark `<format>/<dir>` lives at `<datasets>/<dir>`.
        alter: Box::new(|registry: &FsRegistry, name: &str, alteration| {
            let dir = registry.datasets().join(name.rsplit('/').next().unwrap());
            let corpus = dir.join("corpus.jsonl");
            match alteration {
                Alteration::ChangeContent => {
                    let text = std::fs::read_to_string(&corpus).unwrap();
                    let changed = text.replacen("The cat sat", "The bat sat", 1);
                    assert_ne!(changed, text);
                    std::fs::write(&corpus, changed).unwrap();
                }
                Alteration::Break => std::fs::remove_file(&corpus).unwrap(),
            }
        }),
    })
    .await;
}
