//! `router` returns a `Server`, and `serve` is the one way to listen with it.
//! The doc tests on `Server` prove nothing can be added to it by failing to
//! compile; this file serves one on a real loopback listener.

mod support;

use std::io::{Read, Write};

use support::{app, app_with_assets, FakeAssets, FakeRunStore, BUILD, INDEX_PAGE, SERVED};

#[test]
fn a_server_is_cheap_to_clone_for_each_connection() {
    let server = app(FakeRunStore::default());
    let _for_another_connection = server.clone();
}

/// `GET path` over a real connection to `address`, naming `host`.
fn get_over_tcp(address: std::net::SocketAddr, host: &str, path: &str) -> String {
    let mut stream = std::net::TcpStream::connect(address).expect("the server accepts");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .expect("the request is written");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("the response is read");
    response
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_listens_with_the_server_inside_its_envelope() {
    // The test router serves `SERVED`, so the connection is made to any
    // loopback port and the `Host` names the served authority.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("bound");
    let serving = tokio::spawn(ragondin_api::serve(
        listener,
        app_with_assets(FakeAssets::built()),
    ));

    let (page, refused) = tokio::task::spawn_blocking(move || {
        (
            get_over_tcp(address, SERVED, "/"),
            get_over_tcp(address, "evil.example", "/"),
        )
    })
    .await
    .expect("the client runs");

    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.ends_with(INDEX_PAGE), "{page}");
    assert!(
        page.to_ascii_lowercase()
            .contains(&format!("x-ragondin-build: {BUILD}").to_ascii_lowercase()),
        "{page}"
    );
    assert!(refused.starts_with("HTTP/1.1 421"), "{refused}");
    serving.abort();
}
