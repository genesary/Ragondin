---
id: ADR-C22
title: The parameter grammar stays flat; the two parameter enums become extensible
status: accepted
invariants: [INV-1, INV-8, INV-9]
supersedes: []
superseded_by: null
---

# ADR-C22: The parameter grammar stays flat; the two parameter enums become extensible

## Context

A component parameter has two models. `ParamValue` (`core/ragondin-pipeline/src/node.rs`) is the logical one, which enters the canonical form; `RawParamValue` (`core/ragondin-pipeline/src/raw.rs`) is the wire one, which a configuration file lands in. Both cover exactly `String | Int | Float | Bool | List`. Neither has a `Map` variant; neither has a `Null` variant. The wire type mirrors the logical one deliberately — ADR-C11 separates the wire **format** from the in-memory representation, not the **grammar**, so admitting a variant on one side alone would put the two models out of step.

The consequence, found empirically by the acceptance audit of the wire-schema work:

```yaml
params: { filters: { lang: fr, year: 2024 } }   # hard parse failure
params: { seed: null }                          # hard parse failure
```

and what the user sees is serde's untagged-enum diagnostic:

```
pipeline.nodes[0].params: data did not match any variant of untagged enum RawParamValue
  at line 7 column 9
```

It names neither the offending key nor what was expected. That lands in the one module whose own documentation says the permissive level exists precisely so that *"a user's diagnosable mistake"* does not become *"an opaque parse failure"* — so for this shape the module does the thing it was written to prevent.

The missing shape is not exotic. Restricting a corpus by language, source or date is among the most common things a real retrieval pipeline does, and a metadata filter is the ordinary way to write it.

**Why it could not simply be patched.** Three properties make a variant expensive to add here:

