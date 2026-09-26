# ARCHITECTURE — ragondin-remote

**Status: internal, not an INV-1 boundary.** ADR-C21 places it there: what a
`Remote` author mirrors is the `.proto` text in `ragondin-proto`, never this
crate's Rust API. The one consumer in the tree is `ragondin-engine`, behind its
`remote` feature.

## What lives here

The engine's side of face 2 (ADR-3): code that makes a gRPC service look like
a `Local` component.

| Module | Role |
|---|---|
| `src/convert.rs` | `IntoProto` and `FromProto`: every domain value, params struct, request and response of the seven component services, and `DecodeError` |
| `src/status.rs` | ADR-C35's status conversion, both directions, and ADR-C31 § 1's refusal of an empty identity |
| `src/adapters.rs` | `RemoteRetriever`, `RemoteFusion`, `RemoteReranker`, `RemoteEmbedder`, `RemoteVectorStore`, `RemoteContextBuilder`, `RemoteGenerator`; the message limit and the batch sizes |

**Not here.** Building a channel from an address, and binding a name to it,
are the composition root's (ADR-C32 § 2, § 3). No server-side wrapper is exported: the
tests host their stubs through the same conversions a Rust-hosted service
would use, in `tests/support`.

## Conversions

- **Two traits of this crate's own, not `From`/`TryFrom`.** Both sides are
  foreign here — the domain types live in `ragondin-types` and
  `ragondin-contracts`, the messages in `ragondin-proto` — and the orphan rule
  forbids a `From` between two foreign types. `IntoProto<P>` is total;
  `FromProto<P>` returns a `DecodeError`. Both take their argument by value, so
  converting a response moves its vectors rather than copying them. They are
  generic over the message, because one domain type has several: a
  `Vec<ScoredChunk>` is the response of four rpcs.
- **A trait method's arguments convert as one tuple** to its request, in the
  method's argument order, and its return value to its response: `(Query,
  RetrieveParams)` ⇄ `RetrieveRequest`, `()` ⇄ `UpsertResponse`,
  `Option<String>` ⇄ the embedder's and the reranker's identity request,
  `String` ⇄ the generator's, whose served model is always named, and `()` ⇄
  the context builder's, which carries nothing. One call converts a whole rpc
  on either side.
- **`FromProto` refuses exactly what the domain cannot represent or does not
  accept on the wire**: a required message left out (`types.proto`'s header),
  an `EmbedRole` of `UNSPECIFIED` or of a number the enum does not name
  (ADR-C17), an empty `ModelIdentity` (ADR-C31 § 1), an empty `served_model`
  (ADR-C32 § 4: absent is a domain value, empty is not), a generator's empty
  `served_model` or `template` — what an omitted one decodes as, a proto3
  `string` having no presence — which ADR-C31 § 2 has a service refuse on
  receipt, and a count wider than `usize`. It refuses nothing a component
  refuses by its own contract — a `top_k` or a `budget` of zero, a non-finite
  score, a zero-dimensional embedding, a malformed template. Those cross the
  wire intact and the component decides, so a `Local` and a `Remote`
  component are refused by the same rule.
- **A generator's three optionals keep their presence** (ADR-C31 § 2).
  `temperature`, `seed` and `max_tokens` are proto3 `optional` fields: `None`
  is left off the wire and an absent field decodes as `None`, so a
  temperature nobody set never arrives as `0.0`, which is greedy decoding.
  `an_absent_optional_is_left_off_the_wire_and_a_zero_is_sent` pins it on the
  bytes.
- **The sign of a zero score does not cross.** `prost` leaves a scalar equal
  to its default off the wire, and `-0.0 == 0.0`, so a score of `-0.0` arrives
  as `0.0`. The round trip holds under `PartialEq`, which is ADR-C24's
  property, and a list keeps its order. The sign itself is lost, though:
  the rankings in this tree sort with `total_cmp`, which separates `-0.0` from
  `0.0` (`ragondin-store-memory` normalises its scores for exactly that
  reason), so a tie-break downstream of a `Remote` component could differ from
  an all-`Local` composition. `a_negative_zero_score_arrives_as_zero` pins the
  fact. An embedding's components are a packed repeated field and keep it.

## Status mapping

`src/status.rs` is ADR-C35 written down once: `status_from_error` and
`status_from_request` for a service, `error_from_status`,
`error_from_response` and `error_from_identity_response` for an adapter. No
adapter maps a status itself.
`ComponentError` is `#[non_exhaustive]`, so `status_from_error` has a wildcard
arm; a variant added later is `INTERNAL`, "any other failure", until ADR-C35's
table names it.

