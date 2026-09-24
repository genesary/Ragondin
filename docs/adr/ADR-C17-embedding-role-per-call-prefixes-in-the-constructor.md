---
id: ADR-C17
title: The embedding role is a per-call parameter; prefix text is constructor configuration
status: amended
invariants: [INV-1, INV-7]
supersedes: []
superseded_by: null
---

# ADR-C17: The embedding role is a per-call parameter; prefix text is constructor configuration

## Context

An `Embedder` turns text into vectors. Modern retrieval embedders are **asymmetric**: E5 requires a `"query: "` / `"passage: "` prefix, BGE an instruction prefix on queries only, GTE and Instructor are similar. Embedding both sides of a corpus the same way does not fail — it silently loses several nDCG@10 points on BEIR.

That is the worst failure shape this platform can have. #33 is the M2 exit criterion — *hybrid retrieval beats dense-only on BEIR, reproducibly* — and a silent multi-point regression in the dense leg makes the comparison wrong while every test stays green. No behavioural check catches it either: the conformance suite (#17) does not know which model it is testing, and an embedder that applies the wrong prefix still returns one vector per input, of constant dimensionality, with finite components.

`EmbedParams` was defined empty for exactly this question, and `ragondin-contracts/ARCHITECTURE.md` says so: the struct exists so that the answer is a **field** rather than a change to `embed`'s arity, which would break every implementation in and out of the repository — third-party `Remote` services included, which is the contribution funnel ADR-3 exists to protect. The struct was the hedge; this ADR is the decision.

Two facts narrow the question.

