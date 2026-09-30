//! What the dataset tests share: an in-memory fetcher, a scratch directory
//! per test, and the fixtures.
//!
//! The fetcher stands for the transport `datasets::download` is handed — the
//! experiment plane's API supplies an HTTP one — and serves bytes registered
//! by URL, in small chunks, so that every rule the download applies per chunk
//! is exercised. Nothing here touches the network.

#![allow(dead_code)] // each test binary uses a different part of this module

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ragondin_benchmarks::datasets::{Body, Fetcher};
use sha2::{Digest, Sha256};

/// Serves the bytes registered under a URL, `CHUNK` bytes at a time,
/// announcing their length first unless told not to.
pub struct Memory {
    pub files: BTreeMap<String, Vec<u8>>,
    /// Whether the length is announced before the bytes, as an HTTP
    /// `Content-Length` would.
    pub announce: bool,
    /// A length to announce instead of the true one.
    pub announce_as: Option<u64>,
    /// Keep writing after the download stopped the transfer, and report
    /// success: a transport that ignores the refusal.
    pub ignore_stops: bool,
}

pub const CHUNK: usize = 7;

impl Memory {
    pub fn serving(files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        Self {
            files: files.into_iter().collect(),
            announce: true,
            announce_as: None,
            ignore_stops: false,
        }
    }
}

impl Fetcher for Memory {
    fn fetch(&mut self, url: &str, body: &mut Body<'_>) -> Result<(), String> {
        let bytes = self
            .files
            .get(url)
            .ok_or_else(|| format!("404 Not Found for {url}"))?;
        if self.announce {
            let length = self.announce_as.unwrap_or(bytes.len() as u64);
            if body.announce(length).is_err() && !self.ignore_stops {
                return Err("stopped".to_owned());
            }
        }
        for chunk in bytes.chunks(CHUNK) {
            if body.write(chunk).is_err() && !self.ignore_stops {
                return Err("stopped".to_owned());
            }
        }
        Ok(())
    }
}

/// An empty directory of this test's own, under the target directory.
pub fn scratch(test_name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("ragondin-benchmarks")
        .join(test_name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("the scratch directory is creatable");
    path
}

/// A fixture directory of this crate.
pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// SHA-256 of `bytes`, lowercase hex.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

/// The names in `dir`, sorted.
pub fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
