//! # ragondin
//!
//! The platform binary and the **composition root** — the single place that
//! assembles the engine and the concrete components (P2: one binary, one config
//! file, it runs). One binary, four subcommands:
//!
//! ```text
//! ragondin bench <config> --benchmark beir/scifact  # evaluate a pipeline
//! ragondin compare <run-a> <run-b>                  # compare two runs
//! ragondin serve <config>                           # serve the pipeline
//! ragondin validate <config>                        # validate a configuration
//! ```
//!
//! The subcommands, argument parsing, and component wiring land in later issues;
//! this is the compiling skeleton.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("ragondin — the composition root (skeleton).");
    println!("Planned subcommands: bench, compare, serve, validate.");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(
            name.starts_with("ragondin"),
            "unexpected crate name: {name}"
        );
    }
}
