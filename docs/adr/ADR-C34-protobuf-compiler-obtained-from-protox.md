---
id: ADR-C34
title: The protobuf compiler is `protox`, a build-dependency of `ragondin-proto`; no `protoc` is needed
status: accepted
invariants: [INV-4]
supersedes: []
superseded_by: null
---

# ADR-C34: The protobuf compiler is `protox`, a build-dependency of `ragondin-proto`; no `protoc` is needed

## Context

ADR-C24 decides that the `.proto` files in `ragondin-proto` are
hand-maintained and that `tonic-build` generates the Rust stubs from them in
`ragondin-proto`'s `build.rs`. It stops there. It does not say where the
protobuf compiler that `tonic-build` needs comes from, and no other ADR,
`docs/` page or CI step says so either.

`tonic-build` 0.12 is the line matching the workspace's `tonic = "0.12"`, and
it generates through `prost-build` 0.13. `prost-build` does not bundle a
compiler. Its `compile_protos` path runs a `protoc` executable, named by the
`PROTOC` environment variable or else found on `PATH`. Its `compile_fds` path
takes a pre-built `FileDescriptorSet` and never invokes `protoc`. `tonic-build`
exposes both, as `Builder::compile_protos` and `Builder::compile_fds`.

The question reaches the default build, not only the full one.
`ragondin-proto` is a workspace member, and `runtime/ragondin-config` depends
on it unconditionally. That edge is declared ahead of M6's configuration
delivery and is not yet used. `bin/ragondin` depends on `ragondin-config`, and
`cargo tree -i ragondin-proto` shows the path
`ragondin-proto → ragondin-config → ragondin`. So whatever `build.rs` requires,
`cargo build -p ragondin` from a fresh clone requires too, and so does every CI
run. Today no `protoc` is installed on a contributor's machine by anything in
this repository, and `.github/workflows/ci.yml` installs none.

The answer also changes what a build requires, which `AGENTS.md` § Rules of
engagement escalates on two counts. First, the crate-based answers add
`[workspace.dependencies]` entries that a crate outside `components/` depends
on in the same diff (`ragondin-proto` lives in `wire/`). `tonic-build` itself
is covered only because ADR-C24 names it. Second, the one alternative that
adds no crate and keeps generation in `build.rs` imposes a system tool on the
default binary build, which ADR-C14 requires to stay lean and fast to
compile.

The `protox` route was checked in a scratch copy of the workspace when #281
was opened, and two independent reviews challenged it; the second re-derived
every check. The `.proto` files used were modelled on what the component
services and the configuration service will need. Between them they had one
package split over two files with an import, `GetModelIdentity` rpcs,
proto3 `optional` scalars and strings, an enum with an `_UNSPECIFIED = 0`
value, wrapper, empty and nested messages, a map, a oneof, unary and
server-streaming rpcs, doc comments, and a well-known type. The results:

- **The generated Rust is byte-identical.** `protox` 0.7.2, `protox` 0.8.0 and
  `protoc` 31.1 produced byte-identical Rust in the first check. The second
  review found the same for `protox` 0.8.0 against `protoc` 31.1, through
  `tonic-build` 0.12.3 and `prost-build` 0.13.5.
- **The descriptor sets are not byte-identical.** `protox` and `protoc`
  order the `source_code_info` locations differently. `protox` also bundles
  its own copy of the well-known types, of a different vintage. Neither
  difference affects the wire format or the generated Rust API.
- **Audit.** `cargo deny --all-features check` with this repository's
  `deny.toml` passed with `protox` in the graph, and the second review found
  no problem with advisories, bans or sources. No RustSec advisory exists
  for `protox`, `protox-parse`, `prost-reflect`, `logos` or `miette`, and
  every licence in the added closure is MIT and/or Apache-2.0.
- **Cost.** `protox` adds 13 crates to `Cargo.lock` beyond what `tonic-build`
  brings, all build-dependencies, so nothing is linked into any binary the
  workspace ships. The
  added compile time was measured on one Mac at about 5–7 s, paid once (about
  7 s in the first measurement, 5.0 s wall in the second).
