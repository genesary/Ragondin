---
id: ADR-C40
title: A pipeline document crosses the internal API as a typed document that mirrors the wire schema's shape and whose every parameter value states its kind; the browser never parses or writes the configuration format, and the server alone renders it
status: accepted
invariants: [INV-8, INV-9]
supersedes: []
superseded_by: null
---

# ADR-C40: A pipeline document crosses the internal API as a typed document that mirrors the wire schema's shape and whose every parameter value states its kind; the browser never parses or writes the configuration format, and the server alone renders it

## Context

A pipeline document is a file in the configuration format, read into the
hand-maintained wire schema, `RawPipeline` (`core/ragondin-pipeline/src/raw.rs`),
and lowered from there (INV-9). The editor (ADR-016 § 1, § 3) has to carry such a
document from the server to the browser and back. How it travels was not
decided.

### The kind of a value is part of the pipeline

`ParamValue` (`core/ragondin-pipeline/src/node.rs`) keeps integers and floats
apart, and that kind enters the canonical hash, so `k: 60` and `k: 60.0` are
different pipelines (ADR-C22; INV-8). The server reads and emits the kind
faithfully. The browser does not: its number type turns `60.0` into `60`.

### What the code does today

- **The editor sends text the server reinterprets.** It holds the document as
  `WireDocument` (`ui/src/editor/document.ts`) and sends
  `validationRequest`'s `JSON.stringify(document)`, which the server reads as
  YAML through `parse_document` (`runtime/ragondin-config/src/document.rs`). A
  float typed `1.0` therefore arrives as the integer `1`. Its hash changes, and
  the run can fail when it starts. `ui/ARCHITECTURE.md` § The editor records
  this limit and defers it to #433 or #356.
- **Responses carry a parameter value without its kind.** `ParameterValue`
  (`runtime/ragondin-api/src/response.rs`) is untagged. It carries the
  parameters of a run's graph (`GraphNode`) and the values of a comparison's
  `ParameterRow`, converted from `ParamValue` by `parameter` in `convert.rs`.
  The run graph and the Compare matrix therefore share the problem: a row
  comparing `60` with `60.0` shows "60 | 60" as a difference. Integers above
  2^53 are also rounded silently by the browser.
- **A stored pipeline is served as text only.** `PipelineDetail` holds the
  document byte for byte, and `PipelineDocument` (`request.rs`) is the text
  `PUT /pipelines/{name}` and `POST /pipelines/validate` read. The editor holds
  a wire-schema document it does not parse from text, so it cannot open a
  stored pipeline (`ui/ARCHITECTURE.md` § The editor).

### The options

Decision issue #450 put four options:

- **A — a typed document.** Every parameter value states its kind, and the
  server alone renders YAML.
- **A' — edit operations applied to the stored text by a format-preserving
  editor.**
- **B — numbers as text.**
- **C — a parser of the configuration format in the browser.**

It blocks #433 item 3 (a pipeline document's graph), the rest of #355 (opening
stored pipelines, selection survival) and #356 (the editor writes pipeline
files).

Decided in #450, by the repository owner on 2026-10-04: option A, with the
challenger's amendments.

## Decision

**The API carries a pipeline document to and from the editor as a typed
document. It mirrors the wire schema's shape, and every parameter value in it
states its kind. The browser never parses or writes the configuration format.
The server alone renders it.**

### 1. The typed document

- **It has the wire schema's shape**: an optional schema version, the declared
  inputs in order, and the nodes in order, each with its id, family,
  implementation, inputs in port order, and parameters. That is the shape of
  `RawPipeline`, `RawGraph` and `RawNode`.
- **Every parameter value is tagged with its kind**: string, integer, float,
  boolean or list, and later any kind the parameter grammar adds (ADR-C22).
- **No kind is left untagged**, so a kind added later cannot be confused with
  an existing one.
- **An integer travels as decimal text**, because the browser's number type
  cannot hold every 64-bit integer.
- **A float travels as a number and must be finite.**

### 2. One parameter type across the API

Every response that carries a parameter value uses this tagged type: a run's
graph, the comparison of runs, and a pipeline document. An integer and a float
of the same value are never shown as the same value.

### 3. It is not the configuration format

- **The format on disk, its schema and its schema version are unchanged**
  (INV-9).
- **The typed document is the API's own type**, converted by hand to and from
  the wire schema. It is never derived from the wire schema or from the
  in-memory representation (ADR-C36 § 2).
