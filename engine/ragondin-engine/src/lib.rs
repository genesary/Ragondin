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
//! The engine internals land in a later issue; this is the compiling skeleton.
//! See `ARCHITECTURE.md`.

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(
            name.starts_with("ragondin-"),
            "unexpected crate name: {name}"
        );
    }
}
