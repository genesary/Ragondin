//! The queue's record on disk: `jobs/<id>.json`, one file per job, replaced
//! whole at each transition; and `jobs/<id>/partial/traces.json`, the traces
//! of the queries a failed or cancelled run executed — the shape the store
//! gives a run's `traces.json`, a map by query id, so a reader reads the two
//! alike. Nothing here writes under `runs/`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ragondin_experiments::TraceDocument;
use ragondin_types::QueryId;

use super::Job;
use crate::fs::write_atomically;
use crate::response::JobFault;

/// The partial traces' directory under a job's own, and their file.
const PARTIAL: &str = "partial";
const TRACES: &str = "traces.json";

/// `jobs/<id>.json`.
pub(super) fn path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// `jobs/<id>/partial/traces.json`.
pub(super) fn partial_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(id).join(PARTIAL).join(TRACES)
}

/// Every job `dir` holds, and every file in it that is not one — reported,
/// never repaired or removed. A `dir` that does not exist holds none: the
/// workspace creates it, and a router over a directory no workspace opened
/// has an empty queue until its first submission creates it.
pub(super) fn load(dir: &Path) -> (Vec<Job>, Vec<JobFault>) {
    let fault = |path: &Path, reason: String| JobFault {
        path: path.display().to_string(),
        reason,
    };
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (Vec::new(), Vec::new()),
        Err(error) => {
            return (
                Vec::new(),
                vec![fault(
                    dir,
                    format!("the queue's directory cannot be read: {error}"),
                )],
            )
        }
    };
    let mut jobs = Vec::new();
    let mut faults = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                faults.push(fault(dir, format!("an entry cannot be read: {error}")));
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // A job's own directory (its partial traces), and a write's staging
        // file, `.`-named, are not job files.
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        if name.starts_with('.') || path.is_dir() {
            continue;
        }
        match read(&path) {
            Ok(job) if job.id == stem => jobs.push(job),
            Ok(job) => faults.push(fault(
                &path,
                format!("it holds job {}, not {stem}; it was left out", job.id),
            )),
            Err(reason) => faults.push(fault(&path, format!("{reason}; it was left out"))),
        }
    }
    jobs.sort_by_key(|job| job.position);
    (jobs, faults)
}

fn read(path: &Path) -> Result<Job, String> {
    let bytes = fs::read(path).map_err(|error| format!("it cannot be read: {error}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("it is not a job this build reads: {error}"))
}

/// Replaces `jobs/<id>.json` with `job`, whole: a reader sees the previous
/// transition or this one.
pub(super) fn write(dir: &Path, job: &Job) -> Result<(), String> {
    let path = path(dir, &job.id);
    let bytes = serde_json::to_vec_pretty(job)
        .map_err(|error| format!("{} cannot be serialized: {error}", path.display()))?;
    fs::create_dir_all(dir)
        .and_then(|()| write_atomically(&path, &bytes))
        .map_err(|error| format!("{} cannot be written: {error}", path.display()))
}

/// Writes the traces of the queries a run executed before it stopped.
pub(super) fn write_partial(
    dir: &Path,
    id: &str,
    traces: &BTreeMap<QueryId, TraceDocument>,
) -> Result<(), String> {
    let path = partial_path(dir, id);
    let bytes = serde_json::to_vec(traces)
        .map_err(|error| format!("{} cannot be serialized: {error}", path.display()))?;
    path.parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| write_atomically(&path, &bytes))
        .map_err(|error| format!("{} cannot be written: {error}", path.display()))
}
