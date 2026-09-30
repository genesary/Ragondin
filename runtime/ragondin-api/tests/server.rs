//! `router` returns a `Server`: what `axum::serve` accepts, and nothing a
//! route can be added to. The doc tests on `Server` prove the second half by
//! failing to compile; this file proves the first by compiling.

mod support;

use support::{app, FakeRunStore};

/// Never called: it exists so that the build fails if a `Server` stops being
/// something `axum::serve` can listen with.
#[allow(dead_code)]
async fn serves(listener: tokio::net::TcpListener) -> std::io::Result<()> {
    axum::serve(listener, app(FakeRunStore::default()).into_make_service()).await
}

#[test]
fn a_server_is_cheap_to_clone_for_each_connection() {
    let server = app(FakeRunStore::default());
    let _for_another_connection = server.clone();
}
