# ARCHITECTURE — ragondin-pipeline

**Status: stable API boundary (INV-1).** Versioned; breaking its public API is a
deliberate act.

## What lives here

The pipeline representation, in **three levels**:

| Level | Role | Key property |
|---|---|---|
| `RawPipeline` | Permissive deserialization target — unresolved references | May be malformed. **Never executed.** |
| `LogicalPipeline` | Validated, canonical — names implementations, resolves none | **Content-addressed.** A value type. |
| `PhysicalPipeline` | Implementations resolved to trait objects | Ready to execute. Holds `Box<dyn>`. |

Plus the node graph and the open `Extension` variant.

`LogicalNode` reserves **no `Branch` or `Loop` variant today**, and `src/node.rs`
says so where someone about to add one will read it. That control flow belongs
in the representation at all is argued in
[ADR-2](../../docs/adr/ADR-002-pipeline-representation-is-a-graph-with-control-flow.md)
and sketched in [`docs/code-architecture.md`](../../docs/code-architecture.md)
§6.2; what a bounded loop's mandatory termination guard and a branch's predicate
actually look like is not settled, so neither variant exists yet.

## Local invariants

- **Value types (INV-3).** No global context, no interner, no I/O.
- **No heavy dependency (INV-4).** `ragondin-types`, `serde`, a hashing crate,
  and `thiserror` — nothing else. `thiserror` is *required*, not merely
  tolerated: [ADR-C13](../../docs/adr/ADR-C13-typed-errors-in-libraries-anyhow-in-binary.md)
  requires typed errors via `thiserror` in every library in this workspace,
  and INV-4 targets heavy runtime backends, not a proc-macro that leaves
  nothing in the dependency closure or the public API.
- **The hash is over the canonical logical form (INV-8).** Two semantically
  equivalent configurations formatted differently **must** hash identically, or
  reproducibility is an illusion. Never hash source text.
- **The node enum is closed for primitives, open through `Extension`.** A
  genuinely new node type is expressed through `Extension` **without changing
  the core**. Repeated use of `Extension` for the same shape is the signal to
  promote it to a primitive — not a reason to special-case it here.
- **A pipeline declares its inputs (ADR-C18).** `LogicalPipeline` carries
  them alongside its nodes. A declared input is a producer with no node, and
  the only producer of `ValueKind::Query` — no primitive variant is one. It
  shares one namespace with node ids, which is why a collision is refused.
- **The node variant is the sole source of a node's port kinds** — see
  [ADR-C16](../../docs/adr/ADR-C16-erased-edge-values-checked-before-execution.md),
  which is normative, and the module documentation on `node.rs`, which states
  the constraint where someone about to break it will read it.
- **The parameter grammar is flat, and the two parameter enums are extensible
  by design ([ADR-C22](../../docs/adr/ADR-C22-flat-parameter-grammar-extensible-param-enums.md)).**
  A parameter value is `String | Int | Float | Bool | List` in both models. A
  **nested map is rejected** — not forever, but until a configuration actually
  demands one, because with the two enums extensible a `Map` variant added
  later is additive **on the Rust boundary** rather than breaking. Only there:
  such a variant still bumps `SchemaVersion` under INV-9, and still owes the
  content hash a canonicalization one level deeper. A
  **null is rejected permanently**: a
  null parameter means *absent*, which the grammar already expresses by
  omitting the key, and two spellings of one configuration on a
  content-addressed boundary (INV-8) is a trap, not a convenience. A refused
  value must be diagnosed by **key**, naming what was expected; serde's
  untagged-enum message names neither. Two pieces of that decision are not in
  the code yet — the `#[non_exhaustive]` attribute below, and the diagnostic —
  and land together in its implementation. Tracked as #178.
- **The public enums are not `#[non_exhaustive]`, deliberately — except the two
  parameter enums, which ADR-C22 rules extensible (the attribute lands with its
  implementation).** `LogicalNode` is closed to
  outside crates only by convention, so a consumer may `match` it exhaustively
  and a new variant breaks that `match`. That is the intended signal while
  nothing is published: adding a primitive node kind **should** be a visible,
  deliberate act on a stable boundary (INV-1), not a silent one — and
  `LogicalNode` already has `Extension` as its additive escape hatch (ADR-C3).
  Note that adding `#[non_exhaustive]` later is itself a breaking change, so
  revisit this at the first published version, not after. Same choice, same
  reasoning, as `ragondin-types`. **`ParamValue` and `RawParamValue` do not
  share it.** They are data carriers, not vocabularies the compiler must
  defend: nothing silently does the wrong thing on meeting a parameter kind it
  cannot read, so ADR-C22 makes them extensible instead. Neither carries the
  attribute *today* — it lands with that ADR's implementation, together with
  the correction to `AGENTS.md`'s INV-1 row, whose "none of these types is
  `#[non_exhaustive]`" is still true until it does. Tracked as #178.
  `ValueKind`, `PortSpec` and
  `ValidationError` join the same stable surface under `LogicalNode`'s stance
  and not the parameter enums': an added `ValueKind` variant, a
  new `PortSpec` shape, or a new `ValidationError` variant is a visible,
  deliberate act on this boundary, not a silent one, and none of the three is
  `#[non_exhaustive]` either. `ValidationError::KindMismatch` carries
  `expected: Option<ValueKind>`, where `None` means the consumer declares no
  port at that position at all — chosen over inventing a fourth `ValueKind`
  variant for "no kind", which would have put a non-kind into ADR-C16's edge
  vocabulary that physical planning — `ragondin-engine`'s `plan_physical` —
  reuses. `SchemaVersionPeekError` joins that list on the same terms: it is not
  `#[non_exhaustive]` either, so a consumer may match its two diagnoses
  exhaustively and a third would be a visible, deliberate act on this
  boundary. It carries no `Clone`, `Copy`, `PartialEq` or `Eq`, because no real
  deserializer error implements them — a derive there would be decoration no
  caller could use and that could not later be withdrawn.
- **The wire format is separate (INV-9).** The serialized (wire) form is
  `RawPipeline` (`src/raw.rs`): hand-maintained, carrying its own
  `SchemaVersion`, and **structurally distinct** from the logical model — a
  configuration writes `top_k: 50`, which the externally tagged `ParamValue`
  cannot read at all, so the separation is forced rather than chosen. Never
  `#[derive(Serialize)]` the internal types to produce the wire format, and
  never lower one into the other implicitly. `ragondin-proto` holds the protobuf
  mirror of the same schema; `ragondin-config` reads files into it.
- **A version this build cannot read is a type, not a message.**
  `SchemaVersion`'s `Deserialize` refuses one through
  `serde::de::Error::custom`, which keeps the wording and erases the type, so
  `peek_schema_version` reads the `version` field alone — without the rest of
  the document being interpreted — and returns `UnsupportedSchemaVersion`
  unwrapped. It is generic over the deserializer: the caller supplies the
  format, because this crate carries no format implementation (INV-4) and does
  no I/O (INV-3). `ragondin-config` (#27) is the caller it exists for.
  Uninterpreted is not unread: the deserializer walks the whole document, so a
  syntax error anywhere outranks the version verdict and reads as
  `Unreadable` — which is correct, since what a version bump puts in doubt is
  meaning, not bytes. Its probe accepts only a mapping, hand-written rather
  than derived: `serde`'s derive also reads a struct positionally out of a
  sequence, which would take the JSON `[7]` for version 7 and answer a
  document that is not a configuration with a confident wrong diagnosis.
