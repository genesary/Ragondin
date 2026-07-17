# ADR-C8: async_trait in v0

## Context

Dynamic dispatch is **mandatory**: it is impossible to know at compile time whether a node is `Local` or `Remote`, so component traits must be `dyn`-compatible **and** async. Three options exist for `dyn`-compatible async traits: the `async_trait` macro (boxes each future — one allocation per call), native `async fn` in traits / RPITIT (no boxing, but `dyn`-compatibility is not automatic), and hand-boxed futures (verbose).

## Decision

Use **`async_trait`** in v0.

## Alternatives rejected

- **Native `async fn` in traits (RPITIT).** No boxing, but `dyn`-compatibility is not automatic; reserved for later, for hot-path traits only, if profiling justifies it.
- **Hand-boxed futures.** Verbose; reserved for special cases only.

## Consequences

Simple, proven, `dyn`-compatible without friction. On the hot path, a node's real work — an embedding forward pass, a vector search, a network round trip — dwarfs the cost of boxing a future by orders of magnitude, so the allocation is negligible. The same reasoning justifies the dynamic dispatch itself: the vtable indirection is negligible next to the work being dispatched, and the flexibility it buys (Local and Remote indistinguishable to the engine) is the foundation of the contribution model. The optimization to RPITIT is local and reversible if ever warranted.

## Status

Accepted.
