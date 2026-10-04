//! Under the `ui` feature: what `rust-embed` embeds, and the build identity.
//!
//! `src/ui/assets.rs` includes `$OUT_DIR/assets.rs`, which this script writes:
//! a `rust-embed` derive whose `#[folder]` is the absolute path it chose. So
//! the decision is taken here, at compile time, and the Rust build never needs
//! Node (ADR-C36 § 5). Generating the attribute, rather than interpolating an
//! environment variable into it, spares `rust-embed`'s
//! `interpolate-folder-path` feature, whose dependencies carry a licence
//! `deny.toml` does not allow. The folder:
//!
//! - `ui/dist/index.html` exists: the folder is `ui/dist/`, the real build;
//! - otherwise: the folder is a directory under `OUT_DIR` holding a generated
//!   `index.html` that says the UI was not built and how to build it.
//!
//! The notice is written in every `ui` build, used or not, so a test can read
//! the page a Rust-only build would serve. `RAGONDIN_UI_ASSETS_KIND` says
//! which folder was taken (`built` or `notice`), for the tests that check what
//! the binary serves against what it embedded.
//!
//! `RAGONDIN_BUILD_COMMIT` is the commit the build is from, or `unknown`
//! outside a git checkout, and `RAGONDIN_BUILD_DIRTY` is `true` when a tracked
//! file is modified or a change is staged; an untracked file is not counted.
//! Both come from the git commands `build-identity.rule` names, run by
//! `src/ui/build_identity.rs`, which this script compiles as a module: the
//! same rule file `ui/scripts/build-identity.mjs` reads, so the UI's bundle
//! and the binary carry one identity. `src/ui/mod.rs` makes them the build
//! identity. Both are read when this script runs: it reruns when `HEAD`, the branch it names,
//! `packed-refs` or the index changes, so a commit or a `git add` refreshes
//! them, and an edit left unstaged after the last run does not.
//!
//! Without the feature the script emits nothing but its own rerun line: the
//! lean build compiles nothing of the UI and reads nothing under `ui/`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "src/ui/build_identity.rs"]
mod build_identity;

use build_identity::git;

/// The page a build without `ui/dist/` serves at `/` and on every client-side
/// route. No `<style>` and no `style=`: the server's content security policy
/// is `default-src 'self'`, which refuses inline styles. The `meta` element is
/// the marker the release assertion looks for (`tests/support/ui.rs`).
const NOTICE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="ragondin-ui" content="not-built">
<title>ragondin — the UI was not built</title>
</head>
<body>
<h1>This build of ragondin carries no UI</h1>
<p><code>ui/dist/</code> did not exist when this binary was compiled, so it embedded this page instead of the front end.</p>
<p>To build the UI, with the Node major pinned in <code>ui/.node-version</code>:</p>
<pre>cd ui
npm ci
npm run build
cd ..
cargo build -p ragondin --features ui</pre>
<p>The JSON API is served regardless, under <a href="/api/v1/workspace"><code>/api/v1/</code></a>.</p>
</body>
</html>
"#;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var_os("CARGO_FEATURE_UI").is_none() {
        return;
    }

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo"));

    let notice = out.join("ui-notice");
    fs::create_dir_all(&notice).expect("OUT_DIR is writable");
    fs::write(notice.join("index.html"), NOTICE).expect("OUT_DIR is writable");

    let ui = manifest.join("../../ui");
    let dist = ui.join("dist");
    let (folder, kind) = if dist.join("index.html").is_file() {
        // A rebuild of the UI changes a file under `dist/`, and removing it
        // makes the watched path missing: either reruns this script.
        println!("cargo:rerun-if-changed={}", dist.display());
        (dist, "built")
    } else {
        // `dist/` appearing is a change under `ui/`. Watching a path that does
        // not exist would rerun this script, and rebuild the binary, on every
        // build; `ui/` exists in any checkout. Cargo scans a watched directory
        // recursively and has no way to watch a directory's own entry, so this
        // walks `ui/node_modules/` too when it is installed — a cost paid only
        // while `dist/` is absent, since `npm run build` is what follows
        // `npm ci`, and never in CI's Rust job, which installs no Node module.
        if ui.is_dir() {
            println!("cargo:rerun-if-changed={}", ui.display());
        }
        (notice, "notice")
    };
    let folder = folder.canonicalize().expect("the folder exists");
    let folder = folder.to_str().expect("a UTF-8 path, as `#[folder]` needs");
    // `{folder:?}` is the path as a Rust string literal, escapes included.
    let embed = format!(
        "/// The UI's assets: `ui/dist/`, or the notice page. Generated by \
         `build.rs`.\n#[derive(rust_embed::RustEmbed)]\n#[folder = {folder:?}]\npub struct Assets;\n"
    );
    fs::write(out.join("assets.rs"), embed).expect("OUT_DIR is writable");
    println!("cargo:rustc-env=RAGONDIN_UI_ASSETS_KIND={kind}");
    let (commit, dirty) = commit(&manifest);
    println!("cargo:rustc-env=RAGONDIN_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=RAGONDIN_BUILD_DIRTY={dirty}");
}

/// The commit `HEAD` names and whether the tree differs from it, and the
/// files whose change moves either watched.
fn commit(dir: &Path) -> (String, bool) {
    let Some((sha, dirty)) = build_identity::state(dir) else {
        return ("unknown".to_owned(), false);
    };
    // `HEAD` moves on a checkout; the branch it points at moves on a commit,
    // in its loose ref or in `packed-refs`; the index moves on a `git add`.
    // Only existing paths are watched, for the reason given above.
    let mut watched = vec![
        "HEAD".to_owned(),
        "packed-refs".to_owned(),
        "index".to_owned(),
    ];
    if let Some(branch) = git(dir, &["symbolic-ref", "-q", "HEAD"]) {
        watched.push(branch);
    }
    for path in watched {
        if let Some(file) = git(
            dir,
            &["rev-parse", "--path-format=absolute", "--git-path", &path],
        ) {
            if Path::new(&file).exists() {
                println!("cargo:rerun-if-changed={file}");
            }
        }
    }
    (sha, dirty)
}
