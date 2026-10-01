---
id: ADR-C37
title: Every request input reaches `ragondin-api` through the crate's own extractors, over axum's `query` feature; a parameter is a closed, typed struct, and every query parameter and required header is declared in the API description
status: accepted
invariants: [INV-1, INV-11, INV-12]
supersedes: []
superseded_by: null
---

# ADR-C37: Every request input reaches `ragondin-api` through the crate's own extractors, over axum's `query` feature; a parameter is a closed, typed struct, and every query parameter and required header is declared in the API description

## Context

ADR-C36 § 6 lists the dependencies admitted for the UI's API and the features
each may carry. It names no query-string parser, and it closes the list: "Any
other entry the implementation finds necessary is a new decision." The `axum`
entry it admitted was created with `default-features = false` and three
features, `json`, `tokio` and `http1`, so the `Query` and `Form` extractors are
off.

One endpoint reads a query string today. `GET /runs/{id}/queries` takes
`missing_gold_at=<k>` (#343, PR #368), and parses it with a hand-written
percent-decoder in `ragondin-api`'s handlers: the function `parameters`, over
`decode`. Review hardened it: a malformed escape, a sign inside an escape,
a duplicate and an unknown name are all refused. More endpoints with
parameters are coming, among them `POST /compare` options (#344) and `/jobs`
filtering (#349), and a second hand-rolled parser must not grow beside the
first.

Both standard routes escalate under `AGENTS.md` § Rules of engagement:

- `axum::extract::Query<T>` needs `query` appended to the feature list of the
  shared `axum` entry, which is never a leaf's choice (ADR-C27, decided in
  #223);
- `serde_urlencoded` or `form_urlencoded` would be a new
  `[workspace.dependencies]` entry used by a crate outside `components/`.

Decision issue #371 put the question.

### What the pinned crates do, checked for this decision

Every claim below was checked against the crates `Cargo.lock` resolves.

- **The `query` feature adds no crate.** In `axum` 0.7.9 the feature is
  `query = ["dep:serde_urlencoded"]`. `serde_urlencoded` 0.7.1, and
  `form_urlencoded` beneath it, are already in `ragondin-api`'s closure through
  `reqwest` 0.12, the `Registry` file backend's transport. Turning the feature
  on changes no closure and leaves `Cargo.lock` byte-identical.
- **axum's own rejections are `text/plain`.** ADR-C36 § 2 requires every error
  to be rendered as `application/problem+json`, with a stable code. A raw
  `Query<T>` rejection would break that. The hole already exists for `Path`:
  `GET /api/v1/runs/%FF` answers `400` with a plain-text body today, because
  `%FF` does not decode to UTF-8 and the handler takes `axum::extract::Path`.
  Bodies avoid it already: every endpoint that reads one takes it as bytes and
  parses it in the crate, so a malformed body is `request_invalid`.
- **The decoding is lenient.** `serde_urlencoded` decodes through
  `form_urlencoded`, which follows the WHATWG URL standard:
  - a `%` not followed by two hex digits passes through literally;
  - a byte sequence that is not UTF-8 becomes U+FFFD;
  - a value error does not name the parameter it came from;
  - there is no `Vec`, so a repeated parameter has no type to land in;
  - a `HashMap` or a `#[serde(flatten)]` field loses the duplicate check and
    the unknown-field check that a closed struct gets from
    `deny_unknown_fields`.
- **The UI's type generator refuses any parameter outside the path.**
  `ui/scripts/api-types.mjs` throws on `in: query` or `in: header`, by design
  (`ui/ARCHITECTURE.md` § The generated types). So `missing_gold_at` is stated
  only in the operation's prose description in `api/v1.json`. The same limit
  left the `If-Match` and `If-None-Match` headers of `PUT /pipelines/{name}`
  (#342, PR #373) as prose too, and `runtime/ragondin-api/ARCHITECTURE.md`
  says they are to be declared once #371 lands. A limit of the generator has
  twice become a gap in the contract.

### The evidence

The question was put in #371 with three alternatives: append `query` to the
`axum` entry; add `serde_urlencoded` or `form_urlencoded` as an entry; or keep
the hand-written decoder. An independent review checked option 1 against the
pinned crates and amended it: the lenient decoding above is closed by a
validation pass before the deserializer runs, the `Path` hole is closed in the
same move, and the parameter types are constrained so that the lost checks
cannot come back through a map or a flattened field. The controller of that
round added one amendment, approved in the same round: required request
headers are declared and read the same way, so that the generator's limit is
closed once for both.

Decided in #371, by the repository owner on 2026-10-01: option 1, with the
review's amendments and the controller's.

## Decision

**The `axum` workspace entry gains the `query` feature, and that is the only
grant. `ragondin-api` reads every request input — path, query string, required
headers and JSON body — only through its own extractors, defined in one
module, each rejecting with `ApiError`. A query string is validated as strict
percent-encoded UTF-8 before it is deserialized. A parameter type is a closed
struct whose values are self-validating types. Every query parameter and every
header a handler requires is declared in the API description from its type, and
the UI's type generator reads both.**

### 1. The grant

- **`query` is appended to the `axum` entry in the root `Cargo.toml`.** Every
  crate naming that entry inherits it; today that is `ragondin-api` alone.
- **No other entry.** No `serde_urlencoded`, `form_urlencoded` or
  `serde_path_to_error` entry is admitted. The feature is appended in the pull
  request that first uses it, and the entry's comment names the feature and
  this ADR.
- **This adds to ADR-C36 § 6 and does not supersede it.** That section admits
  `axum` and leaves its features to the implementing pull request, and it says
  that any other entry is a new decision. The feature list of a shared entry is
  that kind of change, so this ADR is the decision. ADR-C36 is otherwise
  untouched.

### 2. The extractors

- **One module of `ragondin-api` defines every extractor that reads request
  input**:
  - `ApiPath<T>`, for path parameters;
  - `ApiQuery<T>`, for the query string;
  - `ApiHeaders<T>`, for the request headers a handler requires;
  - `ApiJson<T>`, for a JSON body.

  Each has `Rejection = ApiError`, so every refusal is a problem body.
- **No handler reads request input any other way.** No handler takes
  `axum::extract::Query`, `axum::extract::Path`, `axum::extract::Json` or a
  `HeaderMap` to read a request header, and none reads `Uri::query()`. A
  handler that receives the `Uri` to name the request in an error, as the
  fallbacks do, reads no input from it.
- **The layers are outside this rule.** The `Host` and `Origin` layers read
  headers as the network envelope, where ADR-C10 puts them, not as a handler's
  input.

### 3. `ApiQuery<T>`: validate, then deserialize

1. **The raw query is validated first.** Every `%` is followed by two hex
   digits, and every name and every value percent-decodes to valid UTF-8.
   Otherwise the request is `parameter_invalid`, "not percent-encoded UTF-8".
   This closes the lenient decoding: nothing malformed reaches the decoder, so
   nothing passes through literally and nothing becomes U+FFFD.
2. **Then it deserializes with `Query::<T>`.** A duplicate, unknown or
   unreadable parameter is `parameter_invalid`, carrying serde's reason.

The hand-written decoder survives only as the validator of step 1. It no
longer produces values.

### 4. What a parameter type is

- **A `struct`, `#[serde(deny_unknown_fields)]`, deriving `Deserialize` and
  `JsonSchema`.** Never a map, never a `#[serde(flatten)]` field, and never a
  bare `String` field. Each of those loses a check: the map and the flattened
  field the unknown and duplicate checks, the bare string the value's own.
- **Each value is an integer, an enum, or a validated newtype.** A newtype's
  error describes itself, since serde's reason does not name the parameter,
  and it refuses the empty string.
- **An endpoint with no parameters takes `ApiQuery<NoParameters>`**, so that
  any parameter sent to it is refused rather than ignored.
- **A repeated parameter is refused.** A list travels as one comma-separated
  parameter, with its own newtype, or in a JSON body.

`ApiHeaders<T>` follows the same rule for its type: a struct whose fields name
the headers it reads, deriving `JsonSchema`, each value typed. It reads the
headers with the module's own code, since no admitted crate deserializes a
`HeaderMap`. Its refusals are problem bodies with the codes the endpoint gives
today.

### 5. Declared in the API description

- **`description.rs` declares each operation's query parameters from the
  parameter type's schema**, as `in: query` parameters, each `required` as the
  schema says. **It declares each required header the same way**, as
  `in: header` parameters from the header type's schema.
- **It refuses a parameter schema whose `additionalProperties` is not
  `false`**, so a type that would accept an unknown parameter cannot be
  declared.
- **Two tests hold the rule over every operation in `OPERATIONS`**:
  - an undeclared query parameter sent to each answers `parameter_invalid`;
  - an invalid path value sent to each operation with a path parameter answers
    `application/problem+json`. Which code it carries is the crate's choice,
    recorded in `runtime/ragondin-api/ARCHITECTURE.md` § The error codes.
- **The UI's type generator learns `in: query` and `in: header`.**
  `ui/scripts/api-types.mjs` accepts both, renders them in `Paths`, optional
  per `required`, and still refuses `in: cookie`, with a test. The client in
  `ui/src/api/client.ts` serializes query parameters with `URLSearchParams`
  and sets declared headers inside `ui/src/api/`, so no screen hand-writes
  either.

## Alternatives rejected

- **A `serde_urlencoded` or `form_urlencoded` workspace entry** (#371's
  option 2). Rejected because it decodes exactly as option 1 does, through the
  same crates, so it needs the same validation pass and buys nothing. It is one
  more entry for a role axum already fills behind a feature, it duplicates that
  role, and it escalates just the same.
- **Keeping the hand-written decoder as the parser** (#371's option 3).
  Rejected because each endpoint would handle its parameters as strings, so
  nothing could be declared from a type and the description would keep stating
  parameters in prose. Each new endpoint would push to extend it. It survives
  only as § 3's validator, which produces no values.
- **A Tower layer that rewrites axum's plain-text rejections into problem
  bodies.** Rejected because it must sniff response bodies to recognise a
  rejection, and it puts a handler's concern into the network envelope, which
  ADR-C10 keeps for the envelope's own work. The extractors reject correctly in
  the first place.
- **`serde_path_to_error`, to name the parameter in a value error.** Rejected
  for now because it is three entries, the crate and its wiring for the query
  and the body, and while the parameter structs are small a self-describing
  value type names its own problem. A later decision may grant it.
- **Lenient WHATWG decoding, as `form_urlencoded` does it.** Rejected because
  it substitutes silently: a malformed escape becomes literal text and invalid
  UTF-8 becomes U+FFFD. The API's rule is that what it cannot read is refused
  rather than ignored (`runtime/ragondin-api/ARCHITECTURE.md` § The error
  codes), and a substituted value is a value nobody sent.

## Consequences

- **`Cargo.lock` is unchanged.** The `cargo metadata` graph gains an
  `axum → serde_urlencoded` edge, which no invariant checks, since
  `serde_urlencoded` is on no deny-list.
- **The `Path` hole closes.** `GET /api/v1/runs/%FF`, and every invalid path
  value, answers a problem body.
- **#343's `parameters()` is replaced by `ApiQuery<…>`**, and its tests stay
  green: every refusal they assert is still `parameter_invalid`.
- **The bodies keep their behaviour.** `ApiJson<T>` takes over today's reading
  of a body as bytes, parsed in the crate as `request_invalid`. What it accepts
  and refuses does not change.
- **`api/v1.json` declares what it states in prose today.** `missing_gold_at`
  becomes an `in: query` parameter, and `If-Match` and `If-None-Match` become
  `in: header` parameters; the prose goes. The generator refuses both
  locations today, so these declarations land in the same pull request as:
  - the generator change in `ui/scripts/api-types.mjs`, with its tests;
  - the client's serialization in `ui/src/api/client.ts`;
  - the regenerated `ui/src/api/types.ts`.
- **The root `Cargo.toml` comment on `axum` names `query` and this ADR**, in
  the pull request that appends the feature.
- **The endpoints still to come have no room to hand-roll.** #344 and #349
  take their parameters through `ApiQuery<T>`. #350 no longer carries the
  `missing_gold_at` obligation, since the generated client has the parameter.
  #356 gets the precondition headers from the generated client.
- **INV-11 is untouched.** The extractors implement axum's extractor traits,
  not `tower::Service`; no handler, trait or component becomes one.
- **INV-12 is untouched.** The grant adds no crate to `ragondin-api`'s
  closure, so it reaches no new crate at all.
- **INV-1 is untouched.** `ragondin-api` is internal by ADR-C21's test
  (ADR-C36 § 2), and nothing here reaches `core/`.
- **Not decided here.** No entry in `docs/OPEN_QUESTIONS.md` is opened, closed
  or changed, and no frozen decision is reopened.

## Status

Accepted.
