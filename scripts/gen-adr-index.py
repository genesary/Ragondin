#!/usr/bin/env python3
"""Generate the ADR index in `docs/adr/README.md` from each ADR's front-matter.

Thirty-one decisions, and until now nothing listed them: answering "which ADRs
are still in force?" meant opening every file. The index answers it in one table,
and it is generated rather than written so that it cannot drift from the ADRs it
describes — a hand-maintained index of immutable documents rots silently, which is
the worst failure mode available to a document whose purpose is to be cited.

The front-matter is deliberately a flat, fixed shape, so it is parsed here with
the standard library rather than by taking on a YAML dependency for six fields.
Anything the parser does not recognize is an error, not a value to guess at.

  --check   Exit 1 if regenerating would change the file, without writing.
            This is what CI runs, so a stale index fails the build.

Run via `just gen-adr-index` (write) or `just check-adr-index` (verify).
Exit code 0 = the index is current; 1 = it is stale, or an ADR's front-matter is
malformed (the message names the file and the field).
"""

from __future__ import annotations

import os
import re
import subprocess
import sys

ADR_DIR = "docs/adr"
README = os.path.join(ADR_DIR, "README.md")

BEGIN = "<!-- BEGIN GENERATED ADR INDEX -->"
END = "<!-- END GENERATED ADR INDEX -->"

FIELDS = ("id", "title", "status", "invariants", "supersedes", "superseded_by")
STATUSES = ("accepted", "amended", "superseded", "proposed")

ADR_ID = re.compile(r"^ADR-C?\d+$")
INVARIANT_ID = re.compile(r"^INV-\d+$")
# The `ADR-004` / `ADR-C05` prefix of a filename — the id the front-matter must
# agree with.
FILENAME_ID = re.compile(r"^(ADR-C?\d+)-")


class Malformed(Exception):
    pass


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


def parse_front_matter(text: str, name: str) -> dict:
    """The six fields, as strings, lists of strings, or None.

    Strict on purpose: an unknown key or a missing one is an error. A front-matter
    block that is half understood produces an index that is half right, and a
    half-right index is cited as if it were whole.
    """
    lines = text.splitlines()
    if not lines or lines[0].strip() != "---":
        raise Malformed(f"{name}: no front-matter block")
    try:
        end = lines.index("---", 1)
    except ValueError:
        raise Malformed(f"{name}: front-matter block is never closed") from None

    parsed: dict = {}
    for line in lines[1:end]:
        if not line.strip():
            continue
        key, separator, raw = line.partition(":")
        if not separator:
            raise Malformed(f"{name}: front-matter line is not `key: value`: {line!r}")
        key = key.strip()
        if key not in FIELDS:
            raise Malformed(f"{name}: unknown front-matter field {key!r}")
        if key in parsed:
            raise Malformed(f"{name}: front-matter field {key!r} appears twice")
        parsed[key] = parse_value(raw.strip())

    missing = [f for f in FIELDS if f not in parsed]
    if missing:
        raise Malformed(f"{name}: front-matter is missing {', '.join(missing)}")
    return parsed


def parse_value(raw: str):
    if raw == "null":
        return None
    if raw.startswith("[") and raw.endswith("]"):
        inner = raw[1:-1].strip()
        return [item.strip() for item in inner.split(",")] if inner else []
    return raw


