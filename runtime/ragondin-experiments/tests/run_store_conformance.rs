//! [`FileSystemRunStore`] against the conformance suite every `RunStore`
//! backend passes (`ragondin_experiments::conformance`).
//!
//! Built only with `--features conformance` (`required-features` in this
//! crate's manifest), which `just test-features` turns on: the suite is not
//! part of a production build of this crate.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use ragondin_experiments::conformance::assert_run_store_conformance;
use ragondin_experiments::{FileSystemRunStore, RunId};

#[test]
fn the_file_system_run_store_is_a_conformant_run_store() {
    // One directory per store the suite asks for, so no case sees another's
    // runs: the suite's contract is a fresh, empty store every time.
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("run_store_conformance");
    let _ = fs::remove_dir_all(&base);
    let next = AtomicUsize::new(0);

    assert_run_store_conformance(
        || FileSystemRunStore::new(base.join(next.fetch_add(1, Ordering::Relaxed).to_string())),
        // A torn run, as a hand deleting a file under the store root makes
        // one: the directory is there, one of the four files every run has is
        // not.
        |store: &FileSystemRunStore, id: &RunId| {
            fs::remove_file(store.root().join(id.to_string()).join("metrics.json"))
                .expect("a stored run has a metrics file to remove");
        },
        // A run directory the harness wrote before the launch record
        // existed, committed as it was: no `provenance.json`.
        || {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/stored-before-typed-trace");
            let id: RunId = "b41e0752792e728f5dd893043b42d2a2d71f0b0039157a177e0a267e0420ea6f"
                .parse()
                .expect("the fixture is named by a run id");
            assert!(!root.join(id.to_string()).join("provenance.json").exists());
            (FileSystemRunStore::new(root), id)
        },
    );
}
