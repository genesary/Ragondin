//! A running `ragondin ui`, and a hand-written HTTP/1.1 client to ask it.
//!
//! The client is a `GET` over `std::net::TcpStream` with `Connection: close`,
//! read to the end: the server answers one request per connection this way,
//! and no HTTP client crate is a dependency of this binary for a test to
//! borrow. Shared by `tests/ui.rs` only.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

/// The variable the CI step that ships a binary sets (ADR-C36 § 5): under
/// it, a binary serving the notice page fails the release assertion.
pub const REQUIRE_ASSETS: &str = "RAGONDIN_REQUIRE_UI_ASSETS";

/// The attribute the notice page carries, and a real build does not. Written
/// by `build.rs` into the notice it generates.
const NOTICE_MARKER: &str = r#"<meta name="ragondin-ui" content="not-built">"#;

/// `ragondin ui` on a port the system chose, killed when dropped.
pub struct Server {
    child: Child,
    // Held open: the server prints to it, and a closed pipe would fail a
    // later write.
    _stdout: BufReader<ChildStdout>,
    url: String,
}

impl Server {
    /// Starts the server over `workspace` with `--port 0` and `extra`
    /// arguments, and waits for the line that prints its address.
    pub fn start(workspace: &Path, extra: &[&str]) -> Self {
        let mut child = Command::new(assert_cmd::cargo::cargo_bin("ragondin"))
            .arg("ui")
            .arg("--workspace")
            .arg(workspace)
            .args(["--port", "0"])
            .args(extra)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the binary starts");
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
        let mut line = String::new();
        stdout
            .read_line(&mut line)
            .expect("the server prints its address");
        let url = line
            .split_whitespace()
            .find(|word| word.starts_with("http://"))
            .unwrap_or_else(|| panic!("the first line names the URL: {line:?}"))
            .to_owned();
        Self {
            child,
            _stdout: stdout,
            url,
        }
    }

    /// The URL it printed, e.g. `http://127.0.0.1:49152/`.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The authority it serves, e.g. `127.0.0.1:49152`: the address to
    /// connect to, and the only `Host` it answers.
    pub fn authority(&self) -> &str {
        self.url
            .strip_prefix("http://")
            .and_then(|rest| rest.strip_suffix('/'))
            .expect("the URL is `http://<authority>/`")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A response, as read off the connection.
#[derive(Debug)]
pub struct Response {
    /// The status code.
    pub status: u16,
    headers: Vec<(String, String)>,
    /// The body, as text.
    pub body: String,
}

impl Response {
    /// The value of the header `name`, matched ignoring case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// `GET path` from the server at `authority`, with `Host: authority`.
pub fn get(authority: &str, path: &str) -> Response {
    let mut stream = TcpStream::connect(authority).expect("the server accepts a connection");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
    )
    .expect("the request is written");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("the response is read");
    let raw = String::from_utf8(raw).expect("the response is UTF-8");
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .expect("a response has a head and a body");
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("a status line");
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect();
    Response {
        status,
        headers,
        body: body.to_owned(),
    }
}

/// Whether `page` is the notice page the build script embeds when the UI was
/// not built.
pub fn is_notice(page: &str) -> bool {
    page.contains(NOTICE_MARKER)
}

/// The release assertion: when `required`, `page` — what the binary serves at
/// `/` — must be a real build, not the notice page.
pub fn assert_shipped(required: bool, page: &str) -> Result<(), String> {
    if required && is_notice(page) {
        return Err(format!(
            "{REQUIRE_ASSETS} is set, and this binary embeds the notice page rather than the \
             UI: build `ui/dist/` (`npm run build` in `ui/`) before building the binary"
        ));
    }
    Ok(())
}
