---
id: ADR-C35
title: A `ComponentError` crosses the wire as one of three gRPC status codes; every `Remote` adapter maps a status back with one total function in `ragondin-remote`
status: accepted
invariants: [INV-1]
supersedes: []
superseded_by: null
---

# ADR-C35: A `ComponentError` crosses the wire as one of three gRPC status codes; every `Remote` adapter maps a status back with one total function in `ragondin-remote`

## Context

ADR-3 makes the component contract two faces that mirror each other exactly:
face 1 is a Rust trait, face 2 a protobuf service. On face 1 every method
returns `ComponentError`, defined in `ragondin-contracts`, with three
variants:

- **`Unavailable`**: the component could not be reached. Its doc names a
  refused connection and a timeout, the failures a `Remote` component has and
  a `Local` one does not.
- **`InvalidRequest`**: the call cannot be honoured as made. The caller, not
  the component, is what needs to change.
- **`Backend`**: the underlying implementation failed. It boxes the cause as
  its `source`. Its doc states that this fidelity does not cross the wire: a
  `Remote` component's error arrives as a gRPC status, so `ragondin-remote`
  can only reconstruct a message, not the original error type.

On face 2 a failure is a gRPC status, and gRPC defines seventeen codes. The
`.proto` carries none of this. It states each call's refusal conditions as
"an invalid request", which is the `ComponentError` term, and names no status.
So two things are undecided: which code a `Remote` service returns for each
failure, and which variant a `Remote` adapter makes of each code it receives.
The second includes codes no service is told to return.

No accepted text settles either:

- **ADR-3** says nothing of errors.
- **ADR-C24** decides how the `.proto` mirrors the domain types and how the
  conversions are tested. It covers values, not errors. It does require the
  adapter to reject a message the domain cannot represent, such as an absent
  required field or an unspecified enum number.
- **ADR-C33 § 5** decides the table for one service only, the reference
  generator relay. `InvalidArgument` covers a malformed request and most
  upstream `4xx`. `Unavailable` covers connection failures and upstream `429`
  and `503`. `Internal` covers the other upstream failures. Its Consequences
  then leave the adapter side open: `Unavailable` must reach
  `ComponentError::Unavailable` and `InvalidArgument`
  `ComponentError::InvalidRequest`, while "`Internal` reaches whatever #261's
  shared mapping gives it".
- **ADR-C32 § 3** builds each `Remote` adapter over a lazily connecting
  channel (`Endpoint::connect_lazy`). An unreachable service is reported at
  its first call as `ComponentError::Unavailable`. It does not say which
  status carries that failure.

The question is on the escalation list in `AGENTS.md` § Rules of engagement:
which status a service must return is part of what a `Remote` service
compiles against. Left open, the two issues that write adapters, #13 (the
`Remote` adapters for the five M2 services in `ragondin-remote`) and #261
(the `Generator` and `ContextBuilder` adapters), would each decide it for
themselves on a shared surface, and could decide it differently.

### What `tonic` 0.12.3 produces on its own

The workspace resolves `tonic` 0.12.3. Before any service code runs, `tonic`
itself turns some failures into statuses, and an adapter receives those too.
Each of the following was checked in its sources for this decision:

- **A call timeout surfaces as `CANCELLED`.** A call timeout, set by
  `Endpoint::timeout`, by `Server::timeout` or by the `grpc-timeout` header,
  expires as `TimeoutExpired`. `tonic` converts it with `Status::cancelled`,
  on the client and on the server alike. None of these timeouts produces
  `DEADLINE_EXCEEDED`, the code gRPC defines for an expired deadline, though a
  gRPC implementation in another language may send it.
- **A connect error surfaces as `UNAVAILABLE`.** A failed connect, including
  a connect timeout or a failed TLS handshake, is a `ConnectError`, and `tonic`
  converts it with `Status::unavailable`. A keep-alive timeout reported by the
  HTTP/2 layer also becomes `UNAVAILABLE`.
- **A lazy channel reports a connect error at the call, as a status.** Over a
  channel built by `connect_lazy`, a failed connect does not fail the
  channel's readiness. It is held and returned by the next call, and the
  generated client turns it into a `Status` like any other failure. Every
  generated client method returns `Result<_, tonic::Status>`: there is no
  failure an adapter receives that is not a status.
- **An HTTP response without a `grpc-status` is mapped from its HTTP status.**
  `429`, `502`, `503` and `504` become `UNAVAILABLE`, `401` becomes
  `UNAUTHENTICATED`, `403` `PERMISSION_DENIED`, `404` `UNIMPLEMENTED`, `400`
  `INTERNAL`, and any other status but `200` `UNKNOWN`. A proxy in front of a
  service produces these.
- **A failure after the connection is made gets no code of its own.** An
  HTTP/2 error is mapped by its reason: `REFUSED_STREAM` to `UNAVAILABLE`,
  `CANCEL` to `CANCELLED`, and the rest to `INTERNAL`, `RESOURCE_EXHAUSTED`,
  `PERMISSION_DENIED` or `UNKNOWN`. An error `tonic` recognises nowhere in its
  chain becomes `UNKNOWN`.
