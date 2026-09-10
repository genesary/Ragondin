---
id: ADR-C21
title: Three core crates are stable API boundaries; the engine is deliberately internal
status: accepted
invariants: [INV-1, INV-2, INV-3, INV-4, INV-7, INV-8, INV-9]
supersedes: []
superseded_by: null
---

# ADR-C21: Three core crates are stable API boundaries; the engine is deliberately internal

## Context

This workspace publishes some of its crates as promises and keeps the rest free to
change. `ragondin-types` (the domain value types), `ragondin-pipeline` (the three-level
pipeline representation) and `ragondin-contracts` (the component traits) are treated as
**stable API boundaries**: breaking their public API is a deliberate, versioned act.
`ragondin-engine` (the registry, physical planning, the executor) is treated as
**internal**: it is refactored freely and none of its internals is a promise to anyone.

That split is already load-bearing. All three `ARCHITECTURE.md` files under `core/` open by
declaring their crate stable; the engine's opens by declaring itself not. ADR-C16 reasoned
*from* the split when it placed `ValueKind` on `ragondin-pipeline`'s stable surface and
kept `NodeValue` inside the engine. The public types and enums of `ragondin-types` and
`ragondin-pipeline` are deliberately **not** `#[non_exhaustive]`, so that adding a field or
a variant *is* a breaking change — a choice that only makes sense if the crate is a
boundary. The invariant checks that CI runs are drawn along the same line.

What was missing is the argument. The split was asserted in every document that restates
it and argued in none, and the nearest ADR is not a substitute: ADR-C1 decides that the
architecture's load-bearing boundaries are routed through *crate* boundaries rather than
module boundaries, which is the mechanism. It does not say **which** crates are
load-bearing, and offering it as the ground would be the nearest-neighbour citation that
this repository has already been burned by once.

A rule with no recorded ground is the one most easily eroded, because an exception to it
cannot be argued against — there is nothing to point at. The concrete erosion this
protects against is small and plausible each time: an engine-internal type re-exported
from `ragondin-contracts` for convenience, quietly making it a promise; or a needed engine
refactor declined "because it would break the API", paying INV-1's cost where INV-1 does
not apply.

Three things decide the question — two facts about the workspace as it stands, and one
commitment it has already made.

**Nobody outside this repository names an engine item.** Every crate that depends on
`ragondin-engine` — `ragondin-server`, `ragondin-harness`, `bin/ragondin` — is a member of
this workspace, recompiled by the same `cargo build --workspace` that would break it.
Meanwhile the crates a contributor writes or mirrors depend on the core and never on the
engine: every crate under `components/` ships `ragondin-contracts` and `ragondin-types` and
no other workspace dependency, taking `ragondin-conformance` only as a dev-dependency that
reaches no consumer; `ragondin-conformance` itself, the suite every implementation must
pass, ships the same two; `ragondin-remote`, which carries the protobuf face, ships those
two plus `ragondin-proto`. This is not a coincidence to be preserved by
vigilance — it is what ADR-C5 arranged, by having the engine depend only on traits and
components be leaves.

**The audience for the core is people this repository does not compile for.** A `Local`
component author writes a crate against `ragondin-contracts` + `ragondin-types`, passes
`ragondin-conformance`, and registers at the composition root — by INV-7 and ADR-C6, the
identical path a built-in takes, with no shortcut available to either. A `Remote`
component author implements a protobuf service in any language at all, and by ADR-C7 that
wire face is generated to mirror `ragondin-types`, so the domain shape reaches them too.
This is the contribution funnel ADR-3 exists to open; a break in the core closes it
silently, in someone else's build, at a time of our choosing and not theirs.

**Some artifacts will outlive the build that produced them, and the commitment to that is
already made.** A configuration file is compiled into a `LogicalPipeline`, whose canonical
form is what INV-8 and ADR-C2 commit to hashing as a run's identity. This is the one ground
here that is prospective rather than present, and it should be read as the commitment it
is: ADR-C18 records that "content-addressed hashing is not yet implemented and no `run_id`
has ever been stored", and `core/ragondin-pipeline/src/lib.rs` says the hash **will be**
computed. That is precisely why the boundary has to exist now. A stored configuration and a
recorded `run_id` are denominated in `ragondin-pipeline`'s public shape and are read back
long after the binary that wrote them is gone; changing that shape does not break a
compile, it invalidates a record, and nothing reports it. ADR-C18 also states the timing
argument in the same breath — "this cost is at its minimum now and rises monotonically" —
which is an argument for fixing the boundary before the first `run_id` is stored, not
after.

## Decision

