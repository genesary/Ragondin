# ARCHITECTURE — ragondin-generator-service

**Status: not an API boundary, and not product surface.** A reference
implementation and a calibration fixture (ADR-C33). Its manifest sets
`publish = false`; nothing in the workspace depends on it, and nothing in the
product reaches it. `ragondin` stays the one binary a user runs and the
composition root (ADR-C15): this crate's binary is what a `Remote` author reads
beside `generator.proto`, and what the generation calibration spawns to run a
real model.

## Where it sits: `testkit/`

`testkit/` holds **reference implementations and fixtures**, not only the
conformance suite (ADR-C33 § 1, Consequences). The two crates there have
opposite shapes, and the difference is deliberate:

- `ragondin-conformance` is a library a component crate pulls into its
  dev-dependencies, so it stays light.
- This crate is a `Remote` service. Its one normal dependency in the
  workspace is `ragondin-proto`, for the generated `Generator` server trait —
  never `ragondin-engine`, never a crate under `components/` — so its HTTP
  client reaches nobody else's graph. Its tests add `ragondin-remote`,
  `ragondin-contracts` and `ragondin-types` as dev-dependencies (§ Tests).

## What it does

A `tonic` server for the `Generator` service of `ragondin-proto`, answering
each call with one request to an inference server that speaks the
**OpenAI-compatible HTTP API** (ADR-C33 § 3):

| rpc | HTTP |
|---|---|
| `Generate` | `POST <base>/v1/chat/completions` |
| `GetModelIdentity` | `GET <base>/v1/models` |

`<base>` is the configured base URL with any one trailing `/` removed.

**It is a stateless relay.** It holds no experiment variable (ADR-C31 § 2):
the served model, the template and the three sampling knobs arrive in each
call, and the service relays them. Per `Generate` call it:

1. refuses an absent `query`, `context` or `params` message (ADR-C24), an empty
   `served_model` or `template`, and a `temperature` that is `NaN` or infinite;
2. renders `template` under ADR-C31 § 2's grammar (`src/template.rs`) and
   refuses a malformed one;
3. sends `model` = `served_model` and `messages` = exactly one `user` message
   holding the rendered text — no system message, no wording of its own;
4. adds `temperature`, `seed` and `max_tokens` **only when the call carries
   them**, as received: an absent knob is absent from the JSON body, so the
   inference server's own default applies. No other field is sent;
5. returns `choices[0].message.content`.

No retry, no streaming, no batching, and no timeout of its own: one call is
one HTTP request. A `seed` is relayed and nothing more; whether the server
honours it is the server's.

**The knobs**, all per call, none held by the service: `served_model` and
`template` (required), `temperature`, `seed`, `max_tokens` (optional, sent
under those JSON names). `max_tokens` is the name the OpenAI-compatible
servers share; vLLM marks it deprecated in favour of `max_completion_tokens`,
and changing the name sent is a change to ADR-C33.

## Configuration

Three values, none of them an experiment variable (ADR-C33 § 3):

