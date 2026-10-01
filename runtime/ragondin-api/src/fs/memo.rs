//! The datasets [`FsRegistry`](super::FsRegistry) keeps loaded between
//! requests, so that per-node and replay requests on one run load and digest
//! its dataset once rather than on every request.
//!
//! **An optimisation, never a truth** (the design document § 6). A dataset is
//! kept only once it has verified — loaded, and digested by
//! `ragondin_benchmarks::identity` to the `dataset_version` it is pinned to —
//! and it is served again only while its directory's *fingerprint* is the
//! one taken before that load. A fingerprint is not a digest: it decides when
//! to load and digest again, never whether a dataset verifies.
//!
//! **What a fingerprint cannot see.** A stamp is only as fine as the
//! filesystem's clock: ext4 before Linux 6.13 ticks per jiffy, FAT every two
//! seconds, NFS and SMB coarsely. A file rewritten in place, at the same size,
//! within one tick of its previous write keeps an equal stamp. So a dataset
//! any of whose stamps falls within [`RACY_MARGIN`] of the moment the
//! fingerprint was taken — or after it — is served but not kept: a later
//! rewrite then lands on a later tick. What remains unseen is a clock that
//! lies — a file's timestamps set back by hand *and* a status change time the
//! platform does not keep (not Unix), or a file server whose clock runs more
//! than the margin behind this machine's.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use crate::backends::{LoadedDataset, RunDataset};

/// How many verified datasets the registry keeps loaded: the one on screen
/// and the one just left, so moving between two benchmarks' runs loads
/// neither again. The bound is a count, not bytes — nothing measures a loaded
/// dataset's memory — so it is kept this small: a large corpus is gigabytes,
/// a kept dataset holds the corpus and its chunk set (about twice the
/// corpus), and a comparison or a matrix reads one benchmark.
pub(super) const CAPACITY: usize = 2;

/// How recent a stamp may be and the dataset still be kept: two seconds, the
/// coarsest clock among the filesystems a dataset is likely to sit on (FAT's).
pub(super) const RACY_MARGIN: Duration = Duration::from_secs(2);

/// Verified datasets, kept loaded between requests, at most `capacity` of
/// them, the least recently used dropped first.
///
/// One slot per dataset directory and pinned version. A request fingerprints
/// the directory with no lock held, locks the list only to find its slot,
/// then locks the slot alone while it compares and, if it must, loads: a
/// second request for the same dataset waits for that load and is served by
/// it, while a request for another dataset is not held up. Serving costs one
/// walk of the directory per request — a `stat` per entry, O(entries), no
/// file read.
pub(super) struct DatasetMemo {
    capacity: usize,
    margin: Duration,
    /// Most recently used first.
    slots: Mutex<Vec<(Key, Slot)>>,
}

/// One dataset's place in the memo: empty until it verifies.
type Slot = Arc<Mutex<Option<Held>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    version: String,
    dir: PathBuf,
}

/// A verified dataset, and the fingerprint its directory had before it was
/// loaded.
struct Held {
    fingerprint: Fingerprint,
    dataset: Arc<LoadedDataset>,
}

impl DatasetMemo {
    /// An empty memo keeping at most `capacity` datasets, none whose stamps
    /// fall within `margin` of its fingerprint ([`RACY_MARGIN`] in the
    /// registry; zero in a test whose files were just written).
    pub(super) fn new(capacity: usize, margin: Duration) -> Self {
        Self {
            capacity,
            margin,
            slots: Mutex::new(Vec::new()),
        }
    }

