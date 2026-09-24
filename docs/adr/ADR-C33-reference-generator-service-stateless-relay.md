---
id: ADR-C33
title: The reference Remote generator service is a stateless relay under testkit/, behind a feature, over one HTTP client, speaking the OpenAI-compatible chat-completions dialect
status: accepted
invariants: [INV-4, INV-11]
supersedes: []
superseded_by: null
---

# ADR-C33: The reference Remote generator service is a stateless relay under testkit/, behind a feature, over one HTTP client, speaking the OpenAI-compatible chat-completions dialect

## Context

ADR-C31 makes the generator the one family the platform runs `Remote` by
design: `docs/system-architecture.md` § 10 lists the LLM inference server among
what is "deliberately not reimplemented" and calls it "over the network". For
face 2 to be exercised end to end, and for the generation calibration (#269) to
run a real model, the workspace needs one real implementation of the
`Generator` gRPC service — a small server that answers each call by asking an
LLM inference server over HTTP. ADR-C31 decided what that service carries and
left what it *is* open by name: "What the service *is* — where it lives, the
HTTP client it uses, the dialect it speaks to its inference server — is not an
experiment variable, and #253 decides it." ADR-C32 left the same item open:
"What the reference generator service is (#253)."

Each of the three choices is on the escalation list in `AGENTS.md` § Rules of
engagement, and no accepted text settles any of them:

- **Where it lives.** ADR-C15 makes `ragondin` "the composition root and the
  entire user-facing surface", and does not say whether a second `[[bin]]`
  that is not user-facing is a packaging violation or outside its scope.
  ADR-C12 lets the controller live outside the workspace behind a clean network
  boundary; a `Remote` service has one by construction (ADR-3), but a program
  outside the workspace is compiled by no gate and cannot be spawned by a test.
- **The HTTP client.** `[workspace.dependencies]` in the root `Cargo.toml`
  carries, among others, `tonic`, `prost`, `tokio`, `serde` and `serde_json`, and no HTTP
  client. `AGENTS.md` escalates a new entry that "a crate outside
  `components/` depends on in the same diff", and a service that is not a
  component is exactly that crate. This entry is also the first of its utility
  role, so it is the one a later "second HTTP client" would duplicate.
- **The dialect.** Nothing in `docs/` names an inference API.

### What the tree has today

- **`testkit/` holds one crate**, `ragondin-conformance`, whose `Cargo.toml`
  declares `ragondin-contracts`, `ragondin-types` and `thiserror`, and whose
  `ARCHITECTURE.md` asks it to "Keep it light": a contributor pulls it into
  their dev-dependencies. `docs/code-architecture.md` § 4.1 describes
  `testkit/` as that suite and nothing else.
- **`ragondin-proto` is a compiling skeleton.** Its crate documentation says
  "Message and service definitions land in a later issue"; the `Generator`
  service and its generated server trait arrive with #257. It depends on
  `tonic` unconditionally, and the workspace entry `tonic = "0.12"` keeps
  `tonic`'s default features, among them `transport`, which enables `server`
  and `channel`. `tonic` and its server side are therefore already compiled by
  a plain `cargo build` of the workspace; an HTTP client is not.
- **The heavy crates are already denied to the core.** `DENY_EXACT` in
  `scripts/check-invariants.py` lists `reqwest` ("HTTP client"), `hyper`,
  `tonic` and `prost`, so INV-4 already refuses any of them in the closure of
  `core/`.
- **The TLS precedent is `rustls`.** ADR-C27 put `ort`'s one HTTPS request on
  `tls-rustls`, and `deny.toml` already carries the one licence allowance that
  choice needed, `CDLA-Permissive-2.0` for `webpki-roots`. `Cargo.lock`
  already resolves `rustls`, `ring` and `webpki-roots`, reached through `ureq`
  under `ort-sys`'s build script.
- **`just test-features` runs `cargo test --workspace --all-features`**, so a
  test gated on a crate's own feature runs in `just check` and in CI.

### Facts about the two external surfaces, checked for this decision

- **`reqwest`'s feature names differ between its two current release lines.**
  In `0.12.28`, the last release of the `0.12` line, `rustls-tls` enables `rustls-tls-webpki-roots`, which selects
  `ring` as the `rustls` crypto provider and `webpki-roots` as the root store;
  `json` enables `serde_json`; the `default` set is `default-tls`, `charset`,
  `http2` and `system-proxy`, and `default-tls` selects the platform's native
  TLS through `hyper-tls`. In `0.13.5`
  there is no `rustls-tls`: the feature is `rustls`, it selects `aws-lc-rs` as
  the crypto provider and `rustls-platform-verifier` for roots, and it is the
  default. The two lines therefore need different feature lists, and the
  version is part of the decision.
- **The OpenAI-compatible surface as vLLM implements it.** vLLM's online
  serving documentation lists `/v1/chat/completions` ("Chat Completions API")
  and `/v1/models` ("List available models"). Its `ChatCompletionRequest`
  carries `messages`, `model`, `temperature`, `seed` (bounded to the signed
  64-bit range), `max_tokens` — marked "deprecated in favor of the
  max_completion_tokens field", and still accepted — and `max_completion_tokens`,
  each optional. The response carries `choices`, each with a `message` whose
  `content` may be null. A request naming a model the server does not serve is
  answered with HTTP 404 and "The model `<name>` does not exist.". Its
  documentation also states: "By default, the server applies
  `generation_config.json` from the Hugging Face model repository if it
  exists" — so the default an omitted sampling parameter receives may be the
  model publisher's rather than the server's.
- **What `/v1/models` reports, and what it must not be trusted for.** vLLM's
  `ModelCard` carries `id` (the served name), `root` (for a base model, the
  path or repository the model was loaded from), `parent` (for an adapter, the
  base model's served name), `owned_by`, `max_model_len`, `permission` and
  `created`. `created` defaults to the time of the response and each
  permission entry to a fresh random id, so neither is stable across two calls.
  No field names a revision.

The repository owner decided #253 on 2026-09-23. Decided in #253.

## Decision

**The reference `Remote` generator service is a crate under `testkit/` whose
one binary compiles only under its own feature; it relays each call to an
inference server over `reqwest`, in the OpenAI-compatible chat-completions
dialect, holding no experiment variable of its own (ADR-C31). It is a reference
implementation and a calibration fixture, never product surface.**

### 1. Location

**A crate `testkit/ragondin-generator-service`**, a workspace member, with one
`[[bin]]` named `ragondin-generator-service` carrying
`required-features = ["service"]`. Its feature `service` enables its optional
dependencies `reqwest` and `tonic`, so a workspace build without the feature
compiles no HTTP client, and compiles `tonic` only as far as `ragondin-proto`
already does. It depends on `ragondin-proto` (for the generated `Generator`
server trait #257 adds), `ragondin-types`, `tokio`, `serde`, `serde_json`, and
the two optional crates; **never on `ragondin-engine` or on any crate under
`components/`**. The implementation of the generated server trait is a plain
struct; `tonic`'s router is the network envelope around it, and the service
itself is never written as a `tower::Service` (INV-11).

**ADR-C15 is untouched, and this is how it reads.** ADR-C15 decides the
user-facing surface: `ragondin` stays the one binary a user runs, and the
composition root. `ragondin-generator-service` is not user-facing surface. It
is a worked example a `Remote` author reads beside the `.proto`, and the
fixture #269 spawns; it ships nowhere — its manifest sets `publish = false`,
which no crate of the workspace sets today, so the sentence holds by
construction — and nothing in the product reaches it.

### 2. The HTTP client

**`reqwest`, declared once in `[workspace.dependencies]` as
`reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }`**,
and referenced by the service as `{ workspace = true, optional = true }` with no
feature of its own. `default-features = false` drops `native-tls`, so TLS runs
on `rustls` with `ring` and `webpki-roots`, the precedent ADR-C27 set for
`ort`. Its `async` client is what the `tonic` handler awaits. Whatever
`just check-deny` reports for the new closure is answered in `deny.toml` by the
pull request that adds the entry.

**This is the workspace's HTTP client.** A second one later is the duplicate
utility `AGENTS.md` § Rules of engagement escalates; moving the entry to
`reqwest` `0.13` changes its feature list and escalates under the same rule.

### 3. The dialect, and what the service relays

**The service speaks the OpenAI-compatible HTTP API**: it answers `Generate`
through `POST <base>/v1/chat/completions` and `GetModelIdentity` through
`GET <base>/v1/models`, where `<base>` is its configured base URL with any one
trailing `/` removed.

**Its whole configuration is three values, none of them an experiment
variable.** The service reads its two flags from `std::env::args` itself,
without `clap`: the root `Cargo.toml` declares `clap` as "the CLI parser, in
`bin/ragondin` and nowhere else", and two required flags need no parser.

- `--base-url <url>`, required: the inference server's root, an absolute
  `http` or `https` URL, without the `/v1` segment;
- `--listen <socket address>`, required: the address the gRPC server binds;
  `127.0.0.1:0` asks the operating system for a free port;
- the environment variable `RAGONDIN_INFERENCE_API_KEY`, optional: when it is
  set to a non-empty value, every request to the inference server carries
  `Authorization: Bearer <value>`; unset and set to the empty string are the
  same, and send no `Authorization` header. The key is never a flag and is never
  written to any output.

**A bad command line is refused at startup**, with a non-zero exit status and
a message on stderr, before any `listening on` line: a missing or unknown
flag; a base URL whose scheme is not `http` or `https`, that carries a query
or a fragment, or whose path ends in `/v1` — the conventional OpenAI base,
which would double to `/v1/v1/…`; and a listen address that does not parse
as a socket address.

**Once bound, the service writes exactly one line to stdout,
`listening on <address>`**, where `<address>` is the bound socket address in
Rust's `SocketAddr` display form, and writes nothing else there. A test or the
calibration reads that line to learn the port.

**Per `Generate` call, the service:**

1. Refuses an empty `served_model` or an empty `template` as
   `InvalidArgument`, as ADR-C31 § 2 requires of a service on receipt, and
   refuses a `temperature` that is present and not finite — `NaN` or an
   infinity, which `serde_json` would write as `null` — as `InvalidArgument`.
2. Renders `template` under ADR-C31 § 2's grammar — `{query}` and `{context}`
   replaced by the query's and the context's text, `{{` and `}}` rendered as
   literal braces, left to right in one pass — and refuses a malformed
   template as `InvalidArgument`.
3. Sends one request whose `messages` hold exactly one message, with role
   `user` and the rendered text as its content — **no system message, and no
   wording of its own** — and whose `model` is `served_model`.
4. Adds `temperature`, `seed` and `max_tokens` to the request **only when the
   call carries them**, each under that JSON name and with the value as
   received. An absent field is absent from the JSON body, never `null` and
   never a value of the service's choosing, so the inference server's own
   default applies: ADR-C31's rule that what a component does in the absence
   of a parameter is the component's to decide, met on the far side of the
   relay. No other request field is sent.
5. Returns `choices[0].message.content` as the answer text.

**No retry, no streaming, no batching, no timeout of its own.** One call is
one HTTP request.

**Determinism is the server's.** A `seed` is relayed and nothing more: some
servers honour it and some ignore it, and ADR-15 already promises statistical
reproducibility rather than determinism. What the service can make certain is
only that the seed asked for is the seed sent.

### 4. Model identity

**`GetModelIdentity(served_model)` reads `GET <base>/v1/models`** and looks for
the first entry of `data` whose `id` equals `served_model` byte for byte.

- **Listed:** the identity is a JSON object encoded as `serde_json` encodes
  one — compact, with its string escaping — with keys in the order `id`,
  `root`, `parent`, `max_model_len`, holding `id`; `root` and `parent` each
  only when the server reports it as a non-empty string; and `max_model_len`
  only when the server reports it as an integer. vLLM serving a base model
  under the name `qwen` from `Qwen/Qwen2.5-7B-Instruct` with a context of 32768
  tokens yields
  `{"id":"qwen","root":"Qwen/Qwen2.5-7B-Instruct","max_model_len":32768}`.
  `max_model_len` is in because it decides the answer: vLLM caps the tokens it
  generates at `max_model_len` minus the prompt's length and refuses a longer
  prompt (`get_max_tokens` in `vllm/entrypoints/serve/utils/api_utils.py`), so
  ADR-C31 § 4's completeness rule reaches it. No other field of the entry
  enters the identity: `owned_by` is a constant that decides nothing, and
  `created` and the permission ids change per response, where ADR-C31 § 4
  requires an identity to be stable.
- **Not listed:** the call is refused as `InvalidArgument` — a refusal, never
  a warning, as ADR-C31 § 4 requires.

This is the case ADR-C31 § 4 anticipated — "a relay in front of an inference
server whose API reports only an alias" — and the identity is that alias plus
what the server reports beside it. No revision field exists in the dialect,
and the service invents none.

### 5. Errors

Every failure the service returns is a gRPC status, which the `Remote`
adapter maps to a `ComponentError` (#261):

| What happened | gRPC status |
|---|---|
| empty `served_model` or `template`; a non-finite `temperature`; malformed template | `InvalidArgument` |
| the HTTP request failed before any response — connection refused, or a connect failure the operating system reports | `Unavailable` |
| the transport failed after the status line — the connection reset while the body was read | `Unavailable` |
| HTTP `429` or `503`, each of which means "try later" | `Unavailable` |
| any other HTTP `4xx` from either endpoint, including a model the server does not serve | `InvalidArgument` |
| any other HTTP `5xx`, or any other non-success status, a `3xx` included | `Internal` |
| a `2xx` chat-completions body that does not decode into the shape of § 3, a response with no choice, or a first choice whose `content` is null | `Internal` |
| a `2xx` models body with no `data` array of objects each carrying a string `id` | `Internal` |
| `served_model` absent from `/v1/models` | `InvalidArgument` |

**The client follows no redirect** (`reqwest::redirect::Policy::none()`), so
a `3xx` is a non-success status like any other: `reqwest`'s default policy
would follow it, and on a `301`, `302` or `303` resend the `POST` as a `GET`.
A `root`, `parent` or `max_model_len` that is null, empty, or not of the type
§ 4 states is omitted from the identity, never a decode failure.
The classes above are deliberately coarse: an operator's wrong API key draws a
`401` or `403`, and reaches the caller as `InvalidRequest` like any other
`4xx`, though the caller's request was not what was wrong. `429` and `503` are the two
statuses carved out because the caller's remedy for both is to try later,
which is what `Unavailable` says; every finer distinction is left to the
status message, which carries the inference server's status code and, where
present, its error text, and never carries the API key.

### 6. Tests

**The service's tests run the binary against an in-process fake inference
server**: HTTP/1.1 written by hand over `tokio::net::TcpListener`, started
inside the test on a loopback port. It adds no dependency: `axum`, `hyper` or
a mock-server crate as a dev-dependency would be a new
`[workspace.dependencies]` entry used outside `components/`, which
`AGENTS.md` § Rules of engagement escalates.
The test spawns the built binary with `--base-url` pointed at the fake and
`--listen 127.0.0.1:0`, reads the `listening on` line, and drives the service
through `RemoteGenerator` from `ragondin-remote` (#261), a dev-dependency. They
cover: an answer round-trip; template rendering, escapes included, observed in
the body the fake received; `temperature`, `seed` and `max_tokens` absent from
that body when the call's fields are `None`, and present when they are set;
each row of the error table; and the identity of a listed and of an unlisted
model. No test touches the network beyond the loopback interface, and none
needs a real model. The tests compile under the `service` feature, and
`just test-features` runs them.

## Alternatives rejected

- **A subcommand of `ragondin`**, behind a `remote` feature. ADR-C15 read
  literally, and one door. It puts an HTTP client and an inference dialect into
  the product binary for the sake of a fixture; a `Remote` author writing
  Python has no reason to run it, and one writing Rust wants it separable from
  the composition root it would otherwise sit inside.
- **Outside the workspace**, under `examples/` or in a sibling repository —
  ADR-C12's route for the controller. Nothing compiles it in CI, so the worked
  example for face 2 is the artefact most likely to drift from the `.proto`,
  and #269 cannot spawn what the workspace does not build.
- **A second crate under `bin/`.** It contradicts ADR-C15 as written, which
  makes a single binary, `ragondin`, the composition root and the entire
  user-facing surface.
- **`hyper` with `hyper-util` and `http-body-util` directly.** Already in
  `tonic`'s closure, so for a plain `http` inference server nothing new would
  reach `check-deny`; an `https` one would still need `hyper-rustls` or
  `tokio-rustls`, which it does not carry. It costs a
  hand-written client — connection handling, body collection, TLS wiring —
  for one JSON POST and one GET, which is more code than a reference fixture
  deserves and more for a `Remote` author to read past.
- **A blocking client** (`ureq`). The smallest closure; but the service is a
  `tonic` server, and a blocking call inside its handler stalls the executor —
  the shape ADR-C25 forbids for a `Local` component, met here on the other
  face.
- **`reqwest` `0.13`.** The current release line; its `rustls` feature selects
  `aws-lc-rs`, a crypto provider with a C build that nothing in the lockfile
  needs today, where `0.12`'s `rustls-tls` reuses the `ring` and `webpki-roots`
  the lockfile already resolves. `0.13`'s `rustls-no-provider` would let `ring`
  be installed by hand, and still verifies through `rustls-platform-verifier`
  rather than `webpki-roots`. `0.12` is the superseded line — its last release
  is `0.12.28` — so a future advisory on it is the likely trigger of the
  escalation § 2 names for moving the entry.
- **A native dialect** — one server's own API rather than the OpenAI-compatible
  one. Tighter for that server and useless for every other; #253 records the
  OpenAI-compatible surface as the one vLLM, Ollama, llama.cpp, TGI and the
  hosted providers share.
- **A service-side system prompt or default sampling values.** Each would be a
  setting that decides the answer and lives outside the pipeline
  representation, which ADR-C31 § 2 withdraws.

## Consequences

- **#267 implements exactly this.** Its scope's constructor configuration —
  "the inference server's base URL, the model name, the system prompt or
  template, temperature, seed, max tokens" — shrinks to the base URL, the API
  key and the listen address; the model name and the template arrive in the
  call (ADR-C31), and the three knobs arrive in it when set. Its crate is
  `testkit/ragondin-generator-service`, which is the `crate:` label its issue
  was waiting on.
- **#269 spawns the binary** exactly as § 6's tests do, and binds it to the
  pipeline through `ragondin bench --remote generator/<name>=<uri>` (ADR-C32
  § 2). **No ADR names the inference server**: this one decides the dialect,
  not the server, and vLLM appears above only as the source of the facts
  checked. #269 chooses an OpenAI-compatible server it can run locally and
  records it, with its version, in `bin/ragondin/ARCHITECTURE.md` — which an
  absent optional taking the server's default, and a `seed` the server may
  ignore, make necessary.
- **#261 maps the statuses of § 5.** `Unavailable` must reach
  `ComponentError::Unavailable`, and `InvalidArgument`
  `ComponentError::InvalidRequest`; `Internal` reaches whatever #261's shared
  mapping gives it.
- **The root manifests change in #267's pull request**, under `AGENTS.md`'s
  dependency rule: the `reqwest` entry in `[workspace.dependencies]`, the new
  workspace member, and `deny.toml` if its closure needs it. The root
  `Cargo.toml` and `Cargo.lock` do not count toward the two-crate limit.
  `tokio`'s `net` feature, which binding a listener needs, arrives in the
  service's graph with `tonic`'s `server` feature; neither the crate nor the
  root adds a feature to the `tokio` entry.
- **`testkit/` widens** from "the conformance suite" to "reference
  implementations and fixtures". `testkit/` has no README; #267 states the
  wider meaning in the new crate's `ARCHITECTURE.md` and corrects the
  `testkit/` entry of `docs/code-architecture.md` § 4.1's layout and the
  `testkit/` subgraph of § 4.3's dependency graph, in the same
  diff that makes the old description incomplete. `ragondin-conformance`'s own
  rule to stay light is not affected: nothing depends on the service crate.
- **`max_tokens` is the deprecated spelling on at least one server.** vLLM
  accepts it and marks it deprecated in favour of `max_completion_tokens`. The
  service sends `max_tokens`, the name the OpenAI-compatible servers share; a
  server that drops it would turn the knob into a `4xx` or into a silently
  ignored field, and changing the name the service sends is a change to this
  decision.
- **A `seed` outside the signed 64-bit range is relayed as received.**
  `GenerateParams::seed` is a `u64`; vLLM bounds `seed` to `i64`, so such a
  value draws a `4xx` and reaches the caller as `InvalidArgument`, never as a
  clamped value the caller did not ask for.
- **INV-4** is untouched: `reqwest` and `hyper` are already in `DENY_EXACT`,
  and nothing under `core/` gains an edge to the service crate. **INV-11**: the
  service implements the generated server trait, and `tonic`'s Tower stack is
  the only `tower::Service` around it.
- **What is deliberately left open.** A reference service for any other family.
  Streaming, retries and batching, which a product service would want and a
  fixture does not. TLS and authentication between `ragondin` and this service
  — ADR-C32 § 3 leaves them to M6 — which are distinct from the TLS this
  service uses to reach its inference server. No entry in
  `docs/OPEN_QUESTIONS.md` is opened, closed, or changed.

## Status

Accepted.
