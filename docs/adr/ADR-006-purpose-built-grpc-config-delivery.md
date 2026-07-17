# ADR-6: Purpose-built gRPC configuration delivery, not xDS

## Context

The platform must run standalone — a binary and a configuration file, no cluster — while also being declaratively configurable in Kubernetes through a controller. Configuration must be pushed to the data plane dynamically.

xDS ("x Discovery Service") is the mature protocol network proxies use for dynamic configuration; it runs **on top of** gRPC. The real choice is therefore: adopt the full xDS protocol — its resource schema and its ACK/NACK state machine — or define a purpose-built gRPC configuration service. xDS supplies typed, network-centric resources (listeners, routes, clusters, endpoints, secrets), an ACK/NACK state machine (each response carries a version and nonce; the client acknowledges or rejects), ordering guarantees for dependent resources, and interoperability with the surrounding proxy ecosystem.

## Decision

Define a **purpose-built gRPC configuration service**. Borrow xDS's good *ideas* — configuration versioning, explicit **ACK/NACK** (knowing whether the data plane actually applied a configuration), and possibly incremental push later — but not its schema. All of it fits in a protobuf definition of a few dozen lines. The data plane exposes a `ConfigSource` abstraction; the controller only translates a custom resource into wire configuration.

## Alternatives rejected

- **Adopt the full xDS protocol.** Three reasons: (1) its schema models network proxying (listeners, clusters, endpoints), not RAG pipelines — reusing it means paying xDS's complexity without gaining its schema; (2) its one genuine benefit, ecosystem interoperability, is worth nothing here because nothing in the world speaks "RAG configuration" in xDS — adopting it would mean implementing the industry's most complex eventual-consistency state machine to interoperate with no one; (3) server-side xDS tooling in Rust is immature, so the state machine would have to be reimplemented anyway — better to reimplement something simple.

## Consequences

A small, domain-fit configuration service with versioning and ACK/NACK, standalone-capable via a `LocalFile` source and cluster-capable via a `Stream` source.

A candid reservation: if the requirement ever becomes *"shard thousands of tenants across hundreds of data planes with fine-grained delta pushes,"* part of what xDS solves will have been reinvented. That bridge is crossed with hindsight about actual requirements, rather than by pre-paying the complexity today.

## Status

Accepted.
