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
//!     registry: MyRegistry::empty_with(manifest_serving_two_datasets()),
//!     obtainable: "beir/mini".to_owned(),
//!     corrupt: "beir/corrupt".to_owned(),
//!     importable: path_to_a_beir_directory_with_qrels_only(),
//!     alter: Box::new(|registry, name, alteration| change_what_is_stored(registry, name, alteration)),
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
//!    size and a licence, and no name twice.
//! 2. **Download.** Downloading `obtainable` reports progress that never goes
//!    back and ends at its total, and returns it `ready` with its
//!    `dataset_version`, its ground truth and its licence; the listing and
//!    [`Registry::verify`] then report exactly that entry. A second download
//!    is `benchmark_exists`.
//! 3. **A failed download leaves nothing.** Downloading `corrupt` is
//!    `download_failed`, and it is still listed `available`, as before.
//! 4. **Cancellation.** A download whose flag is set is `download_cancelled`,
//!    and leaves the benchmark `available`.
//! 5. **Unknown names.** Downloading or verifying a name the registry does
//!    not know is `benchmark_not_found`, naming it, and lists nothing new.
//! 6. **Import.** Importing `importable` returns a `local` entry carrying
//!    qrels, listed and verified the same; importing under the same name
//!    again is `benchmark_exists`, and importing a path that does not exist,
//!    or under a name that is not one, is `import_refused` and registers
//!    nothing.
//! 7. **A run's dataset.** [`Registry::dataset`] finds a downloaded
//!    benchmark, and an imported one, by the `dataset_version` it is pinned
//!    to, loaded and digesting to it; the same digest before anything is on
//!    disk is `Absent`, and a digest nothing is pinned to is `Unknown`; a
//!    downloaded benchmark that verified and whose content is then changed
//!    is `Differs`, with the digest found, and one then broken is
//!    `Unreadable` — so a backend that keeps what it verified must notice.
//! 8. **Pins.** [`Registry::pinned`] names `obtainable` with its
//!    `dataset_version` before anything is downloaded, as after, and an
//!    import with the digest it recorded, no name twice.
//!
//! # Why a fixture
//!
//! A registry obtains what its manifest names from wherever the manifest
//! points, and imports from a path on the machine it runs on. Neither is
//! something the suite can make up for every backend, so the backend's test
//! supplies them: a manifest entry it can really obtain, and one whose source
//! serves bytes that do not match its digests — for the file backend, both
//! served by a local HTTP server — and a BEIR directory carrying qrels and no
//! reference answers.
//!
//! A failed check panics with the case it belongs to, as a test assertion
//! does. The function is `async` and starts no runtime of its own.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use ragondin_benchmarks::identity::dataset_version;

use crate::backends::{DownloadProgress, PinnedBenchmark, Registry, RunDataset};
use crate::error::ApiError;
use crate::response::{BenchmarkEntry, BenchmarkState, GroundTruth};

/// A registry to check, and what it needs to be checked.
pub struct RegistryFixture<R> {
    /// A fresh registry: nothing downloaded, nothing imported.
    pub registry: R,
    /// A name its manifest holds, which it can download.
    pub obtainable: String,
    /// A name its manifest holds, whose source serves bytes its digests
    /// refuse.
    pub corrupt: String,
    /// A BEIR directory with qrels and no `answers.jsonl`, to import.
    pub importable: PathBuf,
    /// Changes what the registry stores for a downloaded benchmark, by name,
    /// behind its back — as a person editing the datasets directory would.
    /// The suite cannot do this through the trait, so the backend's test
    /// does it.
    pub alter: Alter<R>,
}

/// What [`RegistryFixture::alter`] is: given the registry, a benchmark's name
/// and what to do, it does it.
pub type Alter<R> = Box<dyn Fn(&R, &str, Alteration)>;

/// What [`RegistryFixture::alter`] does to a stored benchmark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alteration {
    /// Changes one document's text, so the dataset still loads and digests
    /// to another value.
    ChangeContent,
    /// Breaks it, so the dataset no longer loads.
    Break,
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
    failed_download(fresh()).await;
    cancelled_download(fresh()).await;
    unknown_names(fresh()).await;
    import(fresh()).await;
    run_dataset(fresh(), fresh()).await;
    altered_dataset(fresh(), Alteration::ChangeContent).await;
    altered_dataset(fresh(), Alteration::Break).await;
    pinned(fresh(), fresh()).await;
}

