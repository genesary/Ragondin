#!/usr/bin/env python3
"""Documentation link checks — every ADR citation must resolve to a real file.

`AGENTS.md` sends an agent to `docs/adr/` for the rationale behind a frozen
decision. A citation that does not resolve does not read as a typo: it reads as a
missing record, and the agent re-derives the decision it was sent to read. That is
the architecture eroding through a broken filename, so the filenames get a wall
around them the same way the invariants do.

Four rules are enforced:

  Naming — every ADR file is `ADR-<number>-<slug>.md`, with the number padded to
           three digits in the system series (`ADR-004-…`) and to two digits after
           the `C` in the code series (`ADR-C03-…`). The convention is stated in
           `docs/adr/README.md`; this check keeps the directory matching it.

  Resolution — every `ADR-<n>` / `ADR-C<n>` reference in the repository's tracked
           Markdown **and Rust** sources names an ADR that exists, and — in
           Markdown only — every relative link landing inside `docs/adr/` points
           at something that exists.

  Line citations — no file under `docs/adr/` cites source **by line number**
           (`validate.rs:357`, `plan.rs:114-119`). An accepted ADR is immutable
           (`docs/adr/README.md`, process rule 1) and so cannot follow the line
           it names: the next commit to that file leaves the citation pointing
           at unrelated code, and because a line number resolves to *some* line
           forever, no check downstream can ever report it stale. An ADR cites
           a symbol instead — a function, a type, a test by name — which grep
           finds and `just map` resolves. Scoped to the ADR directory because
           immutability is what makes the rot unfixable there; elsewhere a
           stale line number is an ordinary documentation bug the next edit
           corrects.

  Sections — every `<document> § <Heading>` reference in those same sources
           names a heading that exists in that document. #66 made a section
           name the primary citation form for the binding rules — a skill cites
           `AGENTS.md § Invariants` in place of the text it used to copy — and
           until then nothing resolved one, while the review of that same
           branch found three defects of exactly this shape. The document is
           named by path or by ADR id; a `§` with no document in front of it,
           and a section cited by *number* (`§4.3`), are deliberately not this
           rule's business.

A reference resolves **by number**, not by string equality with the filename. The
ID an ADR carries in its own heading is unpadded (`ADR-4`) and its slug is written
by hand rather than derived from its title, so the filename cannot be
reconstructed from a citation — only looked up.

Rust sources are read because that is where the citations live. Five of the six
wrong `ADR-C3` citations #100 corrected were in `core/ragondin-contracts/src/lib.rs`,
a file this check could not see at all. The scan is line-based and does not
distinguish a doc comment from any other line of Rust: a citation is a citation
wherever it is written.

The **relative-link** rule stays Markdown-only. A `](path)` link has no meaning in
a doc comment, and rustdoc's intra-doc links are a different mechanism that
`cargo doc` already validates.

What this check cannot do is judge whether a citation is **apt**. It resolves a
reference by number and nothing more, so `ADR-C3` cited for `ADR-3`'s decision
passes here — and so does a section reference naming the wrong existing heading. That one is a human habit — `AGENTS.md` § *What you write about the
code is checked against the code* — and not a check.

Run via `just check-doc-links`. Exit code 0 = every reference resolves; 1 = at
least one does not (the message names the file, the line and the reference).
"""

from __future__ import annotations

import os
import re
import subprocess
import sys

ADR_DIR = "docs/adr"

# The documented filename shape, plus the padding width of each series.
ADR_FILENAME = re.compile(r"^ADR-(C?)(\d+)-([a-z0-9]+(?:-[a-z0-9]+)*)\.md$")
PADDING = {"": 3, "C": 2}

# A citation: `ADR-4`, `ADR-C16`, and the numbered prefix of a filename or link.
# `ADR-N` in `000-template.md` is a placeholder, not a citation, and does not
# match — the number is required.
ADR_REFERENCE = re.compile(r"ADR-(C?)(\d+)")

# A Markdown inline link. Only the target matters here.
MARKDOWN_LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")

# A citation of source by line, in either of the two shapes an ADR has used: a
# path or bare filename with a source extension, a colon and a line number
# (`validate.rs:357`), or a backticked bare line number standing in for one
# after the file was named once (`:357`, `:347-407`). A range is either shape
# with a dash. `v0.26.2` has no extension and `ADR-C16` no colon, so neither
# matches; a bare `:357` outside backticks is prose, not a citation.
LINE_CITATION = re.compile(
    r"[\w./-]+\.(?:rs|py|toml|ya?ml|md):\d+(?:-\d+)?\b|`:\d+(?:-\d+)?`"
)