| | |
|---|---|
| `--base-url <url>` | required; an absolute `http` or `https` URL, the server's root **without** `/v1`; no user or password, no query, no fragment |
| `--listen <socket address>` | required; `127.0.0.1:0` asks the operating system for a port |
| `RAGONDIN_INFERENCE_API_KEY` | optional; printable ASCII other than `"` and `\`; when non-empty, every request carries `Authorization: Bearer <key>`. Unset and empty are the same. Never a flag, never written to any output |

The flags are read from `std::env::args` without `clap`, which the root
manifest reserves to `bin/ragondin`. A bad command line — a missing, repeated
or unknown flag, a flag without its value, a base URL of another scheme or
with a query or a fragment or ending in `/v1`, a listen address that is not a
socket address — is refused at startup with exit status 2 and a message on
stderr, before anything is written to stdout.

**Once bound, the service writes exactly one line to stdout:
`listening on <address>`**, the bound `SocketAddr` in Rust's display form, and
nothing else, ever. A test or the calibration reads that line to learn the
port.

## Model identity

`GetModelIdentity(served_model)` reads `/v1/models` and takes the first entry
of `data` whose `id` equals `served_model` byte for byte (ADR-C33 § 4). The
identity is that entry encoded compactly by `serde_json`, keys in the order
`id`, `root`, `parent`, `max_model_len`: `root` and `parent` only when the
server reports a non-empty string, `max_model_len` only when it reports an
integer. Anything else in the entry — `created`, `owned_by`, the permission
ids — is left out, because it is either constant or different per response,
and an identity must be stable (ADR-C31 § 4). A name the server does not list
is refused, never warned about.

## Errors

A `Remote` service returns exactly one of three gRPC codes —
`INVALID_ARGUMENT`, `UNAVAILABLE` and `INTERNAL` — and no other
(ADR-C35 § 1). ADR-C33 § 5's table, which this service applies, is an
instance of that rule:

| What happened | Status |
|---|---|
| an absent request message; empty `served_model` or `template`; a non-finite `temperature`; a malformed template | `INVALID_ARGUMENT` |
| the request failed before any response (connection refused, a connect error) | `UNAVAILABLE` |
| the transport failed after the status line of a `2xx` (its body could not be read) | `UNAVAILABLE` |
| HTTP `429` or `503` | `UNAVAILABLE` |
| any other `4xx`, from either endpoint, a model the server does not serve included | `INVALID_ARGUMENT` |
| any other `5xx`, and any other non-success status, a `3xx` included | `INTERNAL` |
| a `2xx` completion that does not decode, has no choice, or whose first choice's `content` is null | `INTERNAL` |
| a `2xx` model list with no `data` array of objects each carrying a string `id` | `INTERNAL` |
| `served_model` absent from `/v1/models` | `INVALID_ARGUMENT` |

The client **follows no redirect** (`Policy::none()`), so a `3xx` is a
non-success status like any other rather than a `POST` resent as a `GET`.

**A non-success whose body cannot be read is classified by its status code**,
like any other non-success; the message then carries no error text. Only a
`2xx` whose body fails is `UNAVAILABLE` for that reason.

**The transport rows are decided by phase, not by cause.** Every error
`reqwest` returns from sending is `UNAVAILABLE`, and so is every error reading a
`2xx` body. `reqwest`'s public error kinds (`is_connect`, `is_request`,
`is_body`) do not separate a connection dropped by the server from a response
that is not valid HTTP: both reach this crate as the same kind with a `hyper`
error as the source, which it could tell apart only by depending on `hyper`
directly — a new `[workspace.dependencies]` entry, which escalates — or by
matching error text. So a malformed response is reported as `UNAVAILABLE`,
which is ADR-C33 § 5's classification of both phases, rather than the
`INTERNAL` ADR-C35 § 1 gives "any other failure".

An upstream failure's status message carries the HTTP status code and, where
the body has one, its error text: the OpenAI-shaped `error.message` when there
is one, otherwise the body. The API key is replaced by `<redacted>` in the whole
text **before** it is cut to 512 characters, so a key the server echoed back
straddling the cut leaves nothing of itself behind. **Every status the
service returns is then built by one constructor, `Relay::status`, which
replaces the key in the whole message** — so a decode error quoting a
mistyped value the server reflected the key into, or any other text, cannot
carry it either.

## Dependencies and the feature

| Dependency | Why |
|---|---|
| `ragondin-proto` | the generated `Generator` server trait and messages |
| `tokio` | the runtime and the listener |
| `serde`, `serde_json` | the inference server's JSON, and the identity's encoding |
| `thiserror` | the library's typed errors, `TemplateError` and `CliError` |
| `reqwest` (optional) | the HTTP client — the workspace's one, decided in ADR-C33 § 2: `0.12`, `rustls-tls` and `json`, no native TLS |
| `tonic` (optional) | the gRPC server |

The feature `service` enables the two optional dependencies, and the binary
and its tests carry `required-features = ["service"]`. Without the feature the
library holds the template renderer alone, so a workspace build compiles no
HTTP client, and `tonic` only as far as `ragondin-proto` already does.
`tokio`'s `net` and `io-util` features, which binding a listener and the
tests' fake server use, arrive through `tonic`'s transport, which
`ragondin-proto` already enables; neither this crate nor the root adds a
feature to the `tokio` entry.

## Choices made here

Each is inside this crate, and recorded so that a reviewer can disagree with
it.

- **No `ragondin-types` dependency.** ADR-C33 § 1 lists it among what the
  crate may depend on; the relay handles only the generated messages, so it has
  no use for the domain types, and an unused dependency would be one more edge
  for nothing.
- **`thiserror`, which ADR-C33 § 1 does not list.** The library exports two
  error types, `TemplateError` and `CliError`, and the **Errors** row of
  `AGENTS.md` § Frozen decisions puts typed `thiserror` errors in a library.
  It is an existing workspace entry, so no new dependency enters the graph.
- **A library beside the binary.** `src/template.rs` is compiled in every
  configuration and unit-tested by `just test`; `src/cli.rs` and
  `src/relay.rs` are compiled under `service`. `src/main.rs` only wires them.
- **The base URL is validated with `reqwest::Url`**, the `url` crate `reqwest`
  re-exports, rather than by hand: absolute, `http` or `https`, no query, no
  fragment, and a path that does not end in `/v1` with or without a trailing
  `/`. What the service then prefixes to `/v1/…` is the string as given, one
  trailing `/` removed, so a path prefix such as a reverse proxy's is kept.
- **Repeated flags and positional arguments are refused**, as unknown
  arguments are: a command line that could mean two things is refused rather
  than resolved by position.
- **The API key must be printable ASCII other than `"` and `\`**, and any
  other key is refused at startup, with a message that does not show it.
  Those are exactly the characters neither a JSON string nor a
  `{:?}`-quoted one escapes, so a key the inference server reflects into an
  error or a mistyped field is found verbatim in the text that quotes it, and
  redacted; a key with an escaped character would slip past the literal
  replacement. Such a key is also always a valid header value.