fn not_cancelled() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn assert_available(listed: &[BenchmarkEntry], name: &str, case: &str) {
    let entry = find(listed, name).unwrap_or_else(|| panic!("{case}: {name} is not listed"));
    match &entry.state {
        BenchmarkState::Available { size_bytes } => {
            assert!(*size_bytes > 0, "{case}: {name} has no size");
        }
        other => panic!("{case}: {name} is {other:?}, not available"),
    }
    assert!(
        entry
            .licence
            .as_deref()
            .is_some_and(|licence| !licence.is_empty()),
        "{case}: {name} shows no licence"
    );
    assert!(
        entry.ground_truth.is_some(),
        "{case}: {name} shows no ground truth before it is downloaded"
    );
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
    assert_available(&listed, &fixture.obtainable, "listing");
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
            not_cancelled(),
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
    assert!(
        downloaded.licence.is_some(),
        "download: a ready dataset keeps its licence"
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

    let again = fixture
        .registry
        .download(name, Arc::new(|_| {}), not_cancelled())
        .await;
    assert!(
        matches!(again, Err(ApiError::BenchmarkExists { .. })),
        "download: a second download is {again:?}, not benchmark_exists"
    );
}

async fn failed_download<R: Registry>(fixture: RegistryFixture<R>) {
    let name = fixture.corrupt.as_str();
    let before = fixture.registry.benchmarks().await.expect("failed: lists");
    let refused = fixture
        .registry
        .download(name, Arc::new(|_| {}), not_cancelled())
        .await;
    assert!(
        matches!(&refused, Err(ApiError::DownloadFailed { name: n, .. }) if n == name),
        "failed: downloading {name} is {refused:?}, not download_failed"
    );
    let after = fixture.registry.benchmarks().await.expect("failed: lists");
    assert_eq!(before, after, "failed: a failed download left something");
    assert_available(&after, name, "failed");
}

async fn cancelled_download<R: Registry>(fixture: RegistryFixture<R>) {
    let name = fixture.obtainable.as_str();
    let refused = fixture
        .registry
        .download(name, Arc::new(|_| {}), Arc::new(AtomicBool::new(true)))
        .await;
    assert!(
        matches!(&refused, Err(ApiError::DownloadCancelled { name: n }) if n == name),
        "cancelled: {refused:?}, not download_cancelled"
    );
    let listed = fixture
        .registry
        .benchmarks()
        .await
        .expect("cancelled: lists");
    assert_available(&listed, name, "cancelled");
}

async fn unknown_names<R: Registry>(fixture: RegistryFixture<R>) {
    let unknown = "beir/no-such-benchmark";
    let before = fixture.registry.benchmarks().await.expect("unknown: lists");
    let downloaded = fixture
        .registry
        .download(unknown, Arc::new(|_| {}), not_cancelled())
        .await;
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
    let refused = fixture
        .registry
        .import("nul\0name", &fixture.importable)
        .await;
    assert!(
        matches!(refused, Err(ApiError::ImportRefused { .. })),
        "import: a name holding NUL is {refused:?}"
    );
    let after = fixture.registry.benchmarks().await.expect("import: lists");
    assert_eq!(
        listed, after,
        "import: a refused import registered something"
    );
}

async fn run_dataset<R: Registry>(fixture: RegistryFixture<R>, untouched: RegistryFixture<R>) {
    let name = fixture.obtainable.as_str();
    let downloaded = fixture
        .registry
        .download(name, Arc::new(|_| {}), not_cancelled())
        .await
        .unwrap_or_else(|error| panic!("dataset: {name} does not download: {error}"));
    let BenchmarkState::Ready {
        dataset_version: version,
    } = &downloaded.state
    else {
        panic!("dataset: {name} is {:?}, not ready", downloaded.state);
    };
    assert_verified(
        &fixture.registry,
        version,
        "dataset: a downloaded benchmark",
    )
    .await;

    let imported = fixture
        .registry
        .import("run-dataset", &fixture.importable)
        .await
        .unwrap_or_else(|error| panic!("dataset: the fixture does not import: {error}"));
    let BenchmarkState::Local {
        dataset_version: imported_version,
    } = &imported.state
    else {
        panic!("dataset: {:?} is not local", imported.state);
    };
    assert_verified(&fixture.registry, imported_version, "dataset: an import").await;

    let absent = untouched
        .registry
        .dataset(version)
        .await
        .expect("dataset: a fresh registry answers");
    assert!(
        matches!(&absent, RunDataset::Absent { .. }),
        "dataset: {name}'s digest before any download is {absent:?}, not absent"
    );
    let nowhere = "0".repeat(64);
    let unknown = fixture
        .registry
        .dataset(&nowhere)
        .await
        .expect("dataset: answers");
    assert!(
        matches!(unknown, RunDataset::Unknown),
        "dataset: a digest nothing is pinned to is {unknown:?}, not unknown"
    );
}

async fn pinned<R: Registry>(fixture: RegistryFixture<R>, untouched: RegistryFixture<R>) {
    let name = fixture.obtainable.as_str();
    let downloaded = fixture
        .registry
        .download(name, Arc::new(|_| {}), not_cancelled())
        .await
        .unwrap_or_else(|error| panic!("pinned: {name} does not download: {error}"));
    let BenchmarkState::Ready {
        dataset_version: version,
    } = &downloaded.state
    else {
        panic!("pinned: {name} is {:?}, not ready", downloaded.state);
    };
    let pin = |name: &str, version: &str| PinnedBenchmark {
        name: name.to_owned(),
        dataset_version: version.to_owned(),
    };

    // A manifest entry is pinned to its digest whether or not it is on disk:
    // the pinning is the manifest's, and nothing is loaded to answer.
    let before = untouched
        .registry
        .pinned()
        .await
        .unwrap_or_else(|error| panic!("pinned: a fresh registry does not answer: {error}"));
    assert!(
        before.contains(&pin(name, version)),
        "pinned: {name} is pinned to {version} before any download, got {before:?}"
    );

    let imported = fixture
        .registry
        .import("pinned-import", &fixture.importable)
        .await
        .unwrap_or_else(|error| panic!("pinned: the fixture does not import: {error}"));
    let BenchmarkState::Local {
        dataset_version: imported_version,
    } = &imported.state
    else {
        panic!("pinned: {:?} is not local", imported.state);
    };
    let after = fixture
        .registry
        .pinned()
        .await
        .unwrap_or_else(|error| panic!("pinned: does not answer: {error}"));
    assert!(
        after.contains(&pin(name, version)),
        "pinned: {name} is still pinned to {version}, got {after:?}"
    );
    assert!(
        after.contains(&pin(&imported.name, imported_version)),
        "pinned: an import is pinned to the digest it recorded, got {after:?}"
    );
    let mut names: Vec<&str> = after.iter().map(|pinned| pinned.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), after.len(), "pinned: no name twice: {after:?}");
}

