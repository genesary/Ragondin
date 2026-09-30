//! The file backends: the workspace on disk behind [`PipelineSource`],
//! [`WorkspaceSettings`] and [`Registry`] (ADR-C36 § 2 places them in this
//! crate). **Empty today** — the router is tested against in-memory fakes —
//! and filled additively, one backend at a time, so that each lands here
//! without touching the router:
//!
//! - the `PipelineSource` over `pipelines/<name>.yaml` and its layout, and the
//!   `WorkspaceSettings` over `workspace.toml`, by the issue that puts the
//!   workspace on disk (#342);
//! - the `Registry` over the benchmark manifest and the datasets directory, by
//!   the issue that brings the manifest (#341).
//!
//! The run store's file backend is not here: it is `ragondin-experiments`'
//! `FileSystemRunStore`.
//!
//! [`PipelineSource`]: crate::PipelineSource
//! [`WorkspaceSettings`]: crate::WorkspaceSettings
//! [`Registry`]: crate::Registry
