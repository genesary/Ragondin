# ADR-C3: Closed enum of primitive nodes plus an open Extension variant

## Context

The node representation must be exhaustive enough to reason about — a closed set of primitives the engine understands — yet it must not force a change to the core every time someone invents a new technique. These two requirements pull against each other.

## Decision

The node representation is a **closed enum of primitive nodes** — retriever, fusion, reranker, generator, grader, branch (first-class control flow), bounded loop (with a mandatory termination guard) — **plus an open `Extension` variant** carrying a node defined outside the core.

## Alternatives rejected

- **A fully closed enum.** Every new node type forces a change to the core enum — exactly the coupling the architecture avoids.
- **Trait objects everywhere (no enum).** Loses exhaustiveness and the ability to reason about, and optimize over, the closed set of primitives.

## Consequences

A researcher who invents a genuinely new node type goes through `Extension` **without modifying the core**. Repeated use of `Extension` for the same shape is the signal to later promote that shape into a primitive node. The architecture's quality metric sharpens accordingly: what fraction of techniques require not even `Extension`?

## Status

Accepted.
