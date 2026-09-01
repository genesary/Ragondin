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

- **Two levels (raw / validated).** The validated level would have to be both the content-addressed artifact and the executable one, and it cannot be: resolved implementations are `Box<dyn>` trait objects, so the level that holds them is not serializable and cannot be hashed. It would also fold implementation defaults into the hashed form — planning materializes them into the plan (§6.3) — so a configuration's identity would shift with the registry it happened to be planned against. And it would put the hashed artifact out of reach without a registry at all, where the three-level split lets `rag validate` canonicalize, hash and reject a configuration with no `EngineContext` (ADR-C16).

## Consequences

Content addressing is stable across formatting differences: two equivalent configurations formatted differently hash identically (INV-8). The concern of validation is cleanly separated from the concern of implementation resolution, so `rag validate` needs no registry.

**The logical hash identifies a configuration completely, backend included.** The `impl:` value is part of the logical form, so Qdrant and pgvector produce two different logical hashes. That is the property to want, and the decisive reason is **P1**. What travels from bench to serving is the *configuration*: §7.2 of the system architecture states that "the configuration that wins the benchmark is **exactly** the configuration promoted to serving." A configuration hash that elided the backend would leave the promoted artifact ambiguous about the backend it was measured on — evaluation/serving skew, returning at the exact point P1 claims to eliminate it. Two backends also genuinely differ in their results, so one identity for both would be conflation.

Run identity (§7.1) would likely separate them anyway, since a Qdrant index and a pgvector index are distinct derived artifacts and `index_version` is part of the tuple. That coverage is incidental and identity should not rest on it: the property is wanted of the configuration hash itself.

Recognizing *"same RAG strategy, different backends"* therefore belongs to **comparison, not to identity**: it is a projection over the logical form that deliberately elides `implementation`, computable from a complete hash whenever it is wanted. Nothing in the platform currently requires it — `compare` reads runs by `run_id` — so this ADR does not specify it. It records only that identity is the wrong place to obtain it, because what a lossy identity discards cannot be recovered.

## Amendments

**2026-09-01 — the backend-hash claim retracted.** As originally accepted, *Alternatives rejected* read:

> - **Two levels (raw / validated).** Conflates *what* (the logical strategy) with *how* (the resolved backend). Two configurations differing only by an interchangeable backend would then get different hashes and could not be recognized as the same strategy — defeating a core use case of the bench.

and *Consequences* opened:

> Two configurations differing only by backend share the **same logical hash** but have distinct physical plans, which makes *"same RAG strategy, different backends"* a rigorous comparison.

Both are retracted. The assertion does not hold. The backend *is* the `impl:` value — `impl: qdrant_dense` in the system architecture §5.1 illustration — which `LogicalNode` carries as its `implementation` field, and which therefore enters the canonical form and the content hash. Two backends have two logical hashes.

The contradiction was already in the repository and already mechanical: issue #10, which implements the content hash, carries the acceptance criterion that *"changing a param that changes the computation … **or an impl name** changes the hash."* ADR-C2 and its own implementation issue could not both be satisfied. An ADR's claims should be checkable against the acceptance criteria of the issues that implement it; here they were not checked.

The *what/how* framing quoted above is why the claim looked true, which is why it is preserved rather than merely described: the same framing had already propagated into a doc comment in `rag-pipeline` before it was caught.

The decision itself — three levels — is unchanged, and is better supported without the retracted claim than with it: the reasons that actually distinguish three levels from two are listed under *Alternatives rejected*, and none of them depends on it. What changed is the rationale, and the recognition that **complete identity** is the property to preserve.

Corrected in place rather than superseded by a new ADR, on the repository owner's decision: what was wrong was a factual claim inside the justification, never the decision this ADR records.

## Status

Accepted (amended 2026-09-01, see above).
