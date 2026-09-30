//! Rewrites the golden API description, `api/v1.json`, from the crate's
//! declared operations and response types. Run through `just
//! gen-api-description`; `tests/description.rs` fails until the file is
//! current, and the diff it leaves is the API change a reviewer reads.

use std::path::Path;

fn main() -> std::io::Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("api/v1.json");
    std::fs::write(&path, ragondin_api::description::render())?;
    println!("wrote {}", path.display());
    Ok(())
}
