---
id: ADR-016
title: Single front end; graph replay is load-bearing and built first; visual editing is built with it, over pipeline files that stay the source of truth
status: accepted
invariants: [INV-8, INV-9]
supersedes: [ADR-014]
superseded_by: null
---

# ADR-16: Single front end; graph replay is load-bearing and built first; visual editing is built with it, over pipeline files that stay the source of truth

## Context

ADR-14 decided three things about the front end and one about time. The three
stand: **one front end** hosts composition, benchmarking and replay; **graph
replay** — the graph in read mode with execution overlaid per node — is
load-bearing and part of the core; and **read mode comes first**, because
editing is an extension of a graph that can already be drawn. The one about time
is the sentence this ADR replaces: visual graph editing is "added later as an
additional front end over the same representation", and "YAML-first authoring is
the primary path for v0". ADR-14 rejected visual authoring in v0 on two grounds:
a text file is a better tool than a canvas for the v0 research audience, and
"rendering control flow visually is an unsolved design problem; building it first
would delay the load-bearing views".

Three things have changed or become checkable since.

**The product direction.** The repository owner has set the front end as the
main human entry point of the platform — composition included, with node editing
of the kind general-purpose node-graph tools offer — and, once the platform runs
in a cluster, as the human entry point there too. The command line, the pipeline
documents and, later, the custom resources remain the **machine** entry points.
ADR-14's first ground is a claim about one audience, and it is still true of that
audience: a researcher who scripts fifty variants wants a file. What changed is
that the platform now addresses a second audience in the same milestone as the
first, and it can do so only if the canvas and the file are the same object —
which is what the decision below requires.

**The control-flow problem is not in the tree.** `LogicalNode` in
`ragondin-pipeline` has six variants today — `Retriever`, `Fusion`, `Reranker`,
`ContextBuilder`, `Generator` and `Extension` — and no branch and no loop. Every
pipeline the platform can express is acyclic. An editor over today's
representation therefore does not meet ADR-14's second ground at all; it meets
it on the day the control-flow milestone adds `Branch` and `Loop`, and
`docs/OPEN_QUESTIONS.md` § 7 already registers that problem as deliberately
unresolved. Deferring the whole editor to that day defers the part that is
solved together with the part that is not.

**The risk ADR-14 was guarding against has a mechanical mitigation.**
`docs/system-architecture.md` § 12 lists "the UI consuming the schedule" and
answers it by distinguishing the load-bearing views from polish, with visual
editing named as the polish. The order in which the front end is built answers
the same risk more directly: the viewer is built before the editor, on runs that
already exist, and the editor's renderer *is* the viewer's; the viewer therefore
cannot wait on the editor. And the front-end milestone's exit criterion is a
test, not a judgment: a pipeline composed in the canvas and exported hashes
identically under `ragondin validate`.

What would go wrong with no decision: the front-end milestone would build the
editor against an accepted ADR that says it comes later, which `docs/adr/README.md`
process rule 4 forbids — an ADR is never overturned as a side effect of
implementation work.

The repository owner decided this on 2026-09-30, on the front-end design that
accompanies decision issue #327. #327 did not ask it; it resolves into its own
ADR so that #327 still produces exactly one (`docs/adr/README.md` process rule 3).

## Decision

**A single front end hosts composition, benchmarking and replay. Graph replay is
load-bearing, part of the core, and built first. Visual graph editing is built in
the same milestone as replay, as the same front end in write mode, and a pipeline
edited in it is a pipeline document on disk — the file stays the source of truth
and the object a user versions.**

1. **One front end, one canvas.** Replay and editing are the same component in
   two modes — read mode draws a stored run's graph with its execution overlaid,
   write mode lets the same graph be changed — never two front ends, and never a
   second rendering of the graph written for one of them.
2. **Read mode first, as a build order.** The viewer precedes the editor. No part
   of the viewer depends on the editor, so the load-bearing views ship whatever
   happens to the editor's schedule.
3. **The file is the truth.** A pipeline the editor opens is a pipeline document
   in the workspace, in the configuration format and schema version
   `ragondin-config` reads (INV-9), and saving writes that document and nothing
   else that carries the pipeline. The editor writes the file **only when the
   pipeline validates**, through the hand-maintained wire schema and never by
   serializing an in-memory type; a document the editor wrote hashes, under
   `ragondin validate`, to exactly what the canvas showed (INV-8). There is no
   second store of pipelines behind the canvas, in the workspace or anywhere else.
