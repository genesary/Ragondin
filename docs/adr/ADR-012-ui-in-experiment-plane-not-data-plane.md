# ADR-12: UI in the experiment plane, never in the data plane

## Context

The platform needs two things at once: a scalable, embeddable, headless data plane, and a complete product with a user interface. Coupling the UI to the data plane would push product-level state and rendering concerns into the pure-compute path, sacrificing one goal for the other.

## Decision

The user interface lives in the **experiment plane** and **never touches the data plane**. The data plane stays headless and pure-compute; the experiment plane hosts all product-level state — the run store, the registry of benchmarks/datasets/indexes, the metrics catalogue, run comparison, the UI, export adapters, and the controller.

## Alternatives rejected

- **A UI coupled to the data plane.** Sacrifices either the headless scalability of the data plane or the completeness of the product, and forces product state into the compute path.

## Consequences

The platform has simultaneously a scalable, embeddable data plane **and** a complete product with a UI, without tension between the two. The separation is what makes both properties achievable at once.

## Status

Accepted.
