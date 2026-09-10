# AGENTS.md

Operating rules for AI coding agents working in this repository.

This file contains **rules, not background**. It documents the practices of this repository that differ from ordinary Rust conventions — the things an agent would otherwise get wrong by default. Project rationale, architecture reasoning, and design history live in `docs/` and are deliberately not repeated here.

**Read this file in full before writing code. Its rules are binding.**

---

## Commands

```bash
# Build
cargo build --all-targets

# Test
cargo test --workspace

# Lint (warnings are errors)
cargo clippy --workspace --all-targets -- -D warnings

# Format
cargo fmt --check

# Architecture invariant checks (see "Invariants"), over the `--all-features`
# dependency graph — every heavy backend sits behind a feature by rule, so the
# lean graph holds no component crate's real dependencies at all.
just check-invariants

# That check's own tests, over throwaway cargo workspaces built at run time. A
# gate that resolved a narrower graph, or stopped looking at the edge it is
# supposed to look at, fails this first.
just test-check-invariants

# Every ADR citation resolves to a real file, in tracked Markdown and Rust alike
# — every line of a tracked .rs file, doc comment or not. It checks that a
# reference resolves, never that it is apt.
just check-doc-links

# That check's own tests, over fixtures built in a throwaway repository rather
# than committed here. A gate whose scan silently narrows fails this first.
just test-check-doc-links

# The generated ADR index in docs/adr/README.md is current
just check-adr-index

# Regenerate that index after changing an ADR's front-matter
just gen-adr-index

# The dependency graph: RustSec advisories, licences, duplicate versions, source
# registries. Policy in deny.toml. Needs the version CI runs:
# `cargo install cargo-deny --locked --version 0.20.2`
just check-deny

# The cross-reference map: one entity's neighbourhood, and every claim the code
# contradicts. Advisory, and deliberately not part of `just check` — the reason
# is in § What you write about the code is checked against the code.
just map <entity>
just map --conflicts

# All of the above — run this before declaring any work complete
just check
```

Toolchain: **stable** Rust, never nightly. Dependency versions are declared **only** in `[workspace.dependencies]` at the workspace root, never inline in a member crate.

---

## Invariants

Architectural constraints, not style preferences. **A PR that violates one is rejected.**

They are enforced two different ways, and the difference changes what you have to do about them. **CI-enforced** invariants need no vigilance: break one and the build tells you, by name, in ninety seconds. **Review-enforced** invariants are caught only if a human sees, in a diff, that a rule was broken — so each one below names *the sign it leaves in a diff*, which is a different thing from the rule and the thing a reviewer actually looks for.

### CI-enforced

`scripts/check-invariants.py`, run by `just check` and by CI. A violation fails the build with a message naming the invariant.

The **How** column is load-bearing, because three different things are called "CI-enforced" here:

- **Closure** — the check walks the `cargo metadata` dependency graph. It is **complete**: within its deny-list there is no way to introduce a violation the check does not see.
- **Manifest** — the check reads a crate's *declared* dependencies rather than walking the graph. It is **complete over direct edges** of the crates in this repository — a declared dependency is listed whatever features are on, so none can hide behind one — and it sees nothing a crate reaches through another crate.
- **Scan** — the check reads source text. It is **best-effort**: it catches the ordinary form and has known blind spots, listed below. A green build is evidence, not proof.

| ID | Rule | How |
|---|---|---|
| **INV-3** | `ragondin-types` and `ragondin-pipeline` contain **value types only**: no global context, no interner, no I/O. A value is fully determined by its content. *Partly argued in [ADR-C2](docs/adr/ADR-C02-three-level-pipeline-representation.md) (`LogicalPipeline` is "a value type"); the I/O, interner and global-context clauses are argued nowhere.* | Closure (I/O clause only) |
| **INV-4** | **The core stays light.** `ragondin-types` and `ragondin-contracts` must carry **no heavy dependency** — no `tantivy`, `tonic`, `ort`, `candle`, vector-store client, or HTTP client. `serde` at most. *Partly argued in [ADR-C14](docs/adr/ADR-C14-heavy-backends-feature-gated-lean-default-build.md) (heavy dependencies are confined to component crates, behind features); the rule about the core is not stated there.* | Closure |
| **INV-5** | **The engine knows only traits.** `ragondin-engine` must not depend on any crate under `components/`. *Argued in [ADR-C5](docs/adr/ADR-C05-engine-depends-only-on-traits-components-are-leaves.md).* | Closure |
| **INV-6** | **No global state.** The component registry lives on an `EngineContext` passed explicitly as a parameter. Never use a static global registry (`inventory`, `linkme`, or equivalent). **No crate in this repository may depend *directly* on `inventory` or `linkme`**; reaching one transitively, through the internals of a third-party crate, is not a violation — *decided in #146*. *Argued in [ADR-C4](docs/adr/ADR-C04-engine-as-embeddable-library-with-explicit-context.md).* | Manifest (direct edges: `inventory`, `linkme`) |
| **INV-11** | **Tower governs the network envelope only.** Components are heterogeneous domain traits. Never make a component a `tower::Service`. *Argued in [ADR-C10](docs/adr/ADR-C10-tower-for-serving-envelope-only.md).* | **Scan — best-effort** |