async fn altered_dataset<R: Registry>(fixture: RegistryFixture<R>, alteration: Alteration) {
    let name = fixture.obtainable.as_str();
    let case = format!("dataset after {alteration:?}");
    let downloaded = fixture
        .registry
        .download(name, Arc::new(|_| {}), not_cancelled())
        .await
        .unwrap_or_else(|error| panic!("{case}: {name} does not download: {error}"));
    let BenchmarkState::Ready {
        dataset_version: version,
    } = &downloaded.state
    else {
        panic!("{case}: {name} is {:?}, not ready", downloaded.state);
    };
    // Verified first, so a backend that keeps what it verified must notice
    // the change rather than answer from what it kept.
    assert_verified(&fixture.registry, version, &case).await;
    (fixture.alter)(&fixture.registry, name, alteration);
    let found = fixture
        .registry
        .dataset(version)
        .await
        .unwrap_or_else(|error| panic!("{case}: {error}"));
    match (alteration, &found) {
        (Alteration::ChangeContent, RunDataset::Differs { found, .. }) => {
            assert_ne!(found, version, "{case}: the digest found is the pinned one");
        }
        (Alteration::Break, RunDataset::Unreadable { error, .. }) => {
            assert!(!error.is_empty(), "{case}: no reason given");
        }
        _ => panic!("{case}: {found:?}"),
    }
}

async fn assert_verified<R: Registry>(registry: &R, version: &str, case: &str) {
    match registry.dataset(version).await {
        Ok(RunDataset::Verified { dataset, .. }) => assert_eq!(
            dataset_version(dataset.benchmark()),
            version,
            "{case}: the dataset handed back digests to another value"
        ),
        other => panic!("{case}: {other:?}, not verified"),
    }
}

fn find<'a>(listed: &'a [BenchmarkEntry], name: &str) -> Option<&'a BenchmarkEntry> {
    listed.iter().find(|entry| entry.name == name)
}
