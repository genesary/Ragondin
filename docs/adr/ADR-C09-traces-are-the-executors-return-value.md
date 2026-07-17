# ADR-C9: Execution traces are the executor's return value

## Context

The product's differentiating capability is **per-node execution replay** in the UI: the pipeline graph rendered, with what actually happened overlaid on each node. That requires traces to be **structured business data**, addressable per node — not unstructured telemetry. If traces were emitted only through `tracing` / logs, per-node replay would be impossible.

## Decision

The executor's signature **returns** the `ExecutionTrace` (per node: input, output, duration, branch taken); it does not log it. `tracing` remains in use **in parallel** for operational observability — spans, logs — but **never substitutes** for `ExecutionTrace`.

## Alternatives rejected

- **Traces via `tracing` / logs.** Makes per-node UI replay impossible, because logs are unstructured operational telemetry, not addressable business data.

## Consequences

Per-node replay is possible, which is the differentiating UI capability. This is a **signature decision at the heart of the system** and expensive to undo, so the executor signature must be reviewed specifically to keep traces as a return value rather than letting them slip into logging.

## Status

Accepted.