`ragondin-types`, `ragondin-pipeline` and `ragondin-contracts` are **stable public API
boundaries**, and `ragondin-engine` is **deliberately internal** and does not become one.
This ADR records the grounds for that split; it does not change it, and it grounds INV-1
and INV-2 as the two halves of a single choice — a crate is one or the other, and INV-2's
"refactor it freely" is only worth anything against INV-1's promise.

A crate is a stable API boundary **when a change to its public items can break something
that `cargo build --workspace` does not compile**; otherwise it is internal. In this
workspace that "something" takes exactly two forms: **code compiled elsewhere** — a
third-party `Local` component, or a `Remote` service whose face mirrors our types — and
**an artifact that outlives its build** — a stored configuration, and the `run_id` that
will be hashed from the canonical form it compiles to. Applied to a crate that does not
exist yet, the test is answerable without asking anyone: name who would be broken, and say
whether we compile them.

Applied today the test yields exactly the current lists. It puts `ragondin-contracts` and
`ragondin-types` on the stable side because outsiders implement and mirror them, and
`ragondin-pipeline` on it because stored configurations are written in its shape and the
run identity will be hashed from it. It leaves `ragondin-engine` internal because
everything the engine exposes, it exposes to this workspace: a rename there is reported by
the same compile that made it. The engine's internality is therefore a **consequence of ADR-C5**,
not a separate wish — reverse ADR-C5 and INV-2 loses its ground.

**INV-1 names the surfaces whose breakage is a versioned platform event, not everything an
outsider can depend on.** Three crates are outsider-facing and carry their own
compatibility rule instead, so they are deliberately absent from INV-1 rather than missing
from it, and the test is not complete until it says so:

- **`ragondin-conformance`** is what every component crate compiles against and asserts on,
  so a rename there breaks all of them at once. Its own `ARCHITECTURE.md` states the rule
  and scopes it — the family functions and the check-name strings are fixed, the modules
  behind them free — and records the one deliberate break, the `RolePrefixes` parameter
  ADR-C17 required. That is a compatibility promise over a test surface, which is a
  narrower thing than a platform version.
- **`ragondin-proto`** holds the protobuf face an out-of-process `Remote` component
  implements — a skeleton today, but that is the surface it exists to carry — and nothing
  this workspace compiles touches that service. Its compatibility rule is INV-9 and
  ADR-C11: the wire format is separate from the in-memory representation and versioned
  **independently**. Putting it under INV-1 would tie the two versions back together, which
  is the coupling ADR-C11 exists to prevent.
- **`ragondin-remote`** is internal by the test, despite sitting beside the wire face. It is
  the generic adapter and the domain⇄protobuf conversions, and the only crate that depends
  on it is `ragondin-engine`, behind the `remote` feature. What an outsider mirrors is the
  protobuf face above, never this crate's Rust API.

## Alternatives rejected

- **Two ADRs, one per invariant.** The tidier mapping — one invariant, one ADR — and each
  would be citable alone. Rejected because neither document would be self-contained, which
  `docs/adr/README.md` requires of every ADR: the engine's internality is argued almost
  entirely by contrast with what the stable crates promise, and a reader arriving at the
  engine ADR would have to fetch the other one to learn what "not a boundary" is the
  negation of. Under process rule 3 it would also need a second `decision` issue. The cost
  of choosing one ADR is recorded here plainly: this is one ADR grounding two numbered
  invariants, an unusual shape for the invariants→ADR mapping. It is not unprecedented —
  ADR-C2 grounds INV-8 while deciding other things, and ADR-C16 names four invariants —
  and the front-matter's `invariants` list carries both, so the index and any future
  cross-check see two rows pointing at one file rather than a gap. That list is longer than
  two, following the convention the existing ADRs set and `scripts/gen-map.py` enforces in
  both directions: it names every invariant this text speaks to, including INV-3, which
  Consequences names precisely to say this ADR does **not** ground it. The two it
  **grounds** are INV-1 and INV-2 — which is what the Decision says, and what `AGENTS.md`
  § Invariants cites this file for. Read the field as *names*, never as *grounds*.

- **Recording the rationale in `docs/code-architecture.md` instead.** That document already
  carries half of it: §5's invariant table and §8's "Status: **internal** (INV-2)".
  Rejected because it produces no individually citable artifact, which is the whole reason
  `docs/adr/` exists — one cannot be pointed at a paragraph of a long document the way one
  is pointed at a decision. It also leaves the reasoning editable in place, whereas an
  accepted ADR's decision is immutable by process rule 1. For a rule this load-bearing,
  immutability is the property wanted, not an inconvenience.