**The embedder is not a pipeline node.** `ragondin-engine`'s `ComponentFamily` is deliberately wider than `LogicalNode`'s variants: an `Embedder` and a `VectorStore` are components a dense retriever is *built from*, and `ResolvedComponent` has no variant for either. Nothing resolves one from a node, either: `resolve` matches a `Retriever`, a `Fusion` and a `Reranker`, and a `ComponentCtor` receives the node's `Params` and never the `EngineContext`, so the two families are wired into a retriever at the composition root (#23, #31) rather than looked up. `EmbedParams` is therefore never populated from a node's parameter map. In the intended design `embed` has two production call sites, both of them Rust: `ragondin-retriever-dense` (#23), which embeds a query, and the corpus-embedding path the harness and the binary drive at bench time (#29 and #31, by #23's own scope note), which #23 keeps deliberately outside the pipeline formalism so as not to touch open question 5. **Each knows its side statically, for free.** The call sites in the tree today are tests and the conformance suite, listed under Consequences.

**The prefix text is per-model and is not derivable from anything in the core.** Which side a call is about, and what string that side wants, are two different facts, with two different lifetimes: the first varies per call, the second is fixed when the component is built.

## Decision

The two facts are answered at the two levels they belong to.

**The role is a per-call parameter.** `ragondin-contracts` gains `EmbedRole { Query, Passage }`, and `EmbedParams` gains a `role` field. It is **mandatory**: `EmbedParams` loses its `Default` implementation and its constructor takes the role, so a call site cannot omit it and no default silently stands in for a decision only the caller can make. `EmbedRole` is a **closed** enum — not `#[non_exhaustive]` — because a wildcard arm in an implementation is precisely where a role added later would be mishandled without a word, which is the failure this decision exists to prevent. Adding a variant is then a deliberate, visible, breaking act on the INV-1 boundary, and that is the correct cost for it.

**The prefix text is constructor configuration, and it belongs to the component, not to the contract.** An embedder implementation receives its per-role prefixes when it is built, following the split `ragondin-contracts/ARCHITECTURE.md` already states: implementation-specific configuration goes to the constructor, and the params structs carry only what varies per call. The contract names the two roles and says nothing about how an implementation honours them, which keeps it agnostic of the model. A **symmetric** model is then not a special case at all: it is configured with no prefix on either side, and ignoring the role is prepending the empty string.

**Omitting the role is impossible on both faces.** Removing `Default` settles face 1, and face 2 needs its own clause to say the same thing: per-call params ride in every request on the `Remote` face (§6.3), and proto3 gives an omitted enum field the value `0` with no notion of absence. The mirror enum therefore reserves `EMBED_ROLE_UNSPECIFIED = 0`, which is never valid, and the `Remote` adapter rejects it — and any unrecognised number, since a proto3 enum is open on the wire where the Rust one is closed — as an invalid request. Without that clause a `Remote` embedder that forgets the field embeds every query as a passage, silently, on the one axis where the two faces promise equivalence.

The contract states this normatively, where an implementer reads it: an `Embedder` must treat `EmbedRole` as significant unless the model it wraps is symmetric, and a caller must pass the role that is true of the text it is embedding. The role describes **the text**, not the model, which is why the enum has no `Symmetric` variant: a caller that had to choose one would have to know which model is behind the trait object it holds, and that is precisely what the contract exists to hide. One `EmbedParams` covers a whole batch, so a batch is embedded under exactly one role.

## Alternatives rejected

- **The role is constructor configuration; one model registers twice** (`impl: bge_query` and `impl: bge_passage`, two entries over the same weights). It does not remove the silent failure; it moves it into the wiring — the composition root today, a node's parameters if an embedder ever reaches one — where nothing can check it: the registry resolves by family and name, so two entries of one family are indistinguishable to it, and ADR-C16's kind check never sees an embedder because an embedder is not a node. Selecting the passage-side entry for the query side compiles, runs, and quietly loses the points. It also forces **two** `Embedder`s into `ragondin-retriever-dense` (#23), a shape that issue does not anticipate; it costs a second model session over one set of weights; and it makes a run's `model_hashes` (`docs/system-architecture.md` §7.1) report two models where there is one.
- **Symmetric embedders only in M2, decide later.** This is a decision only if the M2 model is named and the asymmetric case declared out of scope. #20 deliberately refuses to name one — no model is fetched at build time, and the model path is runtime configuration — so there is nothing to pin it to. Without that, it is not a decision but a deferral that #20 and #23 would hit blind.
- **Configurable prefixes, with no role on the contract.** Considered as a way to avoid touching the boundary at all, and it is not one: a component holding both prefixes still cannot know which of them applies to the call in front of it. It answers *what text*, never *which side*, and it therefore presupposes one of the two answers above rather than replacing them.
- **Two methods on the trait — `embed_query` and `embed_passage`** (or two traits). The shape the mainstream libraries use, and therefore the first thing a contributor reopening this decision will reach for. It leaves `embed`'s arity alone, so the objection above does not apply to it; four others do. It doubles the surface every implementer owes, including a symmetric model that needs no distinction. It puts the role in a **method name**, so face 2 (#12) mirrors it as two RPCs rather than one field, and ADR-3's `Local` / `Remote` equivalence then rests on two service shapes staying in step instead of one message. A role added later is another breaking method rather than an enum variant. And it moves the fact from a value the compiler audits at every call site to a choice of function name, which is weaker than what a mandatory field buys.
- **A free-form role — a string, or an `#[non_exhaustive]` enum.** Lets a caller and an implementation disagree on spelling or on an unrecognized variant with no compiler error, and turns a contract into a convention. The whole value of putting the role on the boundary is that the compiler audits every call site.
- **Prefix text on `EmbedParams`.** Puts model-specific prompt text on the stable contract, requires every caller to know which model is behind the trait object it holds, and inverts the constructor / per-call split. It is also outside this decision's scope by the issue's own terms.

## Consequences

- **#20 (`ragondin-embedder-onnx`) and #23 (`ragondin-retriever-dense`) are unblocked**, and #33's headline number no longer rests on an unstated assumption. That is the point of the decision.
- **The `EmbedParams` change is a deliberate breaking change on the INV-1 boundary**: a field added, `Default` removed, the constructor's arity changed. It is exactly the act the struct was created to absorb — `Embedder::embed`'s arity is untouched, so no implementation in or out of the repository breaks on the trait's shape.
- **Every existing call site must now state a role** — the conformance suite, `ragondin-engine`'s registry tests, and the contracts crate's own tests. There is no correct default to spare them, by construction.
- **The proto mirror (#12) must carry the role, with the reserved zero.** It is nearly free today, since no `.proto` file exists yet, and the cost rises once #12 lands. If face 2 does not mirror it, a `Remote` service never receives the role, and cannot use it for what ADR-C32 § 4 leaves it: anything that is not text — selecting a query tower or a passage tower, for instance. ADR-C32 § 4 makes the `Remote` adapter, not the service, apply the prefix before sending, and forbids a service to prefix or otherwise transform the text by the role. See the Amendments section: an earlier wording of this sentence gave a broader reason.
- **The reserved zero makes the mirror deliberately asymmetric, and needs a test the current net does not provide.** The proto enum carries three values where the Rust enum carries two, so `from_proto` is not total: `UNSPECIFIED` and any unrecognised number decode to an error, not to a variant. This does not weaken ADR-C7 — the domain types remain the source of truth and the `.proto` is hand-maintained to mirror them — but §7.2's round-trip property, `from_proto(to_proto(x)) == x`, starts from a domain value and therefore **never produces a message with the field missing**. Only a negative decode test covers it, and #12 owes one.
- **The conformance suite (#17) cannot detect a wrong role, and must not pretend to.** It does not know the model, and both a symmetric and an asymmetric embedder are correct. It can require that both roles are accepted, and — where a fixture *declares* that it configured distinct prefixes — that the two roles produce different vectors, which catches an implementation that takes the role and ignores it. What it cannot do is check that a given prefix was the right one. The protection against getting this wrong is the contract's documented statement — which is why that statement is part of this decision rather than a courtesy.
- **Prefixes are configuration that changes the numbers, so they must reach run identity.** `model_hashes` (§7.1) exists so that two runs which differ in what the model was given are not content-addressed identically. Prefixes do reach run identity: `query_prefix` and `passage_prefix` are parameters of the `dense` node, read by `embedder_of` in the composition root, so they enter the pipeline's canonical form and its hash. See the Amendments section: an earlier wording of this bullet said nothing carried a prefix into run identity, and left the hole to #31.
- **A node's parameter map is flat.** `query_prefix` and `passage_prefix` are top-level string keys of the `dense` node, and under ADR-C32 § 1 the node names its embedder by another flat key, `embedder: <name>`, so nothing here needs a nested map. See the Amendments section: an earlier wording of this bullet anticipated a sub-map that would have required #43.
- **INV-7 is preserved.** The role travels on the one API every component implements; it creates no path a built-in has and a third party does not.
- **No entry in `docs/OPEN_QUESTIONS.md` is opened, closed or changed.** This question was never registered there: it lived as an open-question bullet in `ragondin-contracts/ARCHITECTURE.md`, which this decision replaces.
- **How a `Remote` embedder is *built* is ADR-C32's, and how a `Remote` vector store is built stays undecided.** Under ADR-C32 § 1 to § 3, a `dense` node names its embedder by an ordinary implementation name, `embedder: <name>`; the composition root binds that name to an address given outside the pipeline configuration, with `--remote embedder/<name>=<uri>`, and builds the `Remote` embedder adapter inside the `dense` constructor closure it writes. The embedder is still not a node, and the engine never resolves it. ADR-C32 § 5 defers a `Remote` vector store until `VectorStore` gains an operation that makes a store's content addressable per run. See the Amendments section: an earlier wording of this bullet called both undescribed, and named the gap as one this decision leans on.
- **#90 and #82 are untouched.** Whether an embedder may return a zero-dimensional embedding, and whether a reranker consumes the query as an explicit edge, remain open and are independent of this — the embedder is not a node, so #82's answer does not reach it.

## Amendments

### 2026-09-24 — the role on face 2 no longer selects the prefix, only what is not text

**Retracted.** This ADR's Consequences, in the bullet on the proto mirror (#12), originally read, verbatim:

> If face 2 does not mirror it, a `Remote` embedder is deaf to the role and the `Local` / `Remote` equivalence ADR-3 promises fails on the one axis that fails silently.

**Why it is overtaken.** The sentence assumed that whoever holds the role on face 2 is whoever applies the prefix, which this ADR did not decide: no `Remote` embedder was reachable when it was accepted. ADR-C32 § 4 decides it. The `Remote` adapter applies the prefixes and the text on the Embed rpc is final; a service must not prefix or otherwise transform the text by the role, and may use it for anything that is not text — selecting a query tower or a passage tower, for instance. The reason still holds for what the role is now for on the wire, and is narrowed only in that the prefix, the case the sentence had in mind, no longer depends on the service at all.

**Why the decision still stands.** The Decision requires face 2 to carry the role, with `EMBED_ROLE_UNSPECIFIED = 0` reserved and refused, and ADR-C32 keeps both. Its grounds are that the role describes the text, that only the caller knows it, and that omitting it must be impossible on both faces. The Decision's own example for the reserved zero — that without it "a `Remote` embedder that forgets the field embeds every query as a passage, silently" — presumes a service that applies the prefix; under ADR-C32 § 4 the text arrives already prefixed, so that silent failure now survives only for a service that uses the role for what is not text. The Decision cannot be amended under process rule 2, so that sentence is read in this light; the reserved zero is still required for that case, and the Decision stands with it. The prefix text is still the component's constructor configuration — the `Remote` adapter being the component.

**On whose authority.** The repository owner, deciding #101 on 2026-09-23; ADR-C32's Consequences name this passage and assign its retraction to a pull request of its own. The Decision section is untouched, as process rule 2 requires.

### 2026-09-24 — prefixes already reach run identity

**Retracted.** This ADR's Consequences, in the bullet on run identity, originally read, verbatim:

> #23 and #31 wire the embedder at the composition root rather than through a node's parameters, so nothing carries a prefix into run identity today. This ADR names the hole; closing it belongs to #31.

**Why it is false.** The bench subcommand #31 asked for landed in PR #232, and #31 is closed. The composition root reads `query_prefix` and `passage_prefix` from the `dense` node's parameters, in `embedder_of`, so they are parameters of a node: they enter the pipeline's canonical form and its hash, and so the run's identity. The hole the sentence names is closed.

**Why the decision still stands.** The sentence named a gap the decision left to another issue; it was not a ground of the decision. Closing the gap removes a caveat and changes nothing about where the role and the prefix text live.

**On whose authority.** The repository owner, deciding #101 on 2026-09-23; ADR-C32's Consequences name this passage and assign its retraction to a pull request of its own. The Decision section is untouched, as process rule 2 requires.

### 2026-09-24 — the `embedder: { … }` sub-map is moot

**Retracted.** This ADR's Consequences originally read, verbatim:

> - **A node's parameter map is flat.** `query_prefix` and `passage_prefix` as top-level string keys are expressible now; an `embedder: { … }` sub-map under a retriever node would require #43, which is undecided. Not a blocker for M2, where the composition root injects the embedder.

**Why it is moot.** ADR-C22, deciding #43, kept the parameter grammar flat and asked that this pointer be revisited under process rule 2 once #43 closed; #43 is closed. ADR-C32 § 1 has a `dense` node name its embedder by one flat key, `embedder: <name>`, required, beside the flat prefix keys. No sub-map is wanted, so nothing here waits on #43.

**Why the decision still stands.** The bullet pointed at a configuration shape the decision might one day need; it was not a ground of the decision. The role stays per call and the prefix text stays constructor configuration, whatever key a node uses to name its embedder.

**On whose authority.** The repository owner, deciding #101 on 2026-09-23; ADR-C32's Consequences name this passage and assign its retraction to a pull request of its own. The Decision section is untouched, as process rule 2 requires.

### 2026-09-24 — how a `Remote` embedder is built is decided; a `Remote` vector store is not

**Retracted.** This ADR's Consequences originally read, verbatim:

> - **How a `Remote` embedder or vector store is *built* is undescribed, and this ADR does not settle it.** §6.3 resolves a component from a node's `impl:`, and §10 names a `Remote` by URL in the configuration and resolves it to a `Remote<T>` — both routes pass through a node, and an embedder is not one. The gap predates this decision, but the decision leans on it: requiring face 2 to carry the role presumes a `Remote` embedder can exist. Named here, not closed.

**Why it is overtaken.** ADR-C32, deciding #101, describes the embedder half. Under its § 1 a `dense` node names its embedder by an ordinary implementation name, in one flat key, `embedder: <name>`, whether the embedder it names is `Local` or `Remote`. Under its § 2 the composition root binds that name to an address from the deployment, with `--remote embedder/<name>=<uri>`, and the address never enters the pipeline configuration. Under its § 3 the composition root resolves the name inside the `dense` constructor closure it writes and builds the `Remote` embedder adapter over that binding's channel, with the node's prefixes. The route therefore does not need the embedder to be a node: it passes through the `dense` node that names it, and the engine never resolves an embedder. The vector-store half is not described: ADR-C32 § 5 defers a `Remote` vector store until `VectorStore` gains an operation that makes a store's content addressable per run, a decision of its own taken with #24, the Qdrant vector store, and until then `embedder` is the only non-node family `--remote` accepts. The bullet is overtaken for the embedder and stands for the vector store.

**Why the decision still stands.** The bullet named a gap the decision leaned on; it was not a ground of the decision. The Decision requires face 2 to carry the role, which presumes a `Remote` embedder can exist; ADR-C32 now decides how one is built, so the presumption rests on a decision rather than on an open question. The half that stays open does not reach the Decision: the Decision puts the role on `EmbedParams`, the per-call parameters of `embed`, so how a `Remote` vector store is built bears on neither where the role lives nor where the prefix text lives.

**On whose authority.** The repository owner, deciding #101 on 2026-09-23. ADR-C32's Consequences name three passages of this ADR for retraction and not this one, so the pull request for #278, whose scope named those three, left it out; the repository owner opened #284 to retract it under process rule 2. The Decision section is untouched, as process rule 2 requires.

## Status

Accepted (amended 2026-09-24).