**An empty identity from a service is `InvalidRequest`.** ADR-C31 § 1 decides
specifically that an adapter receiving one "refuses it as an
`InvalidRequest`-class failure". ADR-C35 § 2's general row — an OK response
that fails conversion or breaks the family's contract is `Backend` — does not
displace it: ADR-C35 supersedes nothing. So identity responses go through one
more shared function, `error_from_identity_response`, which makes an empty
identity `InvalidRequest` and hands every other refusal, such as the identity
message left out, to `error_from_response`. All four identity adapters here
use it — the embedder's, the reranker's, the context builder's and the
generator's — so no adapter maps its own refusal.

## Adapters

- **Over a caller's `Channel`, connecting nothing** (ADR-C32 § 3). The
  composition root builds one lazily connecting channel per binding and clones
  it into each adapter, so an unreachable service is reported at its first
  call, as `Unavailable`: `tonic` reports a failed connect as `UNAVAILABLE`.
- **The embedder and the reranker refuse an absent or empty `served_model`
  before sending**, as `InvalidRequest`, in every call and in
  `model_identity`. ADR-C32 § 4 has the service refuse `None` and both sides
  refuse the empty name; ADR-C32's Consequences have the adapter refuse `None`
  as well, before sending. A name the service does not serve is the service's
  refusal, arriving as `INVALID_ARGUMENT`.
- **The embedder adapter applies the prefixes** (ADR-C32 § 4). `query_prefix`
  and `passage_prefix` are constructor parameters of `RemoteEmbedder`, the
  `dense` node's two keys, which the composition root reads; the text on the
  Embed rpc is final. The role is still sent.
- **`RemoteVectorStore` is written and not bound.** ADR-C32 § 5 defers a
  `Remote` vector store until the contract can scope a store's content to a
  run; the adapter exists so that the round trip and the conformance suite
  cover the service `ragondin-proto` already defines.
- **The generator adapter renders nothing.** The template, the query and the
  context go out as the call holds them, and the service renders on receipt
  (ADR-C31 § 2); a malformed template is the service's refusal, arriving as
  `INVALID_ARGUMENT`. The adapter refuses an empty `served_model` or
  `template` before sending, as `InvalidRequest`, in `generate` and in
  `model_identity`: ADR-C31 § 2 has the adapter, the service and a `Local`
  generator all refuse it. The context builder adapter refuses nothing
  before sending; a zero budget is the service's refusal.
- **No off-thread hop.** ADR-C25 has a component not block its caller. Every
  adapter call awaits a `tonic` client call, which yields while the service
  works, so the obligation is met by construction.
