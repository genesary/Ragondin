//! No response type is a core type serialized directly (ADR-C36 § 2), nor the
//! experiment plane's: every one is this crate's own, converted to.
//!
//! A source scan, and so a best-effort one: `src/response.rs` holds every
//! type the API serializes, and it may name no other crate of this workspace
//! at all — no `use`, no path, no re-export. The conversions live elsewhere
//! (`src/convert.rs`), where naming the core is their whole job. What the
//! scan does not see is a type defined in another module and serialized
//! anyway; the description's schemas are generated from `response.rs`'s types
//! only, which is where a reviewer would see one arrive.

use std::path::Path;

#[test]
fn the_response_module_names_no_workspace_crate() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/response.rs");
    let source = std::fs::read_to_string(&path).expect("src/response.rs exists");
    for (number, line) in source.lines().enumerate() {
        assert!(
            !line.contains("ragondin_"),
            "{}:{}: a response type names a workspace crate: {line}",
            path.display(),
            number + 1
        );
    }
}
