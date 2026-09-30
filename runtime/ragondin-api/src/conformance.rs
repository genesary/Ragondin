//! The conformance suite every [`Registry`] backend passes, behind this
//! crate's `conformance` feature so that a production build does not carry
//! it.
//!
//! The model is `ragondin-experiments`' suite for `RunStore`: one entry point,
//! called from the backend's own tests, that checks the contract
//! [`Registry`]'s documentation states — never how a backend meets it.
//! [`FsRegistry`](crate::fs::FsRegistry) passes it in this crate's tests, and
//! a cluster backend passes the same one.
//!
//! ```ignore
//! assert_registry_conformance(|| RegistryFixture {
//!     registry: MyRegistry::empty_with(manifest_serving_one_dataset()),
//!     obtainable: "beir/mini".to_owned(),
//!     importable: path_to_a_beir_directory_with_qrels_only(),
//! })
//! .await;
//! ```
//!
//! # What it checks
//!
//! Each case gets a registry of its own from `fresh`, so no case's writes
//! decide another's outcome:
//!
//! 1. **Listing.** A fresh registry lists `obtainable` as `available`, with a
//!    licence and a size, and no name twice.
//! 2. **Download.** Downloading `obtainable` reports progress that never goes
//!    back and ends at its total, and returns it `ready` with its
//!    `dataset_version` and its ground truth; the listing and
//!    [`Registry::verify`] then report exactly that entry. A second download
//!    is `benchmark_exists`.
//! 3. **Unknown names.** Downloading or verifying a name the registry does
//!    not know is `benchmark_not_found`, naming it, and lists nothing new.
//! 4. **Import.** Importing `importable` returns a `local` entry carrying
//!    qrels, listed and verified the same; importing under the same name
//!    again is `benchmark_exists`, and importing a path that does not exist
//!    is `import_refused` and registers nothing.
//!
//! # Why a fixture
//!
//! A registry obtains what its manifest names from wherever the manifest
//! points, and imports from a path on the machine it runs on. Neither is
//! something the suite can make up for every backend, so the backend's test
//! supplies them: a manifest entry it can really obtain — for the file
//! backend, served by a local HTTP server — and a BEIR directory carrying
//! qrels and no reference answers.
//!
//! A failed check panics with the case it belongs to, as a test assertion
//! does. The function is `async` and starts no runtime of its own.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::backends::{DownloadProgress, Registry};
use crate::error::ApiError;
use crate::response::{BenchmarkEntry, BenchmarkState, GroundTruth};

/// A registry to check, and what it needs to be checked.
pub struct RegistryFixture<R> {
    /// A fresh registry: nothing downloaded, nothing imported.
    pub registry: R,
    /// A name its manifest holds, which it can download.
    pub obtainable: String,
    /// A BEIR directory with qrels and no `answers.jsonl`, to import.
    pub importable: PathBuf,
}

/// Runs every case against registries built by `fresh`; panics on the first
/// failure.
pub async fn assert_registry_conformance<R, F>(mut fresh: F)
where
    R: Registry,
    F: FnMut() -> RegistryFixture<R>,
{
    listing(fresh()).await;
    download(fresh()).await;
    unknown_names(fresh()).await;
    import(fresh()).await;
}

async fn listing<R: Registry>(fixture: RegistryFixture<R>) {
    let listed = fixture
        .registry
        .benchmarks()
        .await
        .expect("listing: a fresh registry lists");
    let mut names: Vec<&str> = listed.iter().map(|entry| entry.name.as_str()).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(names.len(), count, "listing: a name is listed twice");
    let entry = find(&listed, &fixture.obtainable)
        .unwrap_or_else(|| panic!("listing: {} is not listed", fixture.obtainable));
    match &entry.state {
        BenchmarkState::Available {
            size_bytes,
            licence,
            ..
        } => {
            assert!(*size_bytes > 0, "listing: {} has no size", entry.name);
            assert!(
                !licence.is_empty(),
                "listing: {} has no licence",
                entry.name
            );
        }
        other => panic!("listing: {} is {other:?}, not available", entry.name),
    }
}

