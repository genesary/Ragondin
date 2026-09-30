//! The file backends: the workspace on disk behind [`PipelineSource`],
//! [`WorkspaceSettings`] and [`Registry`] (ADR-C36 § 2 places them in this
//! crate). Filled additively, one backend at a time, so that each lands here
//! without touching the router:
//!
//! - [`FsRegistry`], the `Registry` over the benchmark manifest and the
//!   datasets directory;
//! - the `PipelineSource` over `pipelines/<name>.yaml` and its layout, and the
//!   `WorkspaceSettings` over `workspace.toml`, not written yet — the issue
//!   that puts the workspace on disk (#342) brings them.
//!
//! The run store's file backend is not here: it is `ragondin-experiments`'
//! `FileSystemRunStore`.
//!
//! [`PipelineSource`]: crate::PipelineSource
//! [`WorkspaceSettings`]: crate::WorkspaceSettings
//! [`Registry`]: crate::Registry

mod registry;

pub use registry::FsRegistry;