    /// The verdict on the directory `dir`, pinned to `version` under `name`:
    /// the dataset held for it while the directory's fingerprint is
    /// unchanged; otherwise `load(name)`'s, kept when it is `Verified` and
    /// none of the directory's stamps is within the margin.
    ///
    /// The fingerprint is taken *before* `load` reads the files — and before
    /// waiting for another request's load of the same dataset — so a file
    /// changed meanwhile is seen as a change by the next request, never
    /// hidden behind the dataset this one loaded. A directory that cannot be
    /// fingerprinted — absent, or unreadable — is loaded, and whatever was
    /// held for it dropped.
    pub(super) fn dataset(
        &self,
        version: &str,
        dir: &Path,
        name: String,
        load: impl FnOnce(String) -> RunDataset,
    ) -> RunDataset {
        let key = Key {
            version: version.to_owned(),
            dir: dir.to_path_buf(),
        };
        let fingerprint = Fingerprint::of(dir);
        let slot = self.slot(&key);
        let mut held = lock(&slot);
        let Ok((fingerprint, taken_at)) = fingerprint else {
            *held = None;
            self.forget(&key, &slot);
            return load(name);
        };
        if let Some(kept) = held.as_ref().filter(|kept| kept.fingerprint == fingerprint) {
            return RunDataset::Verified {
                name,
                dataset: Arc::clone(&kept.dataset),
            };
        }
        // Dropped before the load, so the old dataset and the new one are
        // not both in memory beside the other slots.
        *held = None;
        let found = load(name);
        match &found {
            RunDataset::Verified { dataset, .. } if !fingerprint.is_racy(taken_at, self.margin) => {
                *held = Some(Held {
                    fingerprint,
                    dataset: Arc::clone(dataset),
                });
            }
            _ => self.forget(&key, &slot),
        }
        found
    }

    /// The slot for `key`, made the most recently used — created if there is
    /// none, dropping the least recently used beyond the capacity.
    fn slot(&self, key: &Key) -> Slot {
        let mut slots = lock(&self.slots);
        let slot = match slots.iter().position(|(held, _)| held == key) {
            Some(at) => slots.remove(at).1,
            None => Arc::default(),
        };
        slots.insert(0, (key.clone(), Arc::clone(&slot)));
        slots.truncate(self.capacity);
        slot
    }

    /// Removes `slot` from the list, if it is still the one held for `key`.
    fn forget(&self, key: &Key, slot: &Slot) {
        lock(&self.slots).retain(|(held, kept)| held != key || !Arc::ptr_eq(kept, slot));
    }
}

impl fmt::Debug for DatasetMemo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let slots = lock(&self.slots);
        f.debug_struct("DatasetMemo")
            .field("capacity", &self.capacity)
            .field("margin", &self.margin)
            .field(
                "held",
                &slots.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// A lock whose holder panicked is still usable: what it guards is a cache,
/// and a slot is replaced whole, so no half-written state can be read.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a dataset directory looks like without reading its contents: every
/// entry under it, recursively, with its relative path, kind, size,
/// modification time and — where the platform has them — inode and status
/// change time. A file rewritten in place with its size and modification
/// time restored still changes its status change time, which no user call
/// can set; that is why it is read where it exists.
#[derive(Debug, PartialEq, Eq)]
struct Fingerprint(Vec<Stamp>);

#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    path: PathBuf,
    is_dir: bool,
    len: u64,
    modified: Option<SystemTime>,
    /// `(inode, status change time in seconds, nanoseconds)`, on Unix.
    status: Option<(u64, i64, i64)>,
}

impl Fingerprint {
    /// The directory's fingerprint, and the moment the walk finished.
    fn of(dir: &Path) -> io::Result<(Self, SystemTime)> {
        let mut stamps = Vec::new();
        let mut visited = HashSet::new();
        visited.insert(fs::canonicalize(dir)?);
        walk(dir, Path::new(""), &mut visited, &mut stamps)?;
        stamps.sort_by(|a, b| a.path.cmp(&b.path));
        Ok((Self(stamps), SystemTime::now()))
    }

    /// Whether a stamp is within `margin` of `taken_at`, or after it: a
    /// rewrite in the same tick of a coarse clock could leave it unchanged.
    fn is_racy(&self, taken_at: SystemTime, margin: Duration) -> bool {
        let threshold = taken_at
            .checked_sub(margin)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        self.0.iter().any(|stamp| {
            let changed = stamp.status.and_then(|(_, seconds, nanos)| {
                let seconds = u64::try_from(seconds).ok()?;
                let nanos = u32::try_from(nanos).ok()?;
                SystemTime::UNIX_EPOCH.checked_add(Duration::new(seconds, nanos))
            });
            [stamp.modified, changed]
                .into_iter()
                .flatten()
                .any(|time| time >= threshold)
        })
    }
}

