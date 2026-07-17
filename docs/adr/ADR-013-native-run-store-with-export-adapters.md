# ADR-13: Native run store with export adapters

## Context

A run in this platform is a **content-addressed tuple** whose configuration **is a graph**, and whose central artifact is a **structured per-node execution trace**. Conventional experiment trackers (e.g. MLflow) model a run as scalar parameters, metrics, and opaque artifacts. Run comparison — the diff between two configurations across quality, cost and latency — is the product's value, not a substrate beneath it.

## Decision

The run store is **native** — not built on an existing experiment-tracking platform. Provide an **export trait** with at least one adapter (MLflow, OpenTelemetry / OpenInference) for interoperability with teams already invested in a tracker.

## Alternatives rejected

- **Build on an existing experiment tracker.** Three concrete frictions make the dependency inversion unworkable: (1) the tracker's data model is too flat for a graph-shaped configuration and per-node traces — storing them as blobs loses the very link between run and graph that matters; (2) the differentiating per-node execution replay view cannot be expressed in a generic tracker's interface, so it would have to be built alongside anyway, fracturing the product into two interfaces; (3) it inverts the dependency, making one of the system's three planes — the one that contains the controller — a plugin of an external platform.

## Consequences

A run store fitted to this data model is a modest component; the expensive part — the interface — must be built regardless. Exporting rather than building upon preserves interoperability without surrendering the architecture.

## Status

Accepted.
