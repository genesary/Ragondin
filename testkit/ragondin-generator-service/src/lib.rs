//! # ragondin-generator-service
//!
//! The reference `Remote` generator service (ADR-C33): a `tonic` server
//! implementing the generated `Generator` service of `ragondin-proto`, which
//! answers each call by relaying it to an OpenAI-compatible inference server
//! over HTTP. It holds no experiment variable of its own: the served model,
//! the template and the sampling knobs arrive in each call (ADR-C31 § 2).
//!
//! The binary, `ragondin-generator-service`, compiles only under the `service`
//! feature. Without it this library holds the template renderer alone.
//!
//! See `ARCHITECTURE.md`.

#[cfg(feature = "service")]
pub mod cli;
#[cfg(feature = "service")]
pub mod relay;
pub mod template;
