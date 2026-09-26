//! Generates the Rust stubs from the hand-maintained `.proto` files (ADR-C24).
//!
//! `protox` compiles them to a `FileDescriptorSet` and `tonic-build` generates
//! from that set, so no `protoc` binary is needed, looked up or run (ADR-C34).

/// Every `.proto` file whose messages and services this crate generates.
/// Relative to [`INCLUDE`], which is also the root their imports resolve from.
const FILES: &[&str] = &[
    "ragondin/v1/types.proto",
    "ragondin/v1/retriever.proto",
    "ragondin/v1/fusion.proto",
    "ragondin/v1/reranker.proto",
    "ragondin/v1/embedder.proto",
    "ragondin/v1/vector_store.proto",
    "ragondin/v1/context_builder.proto",
    "ragondin/v1/generator.proto",
    "ragondin/config/v1/config.proto",
];

const INCLUDE: &str = "proto";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={INCLUDE}");
    let descriptors = protox::compile(FILES, [INCLUDE])?;
    tonic_build::configure().compile_fds(descriptors)?;
    Ok(())
}
