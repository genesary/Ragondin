//! # ragondin-harness
//!
//! The evaluation harness: it drives the **same** `ragondin-engine` with an iterator
//! over a benchmark (`ragondin-benchmarks`), collects each `ExecutionTrace` and the
//! metrics (`ragondin-metrics`), and writes a run identified by the content-addressed
//! tuple.
//!
//! It is a **thin driver**. The serving driver (`ragondin-server`) wraps the very
//! same engine; there is literally one execution path, and Cargo proves it —
//! which is what makes evaluation/serving skew impossible (P1).
//!
//! The harness logic lands in a later issue; this is the compiling skeleton.

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
