# ARCHITECTURE — rag-pipeline

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
- **No heavy dependency (INV-4).** `rag-types`, `serde`, and a hashing crate —
  nothing else.
- **The hash is over the canonical logical form (INV-8).** Two semantically
  equivalent configurations formatted differently **must** hash identically, or
  reproducibility is an illusion. Never hash source text.
- **The node enum is closed for primitives, open through `Extension`.** A
  genuinely new node type is expressed through `Extension` **without changing
  the core**. Repeated use of `Extension` for the same shape is the signal to
  promote it to a primitive — not a reason to special-case it here.
- **The wire format is separate (INV-9).** The serialized (wire) form is
  hand-maintained and versioned in `rag-config`/`rag-proto`. Never
  `#[derive(Serialize)]` these internal types to produce the wire format.
