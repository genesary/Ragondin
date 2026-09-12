---
id: ADR-C26
title: Corpus ingestion belongs to the composition root; a driver receives the chunk set
status: accepted
invariants: [INV-1]
supersedes: []
superseded_by: null
---

# ADR-C26: Corpus ingestion belongs to the composition root; a driver receives the chunk set

## Context

A run is named by its content. `docs/system-architecture.md` §7.1 makes
`index_version` part of that identity, alongside the configuration, the dataset
and the model hashes, so that two runs carrying the same `run_id` searched the
same corpus and their numbers may be compared. That is principle P4, and it is
what the M2 deliverable — reproducible numbers — rests on.

The evaluation driver cannot currently keep that promise, and not by oversight.
A corpus reaches a component through the component's **constructor**, and a
driver never holds a constructed component. Four facts close every other route:

- **`EngineContext` is write-only.** Its public surface is `new` and five
  `register_*` methods; every `build_*` is `pub(crate)`. A context holds
  *constructors*, never instances.
- **Planning hands the driver nothing.** `plan_physical` is public, but
  `PhysicalPipeline`'s accessors and `PhysicalNode::component` are `pub(crate)`.
  The components are constructed and immediately out of reach.
- **A BM25 index is a constructor argument.** `Bm25Retriever::new(chunks)`
  builds its index there, and `Retriever` has no post-construction ingest
  method. The component's own documentation makes that a property rather than
  an accident: *"The index is built once, in RAM, and is immutable thereafter …
  Re-indexing means constructing another retriever, which is also what makes a
  run reproducible."*
- **`VectorStore::upsert` exists but is unreachable from a driver.** The only
  public holders of a `Box<dyn VectorStore>` are the composition root and
  `DenseRetriever::new`, and an evaluation driver may not depend on a crate
  under `components/` — that dependency is what would make it a second
  composition root.

So `ragondin-harness` (#29) prepares the chunk set from the benchmark and
content-addresses it, and builds no backend index. `index_version` therefore
names a chunk set the harness derived, while retrieval happens over components
somebody else constructed, over a corpus the harness cannot ask about. A run
can record a version naming a corpus it never searched, and nothing detects it.

Issue #29 contained the contradiction in miniature: its Scope — IN asked the
harness to build an index that its own Scope — OUT made impossible. That is
what surfaced the question rather than any failure at run time.

Decided in #212.

## Decision

**Corpus ingestion is the composition root's job.** The binary prepares the
chunk set, constructs every component over it, and hands the driver the same
value. A driver does not reach a constructed component, no trait grows a
post-construction ingest method, and no `pub(crate)` on the engine is widened.

Concretely, for the evaluation driver: the composition root builds a
`CorpusIndex` from the benchmark's corpus, constructs its components from
`CorpusIndex::chunks`, and passes that `CorpusIndex` to the harness, which
stops deriving one of its own. `index_version` then names the set the
components were built over, because one value is prepared once at the only
place that holds both the engine and the concrete components
(`docs/code-architecture.md` §4.3).

**The obligation is on the composition root, and it is not mechanically
checked.** Nothing stops a caller registering components over one corpus and
passing another; what changes is that a single value now travels where two
could silently disagree, and that the obligation is written down and attached
to the role that is already defined as knowing both halves.

## Alternatives rejected

- **Leave ingestion in the composition root and tell the driver nothing** — the
  state before this decision. Rejected because it leaves `index_version` as a
  claim no part of the system supports: the harness content-addresses a chunk
  set it derived and never put into anything, which is worse than an absent
  field because it reads as evidence.
- **Give `EngineContext` a public way to reach a constructed component**, by
  making `build_*` public or adding an accessor, so a driver can index what it
  will search. Rejected on two grounds: it puts component *instances* into a
  type documented as holding constructors, and a driver that can reach a
  component can call it directly, bypassing the plan — which is the seam P1
  exists to protect and the reason `PhysicalNode::component` is `pub(crate)` in
  the first place.
- **Grow the component contract a post-construction ingest method** —
  `Retriever::index(chunks)`, or a documented `VectorStore::upsert` path a
  driver can reach. The most uniform answer, and conformance could check it.
  Rejected *for now*: it changes `ragondin-contracts`, an INV-1 stable
  boundary, on behalf of a bench fixture; it obliges every implementer present
  and future to answer "what does re-indexing mean for me?"; and it contradicts
  what `Bm25Retriever` documents about itself today. Nothing here forecloses
  it — see the Consequences.
- **Let the evaluation driver depend on the component crates and build the
  index directly.** Rejected because it makes the driver know concrete
  backends, which is the composition root's one job (§4.3). The result is two
  composition roots that must agree, which is the problem this decision is
  about, moved.
- **Drop `index_version` from run identity** and record only what can be
  proved. Rejected because the field is not wrong, only unenforced: the
  identity tuple §7.1 describes is what makes a run comparable, and removing a
  component of it to avoid stating an obligation trades a checkable promise for
  no promise at all.

## Consequences

- **`Evaluation` carries the prepared index, and the harness stops building
  its own.** `CorpusIndex::build` is already public, so the change is a
  parameter rather than a mechanism. It is a behaviour change with tests, and
  lands in its own pull request under its own issue, the way ADR-C19's
  behaviour clause did.
- **The `bench` handler (#31) is where this is first honoured**: it builds the
  `CorpusIndex` from the loaded benchmark, constructs every registered
  component from those chunks, and passes the same value to the harness. That
  sequence is the shape of the M2 composition root.
- **The dense path needs work this decision does not do, and it is a leaf's.**
  A `ComponentCtor` is synchronous — `Fn(&Params) -> Result<Box<T>, ConstructionError>`
  — so a constructor can build a BM25 index from chunks (`Bm25Retriever::new`
  already does) but cannot populate a vector store, which takes
  `Embedder::embed` and `VectorStore::upsert`, both `async`. The composition
  root's `main` is where the `await` can happen, so it embeds the corpus before
  registering, and the store reaches its constructor already populated — by a
  synchronous constructor over pre-embedded entries, or by a registered closure
  capturing what was prepared. Which of the two is a choice inside the store
  component and the binary, recorded where it lands; neither reaches a shared
  surface, and neither reopens this decision. It is named here because it is
  the first thing an implementer meets, not because the ADR settles it.
- **`CorpusIndex::version`'s caveat survives this decision, narrowed.** It
  still addresses *a* chunk set rather than provably the one searched, because
  the guarantee is structural rather than enforced. What changes is that the
  caveat now names an obligation and the ADR that places it, instead of an open
  question.
- **The reviewer's sign is specific**: a component constructed from anything
  other than the `CorpusIndex` the run carries — a second `CorpusIndex::build`
  call in the same composition root, or a retriever built from a benchmark
  loaded separately.
- **The trait-level answer stays available.** When ingestion becomes a product
  concern rather than a bench fixture, the rejected ingest-method option is
  reachable from here at no extra cost: nothing in this decision makes a
  `Retriever` harder to give an `index` method later, and this is part of why it
  was chosen.
- **`docs/OPEN_QUESTIONS.md` #5 is untouched.** Whether indexing shares the IR
  formalism — whether an index build is expressed *as a pipeline* — is
  deliberately unresolved, and this decision takes its "ad hoc for now" as
  given. It says which part of the system performs the ad-hoc build, and
  nothing about how a future one would be expressed.

## Status

Accepted.
