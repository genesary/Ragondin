---
id: ADR-C39
title: A run records the workspace pipeline name it was launched as, and for a prefix run its parent, in a write-once launch record outside its identity, stamped only by the composition root; the record and the current hash matches are two independent facts, and a Pipeline matrix cell is filled from the current content alone
status: accepted
invariants: [INV-1, INV-8, INV-9]
supersedes: []
superseded_by: null
---

# ADR-C39: A run records the workspace pipeline name it was launched as, and for a prefix run its parent, in a write-once launch record outside its identity, stamped only by the composition root; the record and the current hash matches are two independent facts, and a Pipeline matrix cell is filled from the current content alone

## Context

A stored run keeps the canonical hash of the pipeline it ran
(`RunInputs::pipeline`) and the configuration document's text, verbatim. It
does not keep the **name** of the workspace pipeline it was launched from. Three
screens of the front end need that link: Runs groups runs by pipeline, the
Pipeline matrix counts a pipeline's runs, and Compare applies the pairings kept
for a pair of pipelines.

### What the code does today

`ragondin-api` finds a run's pipeline by content, in its `lineage` module:
`pipelines_by_hash` indexes every workspace document by its canonical hash, and
`pipeline_of` names a run only when exactly one current document has its hash.
The module and the crate's `ARCHITECTURE.md` both labelled this **interim**,
pending this decision, until this change. The rule reads the *current* files,
so its answer changes whenever the files change:

