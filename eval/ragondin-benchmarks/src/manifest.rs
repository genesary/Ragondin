//! The benchmark manifest: every dataset this build can obtain, each pinned by
//! digest, versioned with the crate (the design document § 6).
//!
//! An entry names its snapshot file by file — the URL of each file at a fixed
//! revision, its SHA-256 and its size — and the `dataset_version` the loaded
//! benchmark digests to, so that a download is checked against the identity a
//! run will carry and not only against the bytes. The registry knows nothing
//! else about a dataset. `ARCHITECTURE.md` § The benchmark manifest says why
//! each entry is in it, why the others are not, and why an entry is a list of
//! files rather than one archive.

use std::path::Path;

use crate::{BeirAdapter, Benchmark, BenchmarkAdapter, BenchmarkError, SquadAdapter};

/// A dataset format, as the text before the `/` of a benchmark selector names
/// it — the selectors `ragondin bench --benchmark` accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    /// `beir/<dir>`: a BEIR directory read by its qrels alone.
    Beir,
    /// `beir-qa/<dir>`: a BEIR directory with its `answers.jsonl`, which is
    /// then required.
    BeirQa,
    /// `squad/<dir>`: the SQuAD v1.1 file `dev-v1.1.json` in the directory.
    Squad,
}

impl Format {
    /// Every format, in selector order.
    pub const ALL: [Self; 3] = [Self::Beir, Self::BeirQa, Self::Squad];

    /// The selector prefix that names the format.
    pub fn selector(self) -> &'static str {
        match self {
            Self::Beir => "beir",
            Self::BeirQa => "beir-qa",
            Self::Squad => "squad",
        }
    }

    /// The format a selector prefix names, if any.
    pub fn from_selector(selector: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.selector() == selector)
    }

    /// Loads the dataset at `root` with the adapter this format names, as
    /// `ragondin bench` does.
    ///
    /// # Errors
    ///
    /// The adapter's own error, unchanged.
    pub fn load(self, root: &Path) -> Result<Benchmark, BenchmarkError> {
        match self {
            Self::Beir => BeirAdapter::new(root).load(),
            Self::BeirQa => BeirAdapter::new(root).with_reference_answers().load(),
            Self::Squad => SquadAdapter::new(root).load(),
        }
    }
}

/// One dataset the manifest names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestEntry {
    /// The benchmark selector, `<format>/<dir>` — `beir/scifact`.
    pub name: String,
    /// The format that reads it.
    pub format: Format,
    /// The dataset's licence: SPDX identifiers where the upstream states
    /// licences that have one.
    pub licence: String,
    /// Where the upstream states it, at a fixed revision.
    pub licence_url: String,
    /// The snapshot's files, each placed at its `path` under the dataset's
    /// directory.
    pub files: Vec<ManifestFile>,
    /// The `dataset_version` the loaded snapshot digests to
    /// ([`crate::identity::dataset_version`]), recorded once this repository
    /// loaded it.
    pub dataset_version: String,
}

impl ManifestEntry {
    /// The directory it occupies under the datasets directory: the name's
    /// text after the `/`.
    pub fn dir(&self) -> &str {
        self.name
            .split_once('/')
            .map_or(self.name.as_str(), |(_, dir)| dir)
    }

    /// The snapshot's size, every file together.
    pub fn size_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size_bytes).sum()
    }
}

/// One file of a snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestFile {
    /// Where it goes, relative to the dataset's directory: `qrels/test.tsv`.
    pub path: String,
    /// Where it is fetched from, at a fixed revision.
    pub url: String,
    /// Its SHA-256, lowercase hex. A file whose bytes digest otherwise is
    /// refused.
    pub sha256: String,
    /// Its size.
    pub size_bytes: u64,
}

/// The `mteb/scifact` dataset repository at the revision the manifest pins.
const SCIFACT: &str =
    "https://huggingface.co/datasets/mteb/scifact/resolve/cf10ab6856b15b0e670ef8ae5dae4e266c12d035";

/// Every dataset this build can obtain.
pub fn manifest() -> Vec<ManifestEntry> {
    vec![
        ManifestEntry {
            name: "beir/scifact".to_owned(),
            format: Format::Beir,
            licence: "CC-BY-4.0 AND ODC-By-1.0".to_owned(),
            licence_url: "https://github.com/allenai/scifact/blob/a5254a1bf108726a1704d8e191b8a7f2225a029f/LICENSE.md"
                .to_owned(),
            files: vec![
                file(
                    "corpus.jsonl",
                    &format!("{SCIFACT}/corpus.jsonl"),
                    "f0d32db0d156b526d75921ed7a76f2cb912902631c87248c5c97c617bad0b60c",
                    8_023_638,
                ),
                file(
                    "queries.jsonl",
                    &format!("{SCIFACT}/queries.jsonl"),
                    "9db7df096f7414435d52bafcbacf814c30cea50eb565c1e8fa6d11440759bba8",
                    129_085,
                ),
                file(
                    "qrels/test.tsv",
                    &format!("{SCIFACT}/qrels/test.tsv"),
                    "0864bb985e0ca2367ba217977e72004d549054b2b06666ed9d4825ac7c21284c",
                    5_389,
                ),
                file(
                    "qrels/train.tsv",
                    &format!("{SCIFACT}/qrels/train.tsv"),
                    "a53f2114831916c096b6c37d9e54da68cef4efdcdbd5ed46533601af972acf1d",
                    14_502,
                ),
            ],
            dataset_version: "9a07f80c0d4f1e9e74912d033a8d1fbd52c54b758dafcaa85c19abacfdee5f29".to_owned(),
        },
        ManifestEntry {
            name: "squad/dev".to_owned(),
            format: Format::Squad,
            licence: "CC-BY-SA-4.0".to_owned(),
            licence_url: "https://github.com/rajpurkar/SQuAD-explorer/blob/eee5fdbf62f8613a7812b03419e6b29617b74fd1/views/index.pug".to_owned(),
            files: vec![file(
                "dev-v1.1.json",
                "https://raw.githubusercontent.com/rajpurkar/SQuAD-explorer/240e165ab706d95bd4323653bb92421f446208cf/dataset/dev-v1.1.json",
                "95aa6a52d5d6a735563366753ca50492a658031da74f301ac5238b03966972c9",
                4_854_279,
            )],
            dataset_version: "e4e3b7605b66545c91fdfb2ac8b4ddb177df1b1b34e7f8e557f63f0dcf074010".to_owned(),
        },
    ]
}

fn file(path: &str, url: &str, sha256: &str, size_bytes: u64) -> ManifestFile {
    ManifestFile {
        path: path.to_owned(),
        url: url.to_owned(),
        sha256: sha256.to_owned(),
        size_bytes,
    }
}