# A section citation: a document, then the sign, then a heading — `AGENTS.md`
# § Invariants, AGENTS.md (§ Rules of engagement), ADR-C2 § Amendments. The
# document is named by path or by ADR id, and only what sits between it and the
# sign is allowed to vary: a closing backtick, emphasis, a comma, an opening
# parenthesis. A `§` with no document in front of it belongs to the document it
# is written in and is not matched — `AGENTS.md` cites its own sections that
# way, and deciding which file an unqualified reference means would be a guess.
SECTION_REFERENCE = re.compile(
    r"(?:(?P<path>[\w./-]+\.md)|ADR-(?P<series>C?)(?P<number>\d+))"
    r"[`*_]*[,:]?\s*\(?\s*§\s*"
    r"(?P<heading>.*)"
)

# Where a citation's heading stops. It has no closing mark, so the sentence runs
# on after it; the punctuation below ends a clause and appears in no heading of
# this repository, while an em dash and a comma appear in several and so cannot
# be used to cut. Used for the message only — resolution reads the whole window.
CITATION_END = re.compile(r"[.;:)]")

# An ATX heading, with the optional closing hashes Markdown allows.
ATX_HEADING = re.compile(r"^ {0,3}#{1,6}\s+(.+?)\s*#*\s*$")

# How many lines a citation may run over. Prose wraps, and a heading long enough
# to be cited is long enough to wrap with it.
SECTION_WINDOW = 3

# What a wrapped line carries before its prose resumes: a doc-comment marker, a
# block-comment star, a blockquote arrow. Stripped when the window is joined, or
# a heading continuing under `///` reads as `Frozen /// decisions` and a citation
# that is correct fails the build.
CONTINUATION_MARKER = re.compile(r"^\s*(?://[/!]?|\*|>)\s*")


def repo_root() -> str:
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise SystemExit("not inside a git work tree")
    return result.stdout.strip()