- **Upstream.** `protox` has a single maintainer. At review time it had four
  open issues upstream, none touching anything these `.proto` files contain.

Decided in #281, by the repository owner on 2026-09-26.

## Decision

**`ragondin-proto`'s `build.rs` compiles the hand-maintained `.proto` files
(ADR-C24) to a `FileDescriptorSet` with `protox`, and hands that set to
`tonic_build::configure().compile_fds(..)`. No `protoc` binary is needed,
looked up or run.**

Concretely:

- **Two workspace entries, one consumer.** `protox = "0.8"` and
  `tonic-build = "0.12"` are declared in `[workspace.dependencies]`, and are
  used as `[build-dependencies]` of `ragondin-proto` only. This decision
  sanctions both entries under `AGENTS.md` § Rules of engagement.
  `tonic-build` was already named by ADR-C24; `protox` is new here.
- **The versions are coupled, and the coupling is stated here.**
  - `protox` 0.8 is built on `prost` 0.13 and `prost-reflect` 0.15. It pairs
    with `tonic` 0.12 and 0.13, whose `tonic-build` generates through
    `prost-build` 0.13.
  - Moving to `tonic` 0.14 takes this decision along in one diff.
    `tonic-build` becomes `tonic-prost-build`, which has a `compile_fds` with
    the same signature. `prost` moves to 0.14 and `protox` to 0.9 in that
    same diff. The `build.rs` shape is unchanged; the runtime dependency set
    is not. In `tonic` 0.14 the prost codec moved out of `tonic` into the
    `tonic-prost` crate, and the stubs `tonic-prost-build` generates name
    `tonic_prost::ProstCodec` by default, so `ragondin-proto` gains a runtime
    dependency on `tonic-prost`. **This decision does not sanction that
    `[workspace.dependencies]` entry.** It is used outside `components/`, and
    the diff that moves to `tonic` 0.14 escalates it on its own under
    `AGENTS.md` § Rules of engagement.
  - **The `protox` 0.8 line is frozen.** Parser fixes arrive only with the
    move to `prost` 0.14 and `protox` 0.9.
- **What is equivalent to `protoc`, and what is not.** The generated **Rust**
  is byte-identical to what `protoc` produces, for the feature set checked
  above. The **descriptor sets** are not identical: the order of
  `source_code_info` and the vintage of the bundled well-known types differ.
  This has no effect on the wire or on the Rust API. The only thing that
  would show it is a future `tonic-reflection` server, which serves the
  descriptor set itself.
- **A `protoc` lint in CI is allowed, and not required.** A CI-only job that
  runs `protoc` over the repository's `.proto` files is compatible with this
  decision. It adds nothing to the Cargo graph or to the default build. It is
  an available guard for `Remote` authors who compile these files with
  `protoc` in another language. This decision does not require it.

## Alternatives rejected

