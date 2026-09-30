//! The datasets directory: a download pinned by digest, the verification of
//! what is on disk, and the import of a local corpus.
//!
//! Every download goes through an in-memory fetcher (`support::Memory`), the
//! transport seam the experiment plane's API fills with HTTP; no test touches
//! the network.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use ragondin_benchmarks::datasets::{
    download, import, local_entries, sweep_staging, verify, Body, Controls, DiskState,
    DownloadError, Fetcher, ImportError, Progress, Verified,
};
use ragondin_benchmarks::identity::dataset_version;
use ragondin_benchmarks::manifest::{manifest, Format, ManifestEntry, ManifestFile};
use ragondin_benchmarks::{BenchmarkError, CarriedPieces};

use support::{copy_dir, fixture, listing, scratch, sha256, Memory};

/// The `dataset_version` a fixture digests to, loaded with `format`.
fn version_of(format: Format, root: &Path) -> String {
    dataset_version(&format.load(root).expect("the fixture loads"))
}

const SQUAD_URL: &str = "mem://squad/dev-v1.1.json";

fn squad_bytes() -> Vec<u8> {
    fs::read(fixture("squad-mini").join("dev-v1.1.json")).unwrap()
}

/// A manifest entry for the `squad-mini` fixture, pinned to `sha256` and
/// `dataset_version`.
fn squad_entry(sha256: String, dataset_version: String) -> ManifestEntry {
    ManifestEntry {
        name: "squad/mini".to_owned(),
        format: Format::Squad,
        licence: "CC-BY-SA-4.0".to_owned(),
        licence_url: "https://example.invalid/licence".to_owned(),
        files: vec![ManifestFile {
            path: "dev-v1.1.json".to_owned(),
            url: SQUAD_URL.to_owned(),
            sha256,
            size_bytes: squad_bytes().len() as u64,
        }],
        dataset_version,
    }
}

/// The `squad-mini` entry pinned to its true digests.
fn true_squad_entry() -> ManifestEntry {
    squad_entry(
        sha256(&squad_bytes()),
        version_of(Format::Squad, &fixture("squad-mini")),
    )
}

fn serving_squad(bytes: Vec<u8>) -> Memory {
    Memory::serving([(SQUAD_URL.to_owned(), bytes)])
}

/// A download with no cancellation and a generous deadline.
fn run(
    entry: &ManifestEntry,
    datasets: &Path,
    fetcher: &mut dyn Fetcher,
) -> Result<Verified, DownloadError> {
    let cancelled = AtomicBool::new(false);
    download(
        entry,
        datasets,
        fetcher,
        Controls {
            progress: &mut |_| {},
            cancelled: &cancelled,
            deadline: Duration::from_secs(60),
        },
    )
}

#[test]
fn a_download_whose_archive_digest_matches_is_extracted_and_verified_against_the_manifest_dataset_version(
) {
    let bytes = squad_bytes();
    let entry = true_squad_entry();
    let datasets = scratch("download_matches");
    let cancelled = AtomicBool::new(false);

    let mut seen: Vec<Progress> = Vec::new();
    let verified = download(
        &entry,
        &datasets,
        &mut serving_squad(bytes.clone()),
        Controls {
            progress: &mut |progress| seen.push(progress),
            cancelled: &cancelled,
            deadline: Duration::from_secs(60),
        },
    )
    .expect("the snapshot verifies");

    assert_eq!(verified.dataset_version, entry.dataset_version);
    assert_eq!(verified.carries, CarriedPieces::QrelsAndReferenceAnswers);
    assert_eq!(listing(&datasets), ["mini"], "only the dataset's directory");
    assert_eq!(
        fs::read(datasets.join("mini/dev-v1.1.json")).unwrap(),
        bytes
    );
    let last = seen.last().expect("progress is reported");
    assert_eq!(last.received, bytes.len() as u64);
    assert_eq!(last.total, bytes.len() as u64);
    assert!(
        seen.windows(2)
            .all(|pair| pair[0].received <= pair[1].received),
        "bytes received never go back"
    );
    assert!(matches!(
        verify(&datasets.join("mini"), Format::Squad, &entry.dataset_version),
        DiskState::Verified(ref v) if v.dataset_version == entry.dataset_version
    ));
}

