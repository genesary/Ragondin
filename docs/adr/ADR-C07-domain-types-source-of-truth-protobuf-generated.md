# ADR-C7: Domain types are the source of truth; protobuf is generated; round-trip tested

## Context

The component contract has two faces — a Rust trait and a mirror protobuf service — that must not silently diverge as the code evolves. One of the two must be the source of truth.

## Decision

The **domain types (`rag-types`) are the source of truth**, hand-written for Rust ergonomics. The protobuf is **generated** by `tonic-build`. The `From`/`Into` conversions live in `rag-remote`. The two faces are kept in lockstep by **round-trip property tests**: for all `x` in the domain, `from_proto(to_proto(x)) == x`.

## Alternatives rejected

- **Protobuf-first, with the trait generated from protobuf.** Sacrifices Rust ergonomics in the very types every component author touches, and bends the domain model to protobuf's shape rather than to the domain's.

## Consequences

Rust ergonomics in the domain types, plus a **mechanical guarantee the two faces never drift**. The round-trip test is the serialization-side counterpart of the conformance suite, and a blocking CI check.

## Status

Accepted.
