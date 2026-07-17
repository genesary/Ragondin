# ADR-C4: Engine as an embeddable library with an explicit EngineContext

## Context

The engine should be reusable outside our own binaries, testable in isolation, and able to host **several independent configurations in one process** — the evaluation harness compares two configurations side by side. A global registry makes multiple contexts in one process impossible and makes initialization order opaque and untestable.

## Decision

The engine is an **embeddable library** parameterized by an explicit **`EngineContext`** that carries the component registry (a table mapping an implementation name to a component constructor). The context is **passed explicitly, never global**. Binaries are thin shells over the library.

## Alternatives rejected

- **An application-style engine with global state.** Opaque initialization order, untestable, and makes two contexts in one process impossible — which the harness requires.

## Consequences

Several distinct contexts can be instantiated in one process, which is indispensable for the harness. The engine is embeddable and testable. The **registration ergonomics** (explicit registration in the binary versus a helper) remain an open question, but the no-global constraint is fixed.

## Status

Accepted.
