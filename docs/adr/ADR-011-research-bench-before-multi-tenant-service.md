# ADR-11: Research bench before multi-tenant service

## Context

The platform could aim first at a multi-tenant RAG-as-a-Service product, or first at a research-grade evaluation bench. Multi-tenancy introduces isolation, quota and security questions that are substantial in their own right and orthogonal to the platform's core value (credible comparison of RAG configurations).

## Decision

Build the **research bench first** — a benchmarking suite credible on its own, runnable locally with no cluster, no tenancy, no judge. Multi-tenant RAG-as-a-Service is a **productization layered on top of that proven core**.

## Alternatives rejected

- **Multi-tenant service first.** Layers tenancy, isolation, quotas and security onto an unproven core, and defers the actual differentiator — credible comparison — behind infrastructure that only matters once the core is proven.

## Consequences

A benchmarking suite that is credible on its own is a complete and defensible first product. The isolation, quota, security and index-sharing questions of multi-tenancy are deliberately deferred until the research bench is proven.

## Status

Accepted.
