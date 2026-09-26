//! The service, run as its binary, against a fake inference server
//! (ADR-C33 § 6).
//!
//! The fake speaks HTTP/1.1, written by hand over `tokio::net::TcpListener`
//! on a loopback port, and records every request it receives, so a test
//! asserts on the body the service actually sent. Nothing here touches the
//! network beyond the loopback interface, and no real model is involved.
//!
//! The service is driven through the generated `GeneratorClient`, so the
//! tests assert the gRPC status each failure becomes. Which `ComponentError`
//! a status then becomes is the `Remote` adapter's mapping, not this
//! service's.

use std::io::{BufRead, BufReader, Read};
use std::net::SocketAddr;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use ragondin_proto::v1::generator_client::GeneratorClient;
use ragondin_proto::v1::{
    Context, GenerateParams, GenerateRequest, GeneratorModelIdentityRequest, Query,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tonic::transport::Channel;
use tonic::Code;

const BIN: &str = env!("CARGO_BIN_EXE_ragondin-generator-service");
const KEY_VAR: &str = "RAGONDIN_INFERENCE_API_KEY";

// ---------------------------------------------------------------------------
// The fake inference server
// ---------------------------------------------------------------------------

/// One request the fake received.
#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("the service sends a JSON body")
    }
}

/// What the fake answers a request with.
enum Reply {
    /// A complete response.
    Full { status: u16, body: String },
    /// A `302` pointing at `to`.
    Redirect { to: String },
    /// A status line and headers announcing a body longer than what follows,
    /// then the connection closed: the transport fails after the status line.
    CutAfterHead,
}

fn reply(status: u16, body: Value) -> Reply {
    Reply::Full {
        status,
        body: body.to_string(),
    }
}

fn raw(status: u16, body: &str) -> Reply {
    Reply::Full {
        status,
        body: body.to_owned(),
    }
}

fn completion(content: &str) -> Reply {
    reply(
        200,
        json!({
            "id": "cmpl-1",
            "object": "chat.completion",
            "model": "qwen",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": "stop"
            }]
        }),
    )
}

type Route = dyn Fn(&Recorded) -> Reply + Send + Sync;

struct Fake {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Recorded>>>,
}

impl Fake {
    async fn start(route: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let route: Arc<Route> = Arc::new(route);
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((mut conn, _)) = listener.accept().await else {
                    return;
                };
                let route = Arc::clone(&route);
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let Some(request) = read_request(&mut conn).await else {
                        return;
                    };
                    log.lock().unwrap().push(request.clone());
                    let bytes = match route(&request) {
                        Reply::Full { status, body } => format!(
                            "HTTP/1.1 {status} Fake\r\nContent-Type: application/json\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        ),
                        Reply::Redirect { to } => format!(
                            "HTTP/1.1 302 Found\r\nLocation: {to}\r\n\
                             Content-Length: 0\r\nConnection: close\r\n\r\n"
                        ),
                        Reply::CutAfterHead => {
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                             Content-Length: 1000\r\nConnection: close\r\n\r\n{\"choi"
                                .to_owned()
                        }
                    };
                    let _ = conn.write_all(bytes.as_bytes()).await;
                    let _ = conn.shutdown().await;
                });
            }
        });
        Fake { addr, seen }
    }

    fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn seen(&self) -> Vec<Recorded> {
        self.seen.lock().unwrap().clone()
    }

    fn only(&self) -> Recorded {
        let seen = self.seen();
        assert_eq!(
            seen.len(),
            1,
            "exactly one request reached the fake: {seen:?}"
        );
        seen.into_iter().next().unwrap()
    }
}

async fn read_request(conn: &mut tokio::net::TcpStream) -> Option<Recorded> {
    let mut buf = Vec::new();
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let mut chunk = [0u8; 4096];
        let n = conn.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8(buf[..head_end].to_vec()).ok()?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_owned();
    let path = request_line.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .map_or(0, |(_, v)| v.parse().unwrap());
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let mut chunk = [0u8; 4096];
        let n = conn.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some(Recorded {
        method,
        path,
        headers,
        body,
    })
}

/// An address nothing listens on: bound, read, and released.
async fn refused_base() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}")
}

