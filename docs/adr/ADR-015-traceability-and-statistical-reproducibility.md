# ADR-15: Traceability and statistical reproducibility, not strict determinism

## Context

An LLM served with dynamic batching is **not deterministic even at a fixed seed**: the floating-point reduction order varies with batch composition. Strict, bit-for-bit reproducibility of a judge-based run therefore cannot be guaranteed by any amount of engineering.

## Decision

The platform **does not promise strict determinism** for judge-based runs. It guarantees instead:

- **Complete traceability** — everything that ran is recorded: model, prompt, temperature, seed, version.
- **Statistical reproducibility** — replicates, with confidence intervals on scores.

The judge is treated as a **measuring instrument**: versioned (model hash, prompt, parameters), seeded, reported with confidence intervals, and calibrated against a small human-annotated set.

## Alternatives rejected

- **Promise deterministic judge-based runs.** A promise the platform cannot keep, because LLM non-determinism under dynamic batching is irreducible. Claiming determinism would be dishonest and would discredit the bench the first time a run failed to reproduce bit-for-bit.

## Consequences

A serious bench states this nuance rather than papering over it. Without confidence intervals, configuration rankings are noise dressed as science — disqualifying for a research audience; with them, rankings are defensible. Handling judge variance rigorously becomes a first-class concern rather than an afterthought.

## Status

Accepted.
