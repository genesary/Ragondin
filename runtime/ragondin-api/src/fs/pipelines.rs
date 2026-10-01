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
use crate::response::{Layout, NodePair, Pairing};
use crate::validation;

/// A document's extension, and its layout's.
const DOCUMENT: &str = ".yaml";
const LAYOUT: &str = ".layout.json";
/// The directory a pipeline's manual pairings are kept in, beside it.
const PAIRING: &str = ".pairing";
/// The one pairing format this build reads and writes.
const PAIRING_VERSION: u32 = 1;
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

    /// A stored pipeline whose name is `name` in another case, if any.
    fn alias_of(&self, name: &str) -> Result<Option<String>, ApiError> {
        let entries = fs::read_dir(self.directory.as_ref())
            .map_err(|error| failed(&self.directory, error))?;
        for entry in entries {
            let entry = entry.map_err(|error| failed(&self.directory, error))?;
            let file_name = entry.file_name();
            if let Some(stored) = file_name
                .to_str()
                .and_then(|file_name| file_name.strip_suffix(DOCUMENT))
            {
                if stored != name && stored.eq_ignore_ascii_case(name) {
                    return Ok(Some(stored.to_owned()));
                }
            }
        }
        Ok(None)
    }

    /// Refuses `name` when a stored pipeline has it in another case. On a
    /// filesystem that ignores case `Hybrid.yaml` is `hybrid.yaml`: a write
    /// under the one would replace the other behind its etag, a read would
    /// answer the other under this name, and a layout would land beside the
    /// other under a name the listing does not show. Refused for all of them,
    /// on every filesystem, so the answer does not depend on which one.
    fn refuse_alias(&self, name: &str) -> Result<(), ApiError> {
        match self.alias_of(name)? {
            Some(stored) => Err(ApiError::RequestInvalid {
                detail: format!(
                    "`{name}` differs from the stored pipeline `{stored}` only in case, and on a \
                     filesystem that ignores case the two are one file; use `{stored}`"
                ),
            }),
            None => Ok(()),
        }
    }

    /// The stored pipeline `name`, for a read or a layout: `pipeline_not_found`
    /// when there is none, `request_invalid` for a case alias of one.
    fn require(&self, name: &str) -> Result<PipelineFile, ApiError> {
        if !is_name(name) {
            return Err(not_found(name));
        }
        self.refuse_alias(name)?;
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
                     not a device name Windows reserves, and not `{RESERVED}`"
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
            this.refuse_alias(&name)?;
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
                (Precondition::Exists, Some(_)) => None,
                (Precondition::Exists, None) => Some(format!(
                    "pipeline {name} is not stored, so `If-Match: *` matches nothing; create it \
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

    async fn read_pairing(&self, pipeline: &str, other: &str) -> Result<Option<Pairing>, ApiError> {
        let (this, pipeline, other) = (self.clone(), pipeline.to_owned(), other.to_owned());
        blocking(move || {
            this.require(&pipeline)?;
            this.require(&other)?;
            if let Some(kept) = this.load_pairing(&pipeline, &other)? {
                return Ok(Some(kept));
            }
            // Kept the other way round: the same pairs, each turned around.
            Ok(this.load_pairing(&other, &pipeline)?.map(|kept| Pairing {
                pipeline: kept.other,
                other: kept.pipeline,
                pairs: kept
                    .pairs
                    .into_iter()
                    .map(|pair| NodePair {
                        node: pair.other,
                        other: pair.node,
                        label: pair.label,
                    })
                    .collect(),
            }))
        })
        .await
    }

    async fn write_pairing(&self, pairing: &Pairing) -> Result<(), ApiError> {
        let _writing = self.writing.lock().await;
        let (this, pairing) = (self.clone(), pairing.clone());
        blocking(move || {
            this.require(&pairing.pipeline)?;
            this.require(&pairing.other)?;
            let path = this.pairing(&pairing.pipeline, &pairing.other);
            let directory = this
                .directory
                .join(format!("{}{PAIRING}", pairing.pipeline));
            fs::create_dir_all(&directory).map_err(|error| failed(&directory, error))?;
            let file = PairingFile {
                version: PAIRING_VERSION,
                pipeline: pairing.pipeline.clone(),
                other: pairing.other.clone(),
                pairs: pairing
                    .pairs
                    .iter()
                    .map(|pair| FilePair {
                        node: pair.node.clone(),
                        other: pair.other.clone(),
                        label: pair.label.clone(),
                    })
                    .collect(),
            };
            let mut text =
                serde_json::to_string_pretty(&file).map_err(|error| ApiError::BackendFailed {
                    detail: format!("{}: {error}", path.display()),
                })?;
            text.push('\n');
            write_atomically(&path, text.as_bytes()).map_err(|error| failed(&path, error))?;
            // One pairing per pair of pipelines: one kept the other way round
            // would be a second truth, read only when this one is gone.
            this.remove_pairing(&pairing.other, &pairing.pipeline)
        })
        .await
    }

    async fn delete_pairing(&self, pipeline: &str, other: &str) -> Result<(), ApiError> {
        let _writing = self.writing.lock().await;
        let (this, pipeline, other) = (self.clone(), pipeline.to_owned(), other.to_owned());
        blocking(move || {
            this.require(&pipeline)?;
            this.require(&other)?;
            this.remove_pairing(&pipeline, &other)?;
            this.remove_pairing(&other, &pipeline)
        })
        .await
    }
}

impl FsPipelines {
    fn pairing(&self, pipeline: &str, other: &str) -> PathBuf {
        self.directory
            .join(format!("{pipeline}{PAIRING}"))
            .join(format!("{other}.json"))
    }

    /// The pairing kept as `pipelines/<pipeline>.pairing/<other>.json`, if
    /// any: refused, never guessed at, when its version is not this build's
    /// or the names inside are not the two its path gives — a file renamed
    /// by hand would otherwise pair nodes of another pipeline.
    fn load_pairing(&self, pipeline: &str, other: &str) -> Result<Option<Pairing>, ApiError> {
        let path = self.pairing(pipeline, other);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(failed(&path, error)),
        };
        let file: PairingFile =
            serde_json::from_str(&text).map_err(|error| ApiError::BackendFailed {
                detail: format!(
                    "{}: not a pairing this build reads: {error}",
                    path.display()
                ),
            })?;
        if file.version != PAIRING_VERSION {
            return Err(ApiError::BackendFailed {
                detail: format!(
                    "{}: pairing version {}, and this build reads version {PAIRING_VERSION}",
                    path.display(),
                    file.version
                ),
            });
        }
        if file.pipeline != pipeline || file.other != other {
            return Err(ApiError::BackendFailed {
                detail: format!(
                    "{}: the file pairs {} with {}, and its path says {pipeline} with {other} — \
                     renamed by hand? Rename it back, or reset the pairing to automatic",
                    path.display(),
                    file.pipeline,
                    file.other
                ),
            });
        }
        Ok(Some(Pairing {
            pipeline: file.pipeline,
            other: file.other,
            pairs: file
                .pairs
                .into_iter()
                .map(|pair| NodePair {
                    node: pair.node,
                    other: pair.other,
                    label: pair.label,
                })
                .collect(),
        }))
    }

    /// Removes `pipelines/<pipeline>.pairing/<other>.json`, and the
    /// directory once it is empty; absent is already removed.
    fn remove_pairing(&self, pipeline: &str, other: &str) -> Result<(), ApiError> {
        let path = self.pairing(pipeline, other);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(failed(&path, error)),
        }
        // Fails while another pairing is kept there, which is what is meant.
        if let Some(directory) = path.parent() {
            let _ = fs::remove_dir(directory);
        }
        Ok(())
    }
}

/// A pairing as `pipelines/<pipeline>.pairing/<other>.json` holds it: both
/// pipelines named inside, so that a file renamed by hand is detected.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PairingFile {
    version: u32,
    pipeline: String,
    other: String,
    pairs: Vec<FilePair>,
}

/// One pair as the file holds it: its own type, not the API's `NodePair`,
/// so that a change to the API cannot change the format on disk unseen.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePair {
    node: String,
    other: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
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
/// `.`, no Windows device name — and not the name the router reserves.
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
        && !is_device_name(name)
}

/// Whether `name`'s stem — up to its first `.` — is a device name Windows
/// reserves in every directory, in any case: `CON`, `PRN`, `AUX`, `NUL`,
/// `COM1`–`COM9`, `LPT1`–`LPT9`. The rule `ragondin-benchmarks` applies to an
/// import's name, written again here because that check is private to it.
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
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
            "CON",
            "prn",
            "Aux.v1",
            "nul",
            "COM1",
            "lpt9",
        ] {
            assert!(!is_name(name), "{name}");
        }
        // Not device names: `COM0`, `COM10`, a longer word starting the same.
        for name in ["com0", "com10", "console", "nullable", "lpt"] {
            assert!(is_name(name), "{name}");
        }
    }
}
