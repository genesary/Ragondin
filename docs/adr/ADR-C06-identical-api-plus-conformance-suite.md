# ADR-C6: Built-ins and third parties share one API, backed by a conformance suite

## Context

"No privilege for built-in components" is only a slogan unless equivalence between built-in and third-party implementations — and between `Local` and `Remote` natures — is **verified rather than asserted**.

## Decision

Built-in and third-party components use **exactly the same API**. A **conformance suite** of behavioural tests must be passed by **every** implementation of a contract, whatever its nature (`Local` or `Remote`). A contributor plugs their component into the suite and obtains a conformance guarantee.

## Alternatives rejected

- **A privileged API for built-ins.** Creates the two-tier system the architecture exists to avoid; contributors become second-class citizens and the community project decays.

## Consequences

`Local`/`Remote` and built-in/third-party equivalence is **real, not asserted**. The conformance suite is what operationally enforces the no-privilege invariant — without it, the invariant is unenforceable.

## Status

Accepted.
