//! `FsPipelines`: the `PipelineSource` backend over `pipelines/`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use super::{blocking, write_atomically, Workspace};
use crate::backends::{PipelineFile, PipelineSource, Precondition, Revision};
use crate::error::ApiError;
use crate::response::Layout;
use crate::validation;

/// A document's extension, and its layout's.
const DOCUMENT: &str = ".yaml";
const LAYOUT: &str = ".layout.json";
/// The one layout format this build reads and writes.
const LAYOUT_VERSION: u32 = 1;
/// A name the router gives a route of its own, `POST /pipelines/validate`,
/// so a pipeline under it could not be read.
const RESERVED: &str = "validate";

/// The pipelines of a workspace: `pipelines/<name>.yaml`, its layout beside it
/// as `pipelines/<name>.layout.json`.
///
/// **The bytes are kept verbatim.** A document is stored exactly as it was
/// sent and read exactly as it is stored — never parsed and re-serialized —
/// so the comments and the formatting a person gave it survive the editor,
/// as the run store keeps a run's configuration text.
///
/// **The revision is a digest of the bytes**: the SHA-256 of the file, in
/// hex. It needs no clock, so it cannot race on a filesystem whose
/// timestamps are coarse, and a document written back unchanged keeps it.
/// A write states what it expects ([`Precondition`]), and is refused, with
/// the current revision, when the stored bytes digest to anything else.
///
/// A write is validated first, checked against its precondition second, and
/// stored last, written beside the file and renamed over it — so a refused
/// write leaves nothing, and a reader sees the old bytes or the new ones.
/// Writes from this process are serialised; an editor outside it that saves
/// between the check and the rename is the one race left, and the next
/// write's precondition reports it.
#[derive(Clone, Debug)]
pub struct FsPipelines {
    directory: Arc<PathBuf>,
    writing: Arc<tokio::sync::Mutex<()>>,
}

impl FsPipelines {
    /// The pipelines of `workspace`.
    pub fn new(workspace: &Workspace) -> Self {
        Self {
            directory: Arc::new(workspace.pipelines()),
            writing: Arc::default(),
        }
    }

    fn document(&self, name: &str) -> PathBuf {
        self.directory.join(format!("{name}{DOCUMENT}"))
    }

    fn layout(&self, name: &str) -> PathBuf {
        self.directory.join(format!("{name}{LAYOUT}"))
    }

    /// The document `name` as stored, or `None` when there is none.
    fn load(&self, name: &str) -> Result<Option<PipelineFile>, ApiError> {
        let path = self.document(name);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(failed(&path, error)),
        };
        let modified = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .map_err(|error| failed(&path, error))?;
        let revision = revision_of(&bytes);
        let document = String::from_utf8(bytes).map_err(|_| ApiError::BackendFailed {
            detail: format!("{}: the document is not UTF-8", path.display()),
        })?;
        Ok(Some(PipelineFile {
            name: name.to_owned(),
            document,
            revision,
            modified,
        }))
    }

    fn require(&self, name: &str) -> Result<PipelineFile, ApiError> {
        if !is_name(name) {
            return Err(not_found(name));
        }
        self.load(name)?.ok_or_else(|| not_found(name))
    }
}

