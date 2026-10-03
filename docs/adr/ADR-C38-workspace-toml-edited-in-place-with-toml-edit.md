---
id: ADR-C38
title: The settings file `workspace.toml` is read and edited in place with `toml_edit`, a dependency of `ragondin-api` alone; its schema is checked by meaning, and it is written by per-key operations that keep a person's comments
status: accepted
invariants: [INV-4, INV-12]
supersedes: []
superseded_by: null
---

# ADR-C38: The settings file `workspace.toml` is read and edited in place with `toml_edit`, a dependency of `ragondin-api` alone; its schema is checked by meaning, and it is written by per-key operations that keep a person's comments

## Context

`workspace.toml` holds a workspace's deployment settings: the datasets
directory and the `Remote` bindings, `"<family>/<name>" = <uri>`. They are
deployment data under ADR-C32 § 2, so an address stays out of every pipeline
document and every run identity. That is what lets a workspace be shared or
committed, and the front-end design presents the workspace that way
(`docs/design/2026-09-29-front-end-design.md` § 6). The file is therefore
edited by people as well as by the API, and the Setup screen (#345) will write
it on every change it makes to a service.

ADR-C36 § 6 lists the dependencies admitted for the UI's API, and it names no
TOML crate: "Any other entry the implementation finds necessary is a new
decision." A new `[workspace.dependencies]` entry that a crate outside
`components/` depends on escalates under `AGENTS.md` § Rules of engagement.
Neither `toml` nor `toml_edit` is in `Cargo.lock`.

### What the code does today

#342 (PR #373) therefore read and wrote the file by hand.

- **The reader.** `fs/settings_file.rs` in `ragondin-api` reads exactly the
  subset the settings need. `settings_file::parse` accepts blank lines and
  comments, a `datasets` string before any table, and one `[services]` table
  of one-line strings, and refuses everything else with its line. Review
  hardened it: control characters, foreign whitespace, an escape TOML does not
  define, and a byte-order mark are all refused. `Workspace::open` calls it
  before it creates anything, and creates a missing file from
  `settings_file::empty()`, which is the comment `HEADER` and nothing set.
- **The writer.** `WorkspaceSettings` has two methods, `read` and
  `write(Settings)`. `FsSettings::write` renders the whole file from the
  settings with `settings_file::render`, under `HEADER`, and replaces it with
  `write_atomically`. Every comment a person added is lost, and the `datasets`
  line is rewritten by every service write, whether or not it changed.
- **The race.** The handlers `bind` and `unbind` in `endpoints/services.rs`
  call `read`, change the `Settings`, then call `write`. They serialise against
  each other, but a hand edit made between the read and the write is reverted
  by it.

This is the pattern decision #371 closed for query strings (ADR-C37): an
in-house parser grown because the standard one escalates. Decision issue #374
put the question: `toml_edit`, `toml` with serde, or keeping the hand reader.

### What the pinned crate brings, measured for this decision

- **The pin**:
  `toml_edit = { version = "0.25", default-features = false, features = ["parse", "display"] }`.
- **Five packages**: `toml_edit`, `toml_parser`, `toml_datetime`,
  `toml_writer` and `winnow`. Each is licensed MIT and/or Apache-2.0, all on
  `deny.toml`'s allow list, and none duplicates a package already in the lock.
- **Its MSRV is 1.85**, the workspace's `rust-version`.
- **It is reached only through `ragondin-api`**, which only the binary's `ui`
  feature reaches. The default build and the core do not see it.
- **It duplicates no utility role.** No crate in the workspace serves TOML.

### The evidence

The controller recommended `toml_edit`. An independent challenger confirmed
the recommendation with amendments: the reading API that keeps spans, a schema
check by meaning rather than by spelling, per-key write operations in place of
a whole-file write, the write rules those operations must keep, and a
governance rule. The repository owner validated the amended version.

Decided in #374, by the repository owner on 2026-10-01: option 1, as amended
by the independent challenge.

## Decision

**`toml_edit` becomes a `[workspace.dependencies]` entry, used by
`ragondin-api` alone. The file is parsed into a `toml_edit::Document` and then
checked against its schema by meaning, every refusal naming its line.
`WorkspaceSettings` writes by per-key operations, each applied under the file
backend's lock to the document as it is on disk at that moment.**

### 1. The dependency

- **The root `Cargo.toml` gains the entry**
  `toml_edit = { version = "0.25", default-features = false, features = ["parse", "display"] }`,
  and `ragondin-api` is the only crate that depends on it.
- **This adds to ADR-C36 § 6 and does not supersede it.** That section
  closes its list with "Any other entry the implementation finds necessary is
  a new decision", so a new entry is what it provides for, and this ADR is
  that decision. ADR-C36 is untouched.

### 2. Reading

- **The file is parsed with `toml_edit::Document`**, which keeps each item's
  span. `toml_edit::DocumentMut` loses them, so it is not the reading type.
- **A schema check follows.** It accepts a top-level `datasets` string and one
  standard `[services]` table whose entries are `"<family>/<name>" = <string>`.
- **It refuses, naming the line from the span**:
  - any other key or table;
  - a sub-table, an array of tables, an inline table or a dotted key;
  - a value that is not a string;
  - a key without a `/` or with an empty part.
- **It refuses by meaning, never by string spelling.** The forms above are
  refused by name; otherwise a key or a value is read by what it means,
  however it is spelled. `toml_edit` itself writes `"""…"""` and `'…'`, so
  a refusal based on spelling would make the tool refuse its own output.
- **A parse error is reported as `file:line: message`.**

### 3. Writing

- **`WorkspaceSettings` exposes per-key operations**: bind a service or
  replace its address, unbind a service, and set or clear `datasets`. They
  replace the whole-file `write(Settings)`.
- **The file backend applies each operation inside its lock**, to the document
  read at that moment, then writes it atomically, as `FsSettings::write` does
  today.
- **A write must**:
  - keep the trailing comment — the decor — of a value it replaces;
  - move the prefix of a removed first key onto what follows, so that the
    file's header is not deleted with it;
  - place the comments of a file holding only comments above the first item it
    creates, which is the case of `empty()` and of the first `PUT` on a fresh
    workspace;
  - change `datasets` only when the operation changes it, comparing resolved
    paths;
  - perform no write when the operation changes no setting, so the file stays
    byte-identical.

This also closes today's race, in which a hand edit made between the read
and the write is reverted.

### 4. Governance

The rule that a hand-written parser or encoder of a standard format is not a
way around an escalation is stated once, in `AGENTS.md` § Rules of engagement.
This ADR cites it and does not restate it.

## Alternatives rejected

- **`toml`, through serde** (#374's option 2). Rejected because it still
  re-renders the whole file on write, so it solves the parsing and keeps the
  loss of every comment.
- **Surgical line edits on top of the hand reader.** Viable, but rejected
  because it grows an in-house parser and adds an in-house patcher beside it:
  the pattern ADR-C37 closes.
- **Keeping the hand reader and accepting the loss of comments** (#374's
  option 3). Rejected because it contradicts what the file is for: a shared
  file that people edit by hand.
- **JSON.** Rejected because it has no comments.
- **YAML, through `serde_yaml`.** Rejected because it re-renders the file on
  write, and the crate is deprecated.
- **Two files**, one for people and one written by the API. Rejected because
  it gives the settings two sources of truth.
- **A read-only parser that keeps spans.** Rejected because it cannot edit, so
  the writer would remain in-house.

## Consequences

- **CRLF line endings become LF on write**, and a byte-order mark is dropped.
- **TOML 1.1 forms are accepted**, among them the escapes `\e` and `\xHH`.
- **Every upgrade is a deliberate, reviewed bump**, since the crate is on a
  0.x line. No CI job checks the MSRV, so a `cargo update` could raise it
  above the workspace's `rust-version` unseen.
- **`fs/settings_file.rs` is replaced.** Its `HEADER` and `empty()` are kept,
  so a fresh workspace starts with the same file.
- **The `WorkspaceSettings` signature changes.** The trait is internal by
  ADR-C21's test (ADR-C36 § 2); the binary uses only `read`, whose signature
  is unchanged.
- **The implementation carries these tests, on a commented fixture**:
  - comments above, beside and below an entry survive adding, replacing and
    removing it;
  - the header survives removing `datasets`, and creating `[services]` on a
    fresh `empty()` file;
  - key order and blank lines are kept;
  - an operation that changes no setting leaves the file byte-identical;
  - a service write leaves the `datasets` line byte-identical;
  - every refusal names its line;
  - for any settings, what is written is read back, including values holding
    newlines, quotes and backslashes;
  - a binding added by hand between two API writes survives the second write.
- **#345 is unblocked by the implementation**, since its service writes then
  keep a person's comments.
- **INV-4 is untouched.** The crate is reached only under the binary's `ui`
  feature; the default build and the core do not reach it.
- **INV-12 is untouched.** `ragondin-api` gains a TOML crate, and no crate
  under `engine/` or `components/`.
- **Not decided here.** No entry in `docs/OPEN_QUESTIONS.md` is opened, closed
  or changed, and no frozen decision is reopened.

## Status

Accepted.
