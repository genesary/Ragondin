# ARCHITECTURE — ragondin-context-concat

**Status: a component, and therefore a leaf** of the dependency graph. Not an
API boundary: nothing in the workspace depends on it except, once it is wired,
a binary, which constructs it and registers it on an `EngineContext`.

## What lives here

One implementation of
[`ContextBuilder`](../../core/ragondin-contracts/src/lib.rs):
**ordered concatenation under a character budget**. `ConcatContextBuilder`
takes the chunks in the order it is handed them, joins their texts with a
separator fixed at construction, and stops at the first chunk that would take
the text over the per-call budget. It is the simplest context builder that is
real, and the baseline every other prompt-construction strategy is measured
against.

`Context.chunks` names the chunks that made it in, in the order placed, each
with its document and the score it carried in, untouched
([ADR-C31](../../docs/adr/ADR-C31-generation-contracts-template-and-served-model-per-call.md)
§ 1): the builder selects and renders, it never scores. `Context.text` is the
concatenation. The query is not read.

## The choices made here

Each of these is this crate's own, made inside the leaf under `AGENTS.md`
§ Rules of engagement, and recorded here so a reader can disagree with it.

- **The budget counts characters, not tokens.** ADR-C31 § 2 leaves the unit of
  `ContextParams::budget` to the implementation. A token count depends on a
  tokenizer, and the only tokenizer that would make it meaningful is the
  generator's, which this crate does not know and must not pull in — a
  tokenizer is the generator's business, behind its own face. Characters are
  the unit a builder can count without one.
- **A character is a `char`** — a Unicode scalar value — **not a byte.** A byte
  budget would make the same text cost twice as much in a language written
  mostly outside ASCII, which is a property of UTF-8 and not of the text. `é` is
  one character here.
- **Separators count toward the budget.** The budget caps `Context.text`, and
  the separators are in it. The whole text, separators included, is at most
  `budget` characters.
- **The first chunk that does not fit ends the context.** That chunk and every
  later one are left out whole: no chunk is truncated (a chunk is in or out),
  and the builder does not skip ahead to a smaller later chunk that would still
  fit. Skipping ahead would place a lower-ranked chunk in the context while a
  higher-ranked one was refused, which makes the context a packing rather than
  a prefix of the ranking — a different builder, and not one asked for. A first
  chunk longer than the whole budget therefore gives the empty context.
- **No reordering and no deduplication by document.** Each would be a different
  builder; the order handed in is the order placed.
- **A zero budget is refused before anything else**, including when no chunk is
  offered: zero is refused as `InvalidRequest` whatever the chunks
  ([ADR-C31](../../docs/adr/ADR-C31-generation-contracts-template-and-served-model-per-call.md)
  § 2). A non-zero budget over zero chunks is the empty context, `Ok`
  ([ADR-C19](../../docs/adr/ADR-C19-empty-collection-arguments-are-valid.md)).
- **The separator is the whole of the constructor configuration**, and may be
  empty. The budget is per call.
- **`model_identity` is a SHA-256 digest of the configuration**:
  `sha256:` and the hex digest of three fields — the builder's name
  `ragondin-context-concat`, its budget unit `char`, and its separator — each
  entered as its UTF-8 byte length (a little-endian `u64`) followed by its
  bytes. ADR-C31 § 4 requires the identity to be **complete** over everything
  that decides the output and is not in the node's params — for this builder,
  the separator and the unit its budget counts in — and **stable**; the length
  prefix is what keeps two configurations from sharing an encoding. The name and
  the unit are constants today and are digested anyway, so that a builder with
  a different unit, or a different builder with the same separator, cannot
  report the same identity. A test pins one digest, computed outside this
  crate, so a change to the scheme is a deliberate edit rather than a silent
  change to every recorded run's identity.

  The digest uses **`sha2`**, the `[workspace.dependencies]` entry the canonical
  logical-form hash already uses and the binary uses for model-file digests. It
  is used here, not added: no new utility role, and `std`'s `DefaultHasher` is
  not an option because its output is not guaranteed stable across Rust
  releases, which is the one property an identity must have.

## Local invariants

- **It is a leaf ([INV-5](../../AGENTS.md)).** It depends on
  `ragondin-contracts`, `ragondin-types`, `async-trait` and `sha2` — never on
  `ragondin-engine`, never on another component. The engine knows only traits
  ([ADR-C5](../../docs/adr/ADR-C05-engine-depends-only-on-traits-components-are-leaves.md)),
  so the arrow points from a binary to this crate and never the reverse.
- **No privilege for being built-in (INV-7).** It registers the way a
  third-party component registers, and it passes the same suite
  ([ADR-C6](../../docs/adr/ADR-C06-identical-api-plus-conformance-suite.md)) —
  `tests/conformance.rs` is the call a contributor writes, verbatim. The suite
  deliberately checks no budget, because it cannot know the unit;
  `tests/concat.rs` is where this builder's own budget, separator and
  provenance are checked.
- **Nothing blocks
  ([ADR-C25](../../docs/adr/ADR-C25-a-component-does-not-block-the-caller.md)).**
  `build` is one linear pass over text already in memory — no I/O, no model, no
  lock — so it runs inline on the caller's task and needs no runtime of its own.
- **The `concat` feature gates nothing heavy, on purpose.** There is no heavy
  dependency here to confine, so there is nothing to feature-gate in the sense
  [ADR-C14](../../docs/adr/ADR-C14-heavy-backends-feature-gated-lean-default-build.md)
  means, and the crate is always compiled. The feature exists so that every
  component crate names its implementation the same way — one feature per
  backend, named after it — and, gating nothing, it is on by default, as `rrf`
  is in `ragondin-fusion-rrf` (`components/README.md` § Which way a feature
  default goes).

## What is deliberately not here

No token counting, no tokenizer, no model; no template of its own beyond the
separator. Whether a context builder's template should become a parameter of
its node is left open by ADR-C31, and nothing here answers it.