- **A system `protoc`.** Contributors install `protobuf-compiler`, CI installs
  it in one step, and `build.rs` fails with a message naming what to install.
  It adds no crate and uses the reference compiler. Rejected because it
  breaks `cargo build -p ragondin` from a clone: through the
  `ragondin-config` edge, the default binary would need a tool Cargo cannot
  provide. And it pins nothing. The compiler is whatever each machine has:
  Ubuntu 24.04's package ships 3.21, while Homebrew ships the current release
  (31.x when #281 was reviewed, 36.2 when this ADR was written).
- **`protoc-bin-vendored` as a build-dependency.** It ships Google's prebuilt
  `protoc` binaries inside crates, and `build.rs` points `PROTOC` at the one
  for the host. That gives the reference compiler, pinned by `Cargo.lock`,
  with nothing to install. Rejected because version 3.2.0 depends
  unconditionally on eight platform crates, one per binary, about 27 MB of
  compressed crates that every fresh checkout downloads, whatever its
  platform. The host's binary runs at build time, and `cargo-deny` audits
  none of the eight: it sees the wrapper crates, which declare MIT, and not
  the executables they carry.
- **Making the `ragondin-config` → `ragondin-proto` edge optional**, behind a
  feature, so that the default binary needs no code generation. Rejected as
  orthogonal. CI and `just check` build every workspace member anyway, so a
  compiler is still needed on every contributor's machine and every CI run.
  The gate would narrow the cost of each alternative here without choosing
  between them, and it would touch a crate the question is not about.
- **Downloading `protoc` at build time**, the pattern ADR-C27 took for ONNX
  Runtime. Rejected because it needs network access at build time and leaves
  the downloaded binary outside `cargo-deny`'s audit, and it gains nothing
  over `protox`.
- **Committing the generated Rust**, generated out of band, with a CI job that
  checks it is current. Rejected because it contradicts ADR-C24's Decision,
  which puts generation in `ragondin-proto`'s `build.rs`. Taking it would need
  a superseding ADR, not this one.

## Consequences

- **The `.proto` work is unblocked.** #12 writes the `.proto` files and this
  `build.rs`. Through #12, the issues that build on the generated stubs are
  unblocked too: #257 (the `Generator` and `ContextBuilder` services in
  `ragondin-proto`), #13 and #261 (the `Remote` adapters in
  `ragondin-remote`), #267 (the reference `Remote` generator service, a gRPC
  server over `ragondin-proto`) and #300 (binding `Remote` components with
  `--remote` on the `ragondin` command line).
- **The lean default build (ADR-C14) pays for code generation.** The
  `ragondin-config` edge puts `ragondin-proto`'s `build.rs` in
  `cargo build -p ragondin`, so its build-dependencies compile in the default
  build. `tonic-build` and `prost-build` are ADR-C24's cost. `protox` adds 13
  crates on top of them, measured at about 5–7 s once on one Mac. None of
  them is linked into the binary. This is the price of building from a clone
  with nothing but Cargo, and the alternatives that avoid it do so by
  requiring a tool Cargo does not provide.
- **One maintainer, and a bounded fallback.** `protox` has a single
  maintainer. If it stalls, or mis-compiles a file, going back to `protoc` is
  a `build.rs` edit, calling `compile_protos` instead of `compile_fds`, that
  changes nothing outside `ragondin-proto`'s build script: nothing else
  depends on which compiler produced the stubs, because the generated Rust is
  the same. Where that `protoc` would come from is the question this ADR
  answers, so taking the fallback supersedes this ADR.
- **The strongest counter-argument, and why it lost.** `protox` is a second
  implementation of the compiler's front end, not Google's. A `.proto` file
  that `protoc` rejects could therefore merge unnoticed, and fail first for a
  `Remote` author compiling it in another language. It did not win because
  that failure is loud, for the first `Remote` author it reaches, and the
  files at stake are few and all owned by this repository, so they are cheap
  to guard. The optional CI lint in the Decision is that guard, if one is
  wanted.
- **The move to `tonic` 0.14 is known in advance.** It is one diff:
  `tonic-build` is replaced by `tonic-prost-build`, `prost` moves to 0.14,
  and `protox` moves to 0.9. The `build.rs` shape is unchanged, because
  `compile_fds` keeps its signature. The runtime dependency set is not: the
  generated stubs use `tonic_prost::ProstCodec`, so `ragondin-proto` gains a
  `tonic-prost` dependency, and that new `[workspace.dependencies]` entry
  escalates in that diff; this ADR does not sanction it. Until that move,
  `protox` stays on the frozen 0.8 line and receives no parser fixes.
- **A well-known type in a `.proto` needs a runtime entry.** Using one such as
  `google.protobuf.Timestamp` needs a `prost-types` entry as a normal
  dependency, whichever compiler is used. None is planned for the services
  this decision unblocks.
- **INV-4 is untouched.** `ragondin-proto` lives in `wire/`, outside the core
  whose closure INV-4 constrains. No crate under `core/` reaches it, and
  `protox` and `tonic-build` are build-dependencies of `ragondin-proto` alone.

## Status

Accepted.
