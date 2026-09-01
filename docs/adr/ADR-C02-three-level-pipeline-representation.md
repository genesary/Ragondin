# ADR-C2: Three-level pipeline representation (Raw / Logical / Physical)

## Context

A pipeline must be deserialized permissively (from possibly malformed YAML or protobuf), validated and canonicalized so it can be content-addressed, and then resolved to concrete implementations so it can execute — three genuinely different concerns. Separately, two configurations that differ only by an interchangeable backend (Qdrant versus pgvector) should be *comparable* as the same strategy — a need that has to be met without weakening the identity of either run.

## Decision

Represent the pipeline in **three levels**:

- **`RawPipeline`** — a permissive deserialization target (strings, unresolved references). May be malformed. **Never executed.**
- **`LogicalPipeline`** — the validated, canonical form. It **names** each implementation (the `impl:` value) without resolving it. **Content-addressed**, serializable, a value type.
- **`PhysicalPipeline`** — implementations resolved to trait objects, defaults applied. Ready to execute; holds `Box<dyn>`; not serializable.

The content hash is computed over the **canonical `LogicalPipeline`**, never over source text.

## Alternatives rejected

- **Two levels (raw / validated).** The validated level would have to be both the content-addressed artifact and the executable one, and it cannot be: resolved implementations are `Box<dyn>` trait objects, so the level that holds them is not serializable and cannot be hashed. It would also fold into the hashed form two operations that must stay out of it — applying implementation defaults, and resolving an `Extension` node's kinds from the registry (ADR-C16) — so a configuration's identity would change with the registry it happened to be planned against. Keeping validation separate from resolution is also what lets `rag validate` reject a configuration with no `EngineContext` at all.

## Consequences

Content addressing is stable across formatting differences: two equivalent configurations formatted differently hash identically (INV-8). The concern of validation is cleanly separated from the concern of implementation resolution, so `rag validate` needs no registry.

**The logical hash identifies a configuration completely, backend included.** The `impl:` value is part of the logical form, so Qdrant and pgvector produce two different logical hashes. That is the property to want: a run's identity must suffice to reproduce it (P4), and the configuration promoted from bench to serving must pin the backend it was measured on, or evaluation/serving skew returns at the exact point P1 claims to remove it. Two backends also genuinely differ in their results — a bench that gave them one identity would be conflating them.

Recognising *"same RAG strategy, different backends"* therefore belongs to **comparison, not to identity**: it is a projection over the logical form that deliberately elides `implementation`, and it is computable from a complete hash whenever it is wanted. This ADR does not specify that projection. It records only that identity is the wrong place to obtain it, because what a lossy identity discards cannot be recovered.

## Amendments

**2026-09-01 — the backend-hash claim retracted.** As originally accepted, this ADR asserted that two configurations differing only by an interchangeable backend share the **same** logical hash, and rejected the two-level alternative on that basis. The assertion does not hold. The backend *is* the `impl:` value — `impl: qdrant_dense` in the system architecture §5.1 illustration — which `LogicalNode` carries as its `implementation` field, and which therefore enters the canonical form and the content hash. Two backends have two logical hashes.

The decision itself — three levels — is unchanged, and is better supported without the retracted claim than with it: the reasons that actually distinguish three levels from two are listed under *Alternatives rejected*, and none of them depends on it. What changed is the rationale, and the recognition that **complete identity** is the property to preserve.

Corrected in place rather than superseded by a new ADR, on the repository owner's decision: what was wrong was a factual claim inside the justification, never the decision this ADR records.

## Status

Accepted (amended 2026-09-01, see above).
