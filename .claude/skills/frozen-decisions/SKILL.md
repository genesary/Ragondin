---
name: frozen-decisions
description: Load this BEFORE proposing or making any change to HOW this RAG platform is built — the async-trait strategy, the component registry, config delivery, the LLM judge, incrementality/caching, the plan optimizer, or the serving layer. It lists every settled ("frozen") architectural decision and the binding rule that a PR must never reopen one. Reach for it the moment you catch yourself thinking "wouldn't it be better to use native async fn / a global registry / xDS / salsa here" — those are precisely the decisions already made and closed. If you still believe one is wrong, this skill tells you the only allowed path: stop and open a decision issue.
---

# Frozen decisions

These decisions were made **deliberately, after analysis** — with far more depth than a per-issue exercise could reach. Agents (and humans) routinely try to "improve" them, because each rejected alternative looks locally reasonable and modern. That is exactly why they are frozen: the reasoning that ruled them out is not visible from inside a single task.

## The binding rule

**Never reopen a frozen decision inside a PR.** If you believe one is wrong:

1. **Stop.**
2. Open a `decision` issue (see the `opening-a-decision-issue` skill).
3. **Implement nothing that presupposes an answer.**

A frozen decision changes only through a new ADR that explicitly supersedes the old one — never silently, and never as a side effect of implementation work. The point of freezing is to make the cost of reopening visible, so it happens on purpose or not at all.

## The ones agents most often try to "improve"

Each row names the tempting alternative and closes the door on it. If your plan involves the right-hand column, you are about to reopen a frozen decision.

| Area | The rule — and the alternative to NOT reach for |
|---|---|
| **Async traits** | Use `async_trait`. Do **not** substitute RPITIT or native `async fn` in traits. Boxing a future is negligible next to a network round trip or a forward pass. |
| **Registry** | Explicit `EngineContext`. Do **not** introduce a global registry (`inventory`, `linkme`). A global makes two contexts in one process impossible — which the evaluation harness needs. |
| **Config delivery** | A purpose-built gRPC service. Do **not** adopt xDS. Its schema models network proxying, not RAG pipelines, and its one real benefit (ecosystem interop) buys nothing here. |
| **The LLM judge** | It is a component of the IR like any other. Do **not** special-case it inside the evaluation harness. Treating it as a component is what makes self-preference detection free. |
| **Incrementality** | Do **not** introduce `salsa` or any fine-grained incrementality framework. The unit of recomputation is the *run*; a content-addressed run cache is the right, far simpler mechanism. |
| **Plan optimizer** | Preserve the logical→physical *seam*, but the optimizer is the identity function for now. Do **not** build an optimizer. |
| **Serving layer** | Tower governs the network envelope only. Do **not** make a component a `tower::Service`. Domain components are heterogeneous; a `Service` is uniform. |

## Every frozen decision

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

## Where the reasoning lives

Every frozen decision above is recorded as an individually citable ADR under `docs/adr/`. When you want the *why*, read that ADR rather than re-deriving it — and treat a plausible-sounding alternative as a prompt to read the ADR, not as license to change course. Each ADR states the context, the decision, the alternatives that were rejected, and the consequences.

> The source of truth for these decisions is `AGENTS.md` (§ Frozen decisions). This skill transcribes it so you can load it cheaply; if the two ever disagree, `AGENTS.md` wins and this skill should be corrected.