#[test]
fn a_download_of_several_files_places_each_at_its_path() {
    let root = fixture("beir-mini");
    let mut served = Vec::new();
    let mut files = Vec::new();
    for path in [
        "corpus.jsonl",
        "queries.jsonl",
        "qrels/test.tsv",
        "qrels/train.tsv",
    ] {
        let bytes = fs::read(root.join(path)).unwrap();
        let url = format!("mem://beir-mini/{path}");
        files.push(ManifestFile {
            path: path.to_owned(),
            url: url.clone(),
            sha256: sha256(&bytes),
            size_bytes: bytes.len() as u64,
        });
        served.push((url, bytes));
    }
    let entry = ManifestEntry {
        name: "beir/mini".to_owned(),
        format: Format::Beir,
        licence: "CC-BY-4.0".to_owned(),
        licence_url: "https://example.invalid/licence".to_owned(),
        files,
        dataset_version: version_of(Format::Beir, &root),
    };
    let datasets = scratch("download_several");

    let verified = run(&entry, &datasets, &mut Memory::serving(served)).unwrap();

    assert_eq!(verified.dataset_version, entry.dataset_version);
    assert_eq!(verified.carries, CarriedPieces::QrelsOnly);
    assert!(datasets.join("mini/qrels/test.tsv").is_file());
}

#[test]
fn a_download_whose_archive_digest_differs_is_refused_and_leaves_no_directory() {
    let bytes = squad_bytes();
    let mut altered = bytes.clone();
    // One byte changed, inside a string so the JSON would still parse: the
    // digest is what refuses it, not the adapter.
    let at = altered.iter().position(|&b| b == b'a').unwrap();
    altered[at] = b'b';
    let entry = true_squad_entry();
    let datasets = scratch("download_digest_differs");

    let error = run(&entry, &datasets, &mut serving_squad(altered.clone()))
        .expect_err("the digest differs");

    match &error {
        DownloadError::FileDigest {
            entry: name,
            file,
            expected,
            found,
        } => {
            assert_eq!(name, "squad/mini");
            assert_eq!(file, "dev-v1.1.json");
            assert_eq!(expected, &sha256(&bytes));
            assert_eq!(found, &sha256(&altered));
        }
        other => panic!("expected a file digest error, got {other:?}"),
    }
    assert!(
        listing(&datasets).is_empty(),
        "nothing is left under the datasets directory"
    );
}

#[test]
fn a_download_whose_loaded_dataset_version_differs_from_the_manifest_is_refused_and_leaves_no_directory(
) {
    let bytes = squad_bytes();
    let wrong = "0".repeat(64);
    let entry = squad_entry(sha256(&bytes), wrong.clone());
    let datasets = scratch("download_version_differs");

    let error = run(&entry, &datasets, &mut serving_squad(bytes)).expect_err("the version differs");

    match &error {
        DownloadError::DatasetVersion {
            entry: name,
            expected,
            found,
        } => {
            assert_eq!(name, "squad/mini");
            assert_eq!(expected, &wrong);
            assert_eq!(found, &version_of(Format::Squad, &fixture("squad-mini")));
        }
        other => panic!("expected a dataset version error, got {other:?}"),
    }
    assert!(
        listing(&datasets).is_empty(),
        "nothing is left under the datasets directory"
    );
}

#[test]
fn a_download_the_transport_fails_is_refused_and_leaves_no_directory() {
    let datasets = scratch("download_transport_fails");
    let mut nothing = Memory::serving([]);

    let error = run(&true_squad_entry(), &datasets, &mut nothing).expect_err("404");

    assert!(
        matches!(&error, DownloadError::Fetch { entry, url, reason }
            if entry == "squad/mini" && url == SQUAD_URL && reason.contains("404")),
        "{error:?}"
    );
    assert!(listing(&datasets).is_empty());
}

#[test]
fn a_download_into_an_occupied_directory_is_refused_and_leaves_it_untouched() {
    let datasets = scratch("download_occupied");
    fs::create_dir_all(datasets.join("mini")).unwrap();
    fs::write(datasets.join("mini/mine.txt"), "someone's").unwrap();

    let error = run(
        &true_squad_entry(),
        &datasets,
        &mut serving_squad(squad_bytes()),
    )
    .expect_err("occupied");

    assert!(matches!(error, DownloadError::Occupied { .. }), "{error:?}");
    assert_eq!(listing(&datasets), ["mini"]);
    assert_eq!(listing(&datasets.join("mini")), ["mine.txt"]);
}

#[test]
fn an_announced_length_beyond_the_manifest_size_is_refused_before_a_byte_is_written() {
    let datasets = scratch("download_announced_too_large");
    let mut fetcher = serving_squad(squad_bytes());
    fetcher.announce_as = Some(50 * 1024 * 1024);
    let limit = squad_bytes().len() as u64;

    let error = run(&true_squad_entry(), &datasets, &mut fetcher).expect_err("too large");

    assert!(
        matches!(&error, DownloadError::TooLarge { entry, file, limit: l, seen }
            if entry == "squad/mini" && file == "dev-v1.1.json" && *l == limit && *seen == 50 * 1024 * 1024),
        "{error:?}"
    );
    assert!(listing(&datasets).is_empty());
}

