---
id: ADR-C24
title: The .proto is hand-maintained to mirror the domain types; tonic-build generates the Rust stubs
status: accepted
invariants: [INV-9]
supersedes: [ADR-C07]
superseded_by: null
---

# ADR-C24: The `.proto` is hand-maintained to mirror the domain types; `tonic-build` generates the Rust stubs

## Context

The component contract has two faces that must remain exact mirrors (ADR-3):
face 1 is the Rust trait in `ragondin-contracts`, implemented by a `Local`
component; face 2 is the protobuf service in `ragondin-proto`, implemented by a
`Remote` component in any language. ADR-C7 decided which of the two is the
source of truth — the domain types in `ragondin-types`, hand-written for Rust
ergonomics — and how drift between them is caught: round-trip property tests
in `ragondin-remote`, `from_proto(to_proto(x)) == x` for all `x` in the domain.

ADR-C7's Decision also states *how* face 2 comes to exist: *"The protobuf is
**generated** by `tonic-build`."* That sentence describes a mechanism the
toolchain does not perform. `tonic-build` reads a hand-written `.proto` file
and generates Rust — message structs through `prost`, client and server stubs
through `tonic`. Nothing it does, and nothing this workspace depends on,
produces a `.proto` from Rust types. The sentence is not a loose description
of the intended direction; it is the reverse of the actual one.

The error was harmless while `ragondin-proto` was a compiling skeleton. It
stops being harmless at the first `.proto` file: the issue that writes it
(#12) cites the sentence as its instruction, and an implementer following it
would either search for a generator that does not exist or contradict the ADR
silently. Two accepted documents already disagree over it. ADR-C17's
Consequences say the opposite — *"the `.proto` is hand-maintained to mirror
them"* — and `docs/code-architecture.md` §7.2, the `ragondin-proto` crate
docstring and `core/ragondin-types/ARCHITECTURE.md` all repeat ADR-C7's
version. A reader has no way to tell which one binds.

Process rule 2 cannot correct this in place. The sentence sits in the
**Decision** section, which changes only by supersession; and the part that
is wrong is not a fact the decision rested on but a statement of what the
decision *is*. So this ADR supersedes ADR-C7 in full, restates the decision
with the mechanism written the right way round, and carries everything else
forward unchanged. It was decided in #199.

## Decision

The **domain types in `ragondin-types` are the source of truth**, hand-written
for Rust ergonomics. Face 2 is produced from them in three hand-written steps
and one generated one:

1. The **`.proto` files in `ragondin-proto` are hand-maintained** to mirror
   the domain types field for field. They are the artifact a `Remote`
   component author in another language reads as the whole contract, and they
   are written for that reader.
2. **`tonic-build` generates the Rust stubs** — `prost` message types and
   `tonic` client and server traits — from those `.proto` files, in
   `ragondin-proto`'s `build.rs`. Nothing generates in the other direction.
3. **`ragondin-remote` hand-writes the conversions** between each domain value
   and its generated message type.
4. **Round-trip property tests in `ragondin-remote` keep the two faces in
   step**: for all `x` in the domain, `from_proto(to_proto(x)) == x`. They are
   a blocking CI check, and the mechanical guarantee that a change to a domain
   type which the `.proto` does not mirror fails the build rather than the
   first `Remote` call.

The round-trip property starts from a domain value, so it never produces a
message a domain value could not have produced — a field absent, an enum
number the Rust enum has no variant for. The `from_proto` direction is
therefore **not total**, and the guarantee is round-trip **plus negative
decode tests** for every message where the wire admits a value the domain
does not. ADR-C17's `EMBED_ROLE_UNSPECIFIED` is the first such case; it is
the general shape of a proto3 mirror of a closed Rust enum, not an exception
to it.

## Alternatives rejected

- **Generate the `.proto` from the domain types**, so that ADR-C7's sentence
  becomes true. No maintained tool does this for `prost`/`tonic`; a
  home-grown generator is a build step, a dependency and a new surface to
  test, all in service of a sentence. And a generated `.proto` is the worse
  artifact for the one reader it exists for: the `Remote` author who does not
  compile this workspace and needs the contract stable, legible and reviewed
  as text — which a hand-maintained file is and a build product is not.
- **Leave ADR-C7 as it stands and read "generated" loosely**, as a statement
  about which face follows which rather than about what runs. This costs
  nothing today and leaves the defect `AGENTS.md` § *What you write about the
  code is checked against the code* names — prose describing a mechanism the
  code does not perform — standing inside the one class of document least
  allowed to carry it, with an accepted ADR (C17) already contradicting it.
- **Retract the sentence in place under process rule 2.** Rule 2 amends
  Context, Alternatives and Consequences only; the sentence is in the
  Decision. Allowing an amendment there, on the grounds that the decision "did
  not really change", is the route around rule 1 that rule 2's boundary
  exists to close.
- **Protobuf-first, with the trait generated from the protobuf.** Rejected by
  ADR-C7 and rejected again here, for the same reason: it bends the domain
  model every component author touches to protobuf's shape rather than to the
  domain's.

## Consequences

- **The decision ADR-C7 made survives intact.** Source of truth, conversions
  in `ragondin-remote`, round-trip tests as the anti-drift guarantee: all
  carried forward. What changes is the sentence naming the mechanism, and
  every document that repeated it — `docs/code-architecture.md` §7.2, the
  `ragondin-proto` crate docstring, `core/ragondin-types/ARCHITECTURE.md` —
  changes with it, in the same pull request, so no two of them disagree
  again. ADR-C21 repeats the sentence in passing in its Context; it is an
  accepted ADR, and is retracted under process rule 2 in its own PR.
- **The `.proto` is a reviewed artifact, and drift is a review item as well
  as a test failure.** A change to a domain type now owes a matching edit to
  a hand-written file, and a reviewer can see in a diff whether it came. The
  round-trip test catches the case where it did not.
- **#12 writes the `.proto` by hand and sets up `tonic-build` to generate
  from it; #13 writes the conversions and the round-trip tests.** Both
  issues' text already describe this shape; #12's citation of ADR-C7 for the
  generated direction is read against this ADR instead.
- **Negative decode tests are part of the guarantee, not an optional extra.**
  Where the wire admits a value the domain does not — an unspecified or
  unknown enum number, an absent required field — the `Remote` adapter
  rejects it, and a test proves so. ADR-C17 already requires one for the
  embedding role; #13 owes the general case.
- **INV-9 is untouched.** It governs the *configuration* wire format
  (`RawPipeline`, ADR-C11), which is a separate hand-maintained schema. This
  ADR concerns the component-service face. The two are alike in being
  hand-maintained and unlike in what they mirror.

## Status

Accepted.
