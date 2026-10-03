//! The file backends: the workspace on disk behind [`PipelineSource`],
//! [`WorkspaceSettings`] and [`Registry`] (ADR-C36 § 2 places them in this
//! crate).
//!
//! - [`Workspace`], the workspace's layout: its root and every path derived
//!   from it, created when missing, its `workspace.toml` read before
//!   anything is created;
//! - [`FsSettings`], the `WorkspaceSettings` over `workspace.toml`, read on
//!   every call and changed in place, one key per operation, with the
//!   comments a person wrote kept (ADR-C38);
//! - [`FsPipelines`], the `PipelineSource` over `pipelines/<name>.yaml` and
//!   its layout;
//! - [`FsRegistry`], the `Registry` over the benchmark manifest and the
//!   datasets directory.
//!
//! The run store's file backend is not here: it is `ragondin-experiments`'
//! `FileSystemRunStore`.
//!
//! [`PipelineSource`]: crate::PipelineSource
//! [`WorkspaceSettings`]: crate::WorkspaceSettings
//! [`Registry`]: crate::Registry

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::ApiError;

mod memo;
mod pipelines;
mod registry;
mod settings;
mod settings_file;
mod workspace;

pub use pipelines::FsPipelines;
pub use registry::FsRegistry;
pub use settings::FsSettings;
pub(crate) use workspace::jobs_of;
pub use workspace::{Workspace, WorkspaceError};

/// Runs a call that reads or writes the disk on a blocking thread, so an
/// async worker does not wait on it.
async fn blocking<T, F>(call: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
{
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|error| ApiError::BackendFailed {
            detail: format!("a workspace call did not complete: {error}"),
        })?
}

/// Replaces `path` with `bytes` whole: written to a `.`-named file beside it,
/// flushed to disk, and renamed over it, so a reader sees the old bytes or the
/// new ones. The staging file is removed when anything fails.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path.parent().unwrap_or(Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Unique within this process by the clock, across processes by the pid;
    // the backends serialise their own writes, so the clock never repeats
    // for one file.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let staging = directory.join(format!(".{file_name}.write-{}-{nanos}", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&staging)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&staging, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staging);
    }
    result
}
