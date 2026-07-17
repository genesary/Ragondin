# Agent workflow

A short guide for the **humans** directing the AI agents that build this repository. The agents themselves are governed by `AGENTS.md` and the project skills under `.claude/skills/`; this document is about how you, the human, orchestrate them.

## One issue, one branch, one worktree, one agent

Each unit of work is a single issue, developed on its own branch, in its own git worktree, by a single agent. Git worktrees let several agents run in parallel on **disjoint crates** without colliding on the working tree. Keep the mapping strict: if two agents share a worktree, they share a working tree, and the isolation is gone.

Branch naming follows `<type>/<issue-number>-<slug>`, e.g. `feat/12-logical-pipeline-hash`.

## How to launch an agent on an issue

Use this framing, verbatim:

> *"Here is issue #N. The design is settled. Your specification is the issue, plus `AGENTS.md` and `docs/`. Apply test-driven development. Do not reopen any frozen decision. If an architectural choice is required that is not already settled, stop and open a `decision` issue."*

The point of the wording is to switch off the design half of a general-purpose agent framework while keeping its execution half. The design is done; the agent's job is to implement it, test-first.

## Which issues may run in parallel, and which are serialized

The dependency graph is a **topological order**. An agent cannot implement:

- a component before the trait it implements (`rag-contracts`),
- the engine before the IR it executes (`rag-ir`),
- a driver before the engine it drives (`rag-engine`).

Two issues may run **in parallel** when they touch **disjoint crates** — for example, two different `components/` leaves, each depending only on `rag-contracts` and `rag-types`, cannot conflict. Two issues must be **serialized** when one depends on an artifact the other produces. When in doubt, read the crate dependency graph in `AGENTS.md` and serialize.

## When to escalate to a human

- **Any `decision` issue.** These are architectural questions an agent must not answer alone; a human owns the call and the resulting ADR.
- **Any PR touching `rag-ir`, `rag-contracts`, or `rag-engine`.** The core deserves heavier review than the periphery — a mistake there propagates everywhere, whereas a mistake in a leaf component is contained.

## Tooling note

A **Rust language-server integration is strongly recommended**, so that agents *see* types rather than inferring them. On a multi-crate workspace built around trait objects and generics, this is the difference between code that compiles and code that is merely plausible. It is the highest-leverage piece of agent tooling for this repository.

## The milestone roadmap

Work is organized into milestones M0…M7 (create and inspect them on GitHub; each carries its exit criterion in its description). In brief:

| Milestone | Theme |
|---|---|
| **M0** | Foundations — workspace, invariant checks, agentic workflow |
| **M1** | Core contracts & engine skeleton |
| **M2** | First defensible deliverable — hybrid retrieval on BEIR (the primary guard against scope inflation) |
| **M3** | Generation & end-to-end RAG |
| **M4** | The calibrated judge |
| **M5** | Control flow — corrective & agentic RAG |
| **M6** | Cloud-native — Kubernetes, controller, config delivery |
| **M7** | Custom benchmarks |

**Post-M7 horizon** (deliberately beyond the initial roadmap, tracked in `docs/OPEN_QUESTIONS.md`):

- **Visual graph authoring** — the visual-authoring phase; gated on the visual control-flow rendering open question.
- **Multi-tenant RAG-as-a-Service** — tenant isolation, quotas, security, index sharing; deferred until the research bench is proven.

M2 is the project's first defensible deliverable and the primary guard against scope inflation. Resist any pull to bring later milestones' work forward into it.
