---
id: ADR-C17
title: The embedding role is a per-call parameter; prefix text is constructor configuration
status: accepted
invariants: [INV-1, INV-7]
supersedes: []
superseded_by: null
---

# ADR-C17: The embedding role is a per-call parameter; prefix text is constructor configuration

## Context

An `Embedder` turns text into vectors. Modern retrieval embedders are **asymmetric**: E5 requires a `"query: "` / `"passage: "` prefix, BGE an instruction prefix on queries only, GTE and Instructor are similar. Embedding both sides of a corpus the same way does not fail — it silently loses several nDCG@10 points on BEIR.

That is the worst failure shape this platform can have. #33 is the M2 exit criterion — *hybrid retrieval beats dense-only on BEIR, reproducibly* — and a silent multi-point regression in the dense leg makes the comparison wrong while every test stays green. No behavioural check catches it either: the conformance suite (#17) does not know which model it is testing, and an embedder that applies the wrong prefix still returns one vector per input, of constant dimensionality, with finite components.

`EmbedParams` was defined empty for exactly this question, and `ragondin-contracts/ARCHITECTURE.md` says so: the struct exists so that the answer is a **field** rather than a change to `embed`'s arity, which would break every implementation in and out of the repository — third-party `Remote` services included, which is the contribution funnel ADR-C3 exists to protect. The struct was the hedge; this ADR is the decision.

Two facts narrow the question.