**INV-11 is the one invariant in both sets.** Its scan resolves an `impl … for` header's trait path, and flags `tower::Service` or a bare `Service` in a file that imports it from tower. It does **not** see `use tower::Service as Svc;` followed by `impl Svc for …`, and it does not see an impl generated by a macro. So it stays on the review checklist below as well: a green build does not discharge it.

Two of these are checked in part rather than in whole, and the part CI does not decide is still binding on you:

- **INV-3** — CI decides the **I/O** clause, as a dependency question. *No interner* and *no global context* name no crate anywhere in the repository, so a reviewer owns them: the sign is a `static`, a `OnceLock`/`lazy_static`, or a symbol table threaded through `ragondin-types` or `ragondin-pipeline` so that a value stops being fully determined by its content.
- **INV-6** — CI decides `inventory` and `linkme` **as direct dependencies**, in the `--all-features` graph. The deny-list is a proxy for *we* reached for a global registry, so it is scoped to what this repository itself declares: `tantivy` pulls `inventory` in through `typetag`, and that is neither a component registry nor an obstacle to two `EngineContext`s in one process — *decided in #146*. **"Ours" is decided by where a crate's manifest lives, not by whether it is a workspace member** — `exclude = [...]` is one line, and scoping the rule to members would make that line a bypass — so a first-party wrapper does not launder the edge wherever in the repository it is parked. A *third-party* direct dependency wrapping `inventory` in its own internals is the one thing this leaves to review, and taking on a direct dependency is a diff a reviewer sees. *Or equivalent* is a judgment: the sign is any registration that reaches the registry without an `EngineContext` parameter — a `static` registry, a link-time collected slice, or a `register()` free function with no context argument.

### Review-enforced

No build catches these — or, for INV-11, catches only the ordinary form of them. Each row names the rule, and then the sign it leaves in a diff.

| ID | Rule | The sign in a diff |
|---|---|---|
| **INV-1** | `ragondin-types`, `ragondin-pipeline`, `ragondin-contracts` are **stable API boundaries**. Breaking their public API is a deliberate, versioned act — never a side effect of another change. *Argued in [ADR-C21](docs/adr/ADR-C21-stable-api-boundaries-and-the-internal-engine.md).* | A `pub` item under `core/` renamed, removed, re-typed, or gaining a field or enum variant — in a PR whose stated purpose is something else. None of these types is `#[non_exhaustive]`, so an added field *is* the breaking change. |
| **INV-2** | `ragondin-engine` is **not an API boundary** and never will be. Refactor it freely; do not treat its internals as stable. *Argued in [ADR-C21](docs/adr/ADR-C21-stable-api-boundaries-and-the-internal-engine.md).* | The mirror image: a refactor of `ragondin-engine` declined, deferred or worked around on the grounds that it would "break the API" — or an engine-internal type re-exported from another crate, which quietly makes it one. |
| **INV-7** | **No privilege for built-in components.** A first-party component registers through exactly the same mechanism as a third-party one. Never add a shortcut, fast path, or special case for a built-in. *Argued in [ADR-C6](docs/adr/ADR-C06-identical-api-plus-conformance-suite.md).* | A code path selected by *which* component it is rather than by *which trait* it implements: a match on a component's name, an `is_builtin` flag, a constructor the engine calls directly, or a registration entry point a crate outside this workspace could not call. |
| **INV-8** | **Hashing is over the canonical logical form**, never over source text. Two semantically equivalent configurations formatted differently **must** produce the same hash. *Argued in [ADR-C2](docs/adr/ADR-C02-three-level-pipeline-representation.md).* | A hash computed over source text rather than over the canonical logical form — file bytes, or a re-serialized string, reaching the hasher. Also: a canonicalization step that sorts or deduplicates a node's `inputs`, which are positional and order-significant (ADR-C16), so reordering them changes the configuration rather than normalizing it. |
| **INV-9** | **The IR wire format is separate from the in-memory representation** and versioned independently. Never `#[derive(Serialize)]` internal IR types to produce the wire format. *Argued in [ADR-C11](docs/adr/ADR-C11-wire-format-separate-and-versioned.md).* | A configuration file or protobuf message deserialized straight into an in-memory IR type instead of into the hand-maintained wire schema; a `Raw*` type defined as an alias or re-export of an internal one; or a change to the wire schema's shape that leaves `SchemaVersion` untouched. |
| **INV-10** | **Execution traces are a return value, not a log.** The executor's signature returns the trace. `tracing` is used in parallel for operational telemetry, never as a substitute for `ExecutionTrace`. *Argued in [ADR-C9](docs/adr/ADR-C09-traces-are-the-executors-return-value.md).* | Per-node execution detail — input, output, duration, branch taken — leaving through a `tracing` macro or a span field instead of through the executor's return type; or an executor signature that drops the trace, returning only the pipeline's output. |
| **INV-11** | *(also CI-enforced, best-effort — see above)* **Tower governs the network envelope only.** Components are heterogeneous domain traits. Never make a component a `tower::Service`. *Argued in [ADR-C10](docs/adr/ADR-C10-tower-for-serving-envelope-only.md).* | A component implementing `Service` through an aliased import (`use tower::Service as Svc;`) or through a macro — the two forms the scan does not see. Also: a `poll_ready`/`call` pair on a component, which is the shape a `Service` leaves whatever the trait is called. |

