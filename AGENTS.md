# AGENTS.md

Operating rules for AI coding agents working in this repository.

This file contains **rules, not background**. It documents the practices of this repository that differ from ordinary Rust conventions — the things an agent would otherwise get wrong by default. Project rationale, architecture reasoning, and design history live in `docs/` and are deliberately not repeated here.

**Read this file in full before writing code. Its rules are binding.**

---

## Commands

```bash
# Build
cargo build --all-targets

# Test
cargo test --workspace

# Lint (warnings are errors)
cargo clippy --workspace --all-targets -- -D warnings

# Format
cargo fmt --check

# Architecture invariant checks (see "Invariants")
just check-invariants

# All of the above — run this before declaring any work complete
just check
```

Toolchain: **stable** Rust, never nightly. Dependency versions are declared **only** in `[workspace.dependencies]` at the workspace root, never inline in a member crate.

---

## Invariants

Architectural constraints, not style preferences. **A PR that violates one is rejected.** Two are enforced by CI: your build will fail.

| ID | Rule |
|---|---|
| **INV-1** | `rag-types`, `rag-ir`, `rag-contracts` are **stable API boundaries**. Breaking their public API is a deliberate, versioned act — never a side effect of another change. |
| **INV-2** | `rag-engine` is **not an API boundary** and never will be. Refactor it freely; do not treat its internals as stable. |
| **INV-3** | `rag-types` and `rag-ir` contain **value types only**: no global context, no interner, no I/O. A value is fully determined by its content. |
| **INV-4** | **The core stays light.** `rag-types` and `rag-contracts` must carry **no heavy dependency** — no `tantivy`, `tonic`, `ort`, `candle`, vector-store client, or HTTP client. `serde` at most. *CI-enforced.* |
| **INV-5** | **The engine knows only traits.** `rag-engine` must not depend on any crate under `components/`. *CI-enforced.* |
| **INV-6** | **No global state.** The component registry lives on an `EngineContext` passed explicitly as a parameter. Never use a static global registry (`inventory`, `linkme`, or equivalent). |
| **INV-7** | **No privilege for built-in components.** A first-party component registers through exactly the same mechanism as a third-party one. Never add a shortcut, fast path, or special case for a built-in. |
| **INV-8** | **Hashing is over the canonical logical form**, never over source text. Two semantically equivalent configurations formatted differently **must** produce the same hash. |
| **INV-9** | **The IR wire format is separate from the in-memory representation** and versioned independently. Never `#[derive(Serialize)]` internal IR types to produce the wire format. |
| **INV-10** | **Execution traces are a return value, not a log.** The executor's signature returns the trace. `tracing` is used in parallel for operational telemetry, never as a substitute for `ExecutionTrace`. |
| **INV-11** | **Tower governs the network envelope only.** Components are heterogeneous domain traits. Never make a component a `tower::Service`. |

### Crate dependency graph

Dependency arrows point **down only**. Cargo forbids cycles, which makes these boundaries compiler-enforced rather than conventional.

```
bins → planes → engine → contracts → ir → types
                  ↑           ↑
            components ───────┘
```

- `rag-engine` depends on `rag-contracts` (the traits) and on **no** crate under `components/`.
- Crates under `components/` depend on `rag-contracts` and `rag-types`, and on **nothing else in the workspace**. A component is a leaf.
- Only binaries know both the engine and the concrete components. **Binaries are the composition root.**

Each load-bearing crate carries an `ARCHITECTURE.md` stating its local constraints. **Read it before modifying that crate.**

---

## Frozen decisions

Decided deliberately, after analysis. Agents routinely try to "improve" these. **Do not.**

| Area | The rule |
|---|---|
| **Async traits** | Use `async_trait`. Do **not** substitute RPITIT or native `async fn` in traits. |
| **Registry** | Explicit `EngineContext`. Do **not** introduce a global registry. |
| **Config delivery** | A purpose-built gRPC service. Do **not** adopt xDS. |
| **The LLM judge** | It is a component of the IR like any other. Do **not** special-case it inside the evaluation harness. |
| **Incrementality** | Do **not** introduce `salsa` or any fine-grained incrementality framework. The unit of recomputation is the *run*; a content-addressed run cache is the correct mechanism. |
| **Plan optimizer** | Preserve the logical→physical *seam*, but the optimizer is the identity function for now. Do **not** build an optimizer. |
| **IR serialization** | The wire format is hand-maintained and versioned separately (INV-9). Do **not** derive it from internal types. |
| **Crate granularity** | Do **not** split or merge crates. A crate is split only when a real seam proves itself; that has not happened yet. |
| **Errors** | `thiserror` (typed) in libraries; `anyhow` in binaries only. A library never imposes `anyhow` on its consumers. |
| **Feature flags** | Every heavy backend sits behind a feature. The default build must stay lean and fast to compile. |

Rationale for each decision: `docs/adr/`, where every decision is a numbered, individually citable ADR.

**If you believe a frozen decision is wrong: stop, open a `decision` issue, and implement nothing that presupposes an answer.**

---

## Rules of engagement

### You implement; you do not decide

- **The design is settled.** Your specification is the issue, plus this file and `docs/`. Do **not** run a brainstorming or design phase to re-derive decisions that are already made.
- If a task requires an architectural choice that is **not** already settled here or in `docs/`: **stop, open a `decision` issue, and do not proceed.**
- Questions listed in `docs/OPEN_QUESTIONS.md` are **deliberately unresolved**. Never resolve one in passing.

### Scope

- One issue, one branch, one PR. Stay strictly within the issue's **Scope — IN**.
- Honor the issue's **Scope — OUT**. It prevents collisions with other agents working in parallel on disjoint crates.
- No opportunistic refactors. No speculative abstraction. Apply YAGNI.
- If an issue turns out to require touching more than two crates (outside explicit scaffolding issues), it is mis-scoped. **Say so rather than sprawling.**

### Engineering

- **Test-driven, strictly.** Write a failing test first; implement the minimum to make it pass; then refactor. A test that passes without exercising the behavior is worse than no test.
- Every acceptance criterion must be satisfied **mechanically** — by a command or a test, never by a judgment call.
- Comments explain **why**, not what.

### Definition of done

- [ ] Every acceptance criterion in the issue is met.
- [ ] `just check` passes (build, test, clippy with `-D warnings`, fmt, invariant checks).
- [ ] New behavior is covered by tests written **before** the implementation.
- [ ] No frozen decision was reopened; no architectural decision was made implicitly.
- [ ] The PR description names the issue it closes and any invariants it touches.

---

## Conventions

- **Language:** English everywhere — code, comments, documentation, issues, commit messages, PR descriptions.
- **Commits:** Conventional Commits, scoped by crate where useful: `feat(rag-ir): add canonical hashing`.
- **Branches:** `<type>/<issue-number>-<slug>`, e.g. `feat/12-logical-pipeline-hash`.

---

## Where the rationale lives

| You need | Read |
|---|---|
| Why the system is designed this way | `docs/architecture-system.md` |
| Why the code is organized this way | `docs/architecture-code.md` |
| Why a specific decision was made | `docs/adr/` |
| What is deliberately undecided | `docs/OPEN_QUESTIONS.md` |
| A crate's local constraints | `<crate>/ARCHITECTURE.md` |

**When in doubt, stop and ask.** An agent that pauses on an ambiguity costs minutes. An agent that guesses an architectural decision costs a refactor — and erodes the architecture one locally reasonable PR at a time.