#[test]
fn a_body_that_runs_past_the_manifest_size_is_cut_off_and_refused() {
    let datasets = scratch("download_body_too_large");
    let mut bytes = squad_bytes();
    bytes.extend(std::iter::repeat_n(b' ', 1000));
    let mut fetcher = serving_squad(bytes);
    fetcher.announce = false;
    // A transport that keeps going after being told to stop is still refused.
    fetcher.ignore_stops = true;

    let error = run(&true_squad_entry(), &datasets, &mut fetcher).expect_err("too large");

    let limit = squad_bytes().len() as u64;
    assert!(
        matches!(&error, DownloadError::TooLarge { limit: l, seen, .. } if *l == limit && *seen > limit),
        "{error:?}"
    );
    assert!(listing(&datasets).is_empty());
}

#[test]
fn a_body_shorter_than_the_manifest_size_is_refused_as_truncated() {
    let datasets = scratch("download_truncated");
    let bytes = squad_bytes();
    let short = bytes[..bytes.len() - 10].to_vec();
    let mut fetcher = serving_squad(short.clone());
    fetcher.announce = false;

    let error = run(&true_squad_entry(), &datasets, &mut fetcher).expect_err("short");

    assert!(
        matches!(&error, DownloadError::Truncated { expected, received, .. }
            if *expected == bytes.len() as u64 && *received == short.len() as u64),
        "{error:?}"
    );
    assert!(listing(&datasets).is_empty());
}

#[test]
fn a_cancelled_download_is_refused_as_cancelled_and_leaves_no_directory() {
    let datasets = scratch("download_cancelled");
    let cancelled = AtomicBool::new(false);
    let mut chunks = 0;

    let error = download(
        &true_squad_entry(),
        &datasets,
        &mut serving_squad(squad_bytes()),
        Controls {
            // Cancelled from outside once the transfer is under way.
            progress: &mut |_| {
                chunks += 1;
                if chunks == 3 {
                    cancelled.store(true, Ordering::Relaxed);
                }
            },
            cancelled: &cancelled,
            deadline: Duration::from_secs(60),
        },
    )
    .expect_err("cancelled");

    assert!(
        matches!(&error, DownloadError::Cancelled { entry } if entry == "squad/mini"),
        "{error:?}"
    );
    assert!(listing(&datasets).is_empty());
}

/// A transport that trickles: each chunk after a pause.
struct Trickle(Memory);

impl Fetcher for Trickle {
    fn fetch(&mut self, url: &str, body: &mut Body<'_>) -> Result<(), String> {
        let bytes = self.0.files[url].clone();
        for chunk in bytes.chunks(64) {
            thread::sleep(Duration::from_millis(5));
            body.write(chunk).map_err(|stop| stop.to_string())?;
        }
        Ok(())
    }
}

#[test]
fn a_download_that_outlives_its_deadline_is_refused_and_leaves_no_directory() {
    let datasets = scratch("download_deadline");
    let cancelled = AtomicBool::new(false);

    let error = download(
        &true_squad_entry(),
        &datasets,
        &mut Trickle(serving_squad(squad_bytes())),
        Controls {
            progress: &mut |_| {},
            cancelled: &cancelled,
            deadline: Duration::from_millis(20),
        },
    )
    .expect_err("too slow");

    assert!(
        matches!(&error, DownloadError::DeadlineExceeded { entry, .. } if entry == "squad/mini"),
        "{error:?}"
    );
    assert!(listing(&datasets).is_empty());
}

/// A transport that waits at a barrier before sending, so that two downloads
/// are both under way before either finishes.
struct Together(Memory, Arc<Barrier>);

impl Fetcher for Together {
    fn fetch(&mut self, url: &str, body: &mut Body<'_>) -> Result<(), String> {
        self.1.wait();
        self.0.fetch(url, body)
    }
}

