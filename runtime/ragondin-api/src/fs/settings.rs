//! `FsSettings`: the `WorkspaceSettings` backend over `workspace.toml`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use super::settings_file::{self, Edit, GrammarError, SettingsFile};
use super::{blocking, write_atomically, Workspace};
use crate::backends::{Settings, WorkspaceSettings};
use crate::error::ApiError;
use crate::response::ServiceBinding;

/// The workspace's settings, read from `workspace.toml` on every call —
/// nothing is cached, so a hand edit is seen at once — and changed in place,
/// one key at a time.
///
/// The datasets directory is `<root>/datasets` when the file names none, and
/// a relative one is read against the root. Each operation runs under one
/// lock, on the file as it is on disk at that moment: it reads it, decides on
/// the settings it states whether anything changes, and only then edits that
/// one key in the document and replaces the file, beside and then renamed
/// over it, so a reader sees the old file or the new one. An operation that
/// changes no setting writes nothing, and the file keeps its bytes. A write
/// keeps every comment, the key order and the blank lines; it ends lines with
/// LF and drops a byte-order mark (ADR-C38 § Consequences).
#[derive(Clone, Debug)]
pub struct FsSettings {
    root: Arc<PathBuf>,
    file: Arc<PathBuf>,
    default_datasets: Arc<PathBuf>,
    // Held across an operation's read and write of the file, so two
    // operations never start from what the other is replacing.
    writing: Arc<tokio::sync::Mutex<()>>,
}

impl FsSettings {
    /// The settings of `workspace`.
    pub fn new(workspace: &Workspace) -> Self {
        Self {
            root: Arc::new(workspace.root().to_path_buf()),
            file: Arc::new(workspace.settings_file()),
            default_datasets: Arc::new(workspace.default_datasets()),
            writing: Arc::default(),
        }
    }

    /// The datasets directory `stated` resolves to.
    fn datasets(&self, stated: Option<&str>) -> PathBuf {
        match stated {
            Some(datasets) => self.root.join(datasets),
            None => self.default_datasets.as_ref().clone(),
        }
    }

    fn settings(&self, file: SettingsFile) -> Settings {
        Settings {
            datasets: self.datasets(file.datasets.as_deref()),
            services: file
                .services
                .into_iter()
                .map(|(family, name, uri)| ServiceBinding { family, name, uri })
                .collect(),
        }
    }

    fn malformed(&self, error: GrammarError) -> ApiError {
        ApiError::BackendFailed {
            detail: format!("{}:{}: {}", self.file.display(), error.line, error.reason),
        }
    }

    fn read_text(&self) -> Result<String, ApiError> {
        std::fs::read_to_string(self.file.as_ref()).map_err(|error| failed(&self.file, error))
    }

    /// Under the lock: reads the file, asks `plan` for the edit the settings
    /// it states call for, and applies it, when there is one. Returns the
    /// settings left, and whether anything was written.
    async fn change<F>(&self, plan: F) -> Result<(Settings, bool), ApiError>
    where
        F: FnOnce(&Self, &Settings) -> Result<Option<Edit>, ApiError> + Send + 'static,
    {
        let _writing = self.writing.lock().await;
        let this = self.clone();
        blocking(move || {
            let text = this.read_text()?;
            let current =
                this.settings(settings_file::parse(&text).map_err(|e| this.malformed(e))?);
            let Some(edit) = plan(&this, &current)? else {
                return Ok((current, false));
            };
            let edited = settings_file::edit(&text, &edit).map_err(|e| this.malformed(e))?;
            // What is written is read back before it replaces the file.
            let left = this.settings(settings_file::parse(&edited).map_err(|e| this.malformed(e))?);
            write_atomically(&this.file, edited.as_bytes())
                .map_err(|error| failed(&this.file, error))?;
            Ok((left, true))
        })
        .await
    }
}

#[async_trait]
impl WorkspaceSettings for FsSettings {
    async fn read(&self) -> Result<Settings, ApiError> {
        let this = self.clone();
        blocking(move || {
            let text = this.read_text()?;
            let file = settings_file::parse(&text).map_err(|error| this.malformed(error))?;
            Ok(this.settings(file))
        })
        .await
    }

    async fn bind(&self, binding: ServiceBinding) -> Result<Settings, ApiError> {
        let ServiceBinding { family, name, uri } = binding;
        // The key is split at its first `/` when it is read back, so a family
        // holding one, or an empty part, would read back as another binding.
        if family.is_empty() || family.contains('/') || name.is_empty() {
            return Err(ApiError::BindingRefused {
                detail: format!(
                    "`{family}/{name}` cannot be stated in workspace.toml, whose service keys are \
                     `\"<family>/<name>\"` split at the first `/`: the family must be non-empty \
                     and hold no `/`, and the name must be non-empty"
                ),
            });
        }
        let (settings, _) = self
            .change(move |_, current| {
                let unchanged = current
                    .services
                    .iter()
                    .any(|bound| bound.family == family && bound.name == name && bound.uri == uri);
                Ok((!unchanged).then_some(Edit::Bind { family, name, uri }))
            })
            .await?;
        Ok(settings)
    }

    async fn unbind(&self, family: &str, name: &str) -> Result<Option<Settings>, ApiError> {
        let (family, name) = (family.to_owned(), name.to_owned());
        let (settings, written) = self
            .change(move |_, current| {
                let bound = current
                    .services
                    .iter()
                    .any(|bound| bound.family == family && bound.name == name);
                Ok(bound.then_some(Edit::Unbind { family, name }))
            })
            .await?;
        Ok(written.then_some(settings))
    }

    async fn set_datasets(&self, datasets: Option<PathBuf>) -> Result<Settings, ApiError> {
        let (settings, _) = self
            .change(move |this, current| {
                // Compared lexically, after joining to the root: `.` and a
                // trailing separator are no change, and nothing is
                // canonicalised, so a directory not yet created compares too.
                let Some(datasets) = datasets else {
                    return Ok(
                        (current.datasets != *this.default_datasets).then_some(Edit::ClearDatasets)
                    );
                };
                let target = this.root.join(&datasets);
                if target == current.datasets {
                    return Ok(None);
                }
                // Stated relative when it is under the root, so a workspace
                // moved whole keeps its datasets.
                let stated = match target.strip_prefix(this.root.as_ref()) {
                    Ok(relative) if !relative.as_os_str().is_empty() => relative,
                    _ => target.as_path(),
                };
                let stated = stated.to_str().ok_or_else(|| ApiError::BackendFailed {
                    detail: format!(
                        "{}: the datasets directory {} is not UTF-8, which a TOML string cannot \
                         hold",
                        this.file.display(),
                        target.display()
                    ),
                })?;
                Ok(Some(Edit::SetDatasets(stated.to_owned())))
            })
            .await?;
        Ok(settings)
    }
}

fn failed(path: &Path, error: std::io::Error) -> ApiError {
    ApiError::BackendFailed {
        detail: format!("{}: {error}", path.display()),
    }
}