def validate(entry: dict, name: str) -> None:
    file_id = FILENAME_ID.match(name)
    if file_id is None:
        raise Malformed(f"{name}: filename carries no ADR number")
    if entry["id"] != file_id.group(1):
        raise Malformed(
            f"{name}: front-matter id {entry['id']!r} disagrees with the "
            f"filename, which says {file_id.group(1)!r}"
        )
    if not isinstance(entry["title"], str) or not entry["title"]:
        raise Malformed(f"{name}: title is empty")
    if entry["status"] not in STATUSES:
        raise Malformed(
            f"{name}: status {entry['status']!r} is not one of {', '.join(STATUSES)}"
        )
    for field in ("invariants", "supersedes"):
        if not isinstance(entry[field], list):
            raise Malformed(f"{name}: {field} must be a list")
    for invariant in entry["invariants"]:
        if not INVARIANT_ID.match(invariant):
            raise Malformed(f"{name}: {invariant!r} is not an INV-<n> identifier")
    for other in entry["supersedes"]:
        if not ADR_ID.match(other):
            raise Malformed(f"{name}: supersedes {other!r} is not an ADR id")
    superseded_by = entry["superseded_by"]
    if superseded_by is not None and not ADR_ID.match(superseded_by):
        raise Malformed(f"{name}: superseded_by {superseded_by!r} is not an ADR id")
    # The status word and the supersession pointer are two statements of the same
    # fact; a disagreement between them means one of the two is a lie.
    if (entry["status"] == "superseded") != (superseded_by is not None):
        raise Malformed(
            f"{name}: status is {entry['status']!r} but superseded_by is "
            f"{superseded_by!r} — the two must agree"
        )


def sort_key(entry: dict) -> tuple:
    match = re.match(r"^ADR-(C?)(\d+)$", entry["id"])
    assert match is not None  # validate() has already accepted the id
    return (match.group(1) != "", int(match.group(2)))


def read_adrs(root: str) -> list[dict]:
    entries = []
    directory = os.path.join(root, ADR_DIR)
    for name in sorted(os.listdir(directory)):
        if not name.startswith("ADR-") or not name.endswith(".md"):
            continue
        with open(os.path.join(directory, name), encoding="utf-8") as handle:
            entry = parse_front_matter(handle.read(), name)
        validate(entry, name)
        entry["filename"] = name
        entries.append(entry)
    return sorted(entries, key=sort_key)


def render(entries: list[dict]) -> str:
    lines = [
        BEGIN,
        "",
        f"<!-- Generated from each ADR's front-matter by `scripts/gen-adr-index.py`.",
        "     Regenerate with `just gen-adr-index`; do not edit this table by hand. -->",
        "",
        "| ADR | Title | Invariants | Status |",
        "|---|---|---|---|",
    ]
    for entry in entries:
        invariants = ", ".join(entry["invariants"]) if entry["invariants"] else "—"
        lines.append(
            f"| [`{entry['id']}`]({entry['filename']}) | {entry['title']} "
            f"| {invariants} | {entry['status']} |"
        )
    lines += ["", END]
    return "\n".join(lines)


def splice(readme: str, block: str) -> str:
    start = readme.find(BEGIN)
    end = readme.find(END)
    if start == -1 or end == -1:
        raise SystemExit(
            f"{README} has no generated-index block; expected the markers\n"
            f"  {BEGIN}\n  {END}"
        )
    if end < start:
        raise SystemExit(f"{README}: the index markers are in the wrong order")
    return readme[:start] + block + readme[end + len(END):]


def main(argv: list[str]) -> int:
    check_only = "--check" in argv[1:]
    root = repo_root()
    try:
        entries = read_adrs(root)
    except Malformed as error:
        print("ADR FRONT-MATTER VIOLATION — an ADR cannot be indexed.")
        print("  Front-matter describes a decision; it never changes one. The six")
        print("  fields are id, title, status, invariants, supersedes,")
        print("  superseded_by — see `docs/adr/README.md` § Front-matter.")
        print(f"    {error}")
        print("\nADR index generation FAILED. See the message above.", file=sys.stderr)
        return 1

    path = os.path.join(root, README)
    with open(path, encoding="utf-8") as handle:
        current = handle.read()
    updated = splice(current, render(entries))

    if updated == current:
        print(f"ADR index OK — {len(entries)} ADR(s), and the index in {README} is current.")
        return 0

    if check_only:
        print("STALE ADR INDEX — the generated table no longer matches the ADRs.")
        print("  The index is generated so that it cannot drift from the decisions it")
        print("  lists. Run `just gen-adr-index` and commit the result.")
        print("\nADR index is stale.", file=sys.stderr)
        return 1

    with open(path, "w", encoding="utf-8") as handle:
        handle.write(updated)
    print(f"ADR index regenerated — {len(entries)} ADR(s) written to {README}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
