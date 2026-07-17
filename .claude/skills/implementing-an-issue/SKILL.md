---
name: implementing-an-issue
description: Load this at the START of any implementation issue in this RAG platform repository, before writing code or planning an approach. It is the standard operating procedure: the design is already settled, so do NOT open a brainstorming or design phase to re-derive decisions that are made — this explicitly overrides the default of general-purpose agent frameworks that begin every task with a design phase. Use it whenever you pick up an issue here, especially if you feel an urge to first "explore options" or "propose an architecture"; that urge is the exact behavior this skill exists to suppress. It also mandates test-driven development, scope discipline, and running `just check` before declaring completion.
---

# Implementing an issue

This repository is built one issue at a time, largely by AI agents. The whole system's coherence depends on each agent treating the design as settled and staying inside its lane. This skill is the procedure that makes that happen.

## Why the first step matters most

General-purpose agent frameworks open every task with a design or brainstorming phase. **For this project that phase is already complete**, done with far more depth than a per-issue exercise could reach, and recorded in `AGENTS.md`, `docs/`, and the ADRs under `docs/adr/`. Re-deriving those decisions per issue does not just waste effort — it produces *drift*, because a fresh derivation will land somewhere subtly different from the settled design, and the architecture erodes one locally reasonable PR at a time.

So the single most important instruction is: **suppress the design half, keep the execution half.** Use your framework's strengths — strict TDD, review passes, isolated worktrees — and skip its instinct to redesign.

## The procedure

1. **The design is settled.** Your specification is the issue, plus `AGENTS.md` and `docs/`. Do **not** run a brainstorming or design phase to re-derive decisions already made. If the issue tells you what to build, build that.
2. **Read the crate's `ARCHITECTURE.md` before modifying it.** Each load-bearing crate states its local invariants there. Load the `architecture-invariants` skill too, before writing code.
3. **Test-driven, strictly.** Write a failing test first; implement the minimum to make it pass; then refactor. A test that passes without exercising the behavior is worse than no test.
4. **Stay inside Scope — IN; honor Scope — OUT.** Scope — OUT exists to prevent collisions with other agents working in parallel on disjoint crates. Touching a file the issue puts out of scope is not a favor; it is a merge conflict and a boundary violation.
5. **No opportunistic refactors. Apply YAGNI.** No speculative abstraction. If you notice something worth changing outside your scope, note it for a separate issue; do not fix it here.
6. **Run `just check` before declaring completion.** This is the single mandatory pre-completion command. It runs build, tests, `clippy` with `-D warnings`, `fmt --check`, and the architecture invariant checks. Work is not done until it passes — treat a red `just check` as "not finished," not "finished with caveats."
7. **If an architectural choice is required that is not already settled** in `AGENTS.md` or `docs/`: **stop and open a `decision` issue** (see the `opening-a-decision-issue` skill). Do not guess. An agent that pauses on an ambiguity costs minutes; an agent that guesses an architectural decision costs a refactor.

## Definition of done

Before you say the work is complete, confirm every box:

- [ ] Every acceptance criterion in the issue is met — **mechanically**, by a command or a test, never by a judgment call.
- [ ] `just check` passes (build, test, clippy with `-D warnings`, fmt, invariant checks).
- [ ] New behavior is covered by tests written **before** the implementation.
- [ ] No frozen decision was reopened; no architectural decision was made implicitly (see the `frozen-decisions` skill).
- [ ] The PR description names the issue it closes and any invariants it touches.

## A note on scope creep

If an issue turns out to require touching more than two crates (outside explicit scaffolding issues), it is mis-scoped. **Say so rather than sprawling.** Surfacing a mis-scoped issue is a contribution; quietly growing the change to cover it is how a two-crate task becomes an un-reviewable ten-crate diff.

> The source of truth for these rules is `AGENTS.md` (§ Rules of engagement, § Definition of done). This skill operationalizes them; if the two disagree, `AGENTS.md` wins.
