# Architecture Decision Records

An **Architecture Decision Record (ADR)** captures one architectural decision: its context, the decision itself, the alternatives that were rejected, and the consequences that follow. The point of keeping decisions as ADRs is that a decision becomes an **individually citable artifact** — a contributor or an agent can be pointed at `docs/adr/ADR-004-one-engine-two-drivers.md` instead of at a row in a table buried inside a long design document.

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
- **`ADR-001-*.md` … `ADR-015-*.md`** — the frozen decisions of the *system* architecture, one file per decision, cited as `ADR-1` … `ADR-15`.
- **`ADR-C01-*.md` … `ADR-C20-*.md`** — the frozen decisions of the *code* architecture, one file per decision, cited as `ADR-C1` … `ADR-C20`.

Each file is self-contained: it should be understandable and actionable on its own, without first reading `AGENTS.md` or the architecture documents. If an ADR only makes sense after reading something else, it is under-specified and should be fixed.

## The decisions

Generated from each ADR's front-matter. It is generated rather than written
because a hand-maintained index of immutable documents rots silently, and a stale
index of citable decisions is worse than none — `just check` fails when it drifts.

<!-- BEGIN GENERATED ADR INDEX -->

<!-- Generated from each ADR's front-matter by `scripts/gen-adr-index.py`.
     Regenerate with `just gen-adr-index`; do not edit this table by hand. -->

| ADR | Title | Invariants | Status |
|---|---|---|---|
| [`ADR-001`](ADR-001-composable-primitives-and-an-engine.md) | Composable primitives and an engine; techniques are configuration | — | accepted |
| [`ADR-002`](ADR-002-pipeline-representation-is-a-graph-with-control-flow.md) | The pipeline representation is a graph with control flow | — | accepted |
| [`ADR-003`](ADR-003-two-faced-component-contract-local-remote.md) | Two-faced component contract (Rust trait + protobuf), Local / Remote | — | accepted |
| [`ADR-004`](ADR-004-one-engine-two-drivers.md) | One engine, serving and evaluation drivers | — | accepted |
| [`ADR-005`](ADR-005-pure-compute-data-plane.md) | Pure-compute data plane, externalized state | — | accepted |
| [`ADR-006`](ADR-006-purpose-built-grpc-config-delivery.md) | Purpose-built gRPC configuration delivery, not xDS | — | accepted |
| [`ADR-007`](ADR-007-custom-resource-is-serialization-of-the-representation.md) | Custom resource = serialization of the representation; the ConfigSource abstraction | — | accepted |
| [`ADR-008`](ADR-008-benchmark-contract-typed-by-ground-truth.md) | Benchmark contract typed by the presence of qrels and reference answers | — | accepted |
| [`ADR-009`](ADR-009-the-judge-is-a-component.md) | The LLM judge is a component of the representation | — | accepted |
| [`ADR-010`](ADR-010-deterministic-retrieval-metrics-first.md) | Deterministic retrieval metrics first, judge later | — | amended |
| [`ADR-011`](ADR-011-research-bench-before-multi-tenant-service.md) | Research bench before multi-tenant service | — | accepted |
| [`ADR-012`](ADR-012-ui-in-experiment-plane-not-data-plane.md) | UI in the experiment plane, never in the data plane | — | accepted |
| [`ADR-013`](ADR-013-native-run-store-with-export-adapters.md) | Native run store with export adapters | — | accepted |
| [`ADR-014`](ADR-014-single-front-end-graph-replay-load-bearing.md) | Single front end; graph replay is load-bearing, visual authoring is a later trajectory | — | accepted |
| [`ADR-015`](ADR-015-traceability-and-statistical-reproducibility.md) | Traceability and statistical reproducibility, not strict determinism | — | accepted |
| [`ADR-C01`](ADR-C01-multi-crate-workspace-boundaries-are-crate-boundaries.md) | Multi-crate workspace; load-bearing boundaries are crate boundaries | — | accepted |
| [`ADR-C02`](ADR-C02-three-level-pipeline-representation.md) | Three-level pipeline representation (Raw / Logical / Physical) | INV-8 | amended |
| [`ADR-C03`](ADR-C03-closed-enum-plus-open-extension-variant.md) | Closed enum of primitive nodes plus an open Extension variant | — | accepted |
| [`ADR-C04`](ADR-C04-engine-as-embeddable-library-with-explicit-context.md) | Engine as an embeddable library with an explicit EngineContext | — | accepted |
| [`ADR-C05`](ADR-C05-engine-depends-only-on-traits-components-are-leaves.md) | The engine depends only on traits; components are leaves | — | accepted |
| [`ADR-C06`](ADR-C06-identical-api-plus-conformance-suite.md) | Built-ins and third parties share one API, backed by a conformance suite | — | accepted |
| [`ADR-C07`](ADR-C07-domain-types-source-of-truth-protobuf-generated.md) | Domain types are the source of truth; protobuf is generated; round-trip tested | — | accepted |
| [`ADR-C08`](ADR-C08-async-trait-in-v0.md) | async_trait in v0 | — | accepted |
| [`ADR-C09`](ADR-C09-traces-are-the-executors-return-value.md) | Execution traces are the executor's return value | — | accepted |
| [`ADR-C10`](ADR-C10-tower-for-serving-envelope-only.md) | Tower for the serving envelope only | — | accepted |
| [`ADR-C11`](ADR-C11-wire-format-separate-and-versioned.md) | The wire format is separate and versioned | — | accepted |
| [`ADR-C12`](ADR-C12-controller-may-live-outside-the-workspace.md) | The controller may live outside the workspace | — | accepted |
| [`ADR-C13`](ADR-C13-typed-errors-in-libraries-anyhow-in-binary.md) | Typed errors (thiserror) in libraries, anyhow in the binary | — | accepted |
| [`ADR-C14`](ADR-C14-heavy-backends-feature-gated-lean-default-build.md) | Heavy backends feature-gated; lean default build | — | accepted |
| [`ADR-C15`](ADR-C15-one-binary-with-subcommands.md) | One binary with subcommands | — | accepted |
| [`ADR-C16`](ADR-C16-erased-edge-values-checked-before-execution.md) | Erased edge values, with compatibility checked before execution | INV-1, INV-2, INV-7, INV-8 | accepted |
| [`ADR-C17`](ADR-C17-embedding-role-per-call-prefixes-in-the-constructor.md) | The embedding role is a per-call parameter; prefix text is constructor configuration | INV-1, INV-7 | accepted |
| [`ADR-C18`](ADR-C18-a-pipeline-declares-its-inputs.md) | A pipeline declares its inputs; the query is an explicit edge | INV-1, INV-8, INV-9 | amended |
| [`ADR-C19`](ADR-C19-empty-collection-arguments-are-valid.md) | An empty collection argument is a valid call, not an invalid request | INV-1, INV-4, INV-7 | accepted |
| [`ADR-C20`](ADR-C20-zero-dimensional-embedding-is-not-a-valid-output.md) | A zero-dimensional embedding is not a valid Embedder output | INV-1, INV-7 | accepted |

