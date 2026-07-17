# ADR-C10: Tower for the serving envelope only

## Context

The serving layer needs its network envelope hardened — timeouts, retries, concurrency limits, load shedding, backpressure on `Remote` calls, instrumentation — exactly what a mature `tower` `Service`/`Layer` stack provides, and which there is no reason to rewrite. But `tower`'s `Service<Request>` is **uniform**: one request type, one response type. RAG components are **heterogeneous** domain traits: a `Retriever` and a `Generator` share neither input nor output.

## Decision

Use **Tower for the network envelope of the serving layer only**. Do **not** make components Tower services; they remain heterogeneous domain traits, each at its own layer.

## Alternatives rejected

- **Components as `tower::Service`.** Forcing heterogeneous domain contracts into a uniform `Service` shape would destroy the legibility of the domain contracts and gain nothing.

## Consequences

Each abstraction stays at its own layer: Tower governs cross-cutting network concerns; domain traits govern components. The serving layer reuses hardened middleware from a mature service-mesh lineage instead of rewriting it.

## Status

Accepted.
