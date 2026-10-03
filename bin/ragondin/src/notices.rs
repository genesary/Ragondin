//! The licence notices of the Rust crates this binary links, embedded so the
//! installed binary carries them without its source tree.
//!
//! The text is `third-party-notices-rust.txt` beside this crate's manifest,
//! written by `scripts/gen-rust-notices.py` from `cargo metadata` and checked
//! current by `just check-rust-notices`. It lists every crate in the
//! `--all-features` graph on every platform, so the one text serves every
//! build. `ragondin --notices` prints it, and under `ui` it is served at
//! `/third-party-notices-rust.txt`, beside the UI's own notices.
//!
//! `ARCHITECTURE.md` § The licence notices of the Rust crates gives the rules.

/// The notices, as generated.
pub const RUST_CRATES: &str = include_str!("../third-party-notices-rust.txt");

/// Where `ragondin ui` serves them, beside the UI's `third-party-notices.txt`.
#[cfg(feature = "ui")]
pub const PATH: &str = "third-party-notices-rust.txt";