- **A message over the decode limit surfaces as `OUT_OF_RANGE`.** The default
  limit for a received message is 4 MiB.

### The evidence

The question was put in decision issue #311, with four alternatives. An
independent review challenged the proposed answer against the `tonic` 0.12.3
sources and changed it: `CANCELLED` moved from `Backend` to `Unavailable`; a
separate rule for "a transport failure before a status" was dropped, because
`tonic` has no such case; the inverse direction, for a Rust-hosted service,
was added; `RESOURCE_EXHAUSTED` was named as a code a service never uses;
and the asymmetry of a received `UNAUTHENTICATED` or `PERMISSION_DENIED`,
already `Backend` under alternative 1, with ADR-C33's `401` and `403` was
named as deliberate, as set out below.

Decided in #311, by the repository owner on 2026-09-26: alternative 1, with
the table as corrected by the independent review.

## Decision

**A `Remote` service returns exactly one of three gRPC status codes, one per
`ComponentError` variant. A `Remote` adapter maps every status it receives back
to a variant with one total function, stated by code. Both directions live in
`ragondin-remote`, once, and every adapter uses them.**

### 1. Service → status

A `Remote` service, in any language, reports a failed call with one of these
three codes and no other:

| `ComponentError` | gRPC status | What it covers |
|---|---|---|
| `InvalidRequest` | `INVALID_ARGUMENT` | every refusal condition the `.proto` states; a request that fails conversion from the proto to the domain types, such as a missing required message, `EMBED_ROLE_UNSPECIFIED` or an enum number the domain has no value for |
| `Unavailable` | `UNAVAILABLE` | a dependency the service cannot reach; overload, a rate limit or an exhausted quota |
| `Backend` | `INTERNAL` | any other failure |

- **`RESOURCE_EXHAUSTED` is never used.** Overload, a rate limit and a quota
  are `UNAVAILABLE`, because the caller's remedy is to try later.
- **The restriction is deliberate.** A service returns no other code, even
  where gRPC offers a more specific one.
- **The rule is written into `types.proto`'s header** for authors in other
  languages, because a `Remote` author reads the `.proto` as the whole
  contract (ADR-C24).

**ADR-C33 § 5's table is an instance of this rule, and this ADR does not
supersede it.** Its `InvalidArgument` rows (a malformed request, any other
upstream `4xx`, a model the server does not list) are `INVALID_ARGUMENT`. Its
`Unavailable` rows (a failed request, a transport failure after the status
line, `429` and `503`) are `UNAVAILABLE`. Its `Internal` rows (other `5xx`
and `3xx`, and a body that does not decode) are `INTERNAL`.

### 2. Status → `ComponentError`

A `Remote` adapter maps what it receives with one total function:

| What the adapter receives | `ComponentError` |
|---|---|
| `INVALID_ARGUMENT` | `InvalidRequest` |
| `UNAVAILABLE`, `DEADLINE_EXCEEDED` or `CANCELLED` | `Unavailable` |
| an OK response that fails conversion to the domain types, or breaks the family's contract | `Backend` |
| any other code | `Backend`, with the `tonic::Status` boxed as its source |

- **The rule is stated by code.** There is no separate rule for a transport
  failure. Over the lazy channel of ADR-C32 § 3 every failure is already a
  `Status`, and a connect failure arrives as `UNAVAILABLE`.
- **`CANCELLED` is `Unavailable`.** `tonic` reports an expired call timeout
  as `CANCELLED`. A caller that abandons a call drops its future, which
  returns no status at all. So a `CANCELLED` an adapter receives came from
  outside the caller.
- **`DEADLINE_EXCEEDED` is `Unavailable`**, for the same reason: it is a
  timeout, reported by a gRPC implementation that uses the code gRPC defines
  for one.
- **Every other code is `Backend`, and it keeps its code.** The `Status`
  becomes `Backend`'s source, so its code and message reach a caller who
  walks the error chain. That includes `UNAUTHENTICATED`, `PERMISSION_DENIED`,
  `NOT_FOUND`, `FAILED_PRECONDITION`, `OUT_OF_RANGE`, `RESOURCE_EXHAUSTED`,
  `UNIMPLEMENTED` and `UNKNOWN`.

### 3. The inverse, `ComponentError` → status

A service written in Rust and backed by a `Local` component converts the
component's `ComponentError` with the table of § 1:

- `InvalidRequest` becomes `INVALID_ARGUMENT`;
- `Unavailable` becomes `UNAVAILABLE`;
- `Backend` becomes `INTERNAL`.

**A round trip keeps the variant.** A `Local` component's error, converted to a
status by § 3 and back by § 2, is the same variant it started as. A `Backend`
comes back with the `Status` as its source, not with the original cause, as
`ComponentError`'s doc already states.

### 4. Where it lives

