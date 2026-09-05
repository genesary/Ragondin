# ADR-C13: Typed errors (thiserror) in libraries, anyhow in the binary

## Context

Libraries should expose precise, matchable error types so callers can handle failures specifically. A binary, at the end of the chain, benefits from ergonomic aggregation. A library that used `anyhow` in its public API would impose that choice on every consumer and erase the typed errors callers need.

## Decision

Libraries (`core/`, `ragondin-engine`, `components/`, …) use **typed errors via `thiserror`**, each boundary exposing its own enum — `ComponentError`, `PlanError`, `ExecError`. The **binary uses `anyhow`** for aggregation. A library **never imposes `anyhow`** on its consumers.

## Alternatives rejected

- **`anyhow` everywhere.** A library that returns `anyhow::Error` forces its opaque error type onto every consumer, erasing the matchable, typed errors that callers of a library need.

## Consequences

Consumers get precise, matchable errors from libraries; the binary gets ergonomic aggregation at the top of the chain. The error boundary is explicit at each crate.

## Status

Accepted.
