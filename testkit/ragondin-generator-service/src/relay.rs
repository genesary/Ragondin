//! The relay: the generated `Generator` server trait, answered by one HTTP
//! request to an OpenAI-compatible inference server per call (ADR-C33 § 3–§ 5).
//!
//! [`Relay`] is a plain struct implementing a domain service; `tonic`'s
//! router is the network envelope around it, and it is never a
//! `tower::Service` (INV-11).

// Every rpc of the generated trait returns `Result<_, tonic::Status>`, and the
// helpers here produce that `Status`; its size is `tonic`'s choice, as it is
// for the generated code `ragondin-proto` allows the same lint on.
#![allow(clippy::result_large_err)]

use ragondin_proto::v1::generator_server::Generator;
use ragondin_proto::v1::{
    Answer, GenerateRequest, GenerateResponse, GeneratorModelIdentityRequest,
    GeneratorModelIdentityResponse, ModelIdentity,
};
use reqwest::{RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Number, Value};
use tonic::{Code, Request, Response, Status};

use crate::template;

/// The service's state: an HTTP client and where it points. Nothing in it
/// decides an answer.
#[derive(Debug, Clone)]
pub struct Relay {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl Relay {
    /// A relay to the inference server at `base_url` — its root, without a
    /// trailing `/` and without `/v1` — sending `api_key`, when there is one,
    /// as a bearer token.
    ///
    /// The client follows no redirect, so a `3xx` is a non-success status
    /// like any other (ADR-C33 § 5), and sets no timeout of its own.
    pub fn new(base_url: String, api_key: Option<String>) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            base_url,
            api_key,
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// Sends one request and returns the body of a `2xx`, or the status
    /// ADR-C33 § 5 assigns to what went wrong.
    async fn send(&self, request: RequestBuilder, endpoint: &str) -> Result<Vec<u8>, Status> {
        let request = match &self.api_key {
            Some(key) => request.bearer_auth(key),
            None => request,
        };
        let response = request.send().await.map_err(|e| {
            self.status(
                Code::Unavailable,
                format!(
                    "the inference server could not be reached at {endpoint}: {}",
                    chain(&e)
                ),
            )
        })?;
        let http = response.status();
        let body = response.bytes().await;
        if http.is_success() {
            return body.map(|b| b.to_vec()).map_err(|e| {
                self.status(
                    Code::Unavailable,
                    format!(
                        "the inference server's {endpoint} response failed after HTTP {}: {}",
                        http.as_u16(),
                        chain(&e)
                    ),
                )
            });
        }
        let text = body.ok().map(|b| error_text(&b)).unwrap_or_default();
        Err(self.status(
            classify(http),
            format!(
                "the inference server answered {endpoint} with HTTP {}{}{text}",
                http.as_u16(),
                if text.is_empty() { "" } else { ": " }
            ),
        ))
    }

