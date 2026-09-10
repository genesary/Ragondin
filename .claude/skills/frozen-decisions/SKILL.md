---
name: frozen-decisions
description: Load this BEFORE proposing or making any change to HOW this RAG platform is built — the async-trait strategy, the component registry, config delivery, the LLM judge, incrementality/caching, the plan optimizer, or the serving layer. It does not list the settled ("frozen") decisions (AGENTS.md § Frozen decisions does): it gives the binding rule that a PR must never reopen one, and — for the ones agents most often try to reopen — the tempting alternative and the reason it was ruled out. Reach for it the moment you catch yourself thinking "wouldn't it be better to use native async fn / a global registry / xDS / salsa here" — those are precisely the decisions already made and closed. If you still believe one is wrong, this skill tells you the only allowed path: stop and open a decision issue.
---

# Frozen decisions

These decisions were made **deliberately, after analysis** — with far more depth than a per-issue exercise could reach. Agents (and humans) routinely try to "improve" them, because each rejected alternative looks locally reasonable and modern. That is exactly why they are frozen: the reasoning that ruled them out is not visible from inside a single task.

`AGENTS.md` § Frozen decisions is the list — the frozen areas and the binding "do not" for each. (The serving layer is the exception: it is frozen as INV-11, under § Invariants.) This skill does not copy that table; it operationalizes the rule that protects it and glosses the ones agents most often try to reopen.

## The binding rule

`AGENTS.md` § Frozen decisions states it: **never reopen a frozen decision inside a PR.** If you believe one is wrong:

1. **Stop.**
2. Open a `decision` issue (see the `opening-a-decision-issue` skill).
3. **Implement nothing that presupposes an answer.**

A frozen decision changes only through a new ADR that explicitly supersedes the old one — never silently, and never as a side effect of implementation work. The point of freezing is to make the cost of reopening visible, so it happens on purpose or not at all.

## The ones agents most often try to "improve"

For these, the binding "do not" is in `AGENTS.md` § Frozen decisions — except the serving layer, which is INV-11 under § Invariants. What this table adds is the tempting alternative and the reason it was ruled out. If your plan involves the right-hand column, you are about to reopen a frozen decision.

| Area | The tempting alternative, and why it is closed |
|---|---|
| **Async traits** | Tempted by RPITIT or native `async fn` in traits? Boxing a future is negligible next to a network round trip or a forward pass. |
| **Registry** | Tempted by a global registry (`inventory`, `linkme`)? A global makes two contexts in one process impossible — which the evaluation harness needs. |
| **Config delivery** | Tempted by xDS? Its schema models network proxying, not RAG pipelines, and its one real benefit (ecosystem interop) buys nothing here. |
| **The LLM judge** | Tempted to special-case the judge inside the evaluation harness? Treating it as a component of the IR like any other is what makes self-preference detection free. |
| **Incrementality** | Tempted by `salsa` or another fine-grained incrementality framework? The unit of recomputation is the *run*; a content-addressed run cache is the right, far simpler mechanism. |
| **Plan optimizer** | Tempted to build the optimizer? The seam is the point; the optimizer is not. |
| **Serving layer** | Tempted to make a component a `tower::Service`? Tower governs the network envelope only; domain components are heterogeneous, a `Service` is uniform (INV-11). |

## Where the reasoning lives

Every frozen decision is recorded as an individually citable ADR under `docs/adr/`, and `AGENTS.md` § Frozen decisions points at `docs/adr/`. When you want the *why*, read that ADR rather than re-deriving it — and treat a plausible-sounding alternative as a prompt to read the ADR, not as license to change course. Each ADR states the context, the decision, the alternatives that were rejected, and the consequences.