<!-- END GENERATED ADR INDEX -->

## Front-matter

Every ADR carries YAML front-matter above its heading:

```yaml
---
id: ADR-C02
title: Three-level pipeline representation (Raw / Logical / Physical)
status: amended             # accepted | amended | superseded | proposed
invariants: [INV-8]
supersedes: []
superseded_by: null
---
```

- **`id`** carries the **padded, filename-matching** form (`ADR-C02`), because it
  is the machine key for the file. Prose still cites the unpadded ID (`ADR-C2`) —
  see *Numbering and file naming* below.
- **`title`** repeats the ADR's own heading, verbatim.
- **`status`** restates the `## Status` section in one machine-readable word. The
  section stays the source of truth; the field must agree with it.
- **`invariants`** names the invariants the ADR grounds — **only** where the ADR's
  own text names them. An empty list means *this ADR names no invariant*, never
  *no invariant applies*. Nothing here is inferred: an invented cross-reference is
  worse than a missing one, because it will be cited.
- **`supersedes`** / **`superseded_by`** record supersession, and only where an
  ADR states it. Both are empty across the current set — no decision has been
  superseded yet — and they exist so the first supersession has somewhere to go.

**Front-matter is metadata about a decision, never part of one.** Adding it edited
no Context, Decision, Alternatives rejected, Consequences or Status section, and
changed no decision. Process rule 1 forbids editing an accepted ADR's substance;
describing it is not editing it. This is the same reasoning recorded below for the
2026-09-05 rename, and it is recorded for the same purpose: so that a future reader
comparing an ADR against its git history sees metadata rather than unexplained
drift.

## Numbering and file naming

An ADR has an **ID** and a **filename**, and they are deliberately not the same
string. Confusing the two is what makes a citation fail to resolve, so both are
stated here canonically.

**The ID** is what prose cites, and what the ADR's own `#` heading carries. Its
number is **never padded**:

- System-architecture decisions use the bare prefix: `ADR-1` … `ADR-15`.
- Code-architecture decisions use the `C` prefix: `ADR-C1` … `ADR-C20`.
- New decisions (from `decision` issues) continue the appropriate sequence and are added, never inserted retroactively.

**The filename** is `ADR-<number>-<slug>.md`, where the number **is** padded — to
**three** digits in the system series, and to **two** digits after the `C` in the
code series:

| Series | ID | Filename |
|---|---|---|
| System | `ADR-4` | `ADR-004-one-engine-two-drivers.md` |
| Code | `ADR-C3` | `ADR-C03-closed-enum-plus-open-extension-variant.md` |

`<slug>` is a short lowercase kebab-case summary of the decision, written by hand
when the ADR is created. It is **not** derived from the title and need not
reproduce it: `ADR-4` is titled *"One engine, serving and evaluation drivers"* and
is slugged `one-engine-two-drivers`. The slug exists to make a directory listing
readable; **the number is what identifies the ADR**, and a reference resolves by
number alone.

`scripts/check-doc-links.py` enforces both halves of this: every ADR filename
matches the convention above, and every `ADR-<n>` reference in the repository's
Markdown resolves to exactly one file. It runs as part of `just check`.

## The 2026-09-05 rename

The project was renamed to **Ragondin**, and every crate was renamed with it:
`rag-types` became `ragondin-types`, the binary `rag` became `ragondin`, and so
on throughout. That rename was applied **mechanically to these ADRs**, which
means accepted ADRs were edited — normally forbidden by process rule 1.

It is not an exception to that rule, because **no decision changed**. Only
identifiers did: every ADR states exactly what it stated before, about the same
crates under different names. Nothing was reworded, retracted or re-argued, and
no `## Amendments` section was added, because there is nothing to amend.

Recorded here so that a future reader comparing an ADR against its git history
sees a rename rather than unexplained drift.

## Status values

- **Accepted** — the decision is in force.
- **Superseded by ADR-N** — replaced by a later ADR, which it should reference and which should reference it back.
- **Accepted (amended \<date\>)** — in force, with a factual claim in its reasoning retracted in place under process rule 2. The decision itself is unchanged; see the ADR's `## Amendments` section for what was retracted and why.
- **Proposed** — under discussion in a `decision` issue; not yet in force.
