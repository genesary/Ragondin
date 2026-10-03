//! The UI's static assets, embedded at build time: the table `ragondin-api`
//! serves at `/`.
//!
//! `build.rs` writes the `rust-embed` derive included below, over `ui/dist/`
//! when it has been built and over a generated notice page when not
//! (ADR-C36 § 5), so this module never knows which: it hands over whatever
//! was embedded. `debug-embed` makes a debug build embed the files too, so no
//! build reads `ui/` at run time.
//!
//! How a request reaches the table — the single-page fallback, `GET` and
//! `HEAD` only, the `404` — is `ragondin-api`'s, inside the envelope it
//! applies (`runtime/ragondin-api/ARCHITECTURE.md` § The assets).

use ragondin_api::{content_type_for, Asset, Assets};

// `struct Assets`, deriving `RustEmbed` over the folder `build.rs` chose:
// `ui/dist/`, or the generated notice page. In a module of its own, since
// `ragondin-api`'s trait has the same name.
mod generated {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}

/// The embedded files, as `ragondin-api` asks for them: the UI's, and beside
/// them the licence notices of the Rust crates this binary links
/// ([`notices::PATH`](crate::notices::PATH)), which are the binary's own and
/// so served whether `ui/dist/` was embedded or the notice page was.
pub struct Embedded;

impl Assets for Embedded {
    fn get(&self, path: &str) -> Option<Asset> {
        if path == crate::notices::PATH {
            return Some(Asset {
                bytes: crate::notices::RUST_CRATES.as_bytes().into(),
                content_type: content_type_for(path),
            });
        }
        generated::Assets::get(path).map(|file| Asset {
            bytes: file.data,
            content_type: content_type_for(path),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_table_always_has_an_index_page() {
        // `ui/dist/` or the notice: whichever `build.rs` chose, `/` answers.
        let index = Embedded.get("index.html").expect("an index page");

        assert_eq!(index.content_type, "text/html; charset=utf-8");
        assert!(index.bytes.starts_with(b"<!doctype html>"));
    }
}
