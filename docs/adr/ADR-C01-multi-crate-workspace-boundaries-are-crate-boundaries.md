# ADR-C1: Multi-crate workspace; load-bearing boundaries are crate boundaries

## Context

Architectural boundaries documented as *conventions* decay over time as many hands — human and AI — touch the code. A forbidden dependency added inside a single crate compiles fine, and the boundary erodes silently. Cargo, however, forbids dependency cycles **between crates**.

## Decision

Organize the code as a **multi-crate Cargo workspace**, and route the architecture's load-bearing boundaries through **crate** boundaries rather than module boundaries. Where Cargo alone cannot enforce a rule, CI does.

## Alternatives rejected

- **A single crate with modules.** Module boundaries are conventions the compiler does not defend; a forbidden dependency across modules compiles without complaint, and the boundary decays.

## Consequences

The important boundaries become **constraints the compiler refuses to violate** rather than conventions that decay — the single organizing principle of the code architecture. Start with the boundaries known to be load-bearing (the three contracts, the engine, one binary), and split further only when a real seam proves itself; premature over-modularization is as costly as under-modularization.

## Status

Accepted.
