//! What the registry tests share: a local HTTP server, a scratch datasets
//! directory, and manifest entries over `ragondin-benchmarks`' own fixtures.
//!
//! The server speaks just enough HTTP/1.1, by hand over `std::net` on a
//! loopback port, to answer a `GET` with the bytes registered under its path
//! or a `404` — the pattern the `Remote` tests use (ADR-C33 § 6). No test
//! touches the network beyond the loopback interface.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use ragondin_benchmarks::identity::dataset_version;
use ragondin_benchmarks::manifest::{Format, ManifestEntry, ManifestFile};
use ragondin_benchmarks::CarriedPieces;
use sha2::{Digest, Sha256};

/// The BEIR fixture's files, in the order a manifest entry lists them.
pub const BEIR_FILES: [&str; 4] = [
    "corpus.jsonl",
    "queries.jsonl",
    "qrels/test.tsv",
    "qrels/train.tsv",
];

/// A fixture directory of `ragondin-benchmarks`: `beir-mini`, `beir-qa-mini`.
pub fn benchmark_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../eval/ragondin-benchmarks/tests/fixtures")
        .join(name)
}

/// The `dataset_version` of a BEIR fixture, loaded with `format`.
pub fn version_of(format: Format, root: &Path) -> String {
    dataset_version(&format.load(root).expect("the fixture loads"))
}

/// An empty directory of this test's own, under the target directory.
pub fn scratch(test_name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("ragondin-api")
        .join(test_name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("the scratch directory is creatable");
    path
}

/// Copies the directory `from` into `to`, recursively.
pub fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A manifest entry `name` for the `beir-mini` fixture, its files fetched
/// from `base` + `/<path>`, pinned to their true digests and to
/// `dataset_version`.
pub fn beir_mini_entry(name: &str, base: &str, dataset_version: &str) -> ManifestEntry {
    let root = benchmark_fixture("beir-mini");
    ManifestEntry {
        name: name.to_owned(),
        format: Format::Beir,
        licence: "CC-BY-4.0".to_owned(),
        licence_url: "https://example.invalid/licence".to_owned(),
        files: BEIR_FILES
            .iter()
            .map(|path| {
                let bytes = fs::read(root.join(path)).unwrap();
                ManifestFile {
                    path: (*path).to_owned(),
                    url: format!("{base}/{path}"),
                    sha256: sha256(&bytes),
                    size_bytes: bytes.len() as u64,
                }
            })
            .collect(),
        dataset_version: dataset_version.to_owned(),
        carries: CarriedPieces::QrelsOnly,
    }
}

/// SHA-256 of `bytes`, lowercase hex.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A server answering `GET <path>` with the bytes registered under `path`.
pub struct Server {
    base: String,
}

impl Server {
    /// Serves the `beir-mini` fixture's files under `/beir-mini/<path>`, and
    /// the same files with one byte of `queries.jsonl` changed under
    /// `/beir-mini-corrupt/<path>`.
    pub fn beir_mini() -> Self {
        let root = benchmark_fixture("beir-mini");
        let mut files = BTreeMap::new();
        for path in BEIR_FILES {
            let bytes = fs::read(root.join(path)).unwrap();
            let mut corrupt = bytes.clone();
            if path == "queries.jsonl" {
                corrupt[0] ^= 1;
            }
            files.insert(format!("/beir-mini/{path}"), bytes);
            files.insert(format!("/beir-mini-corrupt/{path}"), corrupt);
        }
        Self::serve(files)
    }

    /// Starts serving `files`, keyed by path, on a free loopback port. The
    /// thread lives as long as the test process.
    pub fn serve(files: BTreeMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let files = Arc::new(files);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let files = Arc::clone(&files);
                thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    if reader.read_line(&mut request_line).is_err() {
                        return;
                    }
                    loop {
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) | Err(_) => break,
                            Ok(_) if line == "\r\n" || line == "\n" => break,
                            Ok(_) => {}
                        }
                    }
                    let path = request_line.split_whitespace().nth(1).unwrap_or("");
                    let response = match files.get(path) {
                        Some(body) => {
                            let mut head = format!(
                                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                                body.len()
                            )
                            .into_bytes();
                            head.extend_from_slice(body);
                            head
                        }
                        None => b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                            .to_vec(),
                    };
                    let _ = stream.write_all(&response);
                    let _ = stream.flush();
                });
            }
        });
        Self { base }
    }

    /// The URL of `path` on this server.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}