1. **Edit.** Saving a document changes its hash, and all its earlier runs lose
   its name, and with it the manual pairings kept under the pipeline's name.
   The editor (#355, #356) is designed to save whenever the document
   validates, so this will happen on every change.
2. **Fork.** "Fork this run" (#356) is designed to write the run's
   configuration document to a new file. Until that file is edited, two
   documents share one hash, and the run loses its name in both places.
3. **Prefix runs.** A prefix run's document is the truncated pipeline. Its hash
   matches no file, so the hash alone cannot place it in its parent pipeline's
   group.
4. **Rename and delete**, by hand, since `PipelineSource` has neither
   operation: the runs become orphans.

### What is known at launch, and dropped

The `Submission` the API hands to the launcher already carries `pipeline_name`
and `up_to`. The only part of a prefix run's parent it lacks is the parent's
canonical hash, which #357 adds. The run would receive none of it: under #349's
design the worker files the `Run` that `Launcher::execute` returns, and today
`execute` is provisional and uncalled. `ragondin bench` knows the path of the
configuration file it read, and drops it too.

### Precedents in the store

`Run::bindings` (ADR-C32 § 2) is provenance outside identity: it is not in
`RunInputs`, the run id does not digest it, and it is kept in a file of its own
that a run stored before it existed does without. It is filled by the
composition root, not the harness. #376 decides the same shape for the run's
start time: an optional file outside identity.

### Facts that limit every option

- **One run, one record.** `RunStore::save` does nothing if the run is already
  stored, and under #349's design `POST /runs` refuses an announced run id that
  already exists (`409 run_exists`). A run id therefore carries the facts of
  its *first* launch. An unedited fork launched on the same benchmark is
  refused and points at the original run. This is content addressing (P4)
  working as intended.
- **Names and hashes are many-to-many over time.** One name holds several
  hashes, one per version. One hash can be held by several names: a fork, or
  two people writing the same pipeline.
- **`RawPipeline` has no name field.** That level is deliberately permissive:
  an unknown key is dropped at the parse, so a top-level `name:` key is accepted
  and discarded by design. Reading such a key outside the wire schema would
  break INV-9.

### What the documents said

The front-end design (`docs/design/2026-09-29-front-end-design.md` § 6) and
ADR-016's Alternatives rejected both state that the history of which version of
a pipeline produced which run is already in the run store, since every run
keeps its configuration document. The store holds the text of every version
that ran, but not which name each version was launched under. The issues that
needed the answer each left a piece of it open, and they disagreed: #349 wrote
no provenance field on any run, #353 changed nothing in `ragondin-experiments`,
#357 left "stored on the job only, or also copied into the run" as a leaf
choice, and #346 assumed the name was recorded in the document, where nothing
records it.

### Why this is an ADR

Decision issue #390 put the question, with five families of options, each
weighed against an edit, a fork, a rename or delete, a prefix run, `ragondin
bench` without a workspace, a run copied between machines, and identity. An
independent challenge confirmed option (b), a launch record on the run, and
amended it twelve ways. **The repository owner decided it on 2026-10-01: option
(b), narrowed, with all twelve amendments, binding.** It produces an ADR on
three grounds, which are the record's own:

- the owner reserved this decision;
- it is one rule, applied by `ragondin-api` and `ui/` across Runs, Pipeline and
  Compare;
- it adds an obligation to `RunStore`'s conformance suite that every backend
  inherits.

## Decision

**A run carries an optional launch record, outside its identity, written once
with the run. It holds the workspace pipeline name the run was launched as and,
for a prefix run, which version of that parent it was cut from and where. Only
the composition root stamps it. The record is one fact; the current documents
whose canonical hash is the run's are another; the two are never resolved into
one name, and the content tests run for every run whatever its record says. A
Pipeline matrix cell is filled from the current content alone.**

This ADR fixes the semantics. The record's field and file names below are the
decision record's working names; the final ones are fixed in
`runtime/ragondin-experiments/ARCHITECTURE.md`, and the response fields' in
`runtime/ragondin-api/ARCHITECTURE.md`. Two names are not working names:
`times.json` and `pipeline_names` are #376's, fixed there.

### 1. The record

- **Its shape is `{ name?, prefix_of?: { up_to, parent_pipeline_hash } }`.** It
  carries no launch channel, no path and no version inside the file.
- **Its fields are additive, and the reader tolerates unknown fields**, so a
  later field is added without a version bump.
- **It is outside identity.** It is not in `RunInputs` and it is not digested
  into the run id (INV-8). A run saved with the record and the same run saved
  without it have one id.
- **It is written once, by `save`, in the same atomic step as the rest of the
  run.** An absent record reads back as none, and every run stored so far stays
  complete.
- **The first record wins.** A second `save` of a stored id leaves the first
  record, as #376 decides it leaves the first times.
- **It is stored apart from the times**, as a second file of its own: the
  times in #376's `times.json`, the record in what the decision record calls
  `provenance.json`. The future concurrency degree
  (`docs/design/2026-09-29-front-end-design.md` § 7) becomes a field of this
  record when it is written, not a third file.

### 2. What `name` means

`name` is the workspace pipeline name the run was launched as. A record with
`prefix_of` names the **parent** in `name`: the run is a prefix of `name` at
`parent_pipeline_hash`, cut at `up_to`. Such a run is **never** an earlier
version of `name`.

### 3. Who stamps it

**Only the composition root, `bin/ragondin`**, as it stamps the bindings, and
as #376 decides it stamps the times. The harness assembles the run with no
record. `ragondin-api` never stamps one.

- **`ragondin bench`** records `name` only when the configuration file is
  `<W>/pipelines/<name>.yaml` **and** the store is `<W>/runs`, both
  canonicalized to the same `W`. Otherwise it records no name. It never writes a
  path.
- **The launcher**, on the shared preparation and execution path (#353),
  stamps the record from the `Submission`: `pipeline_name` gives `name`;
  `up_to` and the parent's canonical hash give `prefix_of`.

### 4. Two independent facts

The API exposes two facts about a run's pipeline, never one resolved name:

- **`launched_as`**: the record, which may be null;
- **`pipeline_names`**: #376's list of the current workspace documents whose
  canonical hash is the run's.

There is no field naming which of the two a name came from, and no resolved
name. The Runs screen groups a run by `launched_as.name` when present, otherwise
by `pipeline_names`, and shows the other fact as a secondary label.

### 5. The record never short-cuts the content tests

Because the first record wins, a run's record may name another pipeline than one
it also belongs to. Hash matching and the structural prefix test therefore run
for **every** run, against every current document, and not only for runs
without a record.

### 6. Where a run counts in the Pipeline matrix

For a workspace document *N* whose current canonical hash is *H*, a **cell** is
filled only from two sources:

- **runs of the current canonical form**: the run's hash equals *H*, whatever
  name it was recorded under, or none;
- **prefixes of the current form**: the record's `parent_pipeline_hash` equals
  *H*, or, failing that, #357's structural prefix test against *N*'s current
  document says so (§ 5: it runs for every run).

A run recorded as *N* whose content has since changed — its hash is not *H*, and
it is not a prefix of the current form — goes in the **feeding-runs list**,
never in a cell. A cell whose only run is such a run reads **"not run on this
version"**, with that run linked.

### 7. An earlier-content run is stated, not guessed

It is worded as a fact, **"launched as *N*; content since changed"**, and shown
with its parameter difference against *N*'s current document, through the
existing configuration diff of `compare`. No heuristic decides whether it "is"
an earlier version.

### 8. `ragondin bench` on a run already stored

`bench` reports a run already stored instead of saying nothing: **"already
stored, launched as *X*; this execution was not kept"**. Once the shared
preparation knows the identity before running, it refuses instead, to match the
UI's `409 run_exists` (#349).

### 9. Why this is not a name-and-version table

The editor's issue (#356) leaves a name-and-version table out of its scope, as
ADR-016's rejected alternative. This record is not one: it records a fact about
a past launch, it stores no pipeline, and the pipeline file stays the truth.

### 10. What it does not solve, accepted

- **A name reused for unrelated content, a rename by hand, and a run copied
  from another workspace all keep the recorded name.** That name is a true fact
  about the launch, not proof of lineage. The parameter difference against the
  current document (§ 7) makes the mismatch visible.
- **A run launched under two names keeps the record of its first launch** —
  one run, one record.
- **Runs stored before this change carry only the second fact**,
  `pipeline_names`.

## Alternatives rejected

- **Keep hash matching as the rule, and document its limits** (#390's option
  (a)). Rejected because every edit takes the name away from all earlier runs,
  so the matrix cannot show a run launched under the name with since-changed
  content, and its "most recent run on that benchmark" quietly means the most
  recent run *of the current text*. An unedited fork is ambiguous, so neither
  name gets the run; a delete orphans the runs; a prefix run's parent is unknown
  once the parent document is edited; and a run written by `ragondin bench`
  without a workspace is named only if a workspace later holds the same content.
- **A workspace-side lineage index, with nothing added to the run** (option
  (c)): a history file appended per pipeline, or the queue's job files read as
  the index. Rejected because hand edits bypass a history appended by the API,
  and appending it at launch is the launch record kept in the wrong place; a
  rename or delete takes the history with the document; `ragondin bench` writes
  no job and no history; a copied run leaves the index behind; and reading the
  job files as history makes permanent records of files the queue does not
  promise to keep. In a cluster it has no twin, and a history kept beside the
  custom resources is the second truth
  `docs/design/2026-09-29-front-end-design.md` § 6 rejects.
- **Put the name inside the pipeline document** (option (d)). Hashed, it puts a
  name in the canonical form, so a rename changes the pipeline hash and the run
  id, and two identical pipelines under two names are two runs: content
  addressing would re-run what is not an input of the experiment. Not hashed, it
  is a field the canonical form must exclude by hand, against ADR-016 § 4
  (presentation state lives beside the pipeline document, never inside it); it
  gives two names to keep in step, the file's and the document's; "Fork this
  run" would have to rewrite the text; and since the hash ignores it, two names
  with one hash are still one run that records the first — no gain over the
  record. Either way it changes `ragondin-pipeline`'s public API (INV-1) and the
  wire format and its version (INV-9).
- **Put the name in `RunInputs` without digesting it.** Rejected because it
  breaks what `RunInputs` means: the components the identity digest was
  computed over. A record of its own is the same data without the confusion.
- **Put the name in the run id.** The hashed variant of the previous
  alternative without the document change, rejected on the same ground: a name
  is not an input of the experiment.
- **Structural lineage as identity** — runs whose graphs share node ids and
  edges grouped as one pipeline. Rejected as a heuristic: one added node breaks
  it, and two unrelated pipelines of the same shape merge. It is kept only as
  the structural prefix test of § 5 and § 6.
- **The record as first proposed**, with the launch channel, the configuration
  path and a version inside the file; one resolved name, with a field saying
  where it came from; and runs recorded under a name with another hash shown as
  that pipeline's earlier versions. The independent challenge narrowed it, and
  the owner decided the narrowed form, § 1 to § 8.

## Consequences

- **`ragondin-experiments` gains the record**, its own optional file, and the
  conformance cases every `RunStore` backend inherits: a round trip with a
  record and without one, a rerun keeping the first record, and a run stored
  before this change reading none. That step is the scaffolding issue #391,
  after #376 and before the launcher (#353); the run id and `ragondin bench`'s
  stdout stay byte-identical in that step.
- **`ragondin-api` reads the record and stamps nothing.** `launched_as` joins
  the run listing and detail beside `pipeline_names` (#392), and the `lineage`
  hash index becomes the second fact, no longer interim.
- **The launcher stamps the record from the `Submission`** (#353), and the
  `Submission` gains the parent's canonical hash for a prefix run (#357), which
  is also stored on the run, not on the job only.
- **The Pipeline matrix follows § 6** (#346). Its cells come from hash equality
  and the prefix tests and do not wait on the record; its feeding-runs list
  does.
- **`ragondin bench` follows § 8** (#393): it reports now, and refuses once the
  shared preparation knows the identity before running.
- **The editor's fork from a run (#356) is unchanged**: a forked run keeps its
  recorded name and enters the new file's Pipeline screen by hash equality.
- **The fixture workspace of the front end's exit test holds a run without a
  record and a run whose recorded name's content has since changed** (#358).
- **ADR-016 and `docs/design/2026-09-29-front-end-design.md` § 6 are
  corrected where they say the store already holds which version produced
  which run**: ADR-016 by a separate amendment under `docs/adr/README.md`
  process rule 2, whose rejection still stands on its "two truths" ground; the
  design in this change.
- **INV-8 is untouched.** Nothing is hashed, the run id does not change, and
  `RunInputs` does not change.
- **Not decided here: whether Compare's manual pairings apply to a run whose
  recorded name is *N* but whose content has since changed.** Compare consumes
  the same rule, and #390 names this as the one point its twelve amendments do
  not fix. It is left to the repository owner in #402.

## Status

Accepted.