#[test]
fn two_overlapping_downloads_of_one_entry_publish_once_and_refuse_the_other_cleanly() {
    let datasets = scratch("download_overlapping");
    let barrier = Arc::new(Barrier::new(2));
    let entry = true_squad_entry();

    let results: Vec<Result<Verified, DownloadError>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let entry = &entry;
                let datasets: &PathBuf = &datasets;
                scope.spawn(move || {
                    run(
                        entry,
                        datasets,
                        &mut Together(serving_squad(squad_bytes()), barrier),
                    )
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let published = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(published, 1, "exactly one publishes: {results:?}");
    assert!(
        results
            .iter()
            .any(|result| matches!(result, Err(DownloadError::Occupied { .. }))),
        "the other is refused as occupied: {results:?}"
    );
    assert_eq!(listing(&datasets), ["mini"], "no staging directory is left");
    assert!(matches!(
        verify(
            &datasets.join("mini"),
            Format::Squad,
            &entry.dataset_version
        ),
        DiskState::Verified(_)
    ));
}

#[test]
fn sweeping_removes_staging_left_by_an_interrupted_download_or_import_and_nothing_else() {
    let datasets = scratch("sweep");
    for dir in [".mini.download-1-2", ".mine.import-3-4", "mini", ".hidden"] {
        fs::create_dir_all(datasets.join(dir)).unwrap();
    }

    let removed = sweep_staging(&datasets).unwrap();

    assert_eq!(removed, 2);
    assert_eq!(listing(&datasets), [".hidden", "mini"]);
    assert_eq!(sweep_staging(&datasets.join("absent")).unwrap(), 0);
}

#[test]
fn verification_reports_ready_differs_or_unreadable_for_a_dataset_on_disk() {
    let datasets = scratch("verification");
    let dir = datasets.join("mini");
    copy_dir(&fixture("beir-mini"), &dir);
    let expected = version_of(Format::Beir, &fixture("beir-mini"));

    match verify(&dir, Format::Beir, &expected) {
        DiskState::Verified(verified) => {
            assert_eq!(verified.dataset_version, expected);
            assert_eq!(verified.carries, CarriedPieces::QrelsOnly);
        }
        other => panic!("expected verified, got {other:?}"),
    }

    // The content changed: one query's text.
    let queries = fs::read_to_string(dir.join("queries.jsonl")).unwrap();
    let edited = queries.replacen("\"text\": \"", "\"text\": \"edited ", 1);
    assert_ne!(queries, edited, "the fixture has a query to edit");
    fs::write(dir.join("queries.jsonl"), edited).unwrap();
    let found = version_of(Format::Beir, &dir);
    match verify(&dir, Format::Beir, &expected) {
        DiskState::Differs {
            expected: e,
            found: f,
            carries,
        } => {
            assert_eq!(e, expected);
            assert_eq!(f, found);
            assert_eq!(
                carries,
                CarriedPieces::QrelsOnly,
                "it loaded, so its pieces are known"
            );
        }
        other => panic!("expected differs, got {other:?}"),
    }

    // A directory that does not load: the adapter's own error.
    fs::remove_file(dir.join("corpus.jsonl")).unwrap();
    match verify(&dir, Format::Beir, &expected) {
        DiskState::Unreadable { error } => {
            assert!(matches!(error, BenchmarkError::Io { .. }), "{error:?}")
        }
        other => panic!("expected unreadable, got {other:?}"),
    }

    assert!(matches!(
        verify(&datasets.join("nothing"), Format::Beir, &expected),
        DiskState::Absent
    ));
}

#[test]
fn importing_a_beir_directory_registers_a_local_entry_with_its_carried_pieces() {
    let datasets = scratch("import_beir");

    let qrels_only = import(&datasets, "mine", &fixture("beir-mini"), &[]).expect("it loads");
    assert_eq!(qrels_only.entry.name, "mine");
    assert_eq!(qrels_only.entry.format, Format::Beir);
    assert_eq!(qrels_only.entry.selector(), "beir/mine");
    assert_eq!(
        qrels_only.entry.dataset_version,
        version_of(Format::Beir, &fixture("beir-mini"))
    );
    assert_eq!(qrels_only.carries, CarriedPieces::QrelsOnly);

    let both = import(&datasets, "mine-qa", &fixture("beir-qa-mini"), &[]).expect("it loads");
    assert_eq!(both.entry.format, Format::BeirQa);
    assert_eq!(both.carries, CarriedPieces::QrelsAndReferenceAnswers);

    // Registered: listed, and what is on disk is what was imported.
    let listed: Vec<_> = local_entries(&datasets)
        .unwrap()
        .into_iter()
        .map(|entry| entry.expect("a marker this build wrote"))
        .collect();
    assert_eq!(listed, [qrels_only.entry.clone(), both.entry.clone()]);
    for entry in &listed {
        assert!(matches!(
            verify(
                &datasets.join(&entry.name),
                entry.format,
                &entry.dataset_version
            ),
            DiskState::Verified(_)
        ));
    }
    assert_eq!(
        listing(&datasets),
        ["mine", "mine-qa"],
        "no staging directory is left"
    );
}

#[test]
fn importing_a_squad_file_registers_a_local_entry_with_reference_answers() {
    let datasets = scratch("import_squad");

    let imported = import(
        &datasets,
        "my-squad",
        &fixture("squad-mini").join("dev-v1.1.json"),
        &[],
    )
    .expect("it loads");

    assert_eq!(imported.entry.format, Format::Squad);
    assert_eq!(imported.entry.selector(), "squad/my-squad");
    assert!(matches!(
        imported.carries,
        CarriedPieces::ReferenceAnswersOnly | CarriedPieces::QrelsAndReferenceAnswers
    ));
    assert!(datasets.join("my-squad/dev-v1.1.json").is_file());
    let listed = local_entries(&datasets).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].as_ref().unwrap(), &imported.entry);
}

