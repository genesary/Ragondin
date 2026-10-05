//! The manifest the binary carries: every entry complete, every name unique,
//! every format one `ragondin bench --benchmark` accepts, every URL a pinned
//! snapshot.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ragondin_benchmarks::manifest::{manifest, Format};
use ragondin_benchmarks::CarriedPieces;

#[test]
fn the_manifest_names_every_entry_completely_and_uniquely() {
    let entries = manifest();
    assert!(!entries.is_empty(), "the manifest names no dataset");

    let mut names = BTreeSet::new();
    let mut dirs = BTreeSet::new();
    for entry in &entries {
        assert!(!entry.name.is_empty(), "an entry has no name");
        assert!(
            names.insert(entry.name.clone()),
            "{} is named twice",
            entry.name
        );
        assert!(
            dirs.insert(entry.dir().to_owned()),
            "{}: its directory is another entry's",
            entry.name
        );

        // The name is the selector `bench` takes: `<format>/<dir>`.
        assert!(
            ["beir", "beir-qa", "squad"].contains(&entry.format.selector()),
            "{}: format {:?} is not one `ragondin bench` accepts",
            entry.name,
            entry.format
        );
        assert_eq!(
            entry.name,
            format!("{}/{}", entry.format.selector(), entry.dir()),
            "the name is the selector"
        );
        assert!(!entry.dir().is_empty() && !entry.dir().contains('/'));

        assert!(!entry.licence.is_empty(), "{}: no licence", entry.name);
        assert!(
            entry.licence_url.starts_with("https://"),
            "{}: the licence's terms are not named",
            entry.name
        );
        assert_eq!(
            entry.dataset_version.len(),
            64,
            "{}: dataset_version",
            entry.name
        );
        assert!(entry.size_bytes() > 0, "{}: no size", entry.name);

        assert!(!entry.files.is_empty(), "{}: no file", entry.name);
        let mut paths = BTreeSet::new();
        for file in &entry.files {
            assert!(!file.path.is_empty(), "{}: a file has no path", entry.name);
            assert!(
                paths.insert(file.path.clone()),
                "{}: {} twice",
                entry.name,
                file.path
            );
            assert!(
                !file.path.starts_with('/') && !file.path.split('/').any(|part| part == ".."),
                "{}: {} leaves the dataset directory",
                entry.name,
                file.path
            );
            assert!(
                file.url.starts_with("https://"),
                "{}: {}",
                entry.name,
                file.url
            );
            assert_eq!(
                file.sha256.len(),
                64,
                "{}: {} sha256",
                entry.name,
                file.path
            );
            assert!(
                file.sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{}: {} sha256 is not lowercase hex",
                entry.name,
                file.path
            );
            assert!(
                file.size_bytes > 0,
                "{}: {} has no size",
                entry.name,
                file.path
            );
        }
    }
}

/// A URL that names a branch reads whatever the branch holds today — a moving
/// source (`docs/system-architecture.md` § 9.1). Every URL names a commit.
#[test]
fn every_manifest_url_names_a_pinned_revision() {
    for entry in manifest() {
        for file in &entry.files {
            let pinned = file
                .url
                .split('/')
                .any(|part| part.len() == 40 && part.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(pinned, "{}: {} names no commit", entry.name, file.url);
        }
    }
}

#[test]
fn a_format_is_named_by_its_selector() {
    for format in Format::ALL {
        assert_eq!(Format::from_selector(format.selector()), Some(format));
    }
    assert_eq!(Format::from_selector("crag"), None);
}

/// The fixture corpus in each format's layout, read by that format's adapter.
fn fixture(format: Format) -> PathBuf {
    let dir = match format {
        Format::Beir => "beir-mini",
        Format::BeirQa => "beir-qa-mini",
        Format::Squad => "squad-mini",
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir)
}

/// The ground truth an entry declares is shown before its dataset is on disk,
/// so it must be what the dataset carries once loaded. The adapter of an
/// entry's format decides which pieces it can read: a fixture in the same
/// layout carries what the snapshot will. Each entry's `dataset_version`, which
/// a download is checked against, pins the snapshot itself.
#[test]
fn every_entry_declares_the_ground_truth_its_format_loads() {
    for entry in manifest() {
        let loaded = entry
            .format
            .load(&fixture(entry.format))
            .unwrap_or_else(|error| panic!("{}: the fixture loads: {error}", entry.name));
        assert_eq!(entry.carries, loaded.carries(), "{}", entry.name);
        assert_ne!(
            entry.carries,
            CarriedPieces::Neither,
            "{}: an entry that scores nothing is not worth offering",
            entry.name
        );
    }
}