/// Stamps every entry under `dir`. A symbolic link is stamped by its target
/// and, when that is a directory, walked through — as the loader reads it —
/// unless that directory was already walked, so a link loop cannot recurse.
fn walk(
    dir: &Path,
    relative: &Path,
    visited: &mut HashSet<PathBuf>,
    stamps: &mut Vec<Stamp>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let relative = relative.join(entry.file_name());
        let metadata = fs::metadata(&path)?;
        stamps.push(Stamp {
            path: relative.clone(),
            is_dir: metadata.is_dir(),
            len: metadata.len(),
            modified: metadata.modified().ok(),
            status: status(&metadata),
        });
        if metadata.is_dir() && visited.insert(fs::canonicalize(&path)?) {
            walk(&path, &relative, visited, stamps)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn status(metadata: &fs::Metadata) -> Option<(u64, i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.ino(), metadata.ctime(), metadata.ctime_nsec()))
}

#[cfg(not(unix))]
fn status(_: &fs::Metadata) -> Option<(u64, i64, i64)> {
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{Duration, SystemTime};

    use ragondin_benchmarks::{Benchmark, Qrels};

    use super::{DatasetMemo, CAPACITY};
    use crate::backends::{LoadedDataset, RunDataset};

    const VERSION: &str = "v";

    /// A directory of this test's own holding one file, under the system's
    /// temporary directory and this process's id.
    fn dataset_dir(test: &str, name: &str) -> PathBuf {
        let path = std::env::temp_dir()
            .join(format!("ragondin-api-memo-{}", std::process::id()))
            .join(test)
            .join(name);
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("qrels")).unwrap();
        fs::write(path.join("corpus.jsonl"), "the cat sat").unwrap();
        fs::write(path.join("qrels/test.tsv"), "q-1\td-1\t1").unwrap();
        path
    }

    /// Rewrites `path` with `bytes` and dates it `seconds` after the epoch:
    /// a stamp that differs from the file's previous one whatever the
    /// filesystem's timestamp granularity, so a test of a same-size rewrite
    /// does not depend on the clock having ticked.
    fn rewrite(path: &Path, bytes: &str, seconds: u64) {
        fs::write(path, bytes).unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    }

    /// A loader that counts its calls and answers `verified`.
    struct Loader {
        calls: AtomicUsize,
    }

    impl Loader {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn verified(&self, name: String) -> RunDataset {
            self.calls.fetch_add(1, Ordering::SeqCst);
            RunDataset::Verified {
                name,
                dataset: Arc::new(LoadedDataset::new(Benchmark::new(
                    Vec::new(),
                    Vec::new(),
                    Qrels::new(),
                ))),
            }
        }

        fn differs(&self, name: String) -> RunDataset {
            self.calls.fetch_add(1, Ordering::SeqCst);
            RunDataset::Differs {
                name,
                found: "other".to_owned(),
            }
        }
    }

    fn get(memo: &DatasetMemo, dir: &Path, loader: &Loader) -> Arc<LoadedDataset> {
        match memo.dataset(VERSION, dir, "beir/mini".to_owned(), |name| {
            loader.verified(name)
        }) {
            RunDataset::Verified { name, dataset } => {
                assert_eq!(name, "beir/mini", "the caller's name is answered");
                dataset
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_verified_dataset_is_loaded_once_while_its_files_are_unchanged() {
        let dir = dataset_dir("unchanged", "mini");
        let memo = DatasetMemo::new(CAPACITY, Duration::ZERO);
        let loader = Loader::new();

        let first = get(&memo, &dir, &loader);
        let second = get(&memo, &dir, &loader);

        assert_eq!(loader.calls(), 1);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn any_change_to_the_files_loads_the_dataset_again() {
        let dir = dataset_dir("changed", "mini");
        let memo = DatasetMemo::new(CAPACITY, Duration::ZERO);
        let loader = Loader::new();
        get(&memo, &dir, &loader);

        type Alteration<'a> = (&'a str, &'a dyn Fn(&Path));
        let alterations: [Alteration; 4] = [
            ("one byte, same size", &|dir| {
                rewrite(&dir.join("corpus.jsonl"), "the bat sat", 1_000);
            }),
            ("a file added in a subdirectory", &|dir| {
                fs::write(dir.join("qrels/train.tsv"), "").unwrap();
            }),
            ("a file renamed", &|dir| {
                fs::rename(dir.join("qrels/train.tsv"), dir.join("qrels/dev.tsv")).unwrap();
            }),
            ("a file removed", &|dir| {
                fs::remove_file(dir.join("qrels/dev.tsv")).unwrap();
            }),
        ];
        for (at, (alteration, alter)) in alterations.iter().enumerate() {
            alter(&dir);
            get(&memo, &dir, &loader);
            assert_eq!(loader.calls(), at + 2, "{alteration}: not loaded again");
            get(&memo, &dir, &loader);
            assert_eq!(loader.calls(), at + 2, "{alteration}: then kept");
        }
    }

    /// A symbolic link to a directory is read through by the loader, so a
    /// change under it is a change to the dataset.
    #[cfg(unix)]
    #[test]
    fn a_change_under_a_symlinked_directory_loads_the_dataset_again() {
        let dir = dataset_dir("symlinked", "mini");
        let elsewhere = dataset_dir("symlinked", "elsewhere").join("qrels");
        fs::remove_dir_all(dir.join("qrels")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.join("qrels")).unwrap();
        let memo = DatasetMemo::new(CAPACITY, Duration::ZERO);
        let loader = Loader::new();
        get(&memo, &dir, &loader);
        get(&memo, &dir, &loader);
        assert_eq!(loader.calls(), 1, "kept while nothing changes");

        rewrite(&elsewhere.join("test.tsv"), "q-1\td-1\t2", 1_000);
        get(&memo, &dir, &loader);

        assert_eq!(loader.calls(), 2, "the change under the link is seen");
    }

    /// A link loop is walked once, not forever.
    #[cfg(unix)]
    #[test]
    fn a_symlink_loop_is_fingerprinted_without_recursing_forever() {
        let dir = dataset_dir("loop", "mini");
        std::os::unix::fs::symlink(&dir, dir.join("qrels/back")).unwrap();
        let memo = DatasetMemo::new(CAPACITY, Duration::ZERO);
        let loader = Loader::new();

        get(&memo, &dir, &loader);
        get(&memo, &dir, &loader);

        assert_eq!(loader.calls(), 1);
    }

    /// A file written within the margin of the fingerprint may be rewritten
    /// within the same tick of a coarse filesystem clock and keep its stamp;
    /// so a dataset whose files are that recent is served but not kept, and
    /// is kept once they are older.
    #[test]
    fn a_dataset_written_within_the_margin_is_served_but_not_kept() {
        let dir = dataset_dir("racy", "mini");
        let margin = Duration::from_millis(400);
        let memo = DatasetMemo::new(CAPACITY, margin);
        let loader = Loader::new();

        get(&memo, &dir, &loader);
        get(&memo, &dir, &loader);
        assert_eq!(loader.calls(), 2, "too recent to be kept");

        thread::sleep(margin + Duration::from_millis(200));
        get(&memo, &dir, &loader);
        get(&memo, &dir, &loader);
        assert_eq!(loader.calls(), 3, "kept once older than the margin");
    }

    #[test]
    fn a_verdict_other_than_verified_is_not_kept() {
        let dir = dataset_dir("differs", "mini");
        let memo = DatasetMemo::new(CAPACITY, Duration::ZERO);
        let loader = Loader::new();

        for _ in 0..2 {
            let found = memo.dataset(VERSION, &dir, "beir/mini".to_owned(), |name| {
                loader.differs(name)
            });
            assert!(matches!(found, RunDataset::Differs { .. }), "{found:?}");
        }

        assert_eq!(loader.calls(), 2);
    }

    #[test]
    fn a_removed_directory_is_loaded_again_and_its_dataset_dropped() {
        let dir = dataset_dir("removed", "mini");
        let memo = DatasetMemo::new(CAPACITY, Duration::ZERO);
        let loader = Loader::new();
        let held = get(&memo, &dir, &loader);

        fs::remove_dir_all(&dir).unwrap();
        let absent = memo.dataset(VERSION, &dir, "beir/mini".to_owned(), |name| {
            loader.calls.fetch_add(1, Ordering::SeqCst);
            RunDataset::Absent { name }
        });

        assert!(matches!(absent, RunDataset::Absent { .. }), "{absent:?}");
        assert_eq!(loader.calls(), 2);
        assert_eq!(Arc::strong_count(&held), 1, "the memo let it go");
    }

    #[test]
    fn the_registry_keeps_at_most_its_capacity() {
        let memo = DatasetMemo::new(CAPACITY, Duration::ZERO);
        let loader = Loader::new();
        let dirs: Vec<PathBuf> = (0..=CAPACITY)
            .map(|at| dataset_dir("capacity", &format!("d-{at}")))
            .collect();
        let held: Vec<Arc<LoadedDataset>> =
            dirs.iter().map(|dir| get(&memo, dir, &loader)).collect();
        assert_eq!(loader.calls(), CAPACITY + 1);

        assert_eq!(
            Arc::strong_count(&held[0]),
            1,
            "the least recently used is dropped"
        );
        for dataset in &held[1..] {
            assert_eq!(Arc::strong_count(dataset), 2, "the others are kept");
        }
        for dir in &dirs[1..] {
            get(&memo, dir, &loader);
        }
        assert_eq!(loader.calls(), CAPACITY + 1, "the others are served");
        get(&memo, &dirs[0], &loader);
        assert_eq!(loader.calls(), CAPACITY + 2, "the dropped one is loaded");
    }

    #[test]
    fn a_dataset_read_again_is_the_most_recently_used() {
        let memo = DatasetMemo::new(2, Duration::ZERO);
        let loader = Loader::new();
        let [a, b, c] = ["a", "b", "c"].map(|name| dataset_dir("recency", name));

        get(&memo, &a, &loader);
        get(&memo, &b, &loader);
        get(&memo, &a, &loader);
        get(&memo, &c, &loader);
        assert_eq!(loader.calls(), 3, "a was read again, so b is dropped");

        get(&memo, &a, &loader);
        assert_eq!(loader.calls(), 3, "a is kept");
        get(&memo, &b, &loader);
        assert_eq!(loader.calls(), 4, "b was dropped");
    }

    #[test]
    fn one_dataset_is_keyed_by_its_version_as_well_as_its_directory() {
        let dir = dataset_dir("version", "mini");
        let memo = DatasetMemo::new(CAPACITY, Duration::ZERO);
        let loader = Loader::new();

        get(&memo, &dir, &loader);
        memo.dataset("another version", &dir, "beir/mini".to_owned(), |name| {
            loader.verified(name)
        });

        assert_eq!(loader.calls(), 2);
    }

    #[test]
    fn concurrent_requests_for_one_dataset_load_it_once() {
        let dir = dataset_dir("concurrent", "mini");
        let memo = Arc::new(DatasetMemo::new(CAPACITY, Duration::ZERO));
        let loader = Arc::new(Loader::new());
        let start = Arc::new(Barrier::new(4));

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let (memo, loader, start, dir) = (
                    Arc::clone(&memo),
                    Arc::clone(&loader),
                    Arc::clone(&start),
                    dir.clone(),
                );
                thread::spawn(move || {
                    start.wait();
                    memo.dataset(VERSION, &dir, "beir/mini".to_owned(), |name| {
                        thread::sleep(Duration::from_millis(50));
                        loader.verified(name)
                    })
                })
            })
            .collect();
        for handle in handles {
            assert!(matches!(
                handle.join().unwrap(),
                RunDataset::Verified { .. }
            ));
        }

        assert_eq!(loader.calls(), 1);
    }
}
