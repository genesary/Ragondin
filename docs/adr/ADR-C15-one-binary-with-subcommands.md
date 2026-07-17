# ADR-C15: One binary with subcommands

## Context

The data plane and the command-line workflow could be shipped as separate binaries, but that would force a user to understand the internal architecture — which binary does what — before running anything.

## Decision

A **single binary, `rag`**, is the composition root and the entire user-facing surface, with subcommands: `bench`, `compare`, `serve`, `validate`. This is a **packaging decision, not an architectural one**: the serving driver and the evaluation harness remain distinct crates over the same engine.

## Alternatives rejected

- **Separate data-plane and CLI binaries.** Forces the user to understand the internal architecture before running anything, undermining the standalone-first promise.

## Consequences

Standalone-first becomes real: **one binary, one configuration file, it runs.** The single-engine invariant is untouched — the drivers stay distinct crates; the binary merely exposes them through one door.

## Status

Accepted.
