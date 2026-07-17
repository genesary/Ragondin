# ADR-2: The pipeline representation is a graph with control flow

## Context

The pipeline representation is the data structure describing a RAG pipeline, **independent of the syntax used to write it** (YAML, a programmatic builder) and **of how it is executed** (the engine). It is the analogue of an abstract syntax tree or a bytecode, but for RAG, and it is the central abstraction of the project.

Corrective and agentic techniques — corrective RAG, self-RAG, adaptive RAG — involve **branches and loops decided at runtime**: *"retrieval quality is poor → rewrite the query → retrieve again."* A linear pipeline cannot express a branch; even a purely acyclic graph cannot express a loop decided at runtime.

## Decision

The representation is a **graph with first-class control-flow nodes**: conditional branches and bounded loops. Nodes are either component invocations or control-flow nodes; edges are data flow.

## Alternatives rejected

- **A linear pipeline.** Cannot express branching, so corrective and agentic techniques would require dedicated engine code — the exact outcome the architecture avoids.
- **A pure acyclic graph.** Can express fan-out and fan-in but not a loop decided at runtime (self-RAG's "retrieve again until a quality threshold is met").

## Consequences

Nearly every named technique becomes either a value of the `impl:` field or a `control:` node — none requires engine code, which is the composable-primitives thesis made concrete.

Because the representation *is* a graph and traces are captured **per node**, the user interface can render the graph and overlay execution on it (per-node replay), making debugging a RAG pipeline visual.

It also introduces a hard problem: node-based editors conventionally manipulate acyclic graphs, so rendering branch and bounded-loop nodes visually is a genuine interface-design challenge — tracked as an open question, not solved here.

## Status

Accepted.
