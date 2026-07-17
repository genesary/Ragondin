# ADR-C5: The engine depends only on traits; components are leaves

## Context

If the engine depended on concrete component crates, built-in components would be privileged and coupled into the core, and their heavy dependencies would be pulled into the engine. That creates a two-tier system in which contributors are second-class citizens — the slow death of a community project.

## Decision

`rag-engine` depends on `rag-contracts` (the traits) and on **no crate under `components/`**. Crates under `components/` depend on `rag-contracts` and `rag-types` and **never the reverse** — a component is a **leaf**. Only the binary knows both the engine and the concrete components; it is the **composition root**.

## Alternatives rejected

- **Wiring built-in implementations into the engine.** Creates a two-tier system where built-ins are first-class and third parties second-class, and couples heavy dependencies into the engine.

## Consequences

Decoupling, and external contribution on equal footing. The rule is made **structural by the crate graph** — Cargo fails to compile a violation — and additionally checked in CI. This is load-bearing rule number one of the code architecture.

## Status

Accepted.