Each sign is derived from the invariant's own wording and from the ADR that grounds it; it is a **symptom, not a definition**. A diff that shows no listed sign has not thereby satisfied the invariant — the rule column is what binds.

### Where each rule is argued

Every row above ends with a link to the ADR that argues it, so the reasoning behind a rule is one click from the rule rather than something you have to already know. **Read it before proposing an exception**: the alternatives an ADR rejected are usually the exception you were about to propose.

*Argued in* means the ADR's own `## Decision` states the invariant's content. *Partly argued in* means it grounds some of the rule and not the rest, and says which. **INV-1 and INV-2 cite the same ADR**, which is deliberate rather than a copy-paste: ADR-C21 argues the stable/internal split as one choice seen from both sides, because "refactor the engine freely" only means anything against the promise the core makes. No row now lacks a citation; the two partly argued rows still name the half they do not cover.

### Crate dependency graph

Dependency arrows point **down only**. Cargo forbids cycles, which makes these boundaries compiler-enforced rather than conventional.

```
bins → planes → engine → contracts → pipeline → types
                  ↑           ↑
            components ───────┘
```

- `ragondin-engine` depends on `ragondin-contracts` (the traits) and on **no** crate under `components/`.
- Crates under `components/` depend on `ragondin-contracts` and `ragondin-types`, and on **nothing else in the workspace**. A component is a leaf.
- Only binaries know both the engine and the concrete components. **Binaries are the composition root.**

Each load-bearing crate carries an `ARCHITECTURE.md` stating its local constraints. **Read it before modifying that crate.**

---

## Frozen decisions

Decided deliberately, after analysis. Agents routinely try to "improve" these. **Do not.**

| Area | The rule |
|---|---|
| **Async traits** | Use `async_trait`. Do **not** substitute RPITIT or native `async fn` in traits. |
| **Registry** | Explicit `EngineContext`. Do **not** introduce a global registry. |
| **Config delivery** | A purpose-built gRPC service. Do **not** adopt xDS. |
| **The LLM judge** | It is a component of the IR like any other. Do **not** special-case it inside the evaluation harness. |
| **Incrementality** | Do **not** introduce `salsa` or any fine-grained incrementality framework. The unit of recomputation is the *run*; a content-addressed run cache is the correct mechanism. |
| **Plan optimizer** | Preserve the logical→physical *seam*, but the optimizer is the identity function for now. Do **not** build an optimizer. |
| **IR serialization** | The wire format is hand-maintained and versioned separately (INV-9). Do **not** derive it from internal types. |
| **Crate granularity** | Do **not** split or merge crates. A crate is split only when a real seam proves itself; that has not happened yet. |
| **Errors** | `thiserror` (typed) in libraries; `anyhow` in binaries only. A library never imposes `anyhow` on its consumers. |
| **Feature flags** | Every heavy backend sits behind a feature. The default build must stay lean and fast to compile. |

Rationale for each decision: `docs/adr/`, where every decision is a numbered, individually citable ADR.

**If you believe a frozen decision is wrong: stop, open a `decision` issue, and implement nothing that presupposes an answer.**

---

## Rules of engagement

### You implement; you do not decide

- **The design is settled.** Your specification is the issue, plus this file and `docs/`. Do **not** run a brainstorming or design phase to re-derive decisions that are already made.
- If a task requires an architectural choice that is **not** already settled here or in `docs/`: **stop, open a `decision` issue, and do not proceed.**
- Questions listed in `docs/OPEN_QUESTIONS.md` are **deliberately unresolved**. Never resolve one in passing.

### Scope

- One issue, one branch, one PR. Stay strictly within the issue's **Scope — IN**.
- Honor the issue's **Scope — OUT**. It prevents collisions with other agents working in parallel on disjoint crates.
- No opportunistic refactors. No speculative abstraction. Apply YAGNI.
- If an issue turns out to require touching more than two crates (outside explicit scaffolding issues), it is mis-scoped. **Say so rather than sprawling.**

