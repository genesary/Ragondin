# ADR-C2: Three-level pipeline representation (Raw / Logical / Physical)

## Context

A pipeline must be deserialized permissively (from possibly malformed YAML or protobuf), validated and canonicalized so it can be content-addressed, and then resolved to concrete implementations so it can execute — three genuinely different concerns. Separately, two configurations that differ only by an interchangeable backend (Qdrant versus pgvector) should be recognized as the *same* strategy for the purpose of comparison.

## Decision

Represent the pipeline in **three levels**:

- **`RawPipeline`** — a permissive deserialization target (strings, unresolved references). May be malformed. **Never executed.**
- **`LogicalPipeline`** — the validated, canonical form, independent of implementations. **Content-addressed**, serializable, a value type.
- **`PhysicalPipeline`** — implementations resolved to trait objects, defaults applied. Ready to execute; holds `Box<dyn>`; not serializable.

The content hash is computed over the **canonical `LogicalPipeline`**, never over source text.

## Alternatives rejected

- **Two levels (raw / validated).** Conflates *what* (the logical strategy) with *how* (the resolved backend). Two configurations differing only by an interchangeable backend would then get different hashes and could not be recognized as the same strategy — defeating a core use case of the bench.

## Consequences

Two configurations differing only by backend share the **same logical hash** but have distinct physical plans, which makes *"same RAG strategy, different backends"* a rigorous comparison. Content addressing is stable across formatting differences (two equivalent configurations formatted differently hash identically). The concern of validation is cleanly separated from the concern of implementation resolution.

## Status

Accepted.
