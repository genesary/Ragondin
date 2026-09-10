---
name: architecture-invariants
description: Load this BEFORE writing or modifying any Rust code in this RAG evaluation platform — before adding a dependency, wiring a crate, defining a trait, or changing the engine or the core. It does not restate the 11 binding invariants or the crate dependency graph (AGENTS.md § Invariants holds those): it routes you to the ones that bite in the situation you are in, and calls out the three agents break most often — INV-5 (the engine depends on no crate under components/), INV-4 (no heavy dependency in ragondin-types or ragondin-contracts), and INV-10 (execution traces are a return value, not a log) — with the temptation each one hides behind. Use it even when the task seems unrelated to architecture — a locally reasonable change is exactly how these invariants get broken.
---

# Architecture invariants

These are **architectural constraints, not style preferences**. A PR that violates one is rejected. The eleven invariants (INV-1…INV-11), the rule each one states, how each is enforced, and — for the review-enforced ones — the sign it leaves in a diff, are all in `AGENTS.md` § Invariants. This skill does not copy that list; it tells you where to stand before you read it.

The reason this skill exists: the long-term threat to this project is not defects, it is **erosion**. Each invariant, taken alone, has a plausible-sounding exception ("the engine could just depend on the BM25 crate for speed"; "PhysicalPipeline should really be serializable"). Each exception is defensible in isolation and destructive in aggregate. Knowing the rule is not enough — you need to recognize the moment you are about to talk yourself into breaking it.

## Read the enforcement split first

`AGENTS.md` § Invariants sorts the eleven by **how they are enforced**, and the split changes what you must do about each one:

- **CI-enforced** invariants (INV-3, INV-4, INV-5, INV-6, INV-11) fail the build with a message naming the invariant — but read the *How* column before you trust that: it names three enforcement classes, and only the dependency-**closure** checks are complete. INV-3 is decided by closure only in part (its I/O clause). **INV-6 is a manifest check** — it decides `inventory` and `linkme` as *direct* dependencies of a crate in this repository, and sees nothing a crate reaches through another crate. **INV-11 is a best-effort source scan** — a green build is evidence, not proof, and does not discharge it. What a check does not decide is yours to respect for the rest — for INV-6 that includes a third-party crate that wraps `inventory` in its own internals.
- **Review-enforced** invariants (INV-1, INV-2, INV-7, INV-8, INV-9, INV-10, and INV-11 again) are caught only if a human sees, in a diff, that a rule was broken. `AGENTS.md` pairs each of these with **the sign it leaves in a diff** — what you would *see*, as opposed to what the rule says. Read that column before reviewing a diff or claiming one is clean; it is not reproduced here.

## The three you are most likely to break

Hold these in working memory first. They are the ones a well-meaning change walks into. The binding wording is in `AGENTS.md` § Invariants — what follows is *why* each one is the trap and what the temptation sounds like.

- **INV-5 — the engine knows only traits.** The temptation is "just import the retriever crate directly, it's faster to wire." Doing so creates a two-tier system where built-in components are privileged over third-party ones — the slow death of a contribution-driven project. *(CI-enforced, closure: the build breaks.)*
- **INV-4 — the core stays light.** The temptation is to reach for a convenient type from a heavy crate in a core definition. That single edge pulls the whole dependency into everything downstream. *(CI-enforced, closure: a dependency check on the core fails.)*
- **INV-10 — execution traces are a return value, not a log.** The temptation is to emit per-node detail through `tracing` because it is at hand. The product's differentiating feature — per-node execution replay in the UI — depends on the trace being structured business data returned by the executor, with `tracing` running in parallel for operational telemetry only. This is a *signature* decision at the heart of the system, and expensive to undo later. *(Review-enforced: nothing but a reader catches it.)*

## Before you touch a dependency, a trait, or the engine

- Adding or wiring a dependency? Check **INV-4** and **INV-5** (`AGENTS.md` § Invariants), and the crate dependency graph in the same file — arrows point **down only**, `ragondin-engine` sees no crate under `components/`, a component is a leaf depending only on `ragondin-contracts` and `ragondin-types`, and only binaries know both the engine and the concrete components.
- Defining or changing a public type in `core/`? That is **INV-1** territory: breaking the public API of `ragondin-types`, `ragondin-pipeline` or `ragondin-contracts` is a deliberate, versioned act, never a side effect. Refactoring `ragondin-engine` is the opposite — **INV-2** says it is not an API boundary; do not treat its internals as stable.
- Registering a component, hashing a configuration, serializing the IR, or wrapping something in `tower`? Those are INV-6, INV-8, INV-9 and INV-11 — read them before you write the code, not after review sends it back.

Each rule in `AGENTS.md` ends with a link to the ADR that argues it. **Read that ADR before proposing an exception** — the alternatives an ADR rejected are usually the exception you were about to propose.

Each load-bearing crate also carries an `ARCHITECTURE.md` stating its local constraints, which refine these general invariants. Read it before modifying that crate.

## When a change seems to require breaking an invariant

Stop. An invariant is not an obstacle to route around — it is the architecture speaking. If you genuinely cannot do the task without violating one, that is a signal the design needs revisiting, which is a decision above the pay grade of a single PR: **open a `decision` issue** (see the `opening-a-decision-issue` skill) and implement nothing that presupposes an answer.