- **A base URL carrying a user or a password is refused**, without showing
  it: a credential belongs in `RAGONDIN_INFERENCE_API_KEY`, never on the
  command line. The client's error text is also taken `without_url`, so no
  URL reaches a status message; the message names the endpoint by its path.
- **A decode error is redacted and cut** like an upstream error text:
  `serde_json` quotes a mistyped value whole.
- **Error text** is `error.message` when the body is OpenAI-shaped JSON, then
  `error`, `message` or `detail` as a string, then the body itself; whichever
  it is, the key is redacted and then the text is cut to 512 characters.
- **Proxies.** The client keeps `reqwest`'s reading of `HTTP_PROXY`,
  `HTTPS_PROXY` and `ALL_PROXY`; with `default-features = false` it does not
  read the platform's system proxy settings. A proxy those variables name
  receives every request, the `Authorization: Bearer` header with the API key
  included, so the key is only as private as that proxy. The tests clear those
  variables for the binary they spawn, so a proxy on the test machine cannot
  stand between the service and the loopback fake.
- **`clippy::result_large_err` is allowed in `src/relay.rs`**, for the reason
  `ragondin-proto` allows it on the generated code: every rpc returns
  `Result<_, tonic::Status>`, and `Status` is as large as `tonic` makes it.

## Tests

- `src/template.rs` — the template grammar of ADR-C31 § 2, unit by unit:
  placeholders, escapes, left-to-right precedence, no rescanning, and each
  malformation.
- `tests/service.rs` — the binary, spawned with `--listen 127.0.0.1:0` against
  a fake inference server written by hand over `tokio::net::TcpListener`
  (ADR-C33 § 6; a mock-server crate would be a new workspace entry). The fake
  records what it received, so the tests assert on the body the service sent:
  the round trip, the rendered template and its escapes, the optional knobs
  absent and present, the bearer header, the base URL's path, stdout's one
  line, every row of the error table, the redaction of the key, the identity
  of a listed and of an unlisted model, and the refusal of a bad command line.
  No test touches the network beyond loopback, and none needs a real model.

**The tests drive the service through `RemoteGenerator`** from
`ragondin-remote`, over a lazily connecting channel, as ADR-C33 § 6 requires:
each row of the error table is asserted as the `ComponentError` a caller
finally sees, under ADR-C35 § 2 — `INVALID_ARGUMENT` as `InvalidRequest`,
`UNAVAILABLE` as `Unavailable`, `INTERNAL` as `Backend` whose source is the
`Status`. `ragondin-remote`, `ragondin-contracts` and `ragondin-types` are
dev-dependencies for that alone.

**One group of calls goes through the bare generated `GeneratorClient`
instead**: an empty `served_model` or `template`, and a request missing its
query, context or params message. The adapter refuses the first two itself
before sending (ADR-C31 § 2 asks both it and the service to), and always sends
the three messages, so through it these rows would never reach the service. A
`Remote` caller in another language can send them, and the service must refuse
them on receipt.
