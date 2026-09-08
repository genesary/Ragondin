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

// Private modules with a flat re-export: one path to each item rather than two.
// The crate is internal (INV-2), so its layout owes nothing to anyone; a reader
// should not have to reconcile `ragondin_engine::EngineContext` with
// `ragondin_engine::context::EngineContext`.
mod context;
mod error;

pub use context::{
    ComponentCtor, EmbedderCtor, EngineContext, FusionCtor, RerankerCtor, RetrieverCtor,
    VectorStoreCtor,
};
pub use error::{ComponentFamily, ConstructionError, PlanError};