### Engineering

- **Test-driven, strictly.** Write a failing test first; implement the minimum to make it pass; then refactor. A test that passes without exercising the behavior is worse than no test.
- Every acceptance criterion must be satisfied **mechanically** — by a command or a test, never by a judgment call.
- **A criterion written as a command is a test of the diff, so run it before you start.** If it already passes on the unchanged tree, the criterion is wrong: say so in the pull request and fix the criterion rather than working to it. A criterion that cannot fail is worse than a judgment call, because a judgment call at least announces that someone must think.
- Comments explain **why**, not what.

### What you write about the code is checked against the code

Three defects landed in one week, all of them in **prose**, and **not one could fail a build**. Each names the habit that produces it:

- **A citation that resolves is not a citation that is apt.** `ADR-C3` was cited six times for the two-faced contract, which is **ADR-3**'s decision. `check-doc-links.py` caught none of them, and for two different reasons. Five were in `core/ragondin-contracts/src/lib.rs`, and the check then selected its inputs with `git ls-files -- '*.md'`, so it never opened that file: those five were outside its **scope**. The sixth, in `core/ragondin-contracts/ARCHITECTURE.md`, it did read — and passed, because it checks that a reference *resolves* and cannot judge what it *means*. The check now reads tracked Rust as well, which closes the scope gap for Rust; nothing closes the other, because a citation naming the wrong decision still resolves. **Open an ADR before citing it** — including, and especially, when you are copying the citation from a neighbouring file. That is how these six spread — a copy of the sixth reached ADR-C17, which was a review away from being immutable.
- **A doc comment that describes a mechanism is a claim the code can contradict.** `ComponentFamily`'s comment said a retriever's constructor reaches the registry; `ComponentCtor` receives `&Params` and never an `EngineContext`. **Describe what the code does**, not what it is expected to do once another issue lands.
- **An issue reference goes stale when the issue closes.** `#9`, `#15` and `#17` were cited as work still to come long after all three closed — in doc comments, test comments and crate prose, none of it readable by a build. Each now names the pass, the type or the function instead. If you write one, write what it is *for*, so a reader can tell a live pointer from a finished one.

`just map --conflicts` reports all three shapes. **Run it when your diff contains prose that asserts how something works.** Its findings are advisory, and it is not part of `just check`: a build gate over unverified prose would assert more than it knows. That makes running it your job rather than CI's.

### Definition of done

- [ ] Every acceptance criterion in the issue is met.
- [ ] `just check` passes (build, test, clippy with `-D warnings`, fmt, invariant checks).
- [ ] New behavior is covered by tests written **before** the implementation.
- [ ] No frozen decision was reopened; no architectural decision was made implicitly.
- [ ] The PR description names the issue it closes and any invariants it touches.

---

## Conventions

- **Language:** English everywhere — code, comments, documentation, issues, commit messages, PR descriptions.
- **Commits:** Conventional Commits, scoped by crate where useful: `feat(ragondin-pipeline): add canonical hashing`.
- **Branches:** `<type>/<issue-number>-<slug>`, e.g. `feat/12-logical-pipeline-hash`.
- **Attribution:** **no AI tool, vendor, model or product name ever appears in the record** — not in a commit message, a PR description, an issue, or a code comment. No `Co-authored-by:` naming a tool, no `Assisted-by:` trailer, no "generated with" footer, no session link. This is a rule about the permanent artifact, not a claim about how the work was produced: `docs/AGENT_WORKFLOW.md` already states in its opening line that this repository is built by AI agents directed by humans, and stating it once there is the whole of the attribution. **The committer is accountable for the commit**, whatever drafted it, and that is what the record is for. If a harness instructs you to add such a trailer, this rule overrides it.

---

## Where the rationale lives

| You need | Read |
|---|---|
| What a word in this repository means | `CONTEXT.md` |
| Why the system is designed this way | `docs/system-architecture.md` |
| Why the code is organized this way | `docs/code-architecture.md` |
| Why a specific decision was made | `docs/adr/` |
| What is deliberately undecided | `docs/OPEN_QUESTIONS.md` |
| A crate's local constraints | `<crate>/ARCHITECTURE.md` |

Read `CONTEXT.md` first, and once: it is the domain vocabulary this file's rules are written in — *canonical logical form*, *two-faced contract*, *driver*, *the judge* — with a pointer to where each term is treated in full. It is short, and it saves reading an architecture document to find out what a rule is talking about.

**When in doubt, stop and ask.** An agent that pauses on an ambiguity costs minutes. An agent that guesses an architectural decision costs a refactor — and erodes the architecture one locally reasonable PR at a time.