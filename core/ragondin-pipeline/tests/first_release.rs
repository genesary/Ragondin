//! Tripwire: the first release is the commit at which the workspace version
//! stops being `0.0.0`.
//!
//! Until then a change to the wire schema's shape leaves `SchemaVersion` at 1
//! and is recorded as a row with no number in the crate's history table,
//! `ARCHITECTURE.md` § Schema-version history. From the first release INV-9's
//! bump rule applies, starting from 1.
//!
//! Nothing else in the tree notices that moment, so this test does: it fails on
//! the commit that changes the version, and that commit owes the history table
//! its "version 1 released" row.

/// The root manifest, read as text: the one line wanted is plain enough to
/// find without giving this crate a TOML dependency.
const ROOT_MANIFEST: &str = include_str!("../../../Cargo.toml");

/// The `version` key of `[workspace.package]`, the version every member
/// inherits through `version.workspace = true`.
fn workspace_version() -> &'static str {
    let mut in_package = false;
    for line in ROOT_MANIFEST.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[workspace.package]";
            continue;
        }
        if in_package {
            if let Some(value) = line.strip_prefix("version") {
                if let Some(value) = value.trim_start().strip_prefix('=') {
                    return value.trim().trim_matches('"');
                }
            }
        }
    }
    panic!("the root Cargo.toml has no `version` under [workspace.package]");
}

#[test]
fn the_wire_schema_is_unreleased_while_the_workspace_version_is_0_0_0() {
    let version = workspace_version();
    assert_eq!(
        version, "0.0.0",
        "the workspace version is now {version}: this is the first release. Record \
         `SchemaVersion` 1 as released in the history table of \
         core/ragondin-pipeline/ARCHITECTURE.md — from here every change to the wire schema's \
         shape bumps `SchemaVersion` (INV-9) — and delete this test."
    );
}
