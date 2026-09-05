# ADR-C12: The controller may live outside the workspace

## Context

The Kubernetes controller only **translates** a custom resource into wire configuration and pushes it over a clean network boundary. Its implementation language is a concern isolated from the rest of the stack by that boundary.

## Decision

The controller **may live outside the Cargo workspace**. In Go it does not enter the Cargo graph at all; in Rust it is a thin binary depending only on `ragondin-config` and `ragondin-proto`. Either way the network boundary is clean.

## Alternatives rejected

- **Mandating the controller in-workspace.** Couples an isolated, network-boundaried decision to the workspace, and forecloses a language choice (Go vs Rust) that has no bearing on the rest of the system.

## Consequences

The controller's implementation language becomes an **isolated decision** behind a clean network boundary — an open question (Go vs Rust) that can be settled on its own merits without touching the workspace. The workspace is not burdened by the controller.

## Status

Accepted.