    /// A status whose message never carries the API key, even where the
    /// inference server echoed it back.
    fn status(&self, code: Code, message: String) -> Status {
        let message = match &self.api_key {
            Some(key) => message.replace(key.as_str(), "<redacted>"),
            None => message,
        };
        Status::new(code, message)
    }
}

/// Which gRPC status an HTTP status that is not a success becomes.
fn classify(http: StatusCode) -> Code {
    match http.as_u16() {
        // Both mean "try later", which is what UNAVAILABLE says.
        429 | 503 => Code::Unavailable,
        400..=499 => Code::InvalidArgument,
        _ => Code::Internal,
    }
}

/// The inference server's error text: the OpenAI-shaped `error.message` when
/// there is one, otherwise the body itself, shortened.
fn error_text(body: &[u8]) -> String {
    const LIMIT: usize = 512;
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        let found = [
            value.pointer("/error/message"),
            value.get("error"),
            value.get("message"),
            value.get("detail"),
        ]
        .into_iter()
        .flatten()
        .find_map(Value::as_str);
        if let Some(text) = found {
            return text.to_owned();
        }
    }
    let text = String::from_utf8_lossy(body);
    let text = text.trim();
    match text.char_indices().nth(LIMIT) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

/// An error and its sources, since `reqwest`'s own message names the
/// request and leaves the cause to its source.
fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

fn invalid(message: impl Into<String>) -> Status {
    Status::invalid_argument(message)
}

/// The part of a chat-completions response the service reads.
#[derive(Deserialize)]
struct ChatCompletion {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

/// A model's identity, in the key order ADR-C33 § 4 fixes: `serde_json`
/// writes a struct's fields in declaration order.
#[derive(Serialize)]
struct Identity<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_model_len: Option<&'a Number>,
}

fn non_empty_str(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn integer(value: Option<&Value>) -> Option<&Number> {
    value
        .and_then(Value::as_number)
        .filter(|n| n.is_i64() || n.is_u64())
}

#[tonic::async_trait]
impl Generator for Relay {
    async fn generate(
        &self,
        request: Request<GenerateRequest>,
    ) -> Result<Response<GenerateResponse>, Status> {
        let GenerateRequest {
            query,
            context,
            params,
        } = request.into_inner();
        let query = query.ok_or_else(|| invalid("query is required"))?;
        let context = context.ok_or_else(|| invalid("context is required"))?;
        let params = params.ok_or_else(|| invalid("params is required"))?;
        if params.served_model.is_empty() {
            return Err(invalid("served_model is empty"));
        }
        if params.template.is_empty() {
            return Err(invalid("template is empty"));
        }
        if let Some(t) = params.temperature.filter(|t| !t.is_finite()) {
            return Err(invalid(format!("temperature {t} is not finite")));
        }
        let content = template::render(&params.template, &query.text, &context.text)
            .map_err(|e| invalid(e.to_string()))?;

        let mut body = Map::new();
        body.insert("model".into(), json!(params.served_model));
        body.insert(
            "messages".into(),
            json!([{"role": "user", "content": content}]),
        );
        // Only what the call carries: an absent knob is absent from the body,
        // so the inference server's own default applies (ADR-C33 § 3).
        if let Some(t) = params.temperature {
            body.insert("temperature".into(), json!(t));
        }
        if let Some(seed) = params.seed {
            body.insert("seed".into(), json!(seed));
        }
        if let Some(max) = params.max_tokens {
            body.insert("max_tokens".into(), json!(max));
        }

        let endpoint = "/v1/chat/completions";
        let bytes = self
            .send(
                self.client.post(self.endpoint(endpoint)).json(&body),
                endpoint,
            )
            .await?;
        let completion: ChatCompletion = serde_json::from_slice(&bytes).map_err(|e| {
            Status::internal(format!("the {endpoint} response does not decode: {e}"))
        })?;
        let text = completion
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| Status::internal(format!("the {endpoint} response has no choice")))?
            .message
            .content
            .ok_or_else(|| {
                Status::internal(format!(
                    "the {endpoint} response's first choice has no content"
                ))
            })?;
        Ok(Response::new(GenerateResponse {
            answer: Some(Answer { text }),
        }))
    }

    async fn get_model_identity(
        &self,
        request: Request<GeneratorModelIdentityRequest>,
    ) -> Result<Response<GeneratorModelIdentityResponse>, Status> {
        let served_model = request.into_inner().served_model;
        if served_model.is_empty() {
            return Err(invalid("served_model is empty"));
        }
        let endpoint = "/v1/models";
        let bytes = self
            .send(self.client.get(self.endpoint(endpoint)), endpoint)
            .await?;
        let malformed = || {
            Status::internal(format!(
                "the {endpoint} response is not a list of models with string ids"
            ))
        };
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| malformed())?;
        let entries = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(malformed)?
            .iter()
            .map(|entry| {
                let entry = entry.as_object().ok_or_else(malformed)?;
                let id = entry
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(malformed)?;
                Ok((id, entry))
            })
            .collect::<Result<Vec<_>, Status>>()?;
        let (id, entry) = entries
            .into_iter()
            .find(|(id, _)| *id == served_model)
            .ok_or_else(|| {
                invalid(format!(
                    "the inference server does not serve {served_model:?}"
                ))
            })?;
        let identity = serde_json::to_string(&Identity {
            id,
            root: non_empty_str(entry.get("root")),
            parent: non_empty_str(entry.get("parent")),
            max_model_len: integer(entry.get("max_model_len")),
        })
        .map_err(|e| Status::internal(format!("the identity does not encode: {e}")))?;
        Ok(Response::new(GeneratorModelIdentityResponse {
            identity: Some(ModelIdentity { identity }),
        }))
    }
}