- **One check beyond the conversion: each Embed batch's count.** A batch
  answered with the wrong number of vectors fails the call as `Backend`
  (ADR-C35 § 2's contract row), because batching would otherwise let a short
  batch shift every later vector onto the wrong text, a misalignment no caller
  could see. Nothing else is checked on a response: a ranking out of order is
  the conformance suite's to catch, as it is for a `Local` component.

## Message size and batching

`tonic`'s default decode limit is 4 MiB, and an Embed response of about 1024
vectors of 1024 dimensions reaches it. Over the limit a call fails as
`OUT_OF_RANGE`, which ADR-C35 makes `Backend`, in the middle of a run. **Both
are done, and each covers what the other cannot:**

- **Every client's decode and encode limits are set to `MAX_MESSAGE_SIZE`,
  64 MiB.** A raised limit alone does not bound anything: the corpus is
  embedded in one `embed` call, and a large BEIR corpus is hundreds of
  megabytes of vectors. It is not unbounded either, because a decode limit is
  what stops a misbehaving service from making the client allocate without
  bound.
- **Embed and Upsert are batched**, `EMBED_BATCH` and `UPSERT_BATCH`, 256 each.
  They are the two rpcs whose size the caller does not bound: every other rpc's
  response is bounded by a `top_k`, by the input it was handed, or, for
  Generate, by one answer. 256 vectors of
  16 384 components are 16 MiB, a quarter of the limit. A batch is split in
  order, answered in order, and moved rather than copied; an empty call still
  sends one empty rpc, so the service sees its parameters and refuses a bad
  `served_model` as a `Local` component would. A split Upsert is not atomic: a
  failed later batch leaves the earlier ones written, as a retry would find
  them.

Both limits are the adapter's side only: it sends and accepts at most 64 MiB.
What a service accepts is governed by its own receive limit, which the `.proto`
does not fix and many gRPC stacks default to 4 MiB; the in-process test
services raise theirs to match.

## Feature gating

**No feature inside this crate.** The whole crate is the heavy backend —
`tonic` and everything it pulls — so a feature gating its contents would leave
an empty crate behind it. ADR-C14 names `remote` as the feature, and it is the
consumer's: `ragondin-engine` depends on this crate optionally, behind
`remote`, and the binary's `remote` feature is decided by the issue that binds
`Remote` components into it. `ragondin-proto`, and with it `tonic`, is already
in the default build through `ragondin-config`; this crate adds the adapters,
not the transport.

## Tests

- **`tests/round_trip.rs`** — `from_proto(to_proto(x)) == x` for every value,
  params struct, request and response above, 500 values each, through `prost`
  bytes and back. The values come from a seeded xorshift64\* generator written
  in the file, **not `proptest`**: it is not a workspace dependency, and adding
  one to a crate outside `components/` escalates. The generator is
  deterministic, names the seed and case of a failure, and draws the edges on
  purpose — empty collections, the empty string, non-ASCII text, `-0.0` and
  the extreme finite floats, `0` and `usize::MAX`, `None` and `Some` — and a
  test checks that it does. It draws no value `FromProto` refuses: those are
  the negative tests'.
- **`tests/negative_decode.rs`** — one refusal per message where the wire
  admits what the domain does not (ADR-C24).
- **`tests/status.rs`** — ADR-C35's table, over all seventeen codes.
- **`tests/conformance.rs`** — each adapter passes its family's
  `assert_*_conformance` against an in-process `tonic` server on an ephemeral
  port, hosting a `Local` stub; each stub passes the suite on its own first.
  The five M2 families host in-test stubs (`tests/support/stubs.rs`). The
  generator and the context builder host `ragondin-stub`'s `StubGenerator`
  and `StubContextBuilder`, a dev-dependency: the generator suite owns its
  template and probes the whole of ADR-C31 § 2's grammar, which
  `StubGenerator` already parses, where an in-test copy would be a second
  parser to keep in step. The edge is dev-only, so no consumer of this crate
  reaches a component crate, and `check-invariants.py` walks no dev edge.
  The server is `tonic`'s own, from its default features; the listener is
  `tokio::net::TcpListener`, whose `net` feature `tonic`'s server enables, so no
  workspace entry's feature list changes.
- **`tests/adapters.rs`** — the trait-object coercion of every adapter, lazy
  connection, the refusals before sending, the prefixes, batching, a batch over
  `tonic`'s default limit, and ADR-C35's round trip over a real connection;
  for the two generation adapters, each status class of ADR-C35 § 2 on every
  call, the optionals' presence and the unrendered template as a service
  receives them, and the service's own refusals.
