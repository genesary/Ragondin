//! # rag-benchmarks
//!
//! The `BenchmarkAdapter` contract and its implementations (BEIR, CRAG,
//! MultiHop-RAG, …). A benchmark is a corpus plus queries plus ground truth;
//! the adapter presents any dataset through one iterator the harness can drive.
//!
//! The adapter and its implementations land in a later issue; this is the
//! compiling skeleton.

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("rag-"), "unexpected crate name: {name}");
    }
}
