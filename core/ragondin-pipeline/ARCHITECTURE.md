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

Plus the node graph, first-class control flow (`Branch`, `Loop` with a
mandatory termination guard), and the open `Extension` variant.

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
- **The node variant is the sole source of port kinds** — see
  [ADR-C16](../../docs/adr/ADR-C16-erased-edge-values-checked-before-execution.md),
  which is normative, and the module documentation on `node.rs`, which states
  the constraint where someone about to break it will read it.
- **The public enums are not `#[non_exhaustive]`, deliberately.** `LogicalNode`
  and `ParamValue` are closed to outside crates only by convention, so a
  consumer may `match` them exhaustively and a new variant breaks that `match`.
  That is the intended signal while nothing is published: adding a primitive
  node kind or a parameter kind **should** be a visible, deliberate act on a
  stable boundary (INV-1), not a silent one — and `LogicalNode` already has
  `Extension` as its additive escape hatch (ADR-C3). Note that adding
  `#[non_exhaustive]` later is itself a breaking change, so revisit this at the
  first published version, not after. Same choice, same reasoning, as
  `ragondin-types`. `ValueKind`, `PortSpec` and `ValidationError` (#9) join the
  same stable surface under the same stance: an added `ValueKind` variant, a
  new `PortSpec` shape, or a new `ValidationError` variant is a visible,
  deliberate act on this boundary, not a silent one, and none of the three is
  `#[non_exhaustive]` either. `ValidationError::KindMismatch` carries
  `expected: Option<ValueKind>`, where `None` means the consumer declares no
  port at that position at all — chosen over inventing a fourth `ValueKind`
  variant for "no kind", which would have put a non-kind into ADR-C16's edge
  vocabulary that issue #15 reuses.
- **The wire format is separate (INV-9).** The serialized (wire) form is
  `RawPipeline` (`src/raw.rs`): hand-maintained, carrying its own
  `SchemaVersion`, and **structurally distinct** from the logical model — a
  configuration writes `top_k: 50`, which the externally tagged `ParamValue`
  cannot read at all, so the separation is forced rather than chosen. Never
  `#[derive(Serialize)]` the internal types to produce the wire format, and
  never lower one into the other implicitly. `ragondin-proto` holds the protobuf
  mirror of the same schema; `ragondin-config` reads files into it.
