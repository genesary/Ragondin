//! # ragondin-engine
//!
//! The execution engine: the `EngineContext` (the component registry), physical
//! planning (`LogicalPipeline` + registry → `PhysicalPipeline`), the executor,
//! and `ExecutionTrace`.
//!
//! This crate is **internal — not an API boundary, and never will be** (INV-2):
//! refactor it freely. It **knows only traits** (INV-5): it depends on no crate
//! under `components/`. Composition is **explicit** through `EngineContext`
//! with **no global registry** (INV-6), built-ins get **no privilege** over
//! third-party components (INV-7), and `ExecutionTrace` is a **return value of
//! execution, not a log** (INV-10).
//!
//! Physical planning and the executor land in later issues; the registry is
//! here. See `ARCHITECTURE.md`.

#![warn(missing_docs)]

pub mod context;
pub mod error;

pub use context::{
    ComponentCtor, ConstructionError, EmbedderCtor, EngineContext, FusionCtor, RerankerCtor,
    RetrieverCtor, VectorStoreCtor,
};
pub use error::{ComponentFamily, PlanError};
