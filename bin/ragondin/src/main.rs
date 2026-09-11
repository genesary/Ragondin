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
//! `main` itself is still the compiling skeleton. The crate is not empty,
//! though: `tests/vertical_slice.rs` assembles the composition root for real —
//! a configuration file read from disk, stub components registered on an
//! `EngineContext` through the ordinary API, a plan, and the trace the executor
//! returns. It is where the wiring this binary will do is exercised first.

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
