//! # rag-metrics
//!
//! Deterministic retrieval metrics: nDCG@k, recall@k, MRR. Generation metrics
//! come in a later milestone. Determinism is the point — a metric that varies
//! run to run cannot underpin a reproducible benchmark.
//!
//! The metric implementations land in a later issue; this is the compiling
//! skeleton.

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("rag-"), "unexpected crate name: {name}");
    }
}
