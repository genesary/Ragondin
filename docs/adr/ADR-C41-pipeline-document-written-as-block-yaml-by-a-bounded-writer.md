---
id: ADR-C41
title: The server writes a pipeline document as block YAML, through a small writer of its own that covers the pipeline wire schema and nothing else, every output checked by the one reader
status: accepted
invariants: [INV-8]
supersedes: []
superseded_by: null
---

# ADR-C41: The server writes a pipeline document as block YAML, through a small writer of its own that covers the pipeline wire schema and nothing else, every output checked by the one reader

## Context

ADR-C40 § 5 has the server render a pipeline document, held in the wire schema
`RawPipeline` (`core/ragondin-pipeline/src/raw.rs`), to the configuration
format. It requires the rendering to read back to the same wire document, to
change nothing when rendered again, and to quote any scalar that another reader
of the format could take as a boolean or a date (`yes`, `on`, `2024-01-01`, …).

### What the code does today

The workspace's YAML library, `serde_yaml` 0.9, cannot meet that last
requirement. It quotes only what its own YAML 1.2-core reader would retype, and
it offers no control over a scalar's style.

PR #458 (#433 item 3) therefore renders **indented JSON**, which is YAML that
`ragondin-config` reads back to the same `RawPipeline`. The renderer is
`render_document` in `runtime/ragondin-config/src/document.rs`. It reads its
own output back through `read_document` and refuses, with `RenderError`, any
rendering that differs.

The repository owner agreed on 2026-10-04 to merge #458 with that renderer and
to settle the write format before #356, the editor writing pipeline files.
Until then, a pipeline saved from the canvas would be JSON inside a `.yaml`
file.

### The facts the decision rested on

- **INV-8.** The hash is over the canonical logical form, never the text. A
  reviewer's mutation in #458 swapped in a block-YAML renderer, and every
  round-trip and fixed-point test still passed. Changing the renderer changes
  no hash.
- **Who reads these files.** People keep them in git and edit them by hand.
  The repository owner wants YAML import and export that reads naturally.
- **Two routes were closed under the current rules.** A hand-written encoder of
  a standard format is not a way around an escalation (`AGENTS.md` § Rules of
  engagement, decided in #374, ADR-C38). A new workspace dependency must
  escalate.

### The options

Decision issue #462 put three options:

- **A — keep JSON.**
- **B — adopt a YAML library that controls each scalar's style.** This is a
  new workspace dependency.
- **C — allow a small, tested writer for the fixed wire schema only.**

Decided in #462, by the repository owner on 2026-10-04: option C, as amended by
an independent challenger. Two precisions were confirmed by the repository
owner on #462 on 2026-10-04: keys follow the same rules as values, and
`AGENTS.md` cites this ADR.

## Decision

**The server writes pipeline documents as block YAML, using a small writer of
its own. That writer covers the pipeline wire schema and nothing else.**

The output is what a person reads and edits by hand, which JSON is not. It
meets ADR-C40 § 5 without a second YAML library beside the reader the
workspace already has. The one library that met the quoting requirement was a
year old, had a single maintainer, and would have duplicated the serializer
role.

This is not an exception to the rule that a hand-written encoder is not a way
around an escalation (`AGENTS.md` § Rules of engagement). The writer was chosen
in the escalation itself, by the repository owner.

### 1. The writer's rules

- **Version line.** The document starts with `version: N`. A written file
  states the grammar it was written in, so a later build refuses it rather
  than reading it under another grammar.
- **Maps.** They are block style, in the wire schema's key order. Parameters
  come in the byte order of their keys. Nodes and inputs keep the document's
  order. A blank line separates nodes.
- **Lists.** They are flow style, and an empty list is written `[]`.
- **Plain strings.** A string is written plain only when all three hold:
  - it starts with an ASCII letter or `_`;
  - it continues with `[A-Za-z0-9_./-]`;
  - it is not `y`, `n`, `yes`, `no`, `true`, `false`, `on`, `off` or `null`,
    in any case.

  No YAML 1.1 or 1.2 reader can read such a string as anything else: every
  other implicit type begins with a digit, a sign, `.`, `~`, `=` or `<`.
- **Quoted strings.** Every other string is double-quoted. Inside the quotes,
  these are escaped: `"`, `\`, every character YAML does not count as
  printable, U+2028, U+2029 and U+FEFF. Characters outside the Basic
  Multilingual Plane are written as themselves, never as surrogate escapes.
- **Floats.** A float always has a fractional part. When it has an exponent,
  the exponent is signed. Its digits are the shortest that read back to the
  same value.
- **Integers and booleans.** An integer is written in decimal. A boolean is
  written `true` or `false`.
- **Keys follow the same rules as values**, as the repository owner confirmed
  on #462 on 2026-10-04. Parameter names, node ids, implementation names and
  every other string the writer emits are written plain or quoted under the
  one rule above.

### 2. What it refuses

A rendering that does not read back to the same wire document is refused. The
read-back guard enforces this. It covers a non-finite float, and a parameter
name longer than about 1024 written characters.

### 3. Its tests

- the read-back guard;
- a fixed-point test;
- a table of every letter-first YAML 1.1 boolean and null spelling;
- a round trip over every Unicode scalar value;
- a bit-exact float round trip over random bit patterns and the edge values
  (±0, the extremes, subnormals);
- a golden file in house style that renders byte-identical.

### 4. Its bounds

- **The writer only writes the wire schema.**
- **It reads nothing, edits no text in place, and writes no other document.**
- **Patching stored text stays ADR-C40 § 9's own decision.**
- **The writer is the private body of the existing render function in the
  configuration crate**, `render_document` in `ragondin-config`, beside the
  one reader.

### 5. Later

The writer moves to a library only when the YAML reader is replaced, on the
triggers already recorded for that reader in `deny.toml`. That library must
meet ADR-C40 § 5. The hand-written writer is then deleted.

### 6. The conditions that made this acceptable

A later case must meet **all** of these conditions:

- it was decided in an escalation;
- the schema is closed;
- the writer only writes;
- every output is checked by the one reader;
- the one library that passed failed on maintenance.

## Alternatives rejected

- **A, JSON.** Rejected because it is not what a person writes by hand.
- **B, a library with per-scalar style.** Its output is sound, but it is young
  and has a single maintainer. It would be a second YAML library beside the
  reader, would duplicate the serializer role, and would raise the declared
  minimum Rust version.
- **Replacing the reader now.** Rejected because it reopens a recorded decision
  before its triggers, changes a public error type, and re-reads every stored
  run configuration under new rules.
- **An already-maintained library that both reads and writes.** Rejected
  because its writer leaves dates and YAML 1.1 integers plain, and one octal
  spelling does not round-trip through its own reader.

## Consequences

- **No hash changes (INV-8).**
- **No existing hand-written pipeline file matches its rendering**, so the
  first canvas save of each one shows the ADR-016 § 5 warning.
- **The prose saying a configuration states its version only to pin it
  deliberately is corrected.**
- **ADR-C38 and ADR-C40 are not amended.**
- **`AGENTS.md` only cites this ADR**, as the repository owner confirmed on
  #462 on 2026-10-04. Beside the rule that a hand-written parser or encoder of
  a standard format is not a way around an escalation (`AGENTS.md` § Rules of
  engagement), it cites § 6 for the conditions a later case must meet. It adds
  no exception clause.
- **This ADR records the decision and lists the conditions that made it
  acceptable** (§ 6), so that a later case must meet all of them.

## Status

Accepted.
