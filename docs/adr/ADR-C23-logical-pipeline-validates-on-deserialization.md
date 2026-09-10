---
id: ADR-C23
title: A LogicalPipeline validates on the way in
status: accepted
invariants: [INV-1, INV-3, INV-8, INV-9]
supersedes: []
superseded_by: null
---

# ADR-C23: A LogicalPipeline validates on the way in

## Context

`LogicalPipeline` is the validated, canonical level of the three-level
representation (ADR-C2): the form that *names* implementations without
resolving them, and the form the content hash is computed over. Its fields are
private, its constructor is `pub(crate)`, and
`ragondin_pipeline::validate::validate` is what establishes the properties its
name promises — unique node ids, exactly one declared input (ADR-C18), no
declaration claiming a node's id, no `inputs` entry that resolves to nothing,
no cycle in the data-flow graph, and every edge's value kinds lining up
(ADR-C16).

It also derives `Deserialize`, publicly, with no validating hook. That derive
re-runs none of those checks, so in safe code any crate that can name the type
can construct a `LogicalPipeline` holding a two-cycle, a duplicated id, or no
declared input at all. The gap was **documented rather than hidden**. Before
this decision, `pipeline.rs`'s module documentation said `validate` "is the
only thing that establishes them" and then conceded in the next sentence that
the type is obtainable another way, and `LogicalPipeline::new`'s own rustdoc
stated plainly that "the public `Deserialize` derive produces one and checks
nothing". Both quotations are from the wording the PR that accepted this ADR
replaced. The type's promise was therefore true by convention about how a
value was obtained, not by construction.

Three facts make that more than a tidiness complaint.