async fn download<R: Registry>(fixture: RegistryFixture<R>) {
    let name = fixture.obtainable.as_str();
    let seen: Arc<Mutex<Vec<DownloadProgress>>> = Arc::default();
    let sink = Arc::clone(&seen);
    let downloaded = fixture
        .registry
        .download(
            name,
            Arc::new(move |progress| sink.lock().unwrap().push(progress)),
        )
        .await
        .unwrap_or_else(|error| panic!("download: {name} does not download: {error}"));

    assert_eq!(downloaded.name, name, "download: the entry is another");
    assert!(
        matches!(&downloaded.state, BenchmarkState::Ready { dataset_version } if !dataset_version.is_empty()),
        "download: {name} is {:?}, not ready",
        downloaded.state
    );
    assert!(
        downloaded.ground_truth.is_some(),
        "download: a ready dataset carries a ground truth"
    );
    let seen = seen.lock().unwrap().clone();
    let last = seen.last().expect("download: no progress was reported");
    assert_eq!(last.received, last.total, "download: progress ends short");
    assert!(
        seen.windows(2)
            .all(|pair| pair[0].received <= pair[1].received),
        "download: progress went back"
    );

    let listed = fixture
        .registry
        .benchmarks()
        .await
        .expect("download: lists");
    assert_eq!(
        find(&listed, name),
        Some(&downloaded),
        "download: the listing disagrees with the download"
    );
    let verified = fixture
        .registry
        .verify(name)
        .await
        .expect("download: verifies");
    assert_eq!(verified, downloaded, "download: verify disagrees");

    let again = fixture.registry.download(name, Arc::new(|_| {})).await;
    assert!(
        matches!(again, Err(ApiError::BenchmarkExists { .. })),
        "download: a second download is {again:?}, not benchmark_exists"
    );
}

async fn unknown_names<R: Registry>(fixture: RegistryFixture<R>) {
    let unknown = "beir/no-such-benchmark";
    let before = fixture.registry.benchmarks().await.expect("unknown: lists");
    let downloaded = fixture.registry.download(unknown, Arc::new(|_| {})).await;
    assert!(
        matches!(&downloaded, Err(ApiError::BenchmarkNotFound { name }) if name == unknown),
        "unknown: download is {downloaded:?}"
    );
    let verified = fixture.registry.verify(unknown).await;
    assert!(
        matches!(&verified, Err(ApiError::BenchmarkNotFound { name }) if name == unknown),
        "unknown: verify is {verified:?}"
    );
    let after = fixture.registry.benchmarks().await.expect("unknown: lists");
    assert_eq!(before, after, "unknown: the listing changed");
}

async fn import<R: Registry>(fixture: RegistryFixture<R>) {
    let imported = fixture
        .registry
        .import("conformance", &fixture.importable)
        .await
        .unwrap_or_else(|error| panic!("import: the fixture does not import: {error}"));
    assert!(
        matches!(&imported.state, BenchmarkState::Local { dataset_version } if !dataset_version.is_empty()),
        "import: {:?} is not local",
        imported.state
    );
    assert_eq!(
        imported.ground_truth,
        Some(GroundTruth::Qrels),
        "import: a qrels-only directory carries qrels"
    );

    let listed = fixture.registry.benchmarks().await.expect("import: lists");
    assert_eq!(
        find(&listed, &imported.name),
        Some(&imported),
        "import: the listing disagrees with the import"
    );
    let verified = fixture
        .registry
        .verify(&imported.name)
        .await
        .expect("import: verifies");
    assert_eq!(verified, imported, "import: verify disagrees");

    let again = fixture
        .registry
        .import("conformance", &fixture.importable)
        .await;
    assert!(
        matches!(again, Err(ApiError::BenchmarkExists { .. })),
        "import: a second import under the name is {again:?}"
    );

    let missing = fixture.importable.join("no-such-directory");
    let refused = fixture.registry.import("missing", &missing).await;
    assert!(
        matches!(refused, Err(ApiError::ImportRefused { .. })),
        "import: a path that does not exist is {refused:?}"
    );
    let after = fixture.registry.benchmarks().await.expect("import: lists");
    assert_eq!(
        listed, after,
        "import: a refused import registered something"
    );
}

fn find<'a>(listed: &'a [BenchmarkEntry], name: &str) -> Option<&'a BenchmarkEntry> {
    listed.iter().find(|entry| entry.name == name)
}
