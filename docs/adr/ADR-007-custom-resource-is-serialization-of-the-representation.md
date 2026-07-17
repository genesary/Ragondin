# ADR-7: Custom resource = serialization of the representation; the ConfigSource abstraction

## Context

The platform must be fully usable locally with a static configuration file (standalone first) and also configurable in Kubernetes through custom resources — **without impedance between the two**. A developer should not have to rewrite configuration when moving from a laptop to a cluster.

## Decision

The data plane exposes a `ConfigSource` abstraction with several implementations — `LocalFile` (a static YAML file, standalone mode) and `Stream` (pushed from the controller over the purpose-built gRPC service). **The data plane does not know who configures it.**

The Kubernetes custom resource is **nothing other than a serialization of the pipeline representation** (modulo wire format). The controller only translates: custom resource in, wire configuration out.

## Alternatives rejected

- **A Kubernetes-native configuration format distinct from the local one.** Forces a developer to rewrite configuration when moving from laptop to cluster, and creates two schemas that inevitably drift apart.

## Consequences

A developer works locally with a file, then deploys to Kubernetes without changing anything — zero impedance. Standalone-first becomes coherent rather than aspirational: **the YAML that runs locally *is* the custom resource.**

## Status

Accepted.