**The embedder is not a pipeline node.** `ragondin-engine`'s `ComponentFamily` is deliberately wider than `LogicalNode`'s variants: an `Embedder` and a `VectorStore` are components a dense retriever is *built from*, and `ResolvedComponent` has no variant for either. Nothing resolves one from a node, either: `resolve` matches a `Retriever`, a `Fusion` and a `Reranker`, and a `ComponentCtor` receives the node's `Params` and never the `EngineContext`, so the two families are wired into a retriever at the composition root (#23, #31) rather than looked up. `EmbedParams` is therefore never populated from a node's parameter map. In the intended design `embed` has two production call sites, both of them Rust: `ragondin-retriever-dense` (#23), which embeds a query, and the corpus-embedding path the harness and the binary drive at bench time (#29 and #31, by #23's own scope note), which #23 keeps deliberately outside the pipeline formalism so as not to touch open question 5. **Each knows its side statically, for free.** The call sites in the tree today are tests and the conformance suite, listed under Consequences.

**The prefix text is per-model and is not derivable from anything in the core.** Which side a call is about, and what string that side wants, are two different facts, with two different lifetimes: the first varies per call, the second is fixed when the component is built.

## Decision

The two facts are answered at the two levels they belong to.

**The role is a per-call parameter.** `ragondin-contracts` gains `EmbedRole { Query, Passage }`, and `EmbedParams` gains a `role` field. It is **mandatory**: `EmbedParams` loses its `Default` implementation and its constructor takes the role, so a call site cannot omit it and no default silently stands in for a decision only the caller can make. `EmbedRole` is a **closed** enum — not `#[non_exhaustive]` — because a wildcard arm in an implementation is precisely where a role added later would be mishandled without a word, which is the failure this decision exists to prevent. Adding a variant is then a deliberate, visible, breaking act on the INV-1 boundary, and that is the correct cost for it.

**The prefix text is constructor configuration, and it belongs to the component, not to the contract.** An embedder implementation receives its per-role prefixes when it is built, following the split `ragondin-contracts/ARCHITECTURE.md` already states: implementation-specific configuration goes to the constructor, and the params structs carry only what varies per call. The contract names the two roles and says nothing about how an implementation honours them, which keeps it agnostic of the model. A **symmetric** model is then not a special case at all: it is configured with no prefix on either side, and ignoring the role is prepending the empty string.

The contract states this normatively, where an implementer reads it: an `Embedder` must treat `EmbedRole` as significant unless the model it wraps is symmetric, and a caller must pass the role that is true of the text it is embedding.

## Alternatives rejected

- **The role is constructor configuration; one model registers twice** (`impl: bge_query` and `impl: bge_passage`, two entries over the same weights). It does not remove the silent failure; it moves it into the wiring — the composition root today, a node's parameters if an embedder ever reaches one — where nothing can check it: the registry resolves by family and name, so two entries of one family are indistinguishable to it, and ADR-C16's kind check never sees an embedder because an embedder is not a node. Selecting the passage-side entry for the query side compiles, runs, and quietly loses the points. It also forces **two** `Embedder`s into `ragondin-retriever-dense` (#23), a shape that issue does not anticipate; it costs a second model session over one set of weights; and it makes a run's `model_hashes` (`docs/system-architecture.md` §7.1) report two models where there is one.
- **Symmetric embedders only in M2, decide later.** This is a decision only if the M2 model is named and the asymmetric case declared out of scope. #20 deliberately refuses to name one — no model is fetched at build time, and the model path is runtime configuration — so there is nothing to pin it to. Without that, it is not a decision but a deferral that #20 and #23 would hit blind.
- **Configurable prefixes, with no role on the contract.** Considered as a way to avoid touching the boundary at all, and it is not one: a component holding both prefixes still cannot know which of them applies to the call in front of it. It answers *what text*, never *which side*, and it therefore presupposes one of the two answers above rather than replacing them.
- **Two methods on the trait — `embed_query` and `embed_passage`** (or two traits). The shape the mainstream libraries use, and therefore the first thing a contributor reopening this decision will reach for. It leaves `embed`'s arity alone, so the objection above does not apply to it; four others do. It doubles the surface every implementer owes, including a symmetric model that needs no distinction. It puts the role in a **method name**, so face 2 (#12) mirrors it as two RPCs rather than one field, and ADR-C3's `Local` / `Remote` equivalence then rests on two service shapes staying in step instead of one message. A role added later is another breaking method rather than an enum variant. And it moves the fact from a value the compiler audits at every call site to a choice of function name, which is weaker than what a mandatory field buys.
- **A free-form role — a string, or an `#[non_exhaustive]` enum.** Lets a caller and an implementation disagree on spelling or on an unrecognized variant with no compiler error, and turns a contract into a convention. The whole value of putting the role on the boundary is that the compiler audits every call site.
- **Prefix text on `EmbedParams`.** Puts model-specific prompt text on the stable contract, requires every caller to know which model is behind the trait object it holds, and inverts the constructor / per-call split. It is also outside this decision's scope by the issue's own terms.

## Consequences

- **#20 (`ragondin-embedder-onnx`) and #23 (`ragondin-retriever-dense`) are unblocked**, and #33's headline number no longer rests on an unstated assumption. That is the point of the decision.
- **The `EmbedParams` change is a deliberate breaking change on the INV-1 boundary**: a field added, `Default` removed, the constructor's arity changed. It is exactly the act the struct was created to absorb — `Embedder::embed`'s arity is untouched, so no implementation in or out of the repository breaks on the trait's shape.
- **Every existing call site must now state a role** — the conformance suite, `ragondin-engine`'s registry tests, and the contracts crate's own tests. There is no correct default to spare them, by construction.
- **The proto mirror (#12) must carry the role.** It is nearly free today, since no `.proto` file exists yet, and the cost rises once #12 lands. If face 2 does not mirror it, a `Remote` embedder is deaf to the role and the `Local` / `Remote` equivalence ADR-C3 promises fails on the one axis that fails silently.
- **The conformance suite (#17) cannot detect a wrong role, and must not pretend to.** It does not know the model, and both a symmetric and an asymmetric embedder are correct. It can require that both roles are accepted; it cannot check that either was honoured. The protection against getting this wrong is the contract's documented statement — which is why that statement is part of this decision rather than a courtesy.
- **Prefixes are configuration that changes the numbers, so they must reach run identity.** `model_hashes` (§7.1) exists so that two runs which differ in what the model was given are not content-addressed identically. #23 and #31 wire the embedder at the composition root rather than through a node's parameters, so nothing carries a prefix into run identity today. This ADR names the hole; closing it belongs to #31.
- **A node's parameter map is flat.** `query_prefix` and `passage_prefix` as top-level string keys are expressible now; an `embedder: { … }` sub-map under a retriever node would require #43, which is undecided. Not a blocker for M2, where the composition root injects the embedder.
- **INV-7 is preserved.** The role travels on the one API every component implements; it creates no path a built-in has and a third party does not.
- **No entry in `docs/OPEN_QUESTIONS.md` is opened, closed or changed.** This question was never registered there: it lived as an open-question bullet in `ragondin-contracts/ARCHITECTURE.md`, which this decision replaces.
- **#90 and #82 are untouched.** Whether an embedder may return a zero-dimensional embedding, and whether a reranker consumes the query as an explicit edge, remain open and are independent of this — the embedder is not a node, so #82's answer does not reach it.

## Status

Accepted.