// ---------------------------------------------------------------------------
// The service under test
// ---------------------------------------------------------------------------

struct Service {
    child: Child,
    stdout: BufReader<ChildStdout>,
    addr: SocketAddr,
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn command(args: &[&str], key: Option<&str>) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.args(args);
    // A proxy configured on the machine running the tests must not stand
    // between the service and the loopback fake.
    for var in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        cmd.env_remove(var);
    }
    match key {
        Some(key) => cmd.env(KEY_VAR, key),
        None => cmd.env_remove(KEY_VAR),
    };
    cmd
}

impl Service {
    async fn start(base: &str) -> Self {
        Self::start_with_key(base, None).await
    }

    async fn start_with_key(base: &str, key: Option<&str>) -> Self {
        let mut cmd = command(&["--base-url", base, "--listen", "127.0.0.1:0"], key);
        let base = base.to_owned();
        tokio::task::spawn_blocking(move || {
            let mut child = cmd
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("spawn the service binary");
            let mut stdout = BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            let addr = line
                .strip_suffix('\n')
                .and_then(|l| l.strip_prefix("listening on "))
                .unwrap_or_else(|| panic!("no listening line for {base}: {line:?}"))
                .parse()
                .expect("the listening line carries a socket address");
            Service {
                child,
                stdout,
                addr,
            }
        })
        .await
        .unwrap()
    }

    async fn client(&self) -> GeneratorClient<Channel> {
        GeneratorClient::connect(format!("http://{}", self.addr))
            .await
            .expect("connect to the service")
    }
}

fn params(served_model: &str, template: &str) -> GenerateParams {
    GenerateParams {
        served_model: served_model.to_owned(),
        template: template.to_owned(),
        ..Default::default()
    }
}

fn request(params: GenerateParams) -> GenerateRequest {
    GenerateRequest {
        query: Some(Query {
            id: "q1".into(),
            text: "What is the capital of France?".into(),
        }),
        context: Some(Context {
            chunks: vec![],
            text: "Paris is the capital of France.".into(),
        }),
        params: Some(params),
    }
}

fn identity_request(served_model: &str) -> GeneratorModelIdentityRequest {
    GeneratorModelIdentityRequest {
        served_model: served_model.to_owned(),
    }
}

async fn generate_status(base: &str, req: GenerateRequest) -> tonic::Status {
    let service = Service::start(base).await;
    service
        .client()
        .await
        .generate(req)
        .await
        .expect_err("the call is refused")
}

async fn identity_status(base: &str, served_model: &str) -> tonic::Status {
    let service = Service::start(base).await;
    service
        .client()
        .await
        .get_model_identity(identity_request(served_model))
        .await
        .expect_err("the call is refused")
}

async fn identity_of(models: Value, served_model: &str) -> String {
    let fake = Fake::start(move |_| reply(200, models.clone())).await;
    let service = Service::start(&fake.base()).await;
    let response = service
        .client()
        .await
        .get_model_identity(identity_request(served_model))
        .await
        .expect("a listed model is answered")
        .into_inner();
    let seen = fake.only();
    assert_eq!(
        (seen.method.as_str(), seen.path.as_str()),
        ("GET", "/v1/models")
    );
    response.identity.expect("an identity").identity
}

