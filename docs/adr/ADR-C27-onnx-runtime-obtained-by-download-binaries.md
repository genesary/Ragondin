---
id: ADR-C27
title: ONNX Runtime is obtained by `download-binaries` over `rustls`, declared once for the workspace
status: accepted
invariants: [INV-4]
supersedes: []
superseded_by: null
---

# ADR-C27: ONNX Runtime is obtained by `download-binaries` over `rustls`, declared once for the workspace

## Context

Two component crates wrap the same inference runtime. `ragondin-embedder-onnx`
(#20) and `ragondin-reranker-onnx` (#21) both bind ONNX Runtime through the
`ort` crate, each behind its own `onnx` feature, so that the default build
compiles neither (ADR-C14). ADR-C14 and INV-4 settle *where* the dependency may
live. They say nothing about *how the runtime binary arrives*, and `ort` offers
two answers:

- **`download-binaries`**: the `ort-sys` build script fetches a prebuilt
  archive for the host target from the `ort` maintainer's CDN, verifies its
  SHA-256 against a table shipped inside the crate (`build/download/dist.tsv`),
  extracts it into the user cache directory (`<cache>/ort.pyke.io/dfbin/<target>/<hash>`),
  and links it. The archive is a **static library** (`libonnxruntime.a`); the
  build script emits `cargo:rustc-link-lib=static=onnxruntime`. The runtime is
  inside the executable. Nothing is loaded at run time, and `copy-dylibs`
  finds no shared library to copy.
- **`load-dynamic`**: nothing is fetched. The executable expects a shared
  library the operator installed, and loads it when a session is constructed.

The feature gate confines the *dependency* but not the *cost* of this choice.
`just check` runs `test-features`, `check-features`, `clippy --all-features`,
`doc --all-features` and `check-deny --all-features`; every one of them resolves
or builds the `--all-features` graph and therefore runs `ort-sys`'s build
script. Whatever a component chooses, every contributor and every CI image
that runs the repository's own mandatory gate pays for it.

The choice also has a second knob that neither issue asked about. The download
is one HTTPS request, and `ort-sys` makes it through `ureq`, which needs a TLS
stack: `tls-native` (the `ort` default) selects the platform's, which on Linux
is OpenSSL and requires its headers on the build host; `tls-rustls` selects a
pure-Rust one. Each pulls a different Mozilla CA bundle crate under the same
licence — `webpki-root-certs` under `native-tls`, `webpki-roots` under
`rustls`, both CDLA-Permissive-2.0 — so each needs the same one allowance in
`deny.toml`, naming a different crate.

Both open pull requests chose `download-binaries`, recorded it in their own
`ARCHITECTURE.md`, and diverged on TLS: #174 kept `ort`'s default,
`tls-native`; #222 chose `tls-rustls` and argued it. Under `AGENTS.md`
§ Rules of engagement as written, each choice stayed inside one crate and was
that crate's to make. The rule worked and the outcome was still wrong, for a
reason the rule did not name: **`ort`'s feature list is not a per-crate fact.**
Dependency versions and features are declared once, in
`[workspace.dependencies]`, and Cargo unifies a dependency's features across
every crate that enables it. Two crates cannot merge with two different `ort`
feature lists; the second to merge inherits the first's, and the choice is made
by merge order rather than by anyone. The same is true of every future
`ort`-backed component.

Three facts, verified against the pinned `ort-sys 2.0.0-rc.13` and `ureq 3.4.1`
sources, weigh on the alternatives:

- Under `download-binaries` there is **no missing-library failure at run
  time**, because there is no library to find: the archive is static and
  linked in. That failure mode exists only under `load-dynamic`.
- The download is once per machine and target for a developer, because the
  cache directory sits outside `target/` and survives `cargo clean`. It was
  **not** once per machine for CI, whose cache covered `~/.cargo` and `target`
  and not the runtime's cache directory, so every run fetched the archive.
- `cargo deny` audits the crate graph. The fetched archive is not in the crate
  graph. It is hash-pinned and comes from one named publisher, and nothing in
  `deny.toml` sees it.

Decided in #223.

## Decision

**The workspace obtains ONNX Runtime through `ort`'s `download-binaries`
feature over `tls-rustls`, declared once in `[workspace.dependencies]` and
inherited unchanged by every `ort`-backed component; an offline build supplies
the library through `ORT_LIB_LOCATION`, and the fetched archive is acknowledged
as outside `cargo deny`'s audit.**

Concretely:

- The root `Cargo.toml` carries one `ort` entry with `default-features = false`
  and exactly the features the workspace uses: `std`, `api-27`, `copy-dylibs`,
  `download-binaries`, `tls-rustls`. A component crate references it as
  `ort = { workspace = true, optional = true }` and **adds no features of its
  own**. A change to that list is a change to every crate that names the
  entry, and it is made there, once, with its reason in the entry's comment.
- `deny.toml` carries one `CDLA-Permissive-2.0` allowance, for `webpki-roots`,
  the CA bundle `ureq` ships under `rustls`.
- The offline route is `ort-sys`'s own and is recorded next to `just check` in
  `AGENTS.md` § Commands: point `ORT_LIB_LOCATION` at a directory holding
  `libonnxruntime.a` and set `ORT_SKIP_DOWNLOAD=1` (`ORT_OFFLINE` and Cargo's
  `CARGO_NET_OFFLINE` skip the download too). Nothing in this workspace has
  to change for an air-gapped build.
- CI caches the runtime's cache directory alongside `~/.cargo` and `target`,
  so "once per machine" holds for the CI image as well.
- The archive's provenance is stated once, in the same place: it is fetched
  from `cdn.pyke.io`, verified against the hash `ort-sys` ships, and not
  audited by `check-deny`. Trusting `download-binaries` is trusting that
  publisher, and this decision accepts that trust for the CPU runtime the M2
  bench needs.

## Alternatives rejected

- **`load-dynamic`.** No build-time network access and no payload outside the
  crate graph, which is what a hardened environment wants. Rejected because it
  makes every developer and every CI image install ONNX Runtime at a version
  matching `ort`'s expectation before the workspace builds, and because it
  moves the failure from build time to run time: a missing or mismatched
  library is discovered when `ragondin bench` constructs a component, not when
  the tree compiles. For an evaluation harness that is the worse failure, and
  it is one `download-binaries` does not have at all.
- **`tls-native`.** The `ort` default, and the one #174 inherited without
  choosing. Rejected because it adds a build-host dependency — OpenSSL headers
  on Linux — for the sake of a single HTTPS request in a build script, and
  because `ureq` carries its own root store under either backend, so the
  platform TLS stack buys nothing here.
- **Each component crate keeps deciding for itself.** The status quo, and what
  the escalation list permitted. Rejected because it is not actually
  available: one workspace entry, unified features, so the second crate cannot
  differ from the first except by appending features on its own manifest line,
  which is the silent divergence this decision forbids.
- **Vendoring the runtime.** Commit or self-host the archive and point
  `ORT_LIB_LOCATION` at it. Rejected as machinery ahead of need: the archive is
  tens of megabytes per target, and the escape hatch that would make vendoring
  work is already available to any environment that needs it, without a
  repository change.
- **A CI-only opt-out.** Developers download; CI installs a pinned runtime and
  sets `ORT_LIB_LOCATION`, so the gate is offline and the payload is one the
  image audited. Rejected because it creates two acquisition paths that can
  drift — a version bump in `[workspace.dependencies]` and a version bump in
  the CI image, each able to happen without the other — for a gain that
  caching the download already delivers.

## Consequences

- **One feature list, one place.** The `ort` entry in `[workspace.dependencies]`
  is the record; the two component crates' `ARCHITECTURE.md` files describe
  what the runtime is and cite this decision for how it arrives, instead of
  each arguing the acquisition trade for itself. #174 aligns its entry with
  #222's and drops `tls-native`; the CDLA allowance names `webpki-roots`.
- **The feature list of a shared entry is a shared surface.** `AGENTS.md`
  § Rules of engagement names it: adding to or changing the features of an
  existing `[workspace.dependencies]` entry reaches every crate that names the
  entry, and it is not a leaf's choice. This is the rule gap that let two
  leaves answer one question differently, and it is closed where the rule is
  stated rather than here.
- **INV-4 is untouched.** `ort` stays on the deny-list of
  `scripts/check-invariants.py`, confined to component crates behind their
  features. How the binary arrives changes nothing about where the crate may
  be reached from.
- **`copy-dylibs` stays on and does nothing today.** The static archive holds
  no shared library for it to copy. It is kept because it is cheap, because it
  is what `ort`'s default carries, and because a future execution provider
  distribution that ships a shared library would need it; a comment saying it
  does nothing today is the honest form, and the claim that it places a
  shared library beside the test binary is the false one.
- **A GPU execution provider reopens the archive question, not this
  decision.** `dist.tsv` lists different archives per feature set (`cuda`,
  `tensorrt`, `coreml`, `webgpu`), some of which ship shared libraries and
  carry their own licences. Enabling one is a change to the shared feature
  list, made once at the root, and the trust statement above would then need
  to name what it covers.
- **The trust is explicit and narrow.** The publisher, the hash, and the fact
  that no audit covers the payload are written next to the command that pays
  for them. A threat model that cannot accept a hash-pinned third-party binary
  chooses the offline route this decision keeps open; it does not need a new
  decision to do so.

## Status

Accepted.
