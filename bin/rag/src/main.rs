//! # rag
//!
//! The platform binary and the **composition root** — the single place that
//! assembles the engine and the concrete components (P2: one binary, one config
//! file, it runs). One binary, four subcommands:
//!
//! ```text
//! rag bench <config> --benchmark beir/scifact   # evaluate a pipeline
//! rag compare <run-a> <run-b>                    # compare two runs
//! rag serve <config>                             # serve the pipeline
//! rag validate <config>                          # validate a configuration
//! ```
//!
//! The subcommands, argument parsing, and component wiring land in later issues;
//! this is the compiling skeleton.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("rag — the composition root (skeleton).");
    println!("Planned subcommands: bench, compare, serve, validate.");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("rag"), "unexpected crate name: {name}");
    }
}