**Both directions live in `ragondin-remote`, as one conversion each.** #13
writes them with the M2 adapters, and #261 uses the same functions for the
`Generator` and `ContextBuilder` adapters. No adapter maps a status itself.

**`ComponentError` gains no variant.** The three variants already carry what
crosses the wire, so the public API of `ragondin-contracts` does not change
(INV-1).

### The deliberate asymmetry with ADR-C33

ADR-C33 § 5 maps an upstream HTTP `401` or `403` to `InvalidArgument`, and
says itself that this is a compromise: an operator's wrong API key reaches the
caller as `InvalidRequest`, though the caller's request was not what was
wrong. This ADR maps a received `UNAUTHENTICATED` or `PERMISSION_DENIED` to
`Backend`.

Both stand, because they answer different questions. ADR-C33 decides which of
the three codes a relay returns for an upstream **HTTP** status. This ADR
decides what an adapter makes of a **gRPC** status. A relay that follows
ADR-C33 never returns `UNAUTHENTICATED` or `PERMISSION_DENIED`. An adapter that
receives either got it from a service that broke § 1, from something in
front of the service, such as a proxy that answered `401` or `403`, which
`tonic` maps to those two codes, or from `tonic` itself, which maps an HTTP/2
`INADEQUATE_SECURITY` error to `PERMISSION_DENIED`.

## Alternatives rejected

- **Widening `InvalidRequest` on the adapter side** to `FAILED_PRECONDITION`,
  `OUT_OF_RANGE` and `NOT_FOUND`, to forgive services written in idiomatic
  gRPC. Rejected because it gives the same failure two spellings, and a Rust
  service and a Python one could disagree about which to send. It would also
  mislabel `tonic`'s own failures: `tonic` returns `OUT_OF_RANGE` for a message
  over the decode limit, which is a deployment limit, not a request the caller
  must change.
- **A structured error detail** naming the variant exactly, as a
  `google.rpc.Status` detail or as a Ragondin message in a custom trailer.
  Rejected because it is a second channel that can disagree with the code,
  and every `Remote` author must learn its format. It buys nothing while the
  three variants map one to one onto three codes. A custom trailer would need
  no new dependency; the `google.rpc.Status` form would need the well-known
  types.
- **A new `ComponentError` variant**, for instance one per gRPC code, or one
  for "the service answered with a code outside the contract". Rejected
  because it changes the public API of `ragondin-contracts`, a stable
  boundary under INV-1 whose changes escalate on their own
  (`AGENTS.md` § Rules of engagement), and because no code outside the tests
  and the conformance suite branches on the variant today.
- **Leaving it to each adapter issue.** Rejected because #13 and #261 would
  decide one shared surface separately, and their tables could diverge.

## Consequences

- **#13 and #261 are unblocked on this point.** #13 writes both conversions in
  `ragondin-remote`, and #261 reuses them. A test covers the round trip of
  § 3 for each variant.
- **The three codes are written where service authors read them.** The
  sentence in `types.proto`'s header lands with #13 or #257 (the `.proto` for
  the `Generator` and `ContextBuilder` services); this ADR edits no `.proto`.
  #257 states the same three codes for its services, and #267 (the reference
  generator service) is already bound by ADR-C33 § 5 to return them.
- **The strongest argument against, and why it lost.** gRPC's own guidance
  tells a service author to use `NOT_FOUND`, `FAILED_PRECONDITION` or
  `RESOURCE_EXHAUSTED` where they fit. Here those codes arrive as `Backend`, so
  an idiomatic service is treated as broken rather than as refusing the call.
  It lost because the mitigations cover what matters today:
  - the code survives, as the source of `Backend`;
  - no code outside the tests and the conformance suite branches on the
    variant. The conformance suite asserts `InvalidRequest` where a call is refused, and a refusal is
    `INVALID_ARGUMENT` under § 1, which § 2 maps back to `InvalidRequest`;
  - a `Remote` conformance check could assert that a service returns only the
    three codes. Such a check is permitted, and not required by this ADR.
- **What `tonic` produces on its own lands in a row of § 2.** From an HTTP
  response without a `grpc-status`, `429`, `502`, `503` and `504` land as
  `Unavailable`, and every other non-`200` status as `Backend`. A connect failure and a
  call timeout land as `Unavailable`. A failure after the connection is made
  lands as `Unavailable` only when it arrives as `UNAVAILABLE` or `CANCELLED`;
  otherwise it is `Backend`, because the rule is stated by code and does not
  look behind the code.
- **INV-1 is untouched.** `ComponentError` keeps its three variants.
- **Not decided here.**
  - The decode-size limit. `tonic`'s default of 4 MiB applies until #13 sets
    one deliberately; a message over it is `OUT_OF_RANGE`, so `Backend`.
  - TLS and authentication between `ragondin` and a service, which ADR-C32
    § 3 leaves to M6.
  - No entry in `docs/OPEN_QUESTIONS.md` is opened, closed, or changed.

## Status

Accepted.