#[test]
fn importing_a_directory_the_adapter_refuses_registers_nothing_and_names_the_adapter_error() {
    let datasets = scratch("import_refused");
    let source = scratch("import_refused_source");
    copy_dir(&fixture("beir-mini"), &source);
    fs::write(source.join("corpus.jsonl"), "{ not json\n").unwrap();

    let error = import(&datasets, "broken", &source, &[]).expect_err("the adapter refuses it");

    match &error {
        ImportError::Load { source, .. } => {
            assert!(
                matches!(source, BenchmarkError::MalformedJson { .. }),
                "{source:?}"
            )
        }
        other => panic!("expected the adapter's error, got {other:?}"),
    }
    assert!(listing(&datasets).is_empty(), "nothing is registered");
}

#[test]
fn importing_a_path_that_does_not_exist_is_refused_as_its_source_and_registers_nothing() {
    let datasets = scratch("import_missing");
    let missing = datasets.join("nowhere");

    let error = import(&datasets, "mine", &missing, &[]).expect_err("nothing to import");

    assert!(
        matches!(&error, ImportError::Source { path, .. } if path == &missing),
        "{error:?}"
    );
    assert!(listing(&datasets).is_empty());
}

#[test]
fn an_import_name_outside_the_allowed_characters_is_refused() {
    let datasets = scratch("import_names");
    let long = "a".repeat(65);
    for name in [
        "",
        ".hidden",
        "a/b",
        "..",
        "a\\b",
        "nul\0byte",
        "with space",
        "é",
        long.as_str(),
        // Reserved device names on Windows, whatever the case or extension.
        "CON",
        "nul",
        "aux.txt",
        "COM1",
        "lpt9.json",
        // Windows strips a trailing dot.
        "trailing.",
    ] {
        assert!(
            matches!(
                import(&datasets, name, &fixture("beir-mini"), &[]),
                Err(ImportError::InvalidName { .. })
            ),
            "{name:?} is accepted"
        );
    }
    for name in ["a.b", "_x", "-x", "A-1_b.c", "a".repeat(64).as_str()] {
        import(&datasets, name, &fixture("beir-mini"), &[])
            .unwrap_or_else(|error| panic!("{name:?} is refused: {error}"));
    }
}

#[test]
fn an_import_name_taken_by_a_dataset_or_by_the_given_manifest_is_refused() {
    let datasets = scratch("import_taken");
    import(&datasets, "taken", &fixture("beir-mini"), &[]).unwrap();
    assert!(matches!(
        import(&datasets, "taken", &fixture("beir-mini"), &[]),
        Err(ImportError::Occupied { .. })
    ));

    // The manifest the caller holds reserves its entries' directories.
    let reserved = manifest();
    assert!(matches!(
        import(&datasets, "scifact", &fixture("beir-mini"), &reserved),
        Err(ImportError::Occupied { .. })
    ));
    // A name no manifest given reserves is free.
    import(&datasets, "scifact", &fixture("beir-mini"), &[]).expect("not reserved here");
    assert_eq!(listing(&datasets), ["scifact", "taken"]);
}

#[test]
fn a_marker_that_does_not_parse_is_reported_for_its_directory_alone() {
    let datasets = scratch("marker_malformed");
    import(&datasets, "good", &fixture("beir-mini"), &[]).unwrap();
    fs::create_dir_all(datasets.join("bad")).unwrap();
    fs::write(datasets.join("bad/ragondin-local.json"), "{ not json").unwrap();

    let listed = local_entries(&datasets).unwrap();

    assert_eq!(listed.len(), 2);
    let bad = listed[0].as_ref().expect_err("the malformed marker");
    assert_eq!(bad.name, "bad");
    assert!(bad.reason.contains("ragondin-local.json"), "{}", bad.reason);
    assert_eq!(listed[1].as_ref().unwrap().name, "good");
}
