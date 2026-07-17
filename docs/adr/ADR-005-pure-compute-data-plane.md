# ADR-5: Pure-compute data plane, externalized state

## Context

The data plane executes pipelines. For horizontal scalability, any data-plane replica must be able to serve any request. That is impossible if a replica holds durable state that its peers do not — the request becomes bound to one replica, and the plane becomes a source of truth that must itself be replicated and reconciled.

## Decision

The data plane holds **no durable state**. Indexes, vectors and caches live in **external stores**. Local caches are permitted only as **reconstructible optimizations**, never as a source of truth.

## Alternatives rejected

- **A stateful data plane.** Couples requests to specific replicas, breaking horizontal scalability, and turns the compute plane into a source of truth with all the replication and consistency burden that implies.

## Consequences

Any data-plane replica can serve any request. External stores — vector store, artifact store, LLM inference server — sit behind traits and are out of scope for the platform to implement.

Because local caches must remain reconstructible optimizations, the cache **invalidation model** is left as an open question: it must be defined without ever letting a cache become authoritative.

## Status

Accepted.