#[async_trait]
impl PipelineSource for FsPipelines {
    async fn list(&self) -> Result<Vec<PipelineFile>, ApiError> {
        let this = self.clone();
        blocking(move || {
            let entries = fs::read_dir(this.directory.as_ref())
                .map_err(|error| failed(&this.directory, error))?;
            let mut names = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|error| failed(&this.directory, error))?;
                let file_name = entry.file_name();
                let Some(name) = file_name
                    .to_str()
                    .and_then(|file_name| file_name.strip_suffix(DOCUMENT))
                else {
                    continue;
                };
                // A staging file, a hidden one, or a name the API could not
                // address is not a pipeline of the workspace.
                if is_name(name) {
                    names.push(name.to_owned());
                }
            }
            names.sort();
            let mut files = Vec::with_capacity(names.len());
            for name in names {
                // Removed between the listing and the read: not listed.
                if let Some(file) = this.load(&name)? {
                    files.push(file);
                }
            }
            Ok(files)
        })
        .await
    }

    async fn read(&self, name: &str) -> Result<PipelineFile, ApiError> {
        let (this, name) = (self.clone(), name.to_owned());
        blocking(move || this.require(&name)).await
    }

    async fn write(
        &self,
        name: &str,
        document: &str,
        precondition: &Precondition,
    ) -> Result<PipelineFile, ApiError> {
        if !is_name(name) {
            return Err(ApiError::RequestInvalid {
                detail: format!(
                    "`{name}` is not a pipeline name: one file name of letters, digits, `_`, \
                     `-` and `.`, not starting with `.` nor ending with one, 64 bytes at most, \
                     and not `{RESERVED}`"
                ),
            });
        }
        validation::check(document)?;
        let _writing = self.writing.lock().await;
        let (this, name, document, precondition) = (
            self.clone(),
            name.to_owned(),
            document.to_owned(),
            precondition.clone(),
        );
        blocking(move || {
            let stored = this.load(&name)?;
            let current = stored
                .as_ref()
                .map(|file| file.revision.as_str().to_owned());
            let refusal = match (&precondition, &stored) {
                (Precondition::Matches(expected), Some(file)) if *expected == file.revision => None,
                (Precondition::Matches(expected), Some(file)) => Some(format!(
                    "pipeline {name} changed since it was read: `If-Match` names {}, the stored \
                     document is at {}",
                    expected.as_str(),
                    file.revision.as_str()
                )),
                (Precondition::Matches(_), None) => Some(format!(
                    "pipeline {name} is not stored, so `If-Match` matches nothing; create it \
                     with `If-None-Match: *`"
                )),
                (Precondition::Absent, None) => None,
                (Precondition::Absent, Some(file)) => Some(format!(
                    "pipeline {name} already exists, at {}; write it with `If-Match`",
                    file.revision.as_str()
                )),
                (Precondition::Unstated, _) => Some(format!(
                    "a write of pipeline {name} states neither `If-Match` nor \
                     `If-None-Match: *`{}",
                    current
                        .as_deref()
                        .map(|current| format!("; the stored document is at {current}"))
                        .unwrap_or_default()
                )),
            };
            if let Some(reason) = refusal {
                return Err(ApiError::PreconditionFailed { reason, current });
            }
            let path = this.document(&name);
            write_atomically(&path, document.as_bytes()).map_err(|error| failed(&path, error))?;
            this.load(&name)?.ok_or_else(|| ApiError::BackendFailed {
                detail: format!("{}: written, then not found", path.display()),
            })
        })
        .await
    }

    async fn read_layout(&self, name: &str) -> Result<Option<Layout>, ApiError> {
        let (this, name) = (self.clone(), name.to_owned());
        blocking(move || {
            this.require(&name)?;
            let path = this.layout(&name);
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(failed(&path, error)),
            };
            let layout: Layout =
                serde_json::from_str(&text).map_err(|error| ApiError::BackendFailed {
                    detail: format!("{}: not a layout this build reads: {error}", path.display()),
                })?;
            if layout.version != LAYOUT_VERSION {
                return Err(ApiError::BackendFailed {
                    detail: format!(
                        "{}: layout version {}, and this build reads version {LAYOUT_VERSION}",
                        path.display(),
                        layout.version
                    ),
                });
            }
            Ok(Some(layout))
        })
        .await
    }

    async fn write_layout(&self, name: &str, layout: &Layout) -> Result<(), ApiError> {
        if layout.version != LAYOUT_VERSION {
            return Err(ApiError::RequestInvalid {
                detail: format!(
                    "layout version {}: this build writes version {LAYOUT_VERSION}",
                    layout.version
                ),
            });
        }
        let _writing = self.writing.lock().await;
        let (this, name, layout) = (self.clone(), name.to_owned(), layout.clone());
        blocking(move || {
            this.require(&name)?;
            let path = this.layout(&name);
            let mut text =
                serde_json::to_string_pretty(&layout).map_err(|error| ApiError::BackendFailed {
                    detail: format!("{}: {error}", path.display()),
                })?;
            text.push('\n');
            write_atomically(&path, text.as_bytes()).map_err(|error| failed(&path, error))
        })
        .await
    }
}

/// The SHA-256 of `bytes`, in lowercase hex.
fn revision_of(bytes: &[u8]) -> Revision {
    let digest = Sha256::digest(bytes);
    Revision::new(
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
}

/// Whether `name` is one file name a pipeline may have: the import names'
/// alphabet, `[A-Za-z0-9_-][A-Za-z0-9._-]*`, 64 bytes at most, no trailing
/// `.` — and not the name the router reserves.
fn is_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let first_ok = bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric() || first == b'_' || first == b'-');
    first_ok
        && name.len() <= 64
        && !name.ends_with('.')
        && name != RESERVED
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn not_found(name: &str) -> ApiError {
    ApiError::PipelineNotFound {
        name: name.to_owned(),
    }
}

fn failed(path: &Path, error: io::Error) -> ApiError {
    ApiError::BackendFailed {
        detail: format!("{}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_revision_is_the_hex_sha256_of_the_bytes() {
        assert_eq!(
            revision_of(b"").as_str(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_name_is_one_file_name_and_not_the_reserved_one() {
        for name in ["hybrid", "dense-only", "v1.2", "_draft", &"x".repeat(64)] {
            assert!(is_name(name), "{name}");
        }
        for name in [
            "",
            ".hidden",
            "a.",
            "a/b",
            "..",
            "validate",
            "é",
            "a b",
            &"x".repeat(65),
        ] {
            assert!(!is_name(name), "{name}");
        }
    }
}
