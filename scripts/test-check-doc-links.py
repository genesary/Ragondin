#!/usr/bin/env python3
"""Tests for `scripts/check-doc-links.py` — proof that the gate still gates.

`check-doc-links.py` is a blocking CI step, and the only thing between a broken
ADR citation and `main`. Its failure mode is silence: a refactor that reverted the
scan to Markdown-only, or that stopped enforcing the ADR filename convention,
would keep printing `All documentation links resolve.` while resolving less. A
gate that cannot fail its own test is a gate nobody finds out has stopped gating.

So each case here pins a decision the checker makes, not merely the fact that it
runs. The two that matter most are the ones a refactor is most likely to undo by
accident:

  - a broken citation **in a Rust file** is reported — the scan reads `.rs`;
  - a dangling relative link in a Rust file is **not** reported, while the same
    line in a Markdown file is — the relative-link rule is Markdown-only, and
    that asymmetry is deliberate.

**Fixtures are built at run time, never committed.** The checker selects its
inputs with `git ls-files`, so a fixture committed to this repository would be
scanned by the real check — a deliberately broken citation would fail `just
check-doc-links` for real. Each case therefore initialises a throwaway git
repository under the system temporary directory, stages the fixture there, and
runs the checker with that directory as its working directory. Nothing this file
creates is inside the repository, and `git ls-files` here never sees a fixture.

Standard library only, and no test framework: the repository's Python tooling
carries no dependency by decision, and this file is tooling like the rest.

Run via `just test-check-doc-links`. Exit code 0 = every case passed; 1 = at least
one did not (the report names the case, what was expected and what was produced).
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Callable

SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
CHECKER = os.path.join(SCRIPTS_DIR, "check-doc-links.py")

# One real ADR from each series, so a fixture exercises both padding widths. The
# body carries the unpadded ID the way an ADR's own heading does.
ADR_SYSTEM = "docs/adr/ADR-004-one-engine-two-drivers.md"
ADR_CODE = "docs/adr/ADR-C03-closed-enum-plus-open-extension-variant.md"
ADR_FILES = {
    ADR_SYSTEM: "# ADR-4 — one engine, two drivers\n",
    ADR_CODE: "# ADR-C3 — closed enum plus open extension variant\n",
}


class Failure(Exception):
    """One expectation a case did not meet."""


class Result:
    def __init__(self, returncode: int, output: str) -> None:
        self.returncode = returncode
        self.output = output

    def exits(self, expected: int) -> "Result":
        if self.returncode != expected:
            raise Failure(f"expected exit code {expected}, got {self.returncode}")
        return self

    def says(self, expected: str) -> "Result":
        if expected not in self.output:
            raise Failure(f"expected {expected!r} in the output")
        return self

    def is_silent_about(self, unexpected: str) -> "Result":
        if unexpected in self.output:
            raise Failure(f"expected no mention of {unexpected!r} in the output")
        return self


def git(cwd: str, *args: str) -> None:
    """Run git in `cwd`, isolated from the caller's git environment.

    `GIT_DIR` and friends leak in from a parent process and would point this at
    the wrong repository; the config overrides keep a developer's global
    `core.excludesFile` from deciding what a fixture contains.
    """
    environment = dict(os.environ)
    for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"):
        environment.pop(name, None)
    environment["GIT_CONFIG_GLOBAL"] = os.devnull
    environment["GIT_CONFIG_SYSTEM"] = os.devnull
    environment["GIT_CONFIG_NOSYSTEM"] = "1"
    result = subprocess.run(
        ["git", *args],
        cwd=cwd,
        env=environment,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise Failure(f"git {' '.join(args)} failed in the fixture:\n{result.stderr}")


def run_checker(files: dict[str, str]) -> Result:
    """Build a throwaway repository holding `files` and run the checker in it.

    `files` maps a repository-relative path to its content. Every file is staged:
    the checker reads tracked files, and staging is enough for `git ls-files` —
    no commit, and so no committer identity, is needed.
    """
    root = tempfile.mkdtemp(prefix="check-doc-links-fixture-")
    try:
        for relative, content in files.items():
            path = os.path.join(root, relative)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(content)
        git(root, "init", "-q")
        # `-f`: a fixture is staged whatever any ignore rule in scope thinks.
        git(root, "add", "-f", "--", *sorted(files))
        completed = subprocess.run(
            [sys.executable, CHECKER],
            cwd=root,
            capture_output=True,
            text=True,
        )
        return Result(completed.returncode, completed.stdout + completed.stderr)
    finally:
        shutil.rmtree(root, ignore_errors=True)


CASES: list[Callable[[], None]] = []


def case(function: Callable[[], None]) -> Callable[[], None]:
    CASES.append(function)
    return function


@case
def a_resolving_citation_passes_in_markdown_and_in_rust() -> None:
    """The baseline: citations that resolve, in both languages the check reads.

    The count in the success message is asserted too. It is the only place the
    output says how much was read, so a scan that quietly stopped reading one of
    the two kinds would still be visible here.
    """
    run_checker(
        {
            **ADR_FILES,
            "README.md": (
                "One engine, two drivers:\n"
                "[ADR-4](docs/adr/ADR-004-one-engine-two-drivers.md).\n"
            ),
            "src/lib.rs": "//! The extension variant is ADR-C3's decision.\n",
        }
    ).exits(0).says("ADR naming OK — 2 ADR file(s)").says(
        "every citation in 3 Markdown and 1 Rust file(s) resolves"
    ).says("All documentation links resolve.")


@case
def a_citation_naming_no_adr_fails_in_markdown() -> None:
    """The file, the line and the reference are all in the message."""
    run_checker(
        {
            **ADR_FILES,
            "README.md": "# Notes\n\nThe two-faced contract is ADR-C99's decision.\n",
        }
    ).exits(1).says("BROKEN ADR REFERENCE").says(
        "README.md:3: ADR-C99 — no such ADR"
    ).says("Documentation link checks FAILED.")


@case
def a_citation_naming_no_adr_fails_in_rust() -> None:
    """The Rust half of the scan, which is the half a refactor drops silently."""
    run_checker(
        {
            **ADR_FILES,
            "src/lib.rs": "//! Retrieval.\n\n//! Grounded in ADR-C99.\n",
        }
    ).exits(1).says("BROKEN ADR REFERENCE").says("src/lib.rs:3: ADR-C99 — no such ADR")


@case
def a_citation_outside_a_doc_comment_is_still_a_citation() -> None:
    """The Rust scan is line-based on purpose: a citation is one wherever it sits."""
    run_checker(
        {
            **ADR_FILES,
            "src/lib.rs": 'const GROUNDS: &str = "ADR-98";\n',
        }
    ).exits(1).says("src/lib.rs:1: ADR-98 — no such ADR")


@case
def an_adr_filename_padded_to_the_wrong_width_fails() -> None:
    """A file the convention cannot place is a file nothing can cite."""
    run_checker(
        {"docs/adr/ADR-04-one-engine-two-drivers.md": "# One engine, two drivers\n"}
    ).exits(1).says("ADR NAMING VIOLATION").says(
        "ADR-04-one-engine-two-drivers.md is padded to 2, "
        "but the system series pads to 3 digits"
    )


@case
def an_adr_filename_with_no_slug_fails() -> None:
    run_checker({"docs/adr/ADR-004.md": "# One engine, two drivers\n"}).exits(1).says(
        "ADR NAMING VIOLATION"
    ).says("ADR-004.md does not match ADR-<number>-<slug>.md")


@case
def a_dangling_link_into_the_adr_directory_fails_in_markdown_only() -> None:
    """The deliberate asymmetry, and the one most likely to be undone by accident.

    The same line sits in a Markdown file and in a Rust file, in the same
    directory, so the link resolves to the same missing path from both. Only the
    Markdown one is a failure: a `](path)` link has no meaning in a doc comment,
    and rustdoc's intra-doc links are `cargo doc`'s business.
    """
    link = "The index lives at [the ADR index](../docs/adr/README.md).\n"
    run_checker(
        {
            **ADR_FILES,
            "src/notes.md": f"# Notes\n\n{link}",
            "src/lib.rs": f"//! Retrieval.\n\n//! {link}",
        }
    ).exits(1).says("BROKEN ADR REFERENCE").says(
        "src/notes.md:3: ../docs/adr/README.md — no such path"
    ).is_silent_about("src/lib.rs")


@case
def a_dangling_link_outside_the_adr_directory_is_not_this_checks_business() -> None:
    """Only links landing in `docs/adr/` are checked, however broken the rest are."""
    run_checker(
        {
            **ADR_FILES,
            "README.md": "See [the contributing guide](CONTRIBUTING.md).\n",
        }
    ).exits(0).says("All documentation links resolve.")


def main() -> int:
    if not os.path.exists(CHECKER):
        print(f"cannot find the checker at {CHECKER}", file=sys.stderr)
        return 1

    failed = 0
    print(f"check-doc-links — {len(CASES)} case(s)")
    for function in CASES:
        try:
            function()
        except Failure as failure:
            failed += 1
            print(f"  FAIL  {function.__name__}")
            for line in str(failure).splitlines():
                print(f"          {line}")
        else:
            print(f"  ok    {function.__name__}")

    if failed:
        print(
            f"\n{failed} of {len(CASES)} case(s) FAILED — check-doc-links.py no "
            "longer behaves as documented.",
            file=sys.stderr,
        )
        return 1
    print(f"\ncheck-doc-links.py behaves as documented — {len(CASES)} case(s) passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