4. **What the canvas adds is never hashed.** Node positions and any other
   presentation state live beside the pipeline document, never inside it, and
   never enter the canonical logical form. Two layouts of one pipeline are one
   pipeline and one run identity.
5. **The editor never overwrites what it has not seen, and never silently drops
   what its rendering cannot carry.** A document changed on disk since the editor
   read it is not overwritten. A document whose text is not the editor's own
   rendering of it — comments, key order, formatting a person wrote — is not
   rewritten until the user has been told that saving from the canvas replaces
   that text with the canonical rendering, and offered a new file instead.
6. **Control flow stays open.** Rendering and editing `Branch` and `Loop` are not
   decided here. `docs/OPEN_QUESTIONS.md` § 7 remains deliberately unresolved and
   now blocks visual rendering and editing of control flow, rather than visual
   authoring as a whole; it is answered by its own decision in the control-flow
   milestone, before those nodes reach the canvas.

## Alternatives rejected

- **Keep ADR-14 as it stands: editing in a later milestone.** The ordering
  ADR-14 chose for a research-only audience. Rejected because the platform's
  front end is now its main human entry point, and a front end that can read
  every pipeline and write none sends its user back to a file for the act the
  front end exists for; and because ADR-14's control-flow ground does not apply
  to the representation that exists (see Context), so waiting would defer the
  solved part for the sake of the unsolved one.
- **The canvas as the source of truth** — pipelines held as objects in a store
  of the front end's own, exported to YAML on request. Rejected because it makes
  two truths: in a cluster, the custom resource is already the serialization of
  the pipeline (ADR-7), and a second store would compete with it; locally, a
  researcher's git history would stop being the pipeline's history. The history
  that matters — which version of a pipeline produced which run — is already in
  the run store, since every run keeps its configuration document.
- **Export only** — the canvas writes YAML, but never reads a document a person
  wrote. Rejected because it breaks the round trip that makes the file and the
  canvas one object: a pipeline edited by hand could no longer be opened in the
  canvas, and the two audiences would drift into two formats.
- **An editor before the viewer.** Faster to the most visible feature. Rejected
  because the viewer is the load-bearing view (the part of ADR-14 carried forward
  unchanged), its renderer is half the editor, and it can be proven on runs that
  already exist; building in the other order puts the load-bearing view behind
  the capability that is new.
- **Settling control-flow rendering now, so the editor is complete from the
  start.** Rejected because nothing in the representation has control flow to
  render yet, and `docs/OPEN_QUESTIONS.md` § 7 exists precisely so that it is
  answered deliberately, with the nodes in hand, never in passing.

## Consequences

- **ADR-14 is superseded in full**, and everything in it but its timing is
  carried forward here in its own words: the single front end, replay as
  load-bearing and core, read mode before write mode. Its Status names this ADR.
- **YAML-first is no longer the primary path; YAML-as-truth is the invariant.**
  A researcher who never opens the canvas loses nothing — the command line and
  the pipeline documents are unchanged and remain the scriptable, machine-facing
  contract. A user who never opens a file composes in the canvas, and what they
  produce is the same file.
- **The front-end milestone's exit criterion can state parity in both
  directions**, because this ADR makes the canvas and the document one object:
  a pipeline composed in the canvas and exported hashes identically under
  `ragondin validate`.
- **`docs/system-architecture.md` is corrected in this change** where it stated
  the old timing: § 6.5 (the authoring trajectory and the YAML-first paragraph),
  § 11.1 (visual editing out of scope), § 11.4 (the decision record's ADR-14 row
  and a row for this ADR), and § 12 (the two risks that named the
  visual-authoring milestone and YAML-first as mitigations).
- **`docs/OPEN_QUESTIONS.md` § 7 is narrowed, deliberately and by this ADR**:
  its question is unchanged, and what it blocks becomes visual rendering and
  editing of control flow. `docs/AGENT_WORKFLOW.md` § The milestone roadmap
  stops listing visual graph authoring beyond the roadmap.
- **INV-8 and INV-9 are what make § 3 checkable.** The canvas's output goes
  through the wire schema and hashes over the canonical logical form, so "the
  canvas and the file are one object" is a test — the parity test — rather than a
  promise.
- **What this ADR does not decide**: how the front end is served, what it
  consumes, what replay shows for a ranking, and which toolchain builds it — the
  four questions of #327, decided in that issue's ADR; the layout file's format
  and the editor's saving cadence, which are the implementing crates' leaf
  choices; and control-flow rendering (§ 6 above).

## Status

Accepted.