**INV-8 hashes this value.** The content-addressed hash still to be written
(open #10) is computed over
the canonical `LogicalPipeline` and over nothing else — that is the whole of
ADR-C2's identity story, and `pipeline.rs`'s canonicalization contract is
written to make it true. A hash over a value that was never checked is a stable,
citable identifier for a configuration that could not legitimately exist: a
`run_id` for a pipeline `validate` would have refused. Reproducibility does not
survive that, because what the identifier identifies is no longer a thing the
system can be asked to run again.

**Two shipped layers already read the unvalidated shape as authoritative.**
Physical planning builds its set of declared inputs from `logical.inputs()`
(`plan_physical`, in `engine/ragondin-engine/src/plan.rs`) and copies it into
the plan's `inputs`; the executor seeds its value table from the plan's declared
inputs (the seeding loop in `Engine::execute`, `engine/ragondin-engine/src/execute.rs`). ADR-C18 introduced `inputs` and
gave both layers a reason to trust it. Neither layer re-derives the arity rule,
and neither should: ADR-C16 places the structural checks at validation and
keeps planning's kind check as a *backstop* for the one thing validation cannot
know, an `Extension` node's kinds. The backstop is a kinds check, not a second
structural validation.

**The door is currently in deliberate use as a test affordance.**
`ragondin-engine`'s two `forged` test helpers (one in `plan.rs`, one in `execute.rs`)
deserialize hand-written JSON precisely because `validate` would refuse it, and
that is how planning's and the executor's second-layer behaviour is exercised
at all. `PlanError::KindMismatch`'s rustdoc names the shape in as many words:
"a `LogicalPipeline` deserialized straight from a store or a wire is the shape
that gets here". So the hole has a purpose. What was never settled is whether
it is *also* an accepted production shape.

One more fact decides the cost, and it is the opposite of what #116 assumed.
That issue prices this option as expensive on the grounds that "`validate`
currently consumes a `RawPipeline`; the checks would have to be reachable from
a logical-shaped input too." Read against the code, `validate`
(`core/ragondin-pipeline/src/validate.rs`) does two separable things:

- **lowering**, which is the only Raw-shaped work — reading `raw.pipeline.inputs`
  and passing `raw.pipeline.nodes` through `lower_node`, which carries the
  non-finite-float rejection and the `-0.0` normalization inside
  `lower_param_value`;
- **the structural checks**, everything in `validate` after lowering, which
  touch `raw` nowhere. They operate on a `Vec<NodeId>` and a `Vec<LogicalNode>`
  and nothing else: the node sort, the unique-id index, input arity and the
  input/node-id collision, dangling inputs, `find_cycle` (declared
  `fn find_cycle(nodes: &[LogicalNode], index: &HashMap<NodeId, usize>)`), and
  `check_kinds` (declared over `&[LogicalNode]`).

The second half is already a function of the logical form. Extracting it is a
pure refactor, and the option's headline cost is mostly imaginary.

## Decision

**A `LogicalPipeline` validates on the way in.** Its `Deserialize`
implementation routes through the same structural checks `validate` runs, so
that no `LogicalPipeline` a **default build** can hold — here or in any crate
that can name the type — is one those checks would refuse. The type's promise
stops being a convention about provenance and becomes a property of the type.
The two doors to an unchecked value that remain are deliberate, are named in
point 2 below and in the Consequences, and neither is reachable outside test
configuration.

`Serialize` is untouched. Only the reading side is at issue: writing out a
value that satisfies the invariants has never been able to produce one that
does not.

Two requirements bind the implementation. They are part of this decision, not
notes on it.

1. **The deserializing path canonicalizes before it checks.** It sorts the node
   list by `NodeId` exactly as `validate` does, and for
   both of that sort's reasons: it *is* the canonicalization INV-8 rests on, so
   a path that skipped it would let two documents listing the same nodes in
   different orders produce two `LogicalPipeline`s and two hashes; and it makes
   which of two faults a malformed document reports independent of the order
   the document happened to list its nodes in. A `try_from` that checks without
   sorting recreates precisely the bug the sort exists to prevent, and does so
   silently, because every check would still pass.
2. **The unvalidated door is not removed; it is named, gated, and it
   canonicalizes.** `ragondin-pipeline` grows a constructor that skips the
   checks — `forge_unvalidated`, behind
   `#[cfg(any(test, feature = "test-util"))]` — and the engine's `forged`
   helpers are rebuilt on it. It **sorts**, exactly as the checking path does:
   point 1's reasoning is about *every* path to a `LogicalPipeline`, not only
   the derived one, and a named permanent door that skips canonicalization is
   worse than an accidental one, not better. What it hands back is therefore a
   canonical value that was never checked — never a value that is neither. The
   tests that exercise the second layer keep their subject; what changes is
   that reaching an unchecked value becomes a deliberate, greppable act that a
   default build
   cannot perform, rather than a side effect of calling `serde_json::from_str`.

## Alternatives rejected

- **Remove `Deserialize` and give the tests an honestly-named constructor
  (#116's option B).** Cheapest by a wide margin, and it matches how the derive
  is actually used today: the only callers in the workspace are the two
  `forged` helpers and one round-trip test. It was rejected on a single point, and it is the point that
  decided this ADR. **Option B is correct only under one answer to a question
  nobody has answered yet.** #28 (the run store) has not decided whether a run
  persists the raw configuration or the logical form; ADR-C2 calls the logical
  level "serializable" and ADR-13 puts a native run store in the architecture,
  so persisting the logical form is a live possibility rather than a hypothesis
  invented here. If #28 lands on the logical form, B has to be undone and
  replaced by this decision — a rework, on an INV-1 boundary, to restore a
  derive that was removed for tidiness. Option A is correct under **both**
  answers: it makes the read path safe whether or not anything ends up reading
  it, and it constrains #28 not at all. Buying the cheaper option here means
  buying a coin flip on someone else's undecided question.

- **Keep the derive and document the hole, making the downstream checks
  normative (#116's option C).** Nothing changes and the reasoning stops being
  implicit, which is genuinely worth something. Rejected on two counts. It
  leaves INV-8 hashing values that were never validated, which is the defect
  and not a framing of it. And it re-tasks `plan_physical`: promoting planning
  from backstop to normative layer means it owes a *full* structural check —
  arity, collisions, dangling inputs, cycles — where today it deliberately owes
  only a kinds check and deliberately skips an edge it cannot resolve rather
  than refusing it — `plan.rs`'s test named for exactly that, "a dangling input
  is left for the executor to report". ADR-C16 calls a mismatch reaching
  execution "a defect in one of the
  two layers above, not the expected path"; option C would rewrite that
  reasoning as a side effect of an implementation decision, which
  `docs/adr/README.md` allows only through a superseding ADR.

- **A second, logical-shaped wire type that must be converted before use.**
  This is `RawPipeline`, which already exists and is exactly what INV-9
  requires for anything arriving over a wire. Inventing a third level between
  raw and logical would duplicate the wire schema with none of its versioning.

- **Check at each use site instead.** Every consumer of a `LogicalPipeline`
  re-establishes what it needs. Rejected as the same mistake option C makes,
  distributed: N copies of a check to keep in agreement, each free to drift,
  and the hash site (#10) still has no layer above it to trust.

- **Validate in `Deserialize` *and* drop the second layer at planning.** The
  layer would look redundant once the first one cannot be bypassed. It is not:
  ADR-C16 built it for `Extension` kinds, which are unknowable without a
  registry and therefore unknowable to `ragondin-pipeline` (INV-3). Removing it
  would reopen ADR-C16.

## Consequences

- **The type's promise becomes unconditional.** Every `LogicalPipeline` a
  default build can hold has passed the structural checks, so the hash #10 will
  compute is over a validated value and INV-8's identifier identifies a
  configuration that could
  actually run. This is the invariant the decision **grounds**; INV-1, INV-3
  and INV-9 below are constraints it respects and speaks to, not ones it
  establishes.

- **This is a breaking change on `ragondin-pipeline`'s stable surface (INV-1),
  sanctioned here and nowhere else.** The derive is public API on an INV-1
  boundary, and a document that deserialized before this decision may now fail
  to. No Rust caller's *types* change — `Deserialize` is still implemented —
  but the set of accepted documents shrinks, which is the same kind of break
  ADR-C18 recorded when the same derive gained a required field.

- **Deserialization becomes fallible in a new way, and the typed error is lost
  on that path.** `serde` reports through `serde::de::Error::custom`, so a
  `ValidationError` reaching a caller through `Deserialize` arrives as a
  string. That trade is already made in this crate, by `SchemaVersion`'s
  `Deserialize` for the same reason. A caller that needs the typed error takes
  the wire path — `RawPipeline` → `validate` (INV-9) — which is what it should
  be taking anyway.

- **INV-9 is unchanged, and this decision must not be read as relaxing it.**
  Making `LogicalPipeline`'s read path safe does not make it a wire format.
  Anything arriving from a file, a store or a socket still goes
  `RawPipeline` → `validate`; the hand-maintained, separately versioned wire
  schema stays the only wire schema.

- **INV-3 is what makes this possible at all.** The structural checks need no
  registry, no context and no I/O — they are a function of the value — which is
  precisely why they can live inside a `Deserialize` impl. The one check that
  *does* need a registry, an `Extension` node's kinds, stays at planning where
  ADR-C16 put it.

- **The implementation is a pure extraction plus a `TryFrom`.** `validate.rs`
  gains a private `check_structure` covering the sort and the six
  checks, verified above to touch no `Raw*` type — which `validate` calls after
  lowering and which a `#[serde(try_from = "...")]` shim calls directly. The
  construction of the `LogicalPipeline` stays with each caller, so the extracted function is
  `fn check_structure(inputs: &[NodeId], nodes: &mut [LogicalNode]) ->
  Result<(), ValidationError>`: it sorts in place and reports, and does not
  build the pipeline. Lowering-only variants of
  `ValidationError` (`UnknownComponent`, `NonFiniteParam`) are unreachable from
  the second path; whether that path reports through `ValidationError` anyway
  or through a narrower type is an implementation choice, not a decision, since
  `serde` erases it to a string either way.

- **The node sort is binding on the deserializing path** (Decision, point 1),
  and needs a test that would fail without it: two documents listing the same
  nodes in different orders must produce equal `LogicalPipeline`s. Nothing
  existing would catch its absence: every canonicalization test enters through
  `validate`, and the one fixture that does reach the derive
  (in `validate.rs`'s tests) holds nodes that are **already** in `NodeId` order, so a
  `try_from` that never sorted would pass the entire current suite in silence.

- **The existing round-trip test survives unchanged and gains meaning.**
  `a_logical_pipeline_serde_round_trip_changes_nothing` (`validate.rs`'s tests)
  serializes a validated pipeline and reads it back; under this decision it
  also pins that the new path accepts everything `validate` accepts, which is
  the half of the change a rejection test cannot cover.

- **`forge_unvalidated` is a real, if narrow, addition to a stable boundary.**
  It is gated behind `#[cfg(any(test, feature = "test-util"))]`, so it is
  absent from a default build and from the crate's ordinary public surface;
  `ragondin-engine` enables the feature on its **dev**-dependency, so the door
  is compiled only when that crate's tests are. It **sorts** (Decision, point
  2), which costs nothing and buys the rule: every engine fixture is already
  written in `NodeId` order, so the sort is a no-op on all of them — that they
  are already ordered is what makes sorting free, not a reason to omit it — and
  the fixture-authoring instruction both `forged` doc comments carry can be
  retired with it. What a value it produces falls outside is the **structural**
  guarantee above, and only that; it is still canonical, so INV-8's form
  survives the test door. That is the point of naming it.

- **`LogicalPipeline::new` is the second, narrower door, and stays as it is.**
  It is `pub(crate)`, and its only caller that hands it unchecked arguments is
  `pipeline.rs`'s own `#[cfg(test)]` fixture — which builds
  `["b", "a", "b"]`, a declaration `validate` refuses twice over, precisely to
  pin that declared inputs are never sorted or deduplicated (ADR-C18). That
  test must keep working, so `new` is not made to check. It is out of reach of
  any other crate, which is what keeps it consistent with the Decision.

- **#10 (the content hash) gains an answer**: a hashed value is necessarily a
  validated one, so the hash needs no defensive check of its own.

- **#28 (the run store) is left free.** It may persist the raw configuration or
  the logical form; this decision is compatible with either and settles
  neither.

- **Some prose in the workspace documents the hole and becomes false when the
  implementation lands.** It is listed exhaustively here so the implementation
  issue (#179) inherits the list rather than rediscovering it:

  - `engine/ragondin-engine/src/error.rs` — `PlanError::KindMismatch`'s
    rustdoc, *"a `LogicalPipeline` deserialized straight from a store or a wire
    is the shape that gets here"*;
  - `engine/ragondin-engine/src/error.rs` — the `ExecError` **enum's
    own** doc comment, a different item making the same claim: *"a
    `LogicalPipeline` deserialized straight from a store or a wire is the shape
    that can"*;
  - `engine/ragondin-engine/src/plan.rs` — the `forged` test helper's doc comment,
    including its instruction that a fixture be written in `NodeId` order,
    which a sorting door retires;
  - `engine/ragondin-engine/src/execute.rs` — the second `forged` helper's doc
    comment, same two points;
  - `engine/ragondin-engine/Cargo.toml` — the comment on the `serde_json`
    dev-dependency, *"no pipeline built the legitimate way can reach it"*;
  - `core/ragondin-pipeline/tests/public_api.rs`, the ADR-C18 arity test — *"Reached through
    `validate`, the only door to a `LogicalPipeline` there is"*;
  - the inline comments on the forged fixtures that explain why `validate`
    refuses them (`plan.rs`, `execute.rs`).

  Two more do not become false but shift meaning once the derive checks, and
  the implementer should read them deliberately rather than skip them:
  `plan_physical`'s rustdoc in `plan.rs` (*"the only way to hold one with a dangling input is to
  have bypassed `validate`"*) and `check_kinds`'s rustdoc in the same file (*"only for a
  `LogicalPipeline` that did not come through `validate`"*) — both stay true,
  but "bypassed" now names the test door rather than an open one.

  `core/ragondin-pipeline/src/pipeline.rs` is updated by this ADR's own PR,
  marked as decided-but-not-yet-implemented so it claims no guarantee the code
  does not yet give.

- **`ragondin-engine`'s second layer stops being reachable in a default
  build.** After the implementation, `PlanError::KindMismatch` and the
  executor's structural errors are reachable only through the test door — which
  is the correct state for a backstop, and exactly what ADR-C16 says a
  mismatch reaching a lower layer means. They stay, and stay tested.

- No entry in `docs/OPEN_QUESTIONS.md` is opened, closed, or changed. In
  particular this decision says nothing about open question 3
  (`PhysicalPipeline` serializability): it concerns the logical level only.

## Status

Accepted.
