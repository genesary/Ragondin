# ADR-C11: The wire format is separate and versioned

## Context

If the wire format were derived directly from the in-memory representation (for instance by `#[derive(Serialize)]` on the internal IR types), the first refactor of those internal types would break every stored configuration and every pushed wire message.

## Decision

The IR **wire format is separate** from the in-memory representation and **versioned independently**. Never derive the wire format from internal IR types; it is hand-maintained.

## Alternatives rejected

- **`serde`-derived serialization on the internal representation.** Couples wire and in-memory representations, so any internal refactor breaks stored configurations and in-flight wire messages.

## Consequences

Stored configurations survive internal refactors. The wire format evolves under its own version, decoupled from internal churn — the serialization-side stability the platform's reproducibility guarantees depend on.

## Status

Accepted.
