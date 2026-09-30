//! The manifest the binary carries: every entry complete, every name unique,
//! every format one `ragondin bench --benchmark` accepts, every URL a pinned
//! snapshot.

use std::collections::BTreeSet;

use ragondin_benchmarks::manifest::{manifest, Format};

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
