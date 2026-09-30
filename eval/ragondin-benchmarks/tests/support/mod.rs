//! What the dataset tests share: a local HTTP server, a scratch directory per
//! test, and the fixtures.
//!
//! The server speaks just enough HTTP/1.1, by hand over `std::net` on a
//! loopback port, to answer a `GET` with the bytes registered under its path
//! or a `404` — the pattern the `Remote` tests use (ADR-C33 § 6), without an
//! async runtime. No test touches the network beyond the loopback interface.

#![allow(dead_code)] // each test binary uses a different part of this module

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use sha2::{Digest, Sha256};

/// A server answering `GET <path>` with the bytes registered under `path`.
pub struct Server {
    base: String,
}

impl Server {
    /// Starts serving `files`, keyed by path (`/dev-v1.1.json`), on a free
    /// loopback port. The thread lives as long as the test process.
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
                    // Drain the headers; the request has no body.
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
