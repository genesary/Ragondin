---
name: opening-a-decision-issue
description: Load this the moment an implementation task appears to require an architectural choice that AGENTS.md and docs/ do not already settle — a new trade-off, an unlisted option, an open question you'd have to resolve to proceed, or any temptation to reopen a frozen decision. Instead of guessing, you open a decision issue. This skill tells you how to write one that a human can act on: the question, why the existing docs don't settle it, the alternatives and their trade-offs, which issues it blocks, and what you deliberately left unimplemented while waiting. Reach for it especially when you catch yourself about to "just pick something reasonable" — that instinct is the failure mode this exists to catch.
---

# Opening a decision issue

Some choices are above the pay grade of a single implementation PR: anything that settles an architectural question the design does not already answer. Guessing at one is the most expensive mistake an agent can make here — it costs a refactor and quietly erodes the architecture. The correct move is to **stop and hand the decision to a human, cleanly documented.**

## When to open one

- The task requires an architectural choice **not already settled** in `AGENTS.md` or `docs/`.
- You are tempted to **reopen a frozen decision** (see the `frozen-decisions` skill) because you think it is wrong.
- You would otherwise **resolve an entry in `docs/OPEN_QUESTIONS.md`** in passing. Those are deliberately unresolved; never answer one as a side effect.

In all three cases: stop, open the issue, and **implement nothing that presupposes an answer.**

## How to write it

A good decision issue lets a human decide without reconstructing your context. Include, in order:

1. **The question.** State it sharply and in one place — the specific choice that must be made.
2. **Why `AGENTS.md` and `docs/` do not settle it.** Cite what you checked. If the docs *almost* answer it, say exactly where they stop. This is what distinguishes a real decision from a rule you missed.
3. **The alternatives and their trade-offs.** Lay out the genuine options, each with what it costs and what it buys. Do not pre-select a winner — that is the human's call — but give them enough to make it.
4. **Which issues it blocks.** Name the issues (or the work) that cannot proceed until this is decided. This sets the priority.
5. **What you deliberately left unimplemented while waiting.** The single most useful line for whoever picks the work back up: the exact code, crate, or file you did *not* write because it would presuppose an answer.

## The outcome

A decision issue resolves into **exactly one new ADR** in `docs/adr/`. The ADR is the durable artifact: an individually citable record of the context, the decision, the alternatives rejected, and the consequences. The issue is where the discussion happens; the ADR is where the conclusion lives, immutable once accepted. Use the `decision.yml` issue template, which mirrors this structure.

## Use the issue template

`.github/ISSUE_TEMPLATE/decision.yml` encodes exactly the sections above. Filling it in is the fastest way to write a well-formed decision issue — and it keeps every such issue consistent, so a reviewer knows where to look.

> The obligation to open a decision issue rather than guess comes from `AGENTS.md` (§ Rules of engagement). This skill tells you how to satisfy it well.
