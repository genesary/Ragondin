# ARCHITECTURE — ragondin-retriever-bm25

**Status: a component — a leaf of the dependency graph, and internal.** Nothing
here is an API boundary (INV-1 protects `core/`, not this crate). The stable
surface a caller depends on is the `Retriever` trait in `ragondin-contracts`;
this crate is one implementation of it and may be refactored freely.

## What lives here

In-process **BM25 sparse retrieval** over an in-memory `tantivy` index:
`Bm25Retriever`, constructed over a `Vec<Chunk>`, implementing `Retriever`.

Sparse retrieval **without an Elasticsearch sidecar** is one of the reasons the
data plane is written in Rust (`docs/system-architecture.md` §10). This is the
first crate to make that concrete.

**Not here, deliberately:** fetching or chunking a corpus (the harness and the
`Chunker` family), dense retrieval (`ragondin-retriever-dense`), and registration
on an `EngineContext` — the registry lives on the engine, and the binary is the
composition root that puts the two together (`docs/code-architecture.md` §8.1).

## Local invariants

- **A component is a leaf (INV-5, CI-enforced).** The dependencies are
  `ragondin-contracts`, `ragondin-types`, `async-trait`, `thiserror` and the heavy
  backend — never `ragondin-engine`, never a sibling component. The engine knows
  only traits, and the check walks the dependency graph to prove it.
- **The heavy backend is confined and feature-gated (ADR-C14).** `tantivy` is an
  optional dependency behind `bm25`, and **`bm25` is not a default feature**:
  `cargo build --workspace` compiles none of it. With the feature off the crate
  exports nothing — a BM25 retriever without tantivy would be a different
  component, not a degraded one — so the tests are behind the same `cfg` and
  `just test-bm25` is what runs them. `just check-features` proves the gated code
  still compiles.
- **No privilege for built-ins (INV-7).** This crate is reachable only through
  the `Retriever` trait and its own public constructor. It has no entry point
  into the engine that a third-party crate could not also call, and adding one
  would be the two-tier system ADR-C6 exists to prevent.
- **The ranking contract is honoured, and the ties are too.** `Retriever`
  requires descending, finite scores. tantivy leaves equally scoring documents
  ordered by document address — stable for one index, but meaningless to a
  caller — so this crate sorts on `(score descending, chunk id ascending)`.
  Without that, two chunks with equal BM25 scores could rank two ways over one
  corpus, and the only symptom would be a benchmark number that moves.
- **The tie-break runs before `top_k` truncates.** `TopDocs::with_limit(top_k)`
  would make the cut itself by document address, leaving the chunk-id rule to
  reorder a selection insertion order had already decided. So the collector is
  given the number of documents in the index as its limit, every match is sorted,
  and the list is truncated afterwards. That is the price of the reproducibility
  guarantee — a corpus ranks one way, whatever order it was indexed in — and the
  price is higher than a sort. Stated exactly, because an understated cost is a
  claim the code contradicts:

  - the collector's limit is `searcher.num_docs()`, so tantivy sizes its
    per-segment buffer by the **corpus**, not by the number of matches or by
    `top_k`. That reservation is paid on every call, though it is lazily
    faulted: a query matching one chunk measures flat across corpus sizes
    (8.5 µs at 25k, 5.9 µs at 50k, 5.7 µs at 100k — the spread is noise, and
    the point is that it does not grow), so this shows up as virtual footprint
    rather than as latency;
  - **every match is fully materialized** — `searcher.doc()` decompresses the
    stored document and its text is cloned into a `ScoredChunk` — *before*
    `truncate` runs. This is the dominant cost, and it is linear in the number
    of matches, not in `top_k`.

  Measured, release build, `top_k = 10` throughout — a query matching one chunk
  against one matching every document:

  | corpus | selective | a term every document holds |
  |---|---|---|
  | 25 000 chunks | 8.5 µs | **16 ms** |
  | 50 000 chunks | 5.9 µs | **30 ms** |
  | 100 000 chunks | 5.7 µs | **67 ms** |

  These are one machine's numbers on a synthetic corpus, so read the shape and
  not the digits: the left column does not grow, the right one doubles when the
  corpus doubles.

  **This is the nominal path, not an edge case.** Stopwords are deliberately
  indexed and scored (below), so every natural-language question contains a term
  most of the corpus holds. Extrapolating the measured linearity puts a corpus of
  a million chunks at roughly 0.7 s per query, and a BEIR set like NQ (2.7 M
  documents, 3.5 k queries) into the hours — for a component that answers a
  selective query in microseconds.

  It is nonetheless left as is here. #33 targets SciFact, ~5 k documents, where
  the broad path costs single-digit milliseconds; the fix is a design change,
  not a tweak. The route is the fast-field comparator: index the chunk id as a
  fast field so tantivy breaks the tie inside the collector, which lets the limit
  drop back to `top_k` and materializes only the survivors. Both properties then
  hold at once. Until then, treat a corpus beyond the low hundreds of thousands
  of chunks as out of this component's range.
- **The query is analyzed, never parsed.** A retriever is handed
  natural-language questions. tantivy's `QueryParser` would read `?`, `:` or a
  quote in one as syntax — failing the call, or silently changing what was
  asked — so the query is run through the text field's own analyzer and turned
  into a `Should` disjunction of `TermQuery`s. That also guarantees the query is
  tokenized exactly as the corpus was. The analyzer is named explicitly rather
  than inherited: `"default"`, which in tantivy 0.26 is `SimpleTokenizer` then
  `RemoveLongFilter(40)` then `LowerCaser` — so case folds, accents do not, and
  a token of 40 bytes or more is dropped from corpus and query alike.
- **The index is immutable, and it is entirely in RAM.** It is built once, at
  construction, by `Index::create_in_ram`. Re-indexing means constructing another
  retriever. That is what makes a run reproducible: the same corpus yields the
  same index yields the same ranking. It also sets a ceiling: the text is both
  `STORED` and indexed, so a corpus is held roughly twice over with no spill to
  disk, and a corpus that does not fit in memory does not fit this component.
- **Typed errors (ADR-C13).** `IndexError` for construction, `ComponentError`
  for the call. A library never imposes `anyhow` on its consumers.

## What is not configurable, and why

`k1` and `b` are named in `docs/code-architecture.md` §6.3 as the example of
constructor configuration. **They are not exposed here**, because tantivy holds
them as private constants and offers no way to set them: a knob on this
constructor would be a claim the backend cannot honour. It arrives when the
backend supports it, not before.

**No stemming and no stopword removal.** `default-features = false` on tantivy
drops its `stemmer` and `stopwords` features along with the on-disk machinery,
and the `"default"` analyzer this crate names carries neither in any case. That
is a choice, not a side effect of a size saving: this component is a plain,
deterministic BM25 baseline whose query is analysed exactly as its corpus was,
and stemming is a per-language decision the crate has no way to make — the
corpus arrives as `Vec<Chunk>` with no language on it. The consequence is
measurable, so it is pinned by a test rather than described: `cat` does not
match `cats`, and `the` is scored like any other term instead of being
discarded. Turning either feature back on breaks that test, which is the point.

## Conformance

`tests/conformance.rs` runs `ragondin_conformance::assert_retriever_conformance`
against the same public constructor a third-party caller uses — which is what
makes built-in/third-party equivalence real rather than asserted (ADR-C6). The
rest of that file is what conformance deliberately does not check: the suite
knows nothing of the corpus behind a retriever, so *ranking* — which chunk comes
first, and why — is checked here, against a corpus small enough to read.
