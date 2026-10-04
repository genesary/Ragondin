# Agent workflow

A short guide for the **humans** directing the AI agents that build this repository. The agents themselves are governed by `AGENTS.md` and the project skills under `.claude/skills/`; this document is about how you, the human, orchestrate them.

## One issue, one branch, one worktree, one agent

Each unit of work is a single issue, developed on its own branch, in its own git worktree, by a single agent. Git worktrees let several agents run in parallel on **disjoint crates** without colliding on the working tree. Keep the mapping strict: if two agents share a worktree, they share a working tree, and the isolation is gone.

Branch naming follows `<type>/<issue-number>-<slug>`, e.g. `feat/12-logical-pipeline-hash`.

**Remove a worktree as soon as its agent is finished.** A worktree's agent is finished when its PR is merged or closed, or when its work has been handed to another worktree. Each worktree carries its own `target/` and `ui/node_modules/`, which run to several gigabytes per worktree once `just check` has run. Worktrees left in place fill the disk, and every agent then fails with "no space left on device".

- Run `git worktree remove <path>` right after the merge, and `git worktree prune` after it.
- Keep only the worktrees whose agent is still running, or whose PR is still open and awaiting review or fixes.
- Before removing a worktree, check `git -C <path> status`. Uncommitted changes there are either work to save first, or the stale leftovers of a branch that has already merged.

## How to launch an agent on an issue

Use this framing, verbatim:

> *"Here is issue #N. The design is settled. Your specification is the issue, plus `AGENTS.md` and `docs/`. Apply test-driven development. Do not reopen any frozen decision. If an architectural choice is required that `AGENTS.md` says escalates, stop and open a `decision` issue."*

The point of the wording is to switch off the design half of a general-purpose agent framework while keeping its execution half. The design is done; the agent's job is to implement it, test-first.

## Which issues may run in parallel, and which are serialized

The dependency graph is a **topological order**. An agent cannot implement:

- a component before the trait it implements (`ragondin-contracts`),
- the engine before the pipeline representation it executes (`ragondin-pipeline`),
- a driver before the engine it drives (`ragondin-engine`).

Two issues may run **in parallel** when they touch **disjoint crates** — for example, two different `components/` leaves, each depending only on `ragondin-contracts` and `ragondin-types`, cannot conflict. `ui/`, the front end, is not a crate but counts as one such unit: an issue confined to `ui/` is disjoint in files from every crate, and is governed by `ui/ARCHITECTURE.md`. Two issues must be **serialized** when one depends on an artifact the other produces. When in doubt, read the crate dependency graph in `AGENTS.md` and serialize.

## When to escalate to a human

- **Any `decision` issue.** These are architectural questions an agent must not answer alone; a human owns the call and the resulting ADR.
- **Any PR touching `ragondin-pipeline`, `ragondin-contracts`, or `ragondin-engine`.** The core deserves heavier review than the periphery — a mistake there propagates everywhere, whereas a mistake in a leaf component is contained.

**This list is not the other two, and the three are not meant to coincide.** Three lists in this repository name crates and look alike; each answers a different question, and reading one as another is the mistake this note exists to prevent:

| The question | Where it is answered | What it names |
|---|---|---|
| Which PR does a human read closely? | the bullet above | `ragondin-pipeline`, `ragondin-contracts`, `ragondin-engine` |
| Which choice may an agent not make alone? | `AGENTS.md` § Rules of engagement | shared surfaces, several of which are not crates |
| What may an outside consumer depend on? | INV-1, `AGENTS.md` § Invariants | `ragondin-types`, `ragondin-pipeline`, `ragondin-contracts` |

The two differences that follow are deliberate. **`ragondin-engine` is reviewed heavily and is not an API boundary** — INV-2 says it never will be; it is here for its in-workspace blast radius, which is a different and equally good reason. **`ragondin-types` is an API boundary and is not on the review list** — a change to its public API is already governed by the second row, which escalates the *choice* before a PR exists, and by INV-1's sign in a diff, which a reviewer applies wherever the diff lands.

`ragondin-conformance` and `ragondin-proto` appear on none of the three, although [ADR-C21](adr/ADR-C21-stable-api-boundaries-and-the-internal-engine.md) places both as outsider-facing with compatibility rules of their own. That is recorded here rather than fixed: putting a crate on one of these lists changes a rule, which is not what a note clarifying what the lists mean may do.

## Tooling note

A **Rust language-server integration is strongly recommended**, so that agents *see* types rather than inferring them. On a multi-crate workspace built around trait objects and generics, this is the difference between code that compiles and code that is merely plausible. It is the highest-leverage piece of agent tooling for this repository.

## The milestone roadmap

Work is organized into milestones M0…M8 (create and inspect them on GitHub; each carries its exit criterion in its description). In brief:

| Milestone | Theme |
|---|---|
| **M0** | Foundations — workspace, invariant checks, agentic workflow |
| **M1** | Core contracts & engine skeleton |
| **M2** | First defensible deliverable — hybrid retrieval on BEIR (the primary guard against scope inflation) |
| **M3** | Generation & end-to-end RAG |
| **M4** | The front end — the viewer, the launcher and the editor, at parity with the command line |
| **M5** | The calibrated judge |
| **M6** | Control flow — corrective & agentic RAG |
| **M7** | Cloud-native — Kubernetes, controller, config delivery |
| **M8** | Custom benchmarks |

The front end became M4 on 2026-09-30, and every later milestone moved by one. A milestone number written before that date in a mutable document was corrected in the same change; the accepted ADRs that cite milestones by number were updated as identifiers only, as `docs/adr/README.md` records.

**Every milestone that adds a capability a user can reach — through the command line, a pipeline document or a run record — includes that capability's front-end slice in its exit criterion.** A capability that reaches the command line and not the front end is not finished. This is how every milestone before M4 already worked — `compare` shipped in M2, its parameter diff in M3 — and it is what keeps the front end from becoming a milestone of its own again. Within M4, the viewer is built before the editor and no viewer issue depends on an editor issue ([ADR-16](adr/ADR-016-visual-editing-with-the-front-end-yaml-stays-the-truth.md)), so the load-bearing views ship whatever happens to the editor's schedule.

**Post-M8 horizon** (deliberately beyond the initial roadmap, tracked in `docs/OPEN_QUESTIONS.md`):

- **Multi-tenant RAG-as-a-Service** — tenant isolation, quotas, security, index sharing; deferred until the research bench is proven.

M2 is the project's first defensible deliverable and the primary guard against scope inflation. Resist any pull to bring later milestones' work forward into it.
