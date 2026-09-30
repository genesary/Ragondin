//! The datasets directory: a download pinned by digest, the verification of
//! what is on disk, and the import of a local corpus.
//!
//! Every download is served by a local HTTP server (`support::Server`); no
//! test touches the network.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ragondin_benchmarks::datasets::{
    download, import, local_entries, verify, DiskState, DownloadError, ImportError, Progress,
};
use ragondin_benchmarks::identity::dataset_version;
use ragondin_benchmarks::manifest::{Format, ManifestEntry, ManifestFile};
use ragondin_benchmarks::{BenchmarkError, CarriedPieces};

use support::{copy_dir, fixture, listing, scratch, sha256, Server};

/// The `dataset_version` a fixture digests to, loaded with `format`.
fn version_of(format: Format, root: &Path) -> String {
    dataset_version(&format.load(root).expect("the fixture loads"))
}

/// A manifest entry for the `squad-mini` fixture, served by `server` as
/// `served` — its true bytes, or altered ones.
fn squad_entry(server: &Server, sha256: String, dataset_version: String) -> ManifestEntry {
    let size = fs::metadata(fixture("squad-mini").join("dev-v1.1.json"))
        .unwrap()
        .len();
    ManifestEntry {
        name: "squad/mini".to_owned(),
        format: Format::Squad,
        licence: "CC-BY-SA-4.0".to_owned(),
        licence_url: "https://example.invalid/licence".to_owned(),
        files: vec![ManifestFile {
            path: "dev-v1.1.json".to_owned(),
            url: server.url("/dev-v1.1.json"),
            sha256,
            size_bytes: size,
        }],
        dataset_version,
    }
}

fn squad_bytes() -> Vec<u8> {
    fs::read(fixture("squad-mini").join("dev-v1.1.json")).unwrap()
}

fn serving(path: &str, bytes: Vec<u8>) -> Server {
    Server::serve(BTreeMap::from([(path.to_owned(), bytes)]))
}

#[test]
fn a_download_whose_archive_digest_matches_is_extracted_and_verified_against_the_manifest_dataset_version(
) {
    let bytes = squad_bytes();
    let server = serving("/dev-v1.1.json", bytes.clone());
    let expected = version_of(Format::Squad, &fixture("squad-mini"));
    let entry = squad_entry(&server, sha256(&bytes), expected.clone());
    let datasets = scratch("download_matches");

    let mut seen: Vec<Progress> = Vec::new();
    let verified = download(&entry, &datasets, &mut |progress| seen.push(progress))
        .expect("the snapshot verifies");

    assert_eq!(verified.dataset_version, expected);
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

    // What is on disk now verifies against the manifest.
    assert!(matches!(
        verify(&datasets.join("mini"), Format::Squad, &expected),
        DiskState::Verified(ref v) if v.dataset_version == expected
    ));
}

#[test]
fn a_download_of_several_files_places_each_at_its_path() {
    let root = fixture("beir-mini");
    let mut served = BTreeMap::new();
    let mut files = Vec::new();
    for path in [
        "corpus.jsonl",
        "queries.jsonl",
        "qrels/test.tsv",
        "qrels/train.tsv",
    ] {
        let bytes = fs::read(root.join(path)).unwrap();
        files.push(ManifestFile {
            path: path.to_owned(),
            url: String::new(),
            sha256: sha256(&bytes),
            size_bytes: bytes.len() as u64,
        });
        served.insert(format!("/beir-mini/{path}"), bytes);
    }
    let server = Server::serve(served);
    for file in &mut files {
        file.url = server.url(&format!("/beir-mini/{}", file.path));
    }
    let expected = version_of(Format::Beir, &root);
    let entry = ManifestEntry {
        name: "beir/mini".to_owned(),
        format: Format::Beir,
        licence: "CC-BY-4.0".to_owned(),
        licence_url: "https://example.invalid/licence".to_owned(),
        files,
        dataset_version: expected.clone(),
    };
    let datasets = scratch("download_several");

    let mut last = None;
    let verified = download(&entry, &datasets, &mut |progress| last = Some(progress)).unwrap();

    assert_eq!(verified.dataset_version, expected);
    assert_eq!(verified.carries, CarriedPieces::QrelsOnly);
    assert!(datasets.join("mini/qrels/test.tsv").is_file());
    assert_eq!(last.unwrap().received, entry.size_bytes());
    assert_eq!(last.unwrap().total, entry.size_bytes());
}

#[test]
fn a_download_whose_archive_digest_differs_is_refused_and_leaves_no_directory() {
    let bytes = squad_bytes();
    let mut altered = bytes.clone();
    // One byte changed, inside a string so the JSON would still parse: the
    // digest is what refuses it, not the adapter.
    let at = altered.iter().position(|&b| b == b'a').unwrap();
    altered[at] = b'b';
    let server = serving("/dev-v1.1.json", altered.clone());
    let entry = squad_entry(
        &server,
        sha256(&bytes),
        version_of(Format::Squad, &fixture("squad-mini")),
    );
    let datasets = scratch("download_digest_differs");

    let error = download(&entry, &datasets, &mut |_| {}).expect_err("the digest differs");

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
    let message = error.to_string();
    assert!(
        message.contains("squad/mini") && message.contains(&sha256(&altered)),
        "{message}"
    );
    assert!(
        listing(&datasets).is_empty(),
        "nothing is left under the datasets directory"
    );
}

