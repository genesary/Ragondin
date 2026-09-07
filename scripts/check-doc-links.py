#!/usr/bin/env python3
"""Documentation link checks — every ADR citation must resolve to a real file.

`AGENTS.md` sends an agent to `docs/adr/` for the rationale behind a frozen
decision. A citation that does not resolve does not read as a typo: it reads as a
missing record, and the agent re-derives the decision it was sent to read. That is
the architecture eroding through a broken filename, so the filenames get a wall
around them the same way the invariants do.

Two rules are enforced:

  Naming — every ADR file is `ADR-<number>-<slug>.md`, with the number padded to
           three digits in the system series (`ADR-004-…`) and to two digits after
           the `C` in the code series (`ADR-C03-…`). The convention is stated in
           `docs/adr/README.md`; this check keeps the directory matching it.

  Resolution — every `ADR-<n>` / `ADR-C<n>` reference in the repository's Markdown
           names an ADR that exists, and every relative link landing inside
           `docs/adr/` points at something that exists.

A reference resolves **by number**, not by string equality with the filename. The
ID an ADR carries in its own heading is unpadded (`ADR-4`) and its slug is written
by hand rather than derived from its title, so the filename cannot be
reconstructed from a citation — only looked up.

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


def markdown_files(root: str) -> list[str]:
    """Every tracked Markdown file, repository-relative.

    Tracked rather than walked: an untracked scratch file is not part of the
    repository's documentation, and `.gitignore` already says so.
    """
    result = subprocess.run(
        ["git", "ls-files", "-z", "--", "*.md"],
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


def check_references(root: str, files: list[str], index: dict):
    """Return [(file, line number, reference, reason), …]."""
    failures = []
    for relative in files:
        with open(os.path.join(root, relative), encoding="utf-8") as handle:
            lines = handle.read().splitlines()
        directory = os.path.dirname(relative)
        for number, line in enumerate(lines, start=1):
            for series, digits in ADR_REFERENCE.findall(line):
                if (series, int(digits)) not in index:
                    label = f"ADR-{series}{digits}"
                    failures.append((relative, number, label, "no such ADR"))
            for target in MARKDOWN_LINK.findall(line):
                path = link_into_adr_dir(root, directory, target)
                if path is not None and not os.path.exists(os.path.join(root, path)):
                    failures.append((relative, number, target, "no such path"))
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

    files = markdown_files(root)
    failures = check_references(root, files, index)
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
        print(f"ADR references OK — every citation in {len(files)} Markdown file(s) resolves.")

    if not ok:
        print("\nDocumentation link checks FAILED. See the messages above.", file=sys.stderr)
        return 1
    print("\nAll documentation links resolve.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
