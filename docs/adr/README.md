# Architecture Decision Records

An **Architecture Decision Record (ADR)** captures one architectural decision: its context, the decision itself, the alternatives that were rejected, and the consequences that follow. The point of keeping decisions as ADRs is that a decision becomes an **individually citable artifact** — a contributor or an agent can be pointed at `docs/adr/ADR-4.md` instead of at a row in a table buried inside a long design document.

`AGENTS.md` and the two architecture documents in `docs/` state the decisions tersely, as rules and rationale. The ADRs here are the durable, per-decision record those documents point to.

## The process

1. **An ADR's decision is immutable once accepted.** You do not edit the substance of an accepted ADR. If reality changes the decision, you write a **new** ADR that supersedes the old one and references it. The old ADR stays, marked `Superseded by ADR-N`, so the history of the decision is legible.
2. **A factual error in an ADR's *reasoning* is retracted in place, and never silently.** An accepted decision is sometimes justified by a claim about the system that later proves false. Superseding would misrepresent what happened — the decision did not change — but leaving the claim standing is worse, because an ADR is written to be cited. Such a claim is retracted in place, under **all** of these conditions:
   - only **Context**, **Alternatives rejected** and **Consequences** may be amended this way. **The Decision section changes only by supersession.** That boundary is what stops this from becoming a route around rule 1.
   - the ADR gains an **`## Amendments`** section stating what was retracted, **quoting the original wording verbatim**, when, why, and on whose authority;
   - its Status becomes `Accepted (amended <date>)`;
   - it is its own PR — rule 4 below still applies.

   If the retraction removes the *only* ground the decision rested on, the decision is no longer supported and this mechanism does not apply: supersede it instead.
3. **A `decision` issue produces exactly one ADR.** Architectural questions that an agent must not answer alone (see `docs/OPEN_QUESTIONS.md` and the `opening-a-decision-issue` skill) are resolved in a `decision` issue, and the outcome is a single new accepted ADR.
4. **An ADR is never silently overturned inside a PR.** Reopening a decision is a deliberate, visible act — a superseding ADR — never a side effect of implementation work. This is the mechanism that protects the architecture from erosion.

## Layout

- **`000-template.md`** — the template every ADR follows: Context / Decision / Alternatives rejected / Consequences / Status, plus an optional Amendments section (process rule 2).
- **`ADR-1` … `ADR-15`** — the frozen decisions of the *system* architecture, one file per decision.
- **`ADR-C1` … `ADR-C16`** — the frozen decisions of the *code* architecture, one file per decision.

Each file is self-contained: it should be understandable and actionable on its own, without first reading `AGENTS.md` or the architecture documents. If an ADR only makes sense after reading something else, it is under-specified and should be fixed.

## Numbering

- System-architecture decisions use the bare prefix: `ADR-1` … `ADR-15`.
- Code-architecture decisions use the `C` prefix: `ADR-C1` … `ADR-C16`.
- New decisions (from `decision` issues) continue the appropriate sequence and are added, never inserted retroactively.

## Status values

- **Accepted** — the decision is in force.
- **Superseded by ADR-N** — replaced by a later ADR, which it should reference and which should reference it back.
- **Accepted (amended \<date\>)** — in force, with a factual claim in its reasoning retracted in place under process rule 2. The decision itself is unchanged; see the ADR's `## Amendments` section for what was retracted and why.
- **Proposed** — under discussion in a `decision` issue; not yet in force.
