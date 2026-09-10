# components/

One crate per component implementation. `ragondin-retriever-bm25` (in-process
BM25 over tantivy), `ragondin-retriever-dense` (dense retrieval through a
`VectorStore`), `ragondin-store-memory` (an exact brute-force vector store) and
`ragondin-fusion-rrf` (Reciprocal Rank Fusion) are here;
`ragondin-embedder-onnx`, `ragondin-store-qdrant` and the rest arrive in later
issues.

## What a component crate is

Each component is its own crate and a **leaf** of the dependency graph:

- It depends on **`ragondin-contracts`** (the trait it implements) and **`ragondin-types`**
  (the value types) — and on **nothing else in the workspace**. Never on
  `ragondin-engine`, never on another component.
- Its **heavy dependency** (tantivy, candle, ort, a vector-store client, …) is
  **confined to it** and **feature-gated**. The default workspace build stays
  lean and compiles fast.
- It registers on an `EngineContext` through **exactly the same mechanism** as a
  third-party component. There is **no privilege for built-ins** (INV-7) — no
  shortcut, no fast path, no special case. A built-in and a third-party
  component are indistinguishable to the engine.

## Which way a feature default goes

`bm25` is off by default; `rrf` and `memory` are on. That is one rule applied
twice rather than an inconsistency, and it follows from
[ADR-C14](../docs/adr/ADR-C14-heavy-backends-feature-gated-lean-default-build.md)
— which is where the lean default build is decided — rather than deciding
anything new:

- **A feature that gates a heavy dependency is off by default.**
  `cargo build --workspace` honours each member's **own** defaults, so a heavy
  feature listed in that member's `default` compiles its backend into every
  default workspace build. That is what makes an on-by-default heavy feature a
  lean-build violation rather than a matter of taste: ADR-C14 decides that the
  default build is minimal, and a component's `default` is where its author
  keeps or loses that — provided no workspace member depends on the component
  with the heavy feature turned on, which puts the backend in the build whatever
  the component's own default says. `ragondin-retriever-bm25` has
  `default = []`, so no default workspace build compiles tantivy.
- **A feature that gates nothing is on by default.** There is no compile cost to
  defer, so turning it off would cost a workspace build the component and save
  it nothing. `ragondin-fusion-rrf` has `default = ["rrf"]` and
  `ragondin-store-memory` has `default = ["memory"]`; both gate arithmetic.
  Such a crate still names its feature after its implementation — the uniformity
  is in the **naming, not the invocation**.

A third-party component crate follows the same rule as the three here.

The rule has one mechanical consequence worth knowing before you pick a default.
`cargo test --workspace` builds with default features, so a crate behind an
off-by-default feature has its tests compiled away rather than run — silently,
and a green `just test` says nothing about them. **`just test-features` is where
they execute** (`cargo test --workspace --all-features`), and `just check` runs
it. That recipe is whole-workspace and names no crate, so a new component
inherits it instead of adding a recipe of its own.

## Why the leaf constraint matters

The engine depends on `ragondin-contracts`, not on any crate here (INV-5, CI-enforced).
Because components are leaves and the engine knows only traits, `Local` (Rust,
in-process) and `Remote` (gRPC, any language) components share exactly one API,
and the two-tier system that kills contribution-driven projects is made
structurally impossible.

The naming convention is `ragondin-<role>-<implementation>`, e.g. `ragondin-reranker-onnx`,
`ragondin-store-qdrant` — guessable rather than memorized.

See `CONTRIBUTING.md` for the two contribution paths (`Local` and `Remote`).