#[test]
fn a_download_whose_loaded_dataset_version_differs_from_the_manifest_is_refused_and_leaves_no_directory(
) {
    let bytes = squad_bytes();
    let server = serving("/dev-v1.1.json", bytes.clone());
    let wrong = "0".repeat(64);
    let entry = squad_entry(&server, sha256(&bytes), wrong.clone());
    let datasets = scratch("download_version_differs");

    let error = download(&entry, &datasets, &mut |_| {}).expect_err("the version differs");

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
fn a_download_the_server_does_not_have_is_refused_and_leaves_no_directory() {
    let server = serving("/elsewhere", Vec::new());
    let bytes = squad_bytes();
    let entry = squad_entry(&server, sha256(&bytes), String::new());
    let datasets = scratch("download_not_found");

    let error = download(&entry, &datasets, &mut |_| {}).expect_err("404");

    assert!(
        matches!(&error, DownloadError::Fetch { entry, url, .. } if entry == "squad/mini" && url.ends_with("/dev-v1.1.json")),
        "{error:?}"
    );
    assert!(listing(&datasets).is_empty());
}

#[test]
fn a_download_into_an_occupied_directory_is_refused_and_leaves_it_untouched() {
    let bytes = squad_bytes();
    let server = serving("/dev-v1.1.json", bytes.clone());
    let entry = squad_entry(
        &server,
        sha256(&bytes),
        version_of(Format::Squad, &fixture("squad-mini")),
    );
    let datasets = scratch("download_occupied");
    fs::create_dir_all(datasets.join("mini")).unwrap();
    fs::write(datasets.join("mini/mine.txt"), "someone's").unwrap();

    let error = download(&entry, &datasets, &mut |_| {}).expect_err("occupied");

    assert!(matches!(error, DownloadError::Occupied { .. }), "{error:?}");
    assert_eq!(listing(&datasets.join("mini")), ["mine.txt"]);
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

    let qrels_only = import(&datasets, "mine", &fixture("beir-mini")).expect("it loads");
    assert_eq!(qrels_only.entry.name, "mine");
    assert_eq!(qrels_only.entry.format, Format::Beir);
    assert_eq!(qrels_only.entry.selector(), "beir/mine");
    assert_eq!(
        qrels_only.entry.dataset_version,
        version_of(Format::Beir, &fixture("beir-mini"))
    );
    assert_eq!(qrels_only.carries, CarriedPieces::QrelsOnly);

    let both = import(&datasets, "mine-qa", &fixture("beir-qa-mini")).expect("it loads");
    assert_eq!(both.entry.format, Format::BeirQa);
    assert_eq!(both.carries, CarriedPieces::QrelsAndReferenceAnswers);

    // Registered: listed, and what is on disk is what was imported.
    let listed = local_entries(&datasets).unwrap();
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
}

#[test]
fn importing_a_squad_file_registers_a_local_entry_with_reference_answers() {
    let datasets = scratch("import_squad");

    let imported = import(
        &datasets,
        "my-squad",
        &fixture("squad-mini").join("dev-v1.1.json"),
    )
    .expect("it loads");

    assert_eq!(imported.entry.format, Format::Squad);
    assert_eq!(imported.entry.selector(), "squad/my-squad");
    assert!(matches!(
        imported.carries,
        CarriedPieces::ReferenceAnswersOnly | CarriedPieces::QrelsAndReferenceAnswers
    ));
    assert!(datasets.join("my-squad/dev-v1.1.json").is_file());
    assert_eq!(local_entries(&datasets).unwrap(), [imported.entry]);
}

#[test]
fn importing_a_directory_the_adapter_refuses_registers_nothing_and_names_the_adapter_error() {
    let datasets = scratch("import_refused");
    let source = scratch("import_refused_source");
    copy_dir(&fixture("beir-mini"), &source);
    fs::write(source.join("corpus.jsonl"), "{ not json\n").unwrap();

    let error = import(&datasets, "broken", &source).expect_err("the adapter refuses it");

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
    assert!(local_entries(&datasets).unwrap().is_empty());
}

#[test]
fn importing_a_path_that_does_not_exist_is_refused_as_its_source_and_registers_nothing() {
    let datasets = scratch("import_missing");
    let missing = datasets.join("nowhere");

    let error = import(&datasets, "mine", &missing).expect_err("nothing to import");

    assert!(
        matches!(&error, ImportError::Source { path, .. } if path == &missing),
        "{error:?}"
    );
    assert!(listing(&datasets).is_empty());
}

#[test]
fn an_import_name_that_is_not_one_directory_or_is_taken_is_refused() {
    let datasets = scratch("import_names");
    for name in ["", ".hidden", "a/b", "..", "a\\b"] {
        assert!(
            matches!(
                import(&datasets, name, &fixture("beir-mini")),
                Err(ImportError::InvalidName { .. })
            ),
            "{name:?} is accepted"
        );
    }
    import(&datasets, "taken", &fixture("beir-mini")).unwrap();
    assert!(matches!(
        import(&datasets, "taken", &fixture("beir-mini")),
        Err(ImportError::Occupied { .. })
    ));
    // A manifest entry's directory is reserved for the manifest.
    assert!(matches!(
        import(&datasets, "scifact", &fixture("beir-mini")),
        Err(ImportError::Occupied { .. })
    ));
    assert_eq!(listing(&datasets), ["taken"]);
}
