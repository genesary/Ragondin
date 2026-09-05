# Contributing

Thank you for considering a contribution. This document covers how to build and
test, and the two paths for contributing a component.

Before writing code, read [`AGENTS.md`](AGENTS.md) — the binding operating rules
(invariants, frozen decisions, definition of done). It applies to human and AI
contributors alike.

## Build and test

```bash
cargo build                 # lean default build (no heavy backend)
cargo test --workspace      # run all tests
just check                  # everything CI runs, in one command
```

`just check` runs build, tests, `clippy` (warnings are errors), `cargo fmt
--check`, and the architecture invariant checks. **A change is not done until
`just check` passes.** The toolchain is pinned to stable (`rust-toolchain.toml`);
never rely on nightly.

### The architecture invariant checks

Two invariants are enforced as blocking CI checks, not just documented:

- **INV-4 — the core stays light.** `ragondin-types` and `ragondin-contracts` carry no
  heavy dependency (tantivy, tonic, prost, ort, candle, vector-store clients,
  HTTP clients). If you add one, the build fails and tells you why.
- **INV-5 — the engine knows only traits.** `ragondin-engine` depends on no crate
  under `components/`. Add such a dependency and the build fails.

Run them directly with `just check-invariants` (implemented in
`scripts/check-invariants.py`). If a check blocks you, it is the architecture
speaking — do not route around it. If you believe it is wrong, open a `decision`
issue rather than weakening the check.

## Contributing a component

A component (a `Retriever`, `Reranker`, `Generator`, …) can be contributed two
ways. **Both are benchmarked identically, through the same code path, with no
privilege for built-in components (INV-7).**

### Local — a Rust crate

A new crate under `components/` that implements a trait from `ragondin-contracts`.

- It depends only on `ragondin-contracts` and `ragondin-types` — a component is a **leaf**
  of the dependency graph. It never depends on `ragondin-engine` or on another
  component. You compile only the contracts crate and the value types, not the
  whole engine.
- Its heavy dependency (the retrieval engine, the ML runtime, the store client)
  is confined to the crate and **feature-gated**, so the default workspace build
  stays lean.
- It registers on an `EngineContext` through exactly the same mechanism a
  third-party component would use.
- Naming: `ragondin-<role>-<implementation>`, e.g. `ragondin-reranker-onnx`,
  `ragondin-store-qdrant`.

See [`components/README.md`](components/README.md).

### Remote — a gRPC service in any language

A service (commonly Python) implementing the corresponding protobuf service from
`ragondin-proto`. It runs outside this repository, in any language, and is named by
URL in the configuration; the engine reaches it through a generic `Remote<T>`
adapter and cannot tell it apart from a `Local` component.

A `Remote` component that wins a benchmark can later be ported to `Local`
(Rust) with no configuration change for any user.

### Conformance

Whichever path you take, your component must pass the conformance suite
(`ragondin-conformance`) — the behavioural suite every implementation passes, `Local`
or `Remote`. That is what makes the two paths genuinely equivalent rather than
equivalent by assertion.

## Conventions

- **Language:** English everywhere — code, comments, docs, commits, PRs.
- **Commits:** Conventional Commits, scoped by crate where useful, e.g.
  `feat(ragondin-pipeline): add canonical hashing`.
- **Branches:** `<type>/<issue-number>-<slug>`.
- **Errors:** `thiserror` (typed) in libraries; `anyhow` in binaries only.
