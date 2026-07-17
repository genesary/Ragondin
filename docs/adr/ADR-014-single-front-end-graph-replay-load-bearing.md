# ADR-14: Single front end; graph replay is load-bearing, visual authoring is a later trajectory

## Context

Pipeline composition, benchmarking and execution replay could live in one interface or in several juxtaposed tools. The load-bearing views are **run comparison** (the diff between two configurations across quality, cost and latency — the question the platform exists to answer) and **per-node execution replay** (the graph rendered, with what actually happened overlaid on each node). Visual node-based authoring is attractive but conventionally assumes acyclic graphs, whereas this representation contains control flow.

## Decision

A **single front end** hosts composition, benchmarking and replay. **Graph replay** — the graph in read mode, with execution overlaid per node — is **load-bearing and part of the core**. Visual graph *editing* is an explicit two-step trajectory: read mode first (core), visual editing added later as an additional front end over the same representation. **YAML-first authoring is the primary path for v0**, because the v0 audience is researchers who version configurations in git, submit them as pull requests, and script many variants.

## Alternatives rejected

- **A separate benchmarking tool juxtaposed with a composition tool.** Fractures the product into two interfaces for one coherent workflow.
- **Visual authoring in v0.** A text file is a better tool than a canvas for the v0 research audience, and rendering control flow visually is an unsolved design problem; building it first would delay the load-bearing views for a capability that serves the *next* audience.

## Consequences

One coherent product. Per-node replay is a direct dividend of the graph representation: because the representation is a graph and traces are per node, the interface can draw the graph and superimpose execution on it. Visual control-flow rendering is deferred to a later milestone and tracked as an open question.

## Status

Accepted.
