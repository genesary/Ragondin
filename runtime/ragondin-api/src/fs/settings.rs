//! `FsSettings`: the `WorkspaceSettings` backend over `workspace.toml`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use super::settings_file::{self, SettingsFile};
use super::{blocking, write_atomically, Workspace};
use crate::backends::{Settings, WorkspaceSettings};
use crate::error::ApiError;
use crate::response::ServiceBinding;

/// The workspace's settings, read from and written to `workspace.toml` on
/// every call — nothing is cached, so a hand edit is seen at once.
///
/// The datasets directory is `<root>/datasets` when the file names none, and
/// a relative one is read against the root; written back, the default is
/// left unstated. A write replaces the file whole, beside and then renamed
/// over it, so a reader sees the old file or the new one and never half of
/// either. Comments a person added are not kept by a write: the file is
/// rendered from the settings, under the header every workspace starts with.
#[derive(Clone, Debug)]
pub struct FsSettings {
    root: Arc<PathBuf>,
    file: Arc<PathBuf>,
    default_datasets: Arc<PathBuf>,
    // Writes are whole-file replacements: serialised, so two of them never
    // interleave their staging files.
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
}

#[async_trait]
impl WorkspaceSettings for FsSettings {
    async fn read(&self) -> Result<Settings, ApiError> {
        let this = self.clone();
        blocking(move || {
            let text = std::fs::read_to_string(this.file.as_ref())
                .map_err(|error| failed(&this.file, error))?;
            let file = settings_file::parse(&text).map_err(|error| ApiError::BackendFailed {
                detail: format!("{}:{}: {}", this.file.display(), error.line, error.reason),
            })?;
            Ok(Settings {
                datasets: match file.datasets {
                    Some(datasets) => this.root.join(datasets),
                    None => this.default_datasets.as_ref().clone(),
                },
                services: file
                    .services
                    .into_iter()
                    .map(|(family, name, uri)| ServiceBinding { family, name, uri })
                    .collect(),
            })
        })
        .await
    }

    async fn write(&self, settings: Settings) -> Result<(), ApiError> {
        let _writing = self.writing.lock().await;
        let this = self.clone();
        blocking(move || {
            let datasets = (settings.datasets != *this.default_datasets).then(|| {
                // Stated relative when it is under the root, so a workspace
                // moved whole keeps its datasets.
                match settings.datasets.strip_prefix(this.root.as_ref()) {
                    Ok(relative) if !relative.as_os_str().is_empty() => {
                        relative.display().to_string()
                    }
                    _ => settings.datasets.display().to_string(),
                }
            });
            let text = settings_file::render(&SettingsFile {
                datasets,
                services: settings
                    .services
                    .into_iter()
                    .map(|binding| (binding.family, binding.name, binding.uri))
                    .collect(),
            });
            write_atomically(&this.file, text.as_bytes()).map_err(|error| failed(&this.file, error))
        })
        .await
    }
}

fn failed(path: &Path, error: std::io::Error) -> ApiError {
    ApiError::BackendFailed {
        detail: format!("{}: {error}", path.display()),
    }
}