- **It is versioned with the API's description**, and a change to the wire
  schema's shape changes it in the same change.

### 4. Reading

- **Reading a stored pipeline returns its text, as today.**
- **It also returns the typed document whenever the text reads into the wire
  schema**, whether or not the pipeline validates, and nothing when it does not
  read.
- **A document holding a value the typed document cannot carry**, such as a
  non-finite float, is returned as text only.
- **The configuration loader exposes reading text into the wire schema as the
  first half of its single load**, so the API has no second load of its own.

### 5. Validating and writing

- **The editor sends the typed document.** The server converts it to the wire
  schema, renders it to the configuration format, and loads that rendering
  through the single load.
- **The hash it reports is therefore the hash of exactly the bytes a write
  would store** (INV-8).
- **A write stores that rendering, and only when the pipeline validates.**
- **The rendering parses back to the same wire document, and rendering it
  again changes nothing.**
- **The renderer quotes scalars that another reader of the format could read
  as booleans or dates.**

### 6. Text keeps its own paths

Importing a document, forking a run's configuration byte for byte, and
validating text exactly as the command line does all keep sending text. No
request sends the editor's document as text in another format for the server to
reinterpret.

### 7. What a rewrite drops, said in full

When saving from the canvas would replace text the editor did not write, the
warning (ADR-016 § 5) names everything the rendering does not keep: comments,
formatting, key order, expanded references, and keys the wire schema does not
read.

### 8. The inspector

- **It shows each value's kind and lets the person change it.**
- **A float is drawn with a fractional part.**
- **A non-finite value is refused in words before it is sent.**
- **Text containing a comma, or the text `true`, stays a string** unless the
  person picks another kind.

### 9. Left open, deliberately

A write may later patch the stored text instead of re-rendering it, to keep a
person's comments and formatting. The editor and the API contract would not
change, because every write already carries the whole document and the version
of the text it was made from. That change needs its own decision. It is not
made by a hand-written editor for the format.

## Alternatives rejected

- **Numbers as text without a kind tag** (option B). Rejected because it is
  ambiguous with string values; once tagged it becomes this decision.
- **A parser of the configuration format in the browser** (option C). Rejected
  because it is a second reader of the grammar, which would disagree with the
  server on edge cases, plus a new runtime dependency.
- **Editing by operations applied to the stored text** (option A'). Rejected
  because it still needs typed values to show existing ones, implements the
  editor's edit rules a second time on the server, and needs a
  format-preserving editor this decision does not adopt.
- **Serving the lowered graph for the editor to turn back into a document.**
  Rejected because it rebuilds the wire form from the logical form, and it
  cannot open a document that does not validate.

## Consequences

- **A float written `1.0` stays a float from file to canvas to file, and its
  hash is unchanged.**
- **Integers keep their full width.**
- **The editor can open stored pipelines, including invalid ones.**
- **The known limit recorded in `ui/ARCHITECTURE.md` § The editor is
  withdrawn**, by the implementation that makes it untrue.
- **#433 item 3 is re-scoped**: it serves the typed document instead of a
  graph, and `runtime/ragondin-config` joins its scope.
- **Integers too wide for a 64-bit integer**, which the wire schema currently
  reads as floats, are a separate defect of the wire schema's parameter
  reading, tracked with #178.
- **ADR-016 is not amended.** § 5 requires that the editor never silently drop
  what its rendering cannot carry, and that the user be told before a text the
  editor did not write is replaced; § 7 above names what that warning says.
  ADR-016's Decision changes only by supersession (`docs/adr/README.md`
  process rule 1), and nothing here supersedes it.
- **ADR-C36 is not amended.** The typed document is one of the API's own types
  under § 2, and the browser still holds no implementation of the pipeline
  grammar (§ 1).
- **ADR-C22 is not amended.** The parameter grammar is unchanged, and no `Map`
  is added; a kind it adds later gains its own tag here (§ 1).
- **No format-preserving editor of the configuration format is adopted.**
  ADR-C38 adopted one for `workspace.toml`; whether a write patches a stored
  pipeline's text is left to its own decision (§ 9), and a hand-written editor
  for the format is not a way around that decision (`AGENTS.md` § Rules of
  engagement).
- **INV-9 is untouched**: the configuration format, the wire schema and
  `SchemaVersion` do not change. **INV-8 is kept by § 5**: the hash the editor
  shows is that of the canonical logical form of exactly the bytes a write
  stores.
- **Next**: the implementation issues, #433 item 3 re-scoped, then #356.

## Status

Accepted.
