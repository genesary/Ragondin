---
name: architecture-invariants
description: Load this BEFORE writing or modifying any Rust code in this RAG evaluation platform — before adding a dependency, wiring a crate, defining a trait, or changing the engine or the core. It restates the 11 binding architecture invariants (INV-1…INV-11) and the crate dependency graph, and calls out the three agents break most often: INV-5 (the engine depends on no crate under components/), INV-4 (no heavy dependency in ragondin-types or ragondin-contracts), and INV-10 (execution traces are a return value, not a log). Use it even when the task seems unrelated to architecture — a locally reasonable change is exactly how these invariants get broken.
---

# Architecture invariants

These are **architectural constraints, not style preferences**. A PR that violates one is rejected. Two of them (INV-4, INV-5) are enforced by CI, so violating those fails the build directly. The rest are enforced by review — which means an agent that internalizes them saves everyone a rejected PR.

The reason this skill exists: the long-term threat to this project is not defects, it is **erosion**. Each invariant, taken alone, has a plausible-sounding exception ("the engine could just depend on the BM25 crate for speed"; "PhysicalPipeline should really be serializable"). Each exception is defensible in isolation and destructive in aggregate. Knowing the rule is not enough — you need to recognize the moment you are about to talk yourself into breaking it.

## The three you are most likely to break

Hold these in working memory first. They are the ones a well-meaning change walks into.

- **INV-5 — The engine knows only traits.** `ragondin-engine` must not depend on any crate under `components/`. The temptation is "just import the retriever crate directly, it's faster to wire." Doing so creates a two-tier system where built-in components are privileged over third-party ones — the slow death of a contribution-driven project. *(CI-enforced: the build breaks.)*
- **INV-4 — The core stays light.** `ragondin-types` and `ragondin-contracts` must carry no heavy dependency — no `tantivy`, `tonic`, `ort`, `candle`, vector-store client, or HTTP client. `serde` at most. The temptation is to reach for a convenient type from a heavy crate in a core definition. That single edge pulls the whole dependency into everything downstream. *(CI-enforced: a dependency lint on the core fails.)*
- **INV-10 — Execution traces are a return value, not a log.** The executor's signature *returns* the trace; it does not emit it through `tracing`. The product's differentiating feature — per-node execution replay in the UI — depends on the trace being structured business data. `tracing` runs in parallel for operational telemetry, but never substitutes for `ExecutionTrace`. This is a *signature* decision at the heart of the system, and expensive to undo later.

## All 11 invariants

| ID | Rule |
|---|---|
| **INV-1** | `ragondin-types`, `ragondin-pipeline`, `ragondin-contracts` are **stable API boundaries**. Breaking their public API is a deliberate, versioned act — never a side effect of another change. |
| **INV-2** | `ragondin-engine` is **not an API boundary** and never will be. Refactor it freely; do not treat its internals as stable. |
| **INV-3** | `ragondin-types` and `ragondin-pipeline` contain **value types only**: no global context, no interner, no I/O. A value is fully determined by its content. |
| **INV-4** | **The core stays light.** `ragondin-types` and `ragondin-contracts` must carry **no heavy dependency** — no `tantivy`, `tonic`, `ort`, `candle`, vector-store client, or HTTP client. `serde` at most. *CI-enforced.* |
| **INV-5** | **The engine knows only traits.** `ragondin-engine` must not depend on any crate under `components/`. *CI-enforced.* |
| **INV-6** | **No global state.** The component registry lives on an `EngineContext` passed explicitly as a parameter. Never use a static global registry (`inventory`, `linkme`, or equivalent). |
| **INV-7** | **No privilege for built-in components.** A first-party component registers through exactly the same mechanism as a third-party one. Never add a shortcut, fast path, or special case for a built-in. |
| **INV-8** | **Hashing is over the canonical logical form**, never over source text. Two semantically equivalent configurations formatted differently **must** produce the same hash. |
| **INV-9** | **The IR wire format is separate from the in-memory representation** and versioned independently. Never `#[derive(Serialize)]` internal IR types to produce the wire format. |
| **INV-10** | **Execution traces are a return value, not a log.** The executor's signature returns the trace. `tracing` is used in parallel for operational telemetry, never as a substitute for `ExecutionTrace`. |
| **INV-11** | **Tower governs the network envelope only.** Components are heterogeneous domain traits. Never make a component a `tower::Service`. |

## The crate dependency graph

Dependency arrows point **down only**. Cargo forbids cycles, which is what turns these boundaries from conventions that decay into constraints the compiler refuses to violate.

```
bins → planes → engine → contracts → ir → types
                  ↑           ↑
            components ───────┘
```

- `ragondin-engine` depends on `ragondin-contracts` (the traits) and on **no** crate under `components/`.
- Crates under `components/` depend on `ragondin-contracts` and `ragondin-types`, and on **nothing else in the workspace**. A component is a leaf.
- Only binaries know both the engine and the concrete components. **Binaries are the composition root.**

Each load-bearing crate carries an `ARCHITECTURE.md` stating its local constraints. Read it before modifying that crate — the general invariants here are refined by local ones there.

## When a change seems to require breaking an invariant

Stop. An invariant is not an obstacle to route around — it is the architecture speaking. If you genuinely cannot do the task without violating one, that is a signal the design needs revisiting, which is a decision above your pay grade for a single PR: **open a `decision` issue** (see the `opening-a-decision-issue` skill) and implement nothing that presupposes an answer.

> The source of truth for these invariants is `AGENTS.md` (§ Invariants). This skill transcribes it so you can load it cheaply before coding; if the two ever disagree, `AGENTS.md` wins and this skill should be corrected.