- Both types are on `ragondin-pipeline`'s **INV-1** stable boundary, and `core/ragondin-pipeline/ARCHITECTURE.md` records that neither is `#[non_exhaustive]`, deliberately — so an added variant breaks every exhaustive `match` in every consumer, in and out of the workspace.
- `RawParamValue` is a **versioned wire schema** (INV-9), so a change to its shape is a schema change and bumps `SchemaVersion`, as ADR-C18 did when `RawGraph` gained `inputs`.
- `ParamValue` enters the **canonical hashed form** (INV-8, ADR-C2). A nested map raises one level down the key-ordering question the top level already answers with `BTreeMap` — and the content hash itself (#10) has not landed, so a nesting no hasher has ever seen would decide part of that work by accident.

**The question underneath the question.** The first of those three costs is not a fact about the grammar; it is a **stance about the type**, and it turns out never to have been argued for `ParamValue`. `ragondin-pipeline`'s ARCHITECTURE.md states the stance once, for `LogicalNode` and `ParamValue` together, and the reason it gives belongs to `LogicalNode`: that enum is closed over primitives and open through `Extension` (ADR-C3), so a genuinely new node kind already has an additive route, and the closed enum is exactly what forces a contributor onto it. **`ParamValue` has no `Extension`.** It inherited closedness by proximity to a type whose closedness had a purpose.

ADR-C17 shows what such an argument looks like when there is one. `EmbedRole` is closed *because* a wildcard arm in an embedder implementation is precisely where a role added later would be mishandled without a word, silently costing several nDCG@10 points while every test stays green. That is a claim about specific harm a wildcard arm does at a specific call site. No equivalent claim has ever been made for a parameter value: a consumer that meets a parameter kind it does not recognise has nothing to silently get wrong — it has an unreadable value, and refusing it is the whole of the correct behaviour.

So the issue's question — *may a parameter hold a nested map?* — rests on a prior one that had to be answered first: **is `ParamValue` a closed vocabulary defended by the compiler, or an extensible carrier?**

Nothing is published. There is no release, and no `run_id` has ever been stored, because the content hash does not exist yet. The cost of settling both questions is at its minimum now and rises monotonically.

## Decision

**`ParamValue` and `RawParamValue` become `#[non_exhaustive]`.** They are **extensible carriers, not closed vocabularies**. A parameter kind is data a component reads, not a domain fact whose every handler the compiler must audit, so the reason ADR-C17 gives for keeping `EmbedRole` closed does not transfer: there is no call site at which a wildcard arm over parameter kinds silently does the wrong thing rather than refusing a value it cannot read. The stance recorded in `core/ragondin-pipeline/ARCHITECTURE.md` is reversed **for these two types and for no others**.

**No `Map` variant is added, and none is added later without a demander.** With the attribute in force, a variant added later is additive for every consumer outside the defining crate rather than breaking, so the grammar can wait for the configuration that actually needs it. The parameter grammar is therefore `String | Int | Float | Bool | List` in both models, flat, and a nested map is refused.

**Those two clauses are one decision, not two.** Flatness is affordable *because* extensibility is now in force; extensibility is worth its one breaking change *because* it removes the reason flatness had to be decided under duress. This is the issue's option 2 — stay flat — reframed: **flat for now and cheap to extend, not flat forever.** The only candidate demander known today is #101's first option, where a `Remote` component's URL would have to reach a retriever's parameter map as a nested value, and #101 is itself undecided; deciding the grammar for it now would pre-commit the shape of an answer to a different open decision.

**`Null` is refused, and unlike `Map` it is refused permanently.** A null parameter means *absent*, which the grammar already expresses by omitting the key. Admitting `Null` would put two spellings of one configuration on a boundary that is content-addressed: `seed: null` and no `seed:` at all would be the same configuration written two ways, and INV-8 requires them to hash identically — which means either a canonicalization step that deletes the key (making the variant unreachable in the canonical form and therefore pointless) or two hashes for one configuration (making run identity wrong). Two ways to write the same thing on a hashed boundary is a trap, not a convenience. The same reasoning that keeps `Int` and `Float` distinct — `k: 60` and `k: 60.0` really are two configurations — refuses `Null` for being the opposite case.

**A refused parameter value is diagnosed by key.** The rejection must name the parameter key it was written under and the grammar that was expected, and must remain a parse-level refusal. The untagged enum cannot produce that message, because the key is not in scope inside it; the diagnosis therefore belongs where the key is known — the parameter map — and naming it is a requirement of this decision, not a courtesy.

## Alternatives rejected

- **Add `Map(BTreeMap<String, …>)` to both models now** (the issue's option 1). The most expressive answer, and the one that looks cheapest while nothing has shipped — but nothing in the tree demands it. The single candidate is #101's first option, and #101 is undecided, so adding the variant now would settle by anticipation a shape that another decision owns. It also lands a nesting into the canonical hashed form before the hasher exists (#10), which would decide that work's key-ordering and separator questions one level down without argument. With `#[non_exhaustive]` in force the option loses the property that made it urgent: it stays available at additive cost the day a demander arrives. YAGNI, with the escape route now built.
- **Add `Map` to `RawParamValue` only, and have lowering reject or flatten it** (the issue's option 3). It breaks the mirroring of the two models, which is the property that makes the Raw/Logical split coherent: ADR-C11 separates the format and the version, not the vocabulary, so a wire grammar admitting a shape the logical grammar cannot hold is a divergence rather than a permissiveness. If lowering *rejects* the map, the option buys only a later and worse-placed error than a diagnosed parse refusal. If lowering *flattens* it, the canonical form gains keys (`filters.lang`) that no user ever wrote, so the configuration a `run_id` identifies is not the configuration on disk — and a user acquires two spellings for one thing, the exact trap `Null` is refused for.
- **Require flattened keys as the permanent answer** — `filters.lang: fr`, documented as the way to write a nested value. This is the issue's option 2 read as *flat forever*, and it is rejected in that reading. It pushes an encoding convention onto users and onto every component that parses params, and it invents a sub-grammar inside a string key that nothing validates and nothing canonicalizes. Flatness is adopted here as the current state of an extensible type, not as a convention users are asked to work around.
- **Keep both enums closed and add `Map` anyway, while it is cheap.** The move the three costs above seem to argue for, and it answers the wrong question. What made the change expensive was the closedness, not the grammar; removing the closedness removes the expense for `Map` *and* for every parameter kind anyone proposes after it, at the cost of one breaking change instead of one per variant.
- **Make `ParamValue` a `serde_json::Value`.** Recurrently attractive and already refused in `node.rs`: that type's float and map ordering are not canonical, so it would undermine the content hash INV-8 requires. `#[non_exhaustive]` gives the extensibility that makes `Value` tempting without giving up the canonical ordering that makes it unusable here.
- **Add `Null` for symmetry with the wire formats that have one.** YAML and JSON both have a null, so the wire type could carry it and lowering could reject it. Rejected: the parse succeeding and the validation failing is a worse diagnosis than the parse failing with the reason, and it would leave `RawParamValue` holding a variant whose only purpose is to be refused. What a user needs is the sentence *omit the key*, delivered at the earliest point that can deliver it.
- **Amend AGENTS.md's INV-1 sign in this PR, alongside the ADR.** The tempting reading of "never change a rule silently": land the decision and the documentation of the decision together. Rejected — the argument is in Consequences below. In short: the sentence is a statement of fact about the code, it is still true today, and editing it before the attribute exists would make it false in the *permissive* direction for the whole window between the two PRs.

## Consequences

- **The decision is settled and the implementation is a separate PR.** This ADR changes no code. What follows is what the implementation issue owes, stated so that it can be checked mechanically rather than judged.

- **The implementation adds `#[non_exhaustive]` to `ParamValue` and to `RawParamValue`, and changes nothing else about either enum.** Same five variants; **same declaration order**, which is load-bearing for the untagged derive (`50` must read as `Int`, not `Float`); same derives.

- **That attribute is itself a breaking change on the INV-1 boundary, sanctioned here and nowhere else.** An exhaustive `match` in a consumer outside the defining crate stops compiling when it lands — which is why `ragondin-pipeline`'s ARCHITECTURE.md says to revisit the stance before the first published version rather than after. There is no published version, so the cost is one wildcard arm per external match site and nothing else.

- **The in-tree cost of that break is expected to be zero, and the PR must verify it rather than assume it.** The audit behind this ADR found exactly one `match` on either type outside `ragondin-pipeline` — `engine/ragondin-engine/src/execute.rs`, reading `top_k` — and it already ends in a `_` arm. `core/ragondin-pipeline/tests/public_api.rs` and `tests/raw_schema.rs` are separate crates and so are subject to the attribute, but they only *construct* variants, which stays legal: `#[non_exhaustive]` on an enum constrains matching, not the construction of its variants.

- **`SchemaVersion` is not bumped.** No variant is added or removed and no field changes, so the wire schema's *shape* is untouched and INV-9's bump does not fire. `#[non_exhaustive]` is a Rust API property; it is not part of what a configuration file says. An implementer reaching for a version bump out of caution should not: a bump every configuration must then carry buys nothing here.

- **Adding `#[non_exhaustive]` to an untagged enum has no `serde` effect** — neither on what parses, nor on which variant a value reads as, nor on the error a rejection produces. Verified two ways rather than asserted. In `serde_derive` 1.0.229, the container flag the attribute sets (`internals/attr.rs`, which reads the bare `#[non_exhaustive]` path, not only `#[serde(non_exhaustive)]`) is consulted at exactly one place in the whole crate — `ser.rs`, guarded by `cattrs.remote().is_some()` — so it is inert for any derive that is not a `#[serde(remote = "…")]` mirror, which neither of these is. Empirically, two enums identical but for the attribute, over the scalar, list, nested-list, full-width-`i64` and `1e300` cases plus the `null` and `{lang: fr}` rejections, produce byte-identical output, the same chosen variant, and the same error text. The derive is generated inside the defining crate, where the attribute has no meaning; the constraint it imposes is on downstream `match`, which serde never writes.

- **AGENTS.md's INV-1 row must be amended, and that edit belongs to the implementation PR, not to this one.** The row's *sign in a diff* column ends: *"None of these types is `#[non_exhaustive]`, so an added field **is** the breaking change."* That is a claim about the code, and it stays true until the attribute lands. Editing it here would describe what the code will do once another issue lands — the habit AGENTS.md names in *What you write about the code is checked against the code* — and it would fail in the worse direction: for the whole window between the two PRs the file would tell a reviewer that an added `ParamValue` variant is additive while it is still breaking, licensing exactly the silent break INV-1 exists to prevent. A rule document that is wrong permissively is worse than one that is wrong restrictively. The edit therefore lands **with** the attribute, in the same diff, so the sentence is never false in either direction.

- **The amended sentence should state the criterion, not the census.** The sentence's defect after the change is not that it names the wrong set of types; it is that it *names a set at all* — an inventory of the code embedded in a rule document, which no gate can check and which rots exactly as the hand-maintained ADR index would. A named exception (*"…except `ParamValue` and `RawParamValue`"*) restores truth today and starts a second inventory to keep in step tomorrow. The recommended replacement states the rule that generates the sign: **a type here carries no `#[non_exhaustive]` unless an ADR says so, and where it carries none an added field or variant is itself the breaking change; where it carries the attribute, the break is a change to what already exists.** That stays true whatever the set becomes, and it still tells a reviewer what to look for — read the type, not the list. This ADR states the consequence for the wording; the exact sentence is the implementation PR's to write.

- **INV-1 is not narrowed, and no exception to it is created.** The invariant's Rule column is untouched: breaking these crates' public API remains a deliberate, versioned act. Only the *sign* changes, and AGENTS.md already says a sign is "a symptom, not a definition" and that "the rule column is what binds". What this decision changes is which edits are breaking for two types, not whether a break may be silent. The grounds for INV-1 itself remain unwritten; #72 owns that, and nothing here presupposes its answer.

- **The stance recorded in `core/ragondin-pipeline/ARCHITECTURE.md` is corrected in *this* PR, unlike AGENTS.md's sentence, and the asymmetry is deliberate.** That paragraph asserts an *intent* — the enums are closed "deliberately", to be revisited at the first published version — and the intent is what changed today. Leaving it would leave the document an implementer is required to read before modifying the crate asserting a stance the decision has just reversed. It is amended to say what is true now: the stance for these two types is extensible, the attribute is not on them yet, and this ADR names the work that puts it there. AGENTS.md's sentence, by contrast, asserts only a fact, and that fact is still true.

- **The diagnostic work is a requirement with a free mechanism.** The implementation must make a refused parameter value name its key and the expected grammar, and must keep the refusal at parse time. Where that lives is the PR's choice — the key is in scope in the parameter map's deserialization (`RawNode::params`), never inside `RawParamValue`'s — subject to the two things this ADR fixes: the refusal stays a parse error, and variant order is preserved. A test must pin that the message names the key; a message asserted only in prose is the failure mode this repository has already had.

- **`Null`'s refusal needs its own message, not merely its own behaviour.** It parses no differently after this decision than before; what changes is that the user is told *a null parameter means absent — omit the key*. Non-finite floats are a separate and already-settled boundary (`.inf` and `.nan` parse at the wire level and are refused by lowering) and are untouched here.

- **Everything else on this boundary stays closed.** `LogicalNode`, `ValueKind`, `PortSpec`, `ValidationError` and `SchemaVersionPeekError` in `ragondin-pipeline`; `EmbedRole` in `ragondin-contracts`, whose closedness ADR-C17 argues on its own terms; the types in `ragondin-types`. Two types change stance. This is not a crate-wide policy and must not be read as one — `LogicalNode`'s closedness in particular is what pushes a new node kind onto `Extension` (ADR-C3), and it would be lost by generalizing this decision.

- **INV-8 is preserved by construction.** The canonical form keeps exactly one spelling per configuration: no null beside an omitted key, and no nesting whose key order or separator scheme the hasher would have to invent. #10 canonicalizes the grammar `node.rs` already documents — the float contract, `-0.0` normalized, `Int` and `Float` distinct — over a value tree one level deep.

- **This ADR does not decide #101.** It removes one of that decision's stated costs rather than answering it: option 1 there is blocked on a nested value reaching a retriever's params, and after this decision that variant is additive whenever #101 calls for it, instead of a break to be weighed against option 1's other merits.

- **ADR-C17 needs no amendment.** Its Consequences note that *"an `embedder: { … }` sub-map under a retriever node would require #43, which is undecided"*. #43 is decided by this ADR, and the answer is that no sub-map is expressible today — so the bullet's substance is unchanged and process rule 2 does not apply. Recorded here so that nobody opens an amendment PR against an accepted ADR that does not need one.

- **This ADR names three invariants and grounds two.** It grounds **INV-1**, by fixing what a breaking change *is* for these two types and by requiring the sign that describes it to be corrected in step; and **INV-8**, by refusing both shapes — a null, and a flattened key invented by lowering — that would give one configuration two spellings on a content-addressed boundary. **INV-9** is named but not grounded: that the two models mirror each other is ADR-C11's decision, and this one obeys it rather than establishing it.

- No entry in `docs/OPEN_QUESTIONS.md` is opened, closed, or changed.

## Status

Accepted.