- **Leaving the split ungrounded, and recording that it is deliberately an axiom.** Honest
  and cheap, and the honest worry behind it is real: reasoning invented after the fact to
  dress up a preference is worse than an admitted axiom. Rejected on two counts. First, the
  reasoning here was not invented — it was collected: the compile-path argument is readable
  in the workspace's `Cargo.toml` files, the contribution-funnel argument is ADR-3 and
  ADR-C6, and the stored-artifact argument is INV-8 and ADR-C18. Second, and decisive, this project's governance rests
  on citable artifacts: an ungrounded rule cannot be argued with, only obeyed or ignored,
  and the ignoring is what actually happens. If it genuinely were an axiom, that would
  itself be worth an ADR saying so.

- **Citing ADR-C1 as the grounding ADR.** The nearest neighbour, and wrong. ADR-C1 decides
  that load-bearing boundaries are routed through crate boundaries; INV-1's content is
  *which* crates are load-bearing, which ADR-C1 never says. A citation that resolves is not
  a citation that is apt, and this substitution is precisely the failure this repository
  has already recorded once.

- **Treating every crate as stable.** Removes the need for a test and the risk of putting a
  crate on the wrong side. Rejected because it prices every engine refactor as a breaking
  change, and the engine is exactly where the design is still moving: physical planning
  (whose optimizer is deliberately the identity function for now), the executor's
  scheduling, `ExecutionTrace`'s shape, and `NodeValue`, which ADR-C16 placed in the engine
  so that it stays freely refactorable as kinds arrive. A promise over those would be a
  promise we would break.

- **Treating no crate as stable until a 1.0 release.** Defensible for an unpublished
  workspace at `0.0.0`, and it would cost nothing today. Rejected because the promise is
  what the contribution model is made of, and it is being relied on now rather than at
  1.0 — the conformance suite hands a third party a guarantee against the current contract,
  and the deliberate absence of `#[non_exhaustive]` on the core types is a choice that has
  no meaning unless someone is entitled to notice the break. Deferring stability would also
  defer nothing real: the discipline is easier to keep from the start than to retrofit onto
  a surface that has already drifted.

## Consequences

- **INV-1 and INV-2 have a ground, and it is one click from where they are stated.** Both
  rows in `AGENTS.md` § Invariants cite this ADR instead of recording the gap and pointing
  at the decision issue. The rule's wording is untouched; only the citation changed.

- **A contributor can classify a new crate without asking.** "Can a change here break
  something `cargo build --workspace` does not compile?" is answerable from the crate's own
  `Cargo.toml` and its intended audience. That matters more than the current list, which
  the test reproduces rather than replaces.

- **The secondary rule that looks asymmetric now has a reason.** INV-4 (the core stays
  light) covers `ragondin-types` and `ragondin-contracts` but not `ragondin-pipeline`, and
  under this ADR that is consistent rather than an oversight: those two are on an outside
  contributor's compile path and `ragondin-pipeline` is not — it is stable for the stored-
  artifact reason, not the compile-path one. It follows that if the sanctioned but
  currently undeclared `ragondin-contracts → ragondin-pipeline` edge is ever taken,
  `ragondin-pipeline` joins that compile path and INV-4's crate list should be revisited in
  the same change. That is a trigger to notice, not a rule adopted here.

- **INV-3 is deliberately left where it was.** This ADR touches `ragondin-types` and
  `ragondin-pipeline`, which is INV-3's pair, but it grounds none of INV-3: its *no
  interner* and *no global context* clauses remain argued nowhere, exactly as `AGENTS.md`
  records. Stability and value-type-ness are independent properties — an internal crate can
  hold value types and a stable crate could in principle hold a context — so nothing here
  supplies the missing argument by implication, and no one should read it as having done so.
  Closing that gap needs its own decision issue.

- **The escalation list in `docs/AGENT_WORKFLOW.md` is not INV-1's list, and should not be
  read as one.** It escalates PRs touching `ragondin-pipeline`, `ragondin-contracts` and
  `ragondin-engine`; the engine is on it for blast radius inside the workspace, not because
  it is a boundary, and `ragondin-types` is a boundary that is not on it. The two lists
  answer different questions and this ADR changes neither.

- **The `#[non_exhaustive]` stance in the core has a ground.** `ragondin-types` and
  `ragondin-pipeline` deliberately leave their public types and enums exhaustive so that an
  added field or variant *is* a breaking change. Under this ADR that is the boundary doing
  its job — making the break visible to the people we do not compile for — rather than an
  unexplained preference.

- **The engine stays cheap to change, which is what INV-2 is for.** Nothing in this ADR
  licenses moving an engine type outward for convenience: a re-export of an engine internal
  from a stable crate makes it a promise, and that is a decision, not a convenience.

- **No crate changes status, and no code changes.** The split is exactly as it was; only
  its grounds are now recorded. No entry in `docs/OPEN_QUESTIONS.md` is opened, closed or
  changed, and no frozen decision is reopened.

## Status

Accepted.