// ---------------------------------------------------------------------------
// The answer round trip, and what the service sends
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_round_trips() {
    let fake = Fake::start(|_| completion("Paris.")).await;
    let service = Service::start(&fake.base()).await;

    let response = service
        .client()
        .await
        .generate(request(params("qwen", "{context}\n{query}")))
        .await
        .expect("the call is answered")
        .into_inner();

    assert_eq!(response.answer.expect("an answer").text, "Paris.");
    let seen = fake.only();
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.path, "/v1/chat/completions");
    assert!(seen
        .header("content-type")
        .is_some_and(|v| v.starts_with("application/json")));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_body_is_one_user_message_holding_the_rendered_template() {
    let fake = Fake::start(|_| completion("ok")).await;
    let service = Service::start(&fake.base()).await;

    service
        .client()
        .await
        .generate(request(params(
            "Qwen/Qwen2.5-7B-Instruct",
            "Context: {context}\nQuestion: {query}\nReply as {{\"answer\": ...}}, not {{query}}.",
        )))
        .await
        .expect("the call is answered");

    let body = fake.only().json();
    assert_eq!(
        body,
        json!({
            "model": "Qwen/Qwen2.5-7B-Instruct",
            "messages": [{
                "role": "user",
                "content": "Context: Paris is the capital of France.\n\
                            Question: What is the capital of France?\n\
                            Reply as {\"answer\": ...}, not {query}."
            }]
        }),
        "one user message, no system message, no optional field and nothing else"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_optional_knobs_are_sent_as_received_when_set() {
    let fake = Fake::start(|_| completion("ok")).await;
    let service = Service::start(&fake.base()).await;
    let mut set = params("qwen", "{query}");
    // A zero temperature is a value, greedy decoding, and not an absence.
    set.temperature = Some(0.0);
    // Beyond the signed 64-bit range some servers bound a seed to: relayed as
    // received, never clamped (ADR-C33, Consequences).
    set.seed = Some(u64::MAX);
    set.max_tokens = Some(128);

    service
        .client()
        .await
        .generate(request(set))
        .await
        .expect("the call is answered");

    let body = fake.only().json();
    let mut keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["max_tokens", "messages", "model", "seed", "temperature"]
    );
    assert_eq!(body["temperature"], json!(0.0));
    assert_eq!(body["seed"], json!(u64::MAX));
    assert_eq!(body["max_tokens"], json!(128));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fractional_temperature_is_sent_exactly() {
    let fake = Fake::start(|_| completion("ok")).await;
    let service = Service::start(&fake.base()).await;
    let mut set = params("qwen", "{query}");
    set.temperature = Some(0.7);

    service
        .client()
        .await
        .generate(request(set))
        .await
        .expect("the call is answered");

    let body = fake.only().json();
    assert_eq!(body["temperature"].as_f64(), Some(0.7));
    assert!(body.get("seed").is_none() && body.get("max_tokens").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_api_key_is_sent_as_a_bearer_token_to_both_endpoints() {
    let fake = Fake::start(|r| {
        if r.path == "/v1/models" {
            reply(200, json!({"data": [{"id": "qwen"}]}))
        } else {
            completion("ok")
        }
    })
    .await;
    let service = Service::start_with_key(&fake.base(), Some("sk-test")).await;
    let mut client = service.client().await;

    client
        .generate(request(params("qwen", "{query}")))
        .await
        .unwrap();
    client
        .get_model_identity(identity_request("qwen"))
        .await
        .unwrap();

    let seen = fake.seen();
    assert_eq!(seen.len(), 2);
    for r in &seen {
        assert_eq!(
            r.header("authorization"),
            Some("Bearer sk-test"),
            "{}",
            r.path
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn no_authorization_header_without_a_key_or_with_an_empty_one() {
    for key in [None, Some("")] {
        let fake = Fake::start(|_| completion("ok")).await;
        let service = Service::start_with_key(&fake.base(), key).await;
        service
            .client()
            .await
            .generate(request(params("qwen", "{query}")))
            .await
            .unwrap();
        assert_eq!(fake.only().header("authorization"), None, "key {key:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_base_url_keeps_its_path_and_loses_one_trailing_slash() {
    for (suffix, expected) in [
        ("/", "/v1/chat/completions"),
        ("/proxy", "/proxy/v1/chat/completions"),
        ("/proxy/", "/proxy/v1/chat/completions"),
    ] {
        let fake = Fake::start(|_| completion("ok")).await;
        let service = Service::start(&format!("{}{suffix}", fake.base())).await;
        service
            .client()
            .await
            .generate(request(params("qwen", "{query}")))
            .await
            .unwrap();
        assert_eq!(fake.only().path, expected, "base suffix {suffix:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stdout_holds_the_listening_line_and_nothing_else() {
    let fake = Fake::start(|_| completion("ok")).await;
    let mut service = Service::start_with_key(&fake.base(), Some("sk-secret")).await;
    service
        .client()
        .await
        .generate(request(params("qwen", "{query}")))
        .await
        .unwrap();

    service.child.kill().unwrap();
    service.child.wait().unwrap();
    let mut rest = String::new();
    service.stdout.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "", "nothing after the listening line");
}

// ---------------------------------------------------------------------------
// The error table (ADR-C33 § 5): refusals before any request
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_call_is_refused_before_any_request() {
    let nan = GenerateParams {
        temperature: Some(f64::NAN),
        ..params("qwen", "{query}")
    };
    let inf = GenerateParams {
        temperature: Some(f64::INFINITY),
        ..params("qwen", "{query}")
    };
    let cases = [
        ("empty served_model", request(params("", "{query}"))),
        ("empty template", request(params("qwen", ""))),
        ("NaN temperature", request(nan)),
        ("infinite temperature", request(inf)),
        ("unknown placeholder", request(params("qwen", "{question}"))),
        ("unclosed brace", request(params("qwen", "{query"))),
        ("lone closing brace", request(params("qwen", "a } b"))),
        (
            "no query",
            GenerateRequest {
                query: None,
                ..request(params("qwen", "{query}"))
            },
        ),
        (
            "no context",
            GenerateRequest {
                context: None,
                ..request(params("qwen", "{query}"))
            },
        ),
        (
            "no params",
            GenerateRequest {
                params: None,
                ..request(params("qwen", "{query}"))
            },
        ),
    ];
    let fake = Fake::start(|_| completion("never")).await;
    let service = Service::start(&fake.base()).await;
    let mut client = service.client().await;
    for (what, req) in cases {
        let status = client.generate(req).await.expect_err(what);
        assert_eq!(status.code(), Code::InvalidArgument, "{what}: {status:?}");
    }
    assert!(fake.seen().is_empty(), "no refused call reached the fake");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_served_model_is_refused_by_identity_before_any_request() {
    let fake = Fake::start(|_| reply(200, json!({"data": []}))).await;
    let status = identity_status(&fake.base(), "").await;
    assert_eq!(status.code(), Code::InvalidArgument, "{status:?}");
    assert!(fake.seen().is_empty());
}

// ---------------------------------------------------------------------------
// The error table: the transport
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_inference_server_is_unavailable() {
    let base = refused_base().await;
    let status = generate_status(&base, request(params("qwen", "{query}"))).await;
    assert_eq!(status.code(), Code::Unavailable, "{status:?}");
    let status = identity_status(&base, "qwen").await;
    assert_eq!(status.code(), Code::Unavailable, "{status:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_transport_failure_after_the_status_line_is_unavailable() {
    let fake = Fake::start(|_| Reply::CutAfterHead).await;
    let status = generate_status(&fake.base(), request(params("qwen", "{query}"))).await;
    assert_eq!(status.code(), Code::Unavailable, "{status:?}");
    let status = identity_status(&fake.base(), "qwen").await;
    assert_eq!(status.code(), Code::Unavailable, "{status:?}");
}

// ---------------------------------------------------------------------------
// The error table: HTTP statuses, from either endpoint
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn each_http_status_class_maps_to_its_grpc_status() {
    let cases = [
        (429, Code::Unavailable),
        (503, Code::Unavailable),
        (400, Code::InvalidArgument),
        (401, Code::InvalidArgument),
        (403, Code::InvalidArgument),
        (404, Code::InvalidArgument),
        (422, Code::InvalidArgument),
        (500, Code::Internal),
        (502, Code::Internal),
        (504, Code::Internal),
        (302, Code::Internal),
        (304, Code::Internal),
    ];
    for (http, expected) in cases {
        let fake = Fake::start(move |_| {
            reply(
                http,
                json!({"error": {"message": format!("upstream says {http}")}}),
            )
        })
        .await;
        let service = Service::start(&fake.base()).await;
        let mut client = service.client().await;

        let status = client
            .generate(request(params("qwen", "{query}")))
            .await
            .expect_err("refused");
        assert_eq!(status.code(), expected, "generate, HTTP {http}: {status:?}");
        assert!(
            status.message().contains(&http.to_string()),
            "the message carries the upstream status: {status:?}"
        );
        // A 304 has no body by HTTP's rules, so it has no error text to carry.
        if http != 304 {
            assert!(
                status.message().contains(&format!("upstream says {http}")),
                "the message carries the upstream error text: {status:?}"
            );
        }

        let status = client
            .get_model_identity(identity_request("qwen"))
            .await
            .expect_err("refused");
        assert_eq!(status.code(), expected, "identity, HTTP {http}: {status:?}");

        assert_eq!(
            fake.seen().len(),
            2,
            "one request per call, no redirect followed"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_redirect_is_not_followed() {
    let fake = Fake::start(|r| {
        if r.path == "/elsewhere" {
            completion("followed")
        } else {
            Reply::Redirect {
                to: "/elsewhere".into(),
            }
        }
    })
    .await;
    let status = generate_status(&fake.base(), request(params("qwen", "{query}"))).await;
    assert_eq!(status.code(), Code::Internal, "{status:?}");
    assert_eq!(fake.only().path, "/v1/chat/completions");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_api_key_never_reaches_the_status_message() {
    let fake = Fake::start(|r| {
        let echoed = r.header("authorization").unwrap_or_default().to_owned();
        reply(
            401,
            json!({"error": {"message": format!("invalid key: {echoed}")}}),
        )
    })
    .await;
    let service = Service::start_with_key(&fake.base(), Some("sk-secret-42")).await;
    let status = service
        .client()
        .await
        .generate(request(params("qwen", "{query}")))
        .await
        .expect_err("refused");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(status.message().contains("401"), "{status:?}");
    assert!(!status.message().contains("sk-secret-42"), "{status:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_echoed_across_the_error_text_limit_is_redacted_whole() {
    const KEY: &str = "sk-LEAKYSECRET-123456";
    // A plain-text body whose echo of the key straddles the point where the
    // error text is cut: redacting after the cut would miss the whole key and
    // let its first characters through.
    let fake = Fake::start(|_| raw(401, &format!("{}{KEY} trailing", "x".repeat(505)))).await;
    let service = Service::start_with_key(&fake.base(), Some(KEY)).await;
    let status = service
        .client()
        .await
        .generate(request(params("qwen", "{query}")))
        .await
        .expect_err("refused");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(!status.message().contains(&KEY[..6]), "{status:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_reflected_into_an_undecodable_2xx_body_is_redacted() {
    const KEY: &str = "sk-REFLECTED-987654";
    // `serde_json` quotes a mistyped string value in its error, so a body
    // that puts the key where a list belongs would carry it into the status.
    let cases = [
        ("completion", json!({"choices": KEY})),
        (
            "completion",
            json!({"choices": [{"message": {"content": [KEY]}}]}),
        ),
    ];
    for (what, body) in cases {
        let fake = Fake::start(move |_| reply(200, body.clone())).await;
        let service = Service::start_with_key(&fake.base(), Some(KEY)).await;
        let status = service
            .client()
            .await
            .generate(request(params("qwen", "{query}")))
            .await
            .expect_err("refused");
        assert_eq!(status.code(), Code::Internal, "{what}: {status:?}");
        assert!(!status.message().contains(KEY), "{what}: {status:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_long_json_error_message_is_cut_like_any_other_error_text() {
    let fake = Fake::start(|_| reply(500, json!({"error": {"message": "y".repeat(4000)}}))).await;
    let status = generate_status(&fake.base(), request(params("qwen", "{query}"))).await;
    assert_eq!(status.code(), Code::Internal);
    assert!(status.message().contains("yyyy"), "{status:?}");
    assert!(
        status.message().chars().count() < 700,
        "the upstream text is bounded: {} chars",
        status.message().chars().count()
    );
}

// ---------------------------------------------------------------------------
// The error table: a 2xx the service cannot read
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_completion_is_internal() {
    let cases = [
        ("not JSON", raw(200, "<html>")),
        ("no choices field", reply(200, json!({"id": "x"}))),
        ("no choice", reply(200, json!({"choices": []}))),
        (
            "null content",
            reply(
                200,
                json!({"choices": [{"message": {"role": "assistant", "content": null}}]}),
            ),
        ),
        (
            "content not a string",
            reply(200, json!({"choices": [{"message": {"content": 7}}]})),
        ),
    ];
    for (what, answer) in cases {
        let answer = Arc::new(Mutex::new(Some(answer)));
        let fake = Fake::start(move |_| answer.lock().unwrap().take().unwrap()).await;
        let status = generate_status(&fake.base(), request(params("qwen", "{query}"))).await;
        assert_eq!(status.code(), Code::Internal, "{what}: {status:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_model_list_is_internal() {
    let cases = [
        ("not JSON", raw(200, "nope")),
        ("no data", reply(200, json!({"object": "list"}))),
        (
            "data not an array",
            reply(200, json!({"data": {"id": "qwen"}})),
        ),
        ("entry not an object", reply(200, json!({"data": ["qwen"]}))),
        (
            "entry without id",
            reply(200, json!({"data": [{"root": "x"}]})),
        ),
        ("id not a string", reply(200, json!({"data": [{"id": 3}]}))),
    ];
    for (what, answer) in cases {
        let answer = Arc::new(Mutex::new(Some(answer)));
        let fake = Fake::start(move |_| answer.lock().unwrap().take().unwrap()).await;
        let status = identity_status(&fake.base(), "qwen").await;
        assert_eq!(status.code(), Code::Internal, "{what}: {status:?}");
    }
}

// ---------------------------------------------------------------------------
// Model identity (ADR-C33 § 4)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_listed_base_model_is_identified_by_what_the_server_reports() {
    let models = json!({
        "object": "list",
        "data": [
            {"id": "other", "root": "elsewhere"},
            {
                "id": "qwen",
                "object": "model",
                "created": 1_758_000_000,
                "owned_by": "vllm",
                "root": "Qwen/Qwen2.5-7B-Instruct",
                "parent": null,
                "max_model_len": 32768,
                "permission": [{"id": "modelperm-random"}]
            }
        ]
    });
    assert_eq!(
        identity_of(models, "qwen").await,
        r#"{"id":"qwen","root":"Qwen/Qwen2.5-7B-Instruct","max_model_len":32768}"#
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_adapter_is_identified_with_its_parent() {
    let models = json!({"data": [
        {"id": "qwen", "root": "Qwen/Qwen2.5-7B-Instruct"},
        {"id": "qwen-lora", "root": "/adapters/sql", "parent": "qwen", "max_model_len": 4096}
    ]});
    assert_eq!(
        identity_of(models, "qwen-lora").await,
        r#"{"id":"qwen-lora","root":"/adapters/sql","parent":"qwen","max_model_len":4096}"#
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_field_that_is_empty_null_or_mistyped_is_omitted() {
    let models = json!({"data": [
        {"id": "qwen", "root": "", "parent": 5, "max_model_len": "32768"}
    ]});
    assert_eq!(identity_of(models, "qwen").await, r#"{"id":"qwen"}"#);
    let models = json!({"data": [
        {"id": "qwen", "root": null, "max_model_len": 1.5}
    ]});
    assert_eq!(identity_of(models, "qwen").await, r#"{"id":"qwen"}"#);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_identity_matches_byte_for_byte_and_is_escaped_as_json() {
    let models = json!({"data": [
        {"id": "Qwen", "root": "wrong"},
        {"id": "qwen \"q\"", "root": "right"}
    ]});
    assert_eq!(
        identity_of(models, "qwen \"q\"").await,
        r#"{"id":"qwen \"q\"","root":"right"}"#
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_identity_is_stable_across_calls() {
    let fake = Fake::start(|_| {
        reply(
            200,
            json!({"data": [{"id": "qwen", "root": "r", "created": rand_like()}]}),
        )
    })
    .await;
    let service = Service::start(&fake.base()).await;
    let mut client = service.client().await;
    let a = client
        .get_model_identity(identity_request("qwen"))
        .await
        .unwrap()
        .into_inner();
    let b = client
        .get_model_identity(identity_request("qwen"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(a, b);
}

/// A value that differs per response, as `created` does on a real server.
fn rand_like() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unlisted_model_is_refused() {
    let fake =
        Fake::start(|_| reply(200, json!({"data": [{"id": "qwen"}, {"id": "llama"}]}))).await;
    let status = identity_status(&fake.base(), "mistral").await;
    assert_eq!(status.code(), Code::InvalidArgument, "{status:?}");
    assert!(status.message().contains("mistral"), "{status:?}");
}

// ---------------------------------------------------------------------------
// The command line (ADR-C33 § 3)
// ---------------------------------------------------------------------------

/// Runs the binary to its exit, or kills it and fails the test: a command line
/// wrongly accepted would otherwise leave the service serving, and the test
/// hanging.
fn run_to_exit(mut cmd: Command, what: &str) -> std::process::Output {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the service binary");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().expect("poll the service binary").is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "{what}: still running after 10 s, so it was accepted; stdout {:?}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    child.wait_with_output().expect("collect the output")
}

#[test]
fn a_bad_command_line_is_refused_before_listening() {
    let cases: &[(&str, &[&str])] = &[
        ("no arguments", &[]),
        ("no --listen", &["--base-url", "http://127.0.0.1:8000"]),
        ("no --base-url", &["--listen", "127.0.0.1:0"]),
        (
            "a flag without its value",
            &["--listen", "127.0.0.1:0", "--base-url"],
        ),
        (
            "an unknown flag",
            &[
                "--base-url",
                "http://127.0.0.1:8000",
                "--listen",
                "127.0.0.1:0",
                "--model",
                "qwen",
            ],
        ),
        (
            "a positional argument",
            &[
                "--base-url",
                "http://127.0.0.1:8000",
                "--listen",
                "127.0.0.1:0",
                "extra",
            ],
        ),
        (
            "a repeated flag",
            &[
                "--base-url",
                "http://127.0.0.1:8000",
                "--base-url",
                "http://127.0.0.1:9000",
                "--listen",
                "127.0.0.1:0",
            ],
        ),
        (
            "an ftp base URL",
            &["--base-url", "ftp://127.0.0.1", "--listen", "127.0.0.1:0"],
        ),
        (
            "a relative base URL",
            &["--base-url", "localhost:8000", "--listen", "127.0.0.1:0"],
        ),
        (
            "not a URL",
            &["--base-url", "not a url", "--listen", "127.0.0.1:0"],
        ),
        (
            "a base URL with a query",
            &[
                "--base-url",
                "http://127.0.0.1:8000/?a=b",
                "--listen",
                "127.0.0.1:0",
            ],
        ),
        (
            "a base URL with a fragment",
            &[
                "--base-url",
                "http://127.0.0.1:8000/#top",
                "--listen",
                "127.0.0.1:0",
            ],
        ),
        (
            "a base URL ending in /v1",
            &[
                "--base-url",
                "http://127.0.0.1:8000/v1",
                "--listen",
                "127.0.0.1:0",
            ],
        ),
        (
            "a base URL ending in /v1/",
            &[
                "--base-url",
                "http://127.0.0.1:8000/v1/",
                "--listen",
                "127.0.0.1:0",
            ],
        ),
        (
            "a listen address that is not a socket address",
            &[
                "--base-url",
                "http://127.0.0.1:8000",
                "--listen",
                "localhost",
            ],
        ),
    ];
    for (what, args) in cases {
        let output = run_to_exit(command(args, None), what);
        assert!(!output.status.success(), "{what}: exits non-zero");
        assert!(!output.stderr.is_empty(), "{what}: says why on stderr");
        assert!(
            output.stdout.is_empty(),
            "{what}: no listening line: {:?}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn a_refusal_never_echoes_the_api_key() {
    let good = [
        "--base-url",
        "http://127.0.0.1:8000",
        "--listen",
        "127.0.0.1:0",
    ];
    let cases: [(&str, &[&str], &str); 2] = [
        (
            "a bad command line",
            &["--listen", "127.0.0.1:0"],
            "sk-secret-42",
        ),
        // A key that cannot be sent in a header is refused at startup.
        (
            "a key with a line break",
            &good,
            "sk-secret-42\nX-Injected: 1",
        ),
    ];
    for (what, args, key) in cases {
        let output = run_to_exit(command(args, Some(key)), what);
        assert!(!output.status.success(), "{what}: exits non-zero");
        assert!(output.stdout.is_empty(), "{what}: no listening line");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.is_empty(), "{what}: says why");
        assert!(!stderr.contains("sk-secret-42"), "{what}: {stderr}");
    }
}