def tracked_files(root: str, *patterns: str) -> list[str]:
    """Every tracked file matching `patterns`, repository-relative.

    Tracked rather than walked: an untracked scratch file is not part of the
    repository's documentation, and `.gitignore` already says so.
    """
    result = subprocess.run(
        ["git", "ls-files", "-z", "--", *patterns],
        capture_output=True,
        text=True,
        cwd=root,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise SystemExit("git ls-files failed")
    return sorted(p for p in result.stdout.split("\0") if p)


def index_adrs(root: str):
    """Return ((series, number) -> filename, [naming violations])."""
    index: dict[tuple[str, int], str] = {}
    violations: list[str] = []
    directory = os.path.join(root, ADR_DIR)
    for name in sorted(os.listdir(directory)):
        if not name.startswith("ADR-") or not name.endswith(".md"):
            continue
        match = ADR_FILENAME.match(name)
        if match is None:
            violations.append(f"{name} does not match ADR-<number>-<slug>.md")
            continue
        series, digits, _slug = match.groups()
        if len(digits) != PADDING[series]:
            series_name = "code" if series else "system"
            violations.append(
                f"{name} is padded to {len(digits)}, but the {series_name} "
                f"series pads to {PADDING[series]} digits"
            )
            continue
        key = (series, int(digits))
        if key in index:
            violations.append(f"{name} and {index[key]} claim the same ADR number")
            continue
        index[key] = name
    return index, violations


def check_references(root: str, files: list[str], index: dict, linked: set[str]):
    """Return [(file, line number, reference, reason), …].

    Citations are checked in every file in `files`. Relative links are checked in
    the files in `linked` — the caller's Markdown set, named rather than sniffed
    from the extension, so that adding a pattern cannot silently switch the rule
    off. The module docstring says why a `](path)` in a doc comment is not this
    check's business.
    """
    failures = []
    for relative in files:
        # `errors="replace"` rather than a decode error: a tracked file need not
        # be UTF-8 (a `.rs` fixture may deliberately not be), and a citation is
        # ASCII, so replacement loses no reference. A gate whose job is a legible
        # message must not exit through a traceback naming the file only in it.
        with open(os.path.join(root, relative), encoding="utf-8", errors="replace") as handle:
            lines = handle.read().splitlines()
        directory = os.path.dirname(relative)
        links_apply = relative in linked
        for number, line in enumerate(lines, start=1):
            for series, digits in ADR_REFERENCE.findall(line):
                if (series, int(digits)) not in index:
                    label = f"ADR-{series}{digits}"
                    failures.append((relative, number, label, "no such ADR"))
            if not links_apply:
                continue
            for target in MARKDOWN_LINK.findall(line):
                path = link_into_adr_dir(root, directory, target)
                if path is not None and not os.path.exists(os.path.join(root, path)):
                    failures.append((relative, number, target, "no such path"))
    return failures


def check_line_citations(root: str, files: list[str]):
    """Return [(file, line number, citation), …] for every line-number citation.

    The caller passes the files the rule applies to — the tracked Markdown under
    `docs/adr/` — named rather than sniffed from the path here, so that widening
    or narrowing the scope is a visible change at one call site.
    """
    failures = []
    for relative in files:
        with open(os.path.join(root, relative), encoding="utf-8", errors="replace") as handle:
            lines = handle.read().splitlines()
        for number, line in enumerate(lines, start=1):
            for citation in LINE_CITATION.findall(line):
                failures.append((relative, number, citation))
    return failures


def normalize_prose(text: str) -> str:
    """Prose as the eye reads it: no emphasis, no backticks, one space between.

    A citation is written `§ *Documentation ships…*` as readily as `§
    Documentation ships…`, and a heading is written in the document without
    either. Both sides are normalized the same way so the comparison is about
    the words.
    """
    for mark in ("`", "*", "_"):
        text = text.replace(mark, "")
    return " ".join(text.split())


def index_headings(root: str, relative: str) -> list[str]:
    """Every ATX heading in a document, normalized, longest first.

    Longest first because one heading can be a prefix of another: the longer is
    the one a citation naming it meant.
    """
    headings = []
    with open(os.path.join(root, relative), encoding="utf-8", errors="replace") as handle:
        for line in handle.read().splitlines():
            match = ATX_HEADING.match(line)
            if match is not None:
                headings.append(normalize_prose(match.group(1)))
    return sorted(set(headings), key=len, reverse=True)


def resolve_heading(candidate: str, headings: list[str]) -> str | None:
    """The heading `candidate` starts with, if any.

    A citation has no closing mark: the heading is followed by the rest of the
    sentence, so the reference resolves when the candidate *starts with* a real
    heading and the next character does not continue a word. Without that last
    condition `§ Scope` would resolve against a document whose only heading is
    `Scoped`.
    """
    for heading in headings:
        if not candidate.startswith(heading):
            continue
        rest = candidate[len(heading):]
        if not rest or not rest[0].isalnum():
            return heading
    return None


def locate_document(root: str, directory: str, path: str) -> str | None:
    """Where a cited document is, repository-relative, or `None`.

    The repository root first, which is how `AGENTS.md` and
    `docs/AGENT_WORKFLOW.md` are cited from anywhere; then the citing file's own
    directory and each directory above it, which is how a crate's
    `ARCHITECTURE.md` is cited from `src/lib.rs` inside that crate. Nearest
    first among those, because that is the file the citing crate means.
    """
    candidates = [path]
    while True:
        candidates.append(os.path.join(directory, path))
        if not directory:
            break
        directory = os.path.dirname(directory)
    for candidate in candidates:
        relative = os.path.normpath(candidate)
        if os.path.isfile(os.path.join(root, relative)):
            return relative
    return None


def check_section_references(root: str, files: list[str], index: dict):
    """Return [(file, line number, reference, reason), …].

    #66 made `AGENTS.md § <Heading>` the primary citation form for the binding
    rules, and nothing resolved one until this check. The review of that same
    branch found three defects of exactly this shape — a cited box that did not
    exist among them — each caught by a human read rather than by a build.

    Like the ADR rule, this resolves a reference and cannot judge that it is
    **apt**: a citation naming the wrong-but-existing section passes. That is
    the gap that let six wrong `ADR-C3` citations through until #100, and it is
    a human habit (`AGENTS.md` § What you write about the code is checked
    against the code), not a check.

    Two references are deliberately out of scope, and each is a decision rather
    than a limitation. A `§` with no document in front of it is a reference
    inside its own document. A heading that begins with a digit is a section
    *number* (`§4.3`) — the architecture documents cite themselves that way
    throughout, and a number is not a heading to look up.
    """
    failures = []
    headings_by_document: dict[str, list[str]] = {}
    for relative in files:
        with open(os.path.join(root, relative), encoding="utf-8", errors="replace") as handle:
            lines = handle.read().splitlines()
        directory = os.path.dirname(relative)
        for position, line in enumerate(lines):
            window = " ".join(
                [line]
                + [
                    CONTINUATION_MARKER.sub("", following)
                    for following in lines[position + 1:position + SECTION_WINDOW]
                ]
            )
            for match in SECTION_REFERENCE.finditer(window):
                # The citation belongs to the line its document sits on; the
                # window exists only so the heading may wrap onto the next.
                if match.start() >= len(line):
                    continue
                heading = normalize_prose(match.group("heading"))
                if not heading or heading[0].isdigit():
                    continue
                stop = CITATION_END.search(heading)
                cited = (heading[: stop.start()] if stop else heading)[:60].strip()
                path = match.group("path")
                if path is not None:
                    document = locate_document(root, directory, path)
                    label = path
                else:
                    label = f"ADR-{match.group('series')}{match.group('number')}"
                    name = index.get((match.group("series"), int(match.group("number"))))
                    if name is None:
                        # An ADR that does not exist is the ADR rule's finding,
                        # reported there with the message that fits it.
                        continue
                    document = f"{ADR_DIR}/{name}"
                if document is None:
                    failures.append(
                        (relative, position + 1, f"{label} § {cited}", "no such document")
                    )
                    continue
                if document not in headings_by_document:
                    headings_by_document[document] = index_headings(root, document)
                if resolve_heading(heading, headings_by_document[document]) is None:
                    failures.append(
                        (relative, position + 1, f"{label} § {cited}", "no such heading")
                    )
    return failures


def link_into_adr_dir(root: str, directory: str, target: str) -> str | None:
    """The repository-relative path a link points at, if it lands in `docs/adr/`.

    `None` for anything else: an absolute URL, a same-document anchor, or a
    relative link elsewhere in the tree. Only ADR links are this check's business.
    """
    if "://" in target or target.startswith(("#", "mailto:", "/")):
        return None
    path = os.path.normpath(os.path.join(directory, target.split("#", 1)[0]))
    if path == ADR_DIR or path.startswith(ADR_DIR + os.sep):
        return path
    return None


def main() -> int:
    root = repo_root()
    index, naming = index_adrs(root)
    ok = True

    if naming:
        ok = False
        print("ADR NAMING VIOLATION — a filename does not match the convention.")
        print("  `docs/adr/README.md` states the convention: ADR-<number>-<slug>.md,")
        print("  the number padded to three digits in the system series and to two")
        print("  after the C in the code series. A citation resolves by number, so a")
        print("  file the convention cannot place is a file nothing can cite.")
        for violation in naming:
            print(f"    {violation}")
    else:
        print(f"ADR naming OK — {len(index)} ADR file(s) match the documented convention.")

    markdown = tracked_files(root, "*.md")
    rust = tracked_files(root, "*.rs")
    failures = check_references(root, markdown + rust, index, set(markdown))
    if failures:
        ok = False
        print("BROKEN ADR REFERENCE — a citation does not resolve.")
        print("  An agent sent to an ADR that does not resolve concludes the record is")
        print("  missing and re-derives the decision. Cite an ADR by its unpadded ID")
        print("  (ADR-4, ADR-C3); link to it by its full filename")
        print("  (docs/adr/ADR-004-one-engine-two-drivers.md).")
        for path, number, reference, reason in failures:
            print(f"    {path}:{number}: {reference} — {reason}")
    else:
        print(
            f"ADR references OK — every citation in {len(markdown)} Markdown "
            f"and {len(rust)} Rust file(s) resolves."
        )

    adrs = [path for path in markdown if path.startswith(ADR_DIR + "/")]
    cited = check_line_citations(root, adrs)
    if cited:
        ok = False
        print("LINE-NUMBER CITATION — an ADR cites code by line.")
        print("  An accepted ADR is immutable, so it cannot follow the line it names:")
        print("  the next commit to that file leaves the citation pointing at unrelated")
        print("  code, and nothing can report it. Name the symbol instead — the function,")
        print("  the type, the test — which grep finds and `just map` resolves.")
        for path, number, citation in cited:
            print(f"    {path}:{number}: {citation}")
    else:
        print(f"ADR line citations OK — no file under {ADR_DIR}/ cites code by line number.")

    sections = check_section_references(root, markdown + rust, index)
    if sections:
        ok = False
        print("BROKEN SECTION REFERENCE — a § citation does not resolve.")
        print("  A section name is the citation form the binding rules are cited by,")
        print("  so a heading that does not exist sends a reader to a rule nobody can")
        print("  read. Cite a heading by its text as the document writes it.")
        for path, number, reference, reason in sections:
            print(f"    {path}:{number}: {reference} — {reason}")
    else:
        print(
            f"Section references OK — every '<document> § <Heading>' citation in "
            f"{len(markdown)} Markdown and {len(rust)} Rust file(s) resolves."
        )

    if not ok:
        print("\nDocumentation link checks FAILED. See the messages above.", file=sys.stderr)
        return 1
    print("\nAll documentation links resolve.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
