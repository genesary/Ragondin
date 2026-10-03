#!/usr/bin/env python3
"""Tests for `scripts/gen-rust-notices.py`: proof that the notices still cover
what the binary links.

The generator's failure mode is a notices file that reads as complete while
leaving something out. A walk that stopped following an edge, a licence file it
stopped recognising, or a check that compared less than it says would each keep
`just check-rust-notices` green while the binary ships a crate with no notice.
So each case pins one decision the generator makes:

  - the crates listed are those reached from `ragondin` over **normal**
    dependency edges, transitively, on every platform. A dev-dependency or a
    build-dependency, which is never linked, is not listed. A crate of this
    repository is not listed. A path dependency from outside the repository
    is listed;
  - a crate's own licence and notice files are reproduced with their line
    endings normalised, and a text shipped by several crates is written once;
  - a crate that ships no licence file gets the standard text of every
    alternative of its licence that `deny.toml` allows and
    `scripts/licence-texts/` holds, and the authors its manifest names. If no
    alternative qualifies, generation fails and names the crate;
  - the output is the same whatever order `cargo metadata` lists packages in,
    and holds no absolute path;
  - `--check` passes on a current file and fails on a stale one, naming each
    crate that has no notice in it.

**Fixtures are built at run time, never committed.** Each case writes a
throwaway root under the system temporary directory, holding a `deny.toml`, a
`scripts/licence-texts/` directory and one directory per crate, and hands the
generator a `cargo metadata` document written by hand with `--metadata`. No
cargo, registry or network is involved.

Standard library only, and no test framework: the repository's Python tooling
carries no dependency by decision, and this file is tooling like the rest.

Run via `just test-gen-rust-notices`. Exit code 0 means every case passed, and
1 means at least one did not; the report names the case, what was expected and
what was produced.
"""

from __future__ import annotations

import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Callable

SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
GENERATOR = os.path.join(SCRIPTS_DIR, "gen-rust-notices.py")
OUTPUT = os.path.join("bin", "ragondin", "third-party-notices-rust.txt")

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"

DENY = """\
[licenses]
allow = [
    "Apache-2.0",
    "MIT",
]
"""

MIT_STANDARD = "MIT License\n\nCopyright (c) <year> <copyright holders>\n\nThe MIT text.\n"
APACHE_STANDARD = "Apache License\nVersion 2.0, January 2004\n\nThe Apache text.\n"


class Failure(Exception):
    """One expectation a case did not meet."""


class Result:
    def __init__(self, returncode: int, output: str, notices: str | None) -> None:
        self.returncode = returncode
        self.output = output
        self.notices = notices

    def exits(self, expected: int) -> "Result":
        if self.returncode != expected:
            raise Failure(
                f"expected exit code {expected}, got {self.returncode}; output was:\n"
                f"{self.output}"
            )
        return self

    def says(self, expected: str) -> "Result":
        if expected not in self.output:
            raise Failure(f"expected {expected!r} in the output; output was:\n{self.output}")
        return self

    def lists(self, expected: str) -> "Result":
        if self.notices is None or expected not in self.notices:
            raise Failure(f"expected {expected!r} in the notices; they were:\n{self.notices}")
        return self

    def omits(self, unexpected: str) -> "Result":
        if self.notices is not None and unexpected in self.notices:
            raise Failure(f"expected no {unexpected!r} in the notices; they were:\n{self.notices}")
        return self


class Fixture:
    """A throwaway root: `deny.toml`, the vendored texts, crates, and the
    `cargo metadata` document describing them."""

    def __init__(self) -> None:
        self.root = tempfile.mkdtemp(prefix="gen-rust-notices-")
        self.outside = tempfile.mkdtemp(prefix="gen-rust-notices-outside-")
        self.packages: list[dict] = []
        self.nodes: dict[str, list[dict]] = {}
        self.members: list[str] = []
        self.write(self.root, "deny.toml", DENY)
        self.write(self.root, "scripts/licence-texts/MIT.txt", MIT_STANDARD)
        self.write(self.root, "scripts/licence-texts/Apache-2.0.txt", APACHE_STANDARD)
        os.makedirs(os.path.join(self.root, "bin", "ragondin"), exist_ok=True)
        self.member("ragondin")

    def close(self) -> None:
        shutil.rmtree(self.root, ignore_errors=True)
        shutil.rmtree(self.outside, ignore_errors=True)

    @staticmethod
    def write(base: str, path: str, text: str | bytes) -> None:
        full = os.path.join(base, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        mode = "wb" if isinstance(text, bytes) else "w"
        with open(full, mode, **({} if isinstance(text, bytes) else {"encoding": "utf-8", "newline": ""})) as handle:
            handle.write(text)

    def _package(
        self,
        name: str,
        version: str,
        directory: str,
        source: str | None,
        license: str | None,
        authors: list[str],
    ) -> str:
        package_id = f"{name} {version}"
        self.write(directory, "Cargo.toml", f'[package]\nname = "{name}"\n')
        self.packages.append(
            {
                "id": package_id,
                "name": name,
                "version": version,
                "source": source,
                "license": license,
                "license_file": None,
                "authors": authors,
                "manifest_path": os.path.join(directory, "Cargo.toml"),
            }
        )
        self.nodes[package_id] = []
        return package_id

    def member(self, name: str) -> str:
        """A crate of this repository: under the root, with no source."""
        package_id = self._package(
            name, "0.0.0", os.path.join(self.root, "crates", name), None, "Apache-2.0", []
        )
        self.members.append(package_id)
        return package_id

    def crate(
        self,
        name: str,
        version: str = "1.0.0",
        license: str | None = "MIT",
        files: dict[str, str | bytes] | None = None,
        authors: list[str] | None = None,
        path_outside: bool = False,
    ) -> str:
        """A crate from outside the repository, shipping `files`."""
        if path_outside:
            directory = os.path.join(self.outside, f"{name}-{version}")
            source = None
        else:
            directory = os.path.join(self.root, "registry", f"{name}-{version}")
            source = REGISTRY
        package_id = self._package(name, version, directory, source, license, authors or [])
        for file, text in (files or {}).items():
            self.write(directory, file, text)
        return package_id

    def edge(self, parent: str, child: str, kind: str | None = None, target: str | None = None) -> None:
        self.nodes[parent].append(
            {"pkg": child, "dep_kinds": [{"kind": kind, "target": target}]}
        )

    def metadata(self, shuffle: int | None = None) -> str:
        packages = list(self.packages)
        nodes = [{"id": key, "deps": deps} for key, deps in self.nodes.items()]
        if shuffle is not None:
            random.Random(shuffle).shuffle(packages)
            random.Random(shuffle + 1).shuffle(nodes)
        document = {
            "packages": packages,
            "workspace_members": self.members,
            "resolve": {"nodes": nodes, "root": None},
            "workspace_root": self.root,
        }
        path = os.path.join(self.root, f"metadata-{shuffle}.json")
        with open(path, "w", encoding="utf-8") as handle:
            json.dump(document, handle)
        return path

    def run(self, *arguments: str, shuffle: int | None = None) -> Result:
        completed = subprocess.run(
            [
                sys.executable,
                GENERATOR,
                "--root",
                self.root,
                "--metadata",
                self.metadata(shuffle),
                *arguments,
            ],
            capture_output=True,
            text=True,
        )
        output_path = os.path.join(self.root, OUTPUT)
        notices = None
        if os.path.exists(output_path):
            with open(output_path, encoding="utf-8", newline="") as handle:
                notices = handle.read()
        return Result(completed.returncode, completed.stdout + completed.stderr, notices)


CASES: list[Callable[[], None]] = []


def case(function: Callable[[], None]) -> Callable[[], None]:
    CASES.append(function)
    return function


def with_fixture(body: Callable[[Fixture], None]) -> None:
    fixture = Fixture()
    try:
        body(fixture)
    finally:
        fixture.close()


@case
def only_crates_reached_over_normal_edges_from_the_binary_are_listed() -> None:
    def body(f: Fixture) -> None:
        root = f.members[0]
        library = f.member("ragondin-library")
        direct = f.crate("direct", files={"LICENSE": "direct licence\n"})
        transitive = f.crate("transitive", files={"LICENSE": "transitive licence\n"})
        windows = f.crate("windows-only", files={"LICENSE": "windows licence\n"})
        tested = f.crate("tested-only", files={"LICENSE": "dev licence\n"})
        built = f.crate("built-only", files={"LICENSE": "build licence\n"})
        outside = f.crate("outside-path", files={"LICENSE": "outside licence\n"}, path_outside=True)
        unrelated = f.crate("unrelated", files={"LICENSE": "unrelated licence\n"})
        f.edge(root, library)
        f.edge(library, direct)
        f.edge(direct, transitive)
        f.edge(root, windows, target="cfg(windows)")
        f.edge(root, tested, kind="dev")
        f.edge(root, built, kind="build")
        f.edge(root, outside)
        f.edge(library, unrelated, kind="dev")

        result = f.run().exits(0)
        for listed in ["direct 1.0.0", "transitive 1.0.0", "windows-only 1.0.0", "outside-path 1.0.0"]:
            result.lists(f"\n{listed}\n")
        for omitted in ["tested-only", "built-only", "unrelated", "ragondin-library", "\nragondin 0.0.0"]:
            result.omits(omitted)

    with_fixture(body)


@case
def licence_and_notice_files_are_reproduced_normalised_and_written_once() -> None:
    def body(f: Fixture) -> None:
        root = f.members[0]
        shared = "The shared MIT text.\r\nSecond line.\r\n\r\n"
        first = f.crate(
            "first",
            license="MIT OR Apache-2.0",
            files={
                "LICENSE-MIT": shared,
                "LICENSE-APACHE": "Apache text of first.\n",
                "NOTICE": "first's notice.\n",
                "README.md": "not a licence\n",
            },
        )
        second = f.crate("second", files={"COPYING": shared})
        f.edge(root, first)
        f.edge(root, second)

        result = f.run().exits(0)
        result.lists("The shared MIT text.\nSecond line.\n").lists("first's notice.").lists(
            "Apache text of first."
        )
        result.omits("\r").omits("not a licence")
        if result.notices.count("The shared MIT text.") != 1:
            raise Failure(f"a text shipped twice is written once; notices were:\n{result.notices}")
        result.lists("LICENSE-MIT: text ").lists("COPYING: text ").lists("NOTICE: text ")

    with_fixture(body)


@case
def a_crate_with_no_licence_file_gets_the_standard_text_of_each_allowed_alternative() -> None:
    def body(f: Fixture) -> None:
        root = f.members[0]
        bare = f.crate(
            "bare",
            license="MIT/Apache-2.0 / LGPL-2.1-or-later",
            authors=["Ada <ada@example.org>"],
        )
        f.edge(root, bare)

        result = f.run().exits(0)
        result.lists("The MIT text.").lists("The Apache text.")
        result.lists("ships no licence file").lists("Ada <ada@example.org>")
        result.lists("MIT's <year> <copyright holders> placeholder is filled by the line above")

    with_fixture(body)


@case
def a_crate_with_no_licence_file_and_no_usable_alternative_fails_naming_it() -> None:
    def body(f: Fixture) -> None:
        root = f.members[0]
        f.edge(root, f.crate("lgpl-only", license="LGPL-2.1-or-later"))
        f.edge(root, f.crate("unlicensed", license=None))
        f.edge(root, f.crate("compound", license="MIT AND Apache-2.0"))

        result = f.run().exits(1)
        result.says("lgpl-only 1.0.0").says("unlicensed 1.0.0").says("compound 1.0.0")
        if result.notices is not None:
            raise Failure("a failed generation writes no file")

    with_fixture(body)


@case
def the_output_is_deterministic_and_holds_no_absolute_path() -> None:
    def body(f: Fixture) -> None:
        root = f.members[0]
        for index in range(6):
            crate = f.crate(f"crate-{index}", files={"LICENSE": f"licence {index % 2}\n"})
            f.edge(root, crate)
        f.edge(root, f.crate("bare", authors=["Bo"]))

        first = f.run(shuffle=1).exits(0).notices
        second = f.run(shuffle=7).exits(0).notices
        if first != second:
            raise Failure("two package orders gave two files")
        for absolute in [f.root, f.outside, os.path.expanduser("~")]:
            if absolute in first:
                raise Failure(f"the notices hold the absolute path {absolute}")
        if not first.endswith("\n") or first.endswith("\n\n"):
            raise Failure("the file ends with exactly one newline")

    with_fixture(body)


@case
def the_check_passes_on_a_current_file_and_fails_naming_a_crate_with_no_notice() -> None:
    def body(f: Fixture) -> None:
        root = f.members[0]
        f.edge(root, f.crate("kept", files={"LICENSE": "kept licence\n"}))
        f.edge(root, f.crate("dropped", files={"LICENSE": "dropped licence\n"}))
        notices = f.run().exits(0).notices

        f.run("--check").exits(0).says("current")

        # A crate's entry deleted from the committed file: the check names it.
        lines = notices.split("\n")
        at = lines.index("dropped 1.0.0")
        edited = "\n".join(lines[: at - 1] + lines[at + 3 :])
        f.write(f.root, OUTPUT, edited)
        f.run("--check").exits(1).says("dropped 1.0.0").says("no notice")

        # A crate newly linked, and the file not regenerated: named too.
        f.write(f.root, OUTPUT, notices)
        f.edge(root, f.crate("added", files={"LICENSE": "added licence\n"}))
        result = f.run("--check").exits(1).says("added 1.0.0").says("no notice")
        if result.notices != notices:
            raise Failure("--check writes nothing")

    with_fixture(body)


def main() -> int:
    if not os.path.exists(GENERATOR):
        print(f"cannot find the generator at {GENERATOR}", file=sys.stderr)
        return 1

    failed = 0
    print(f"gen-rust-notices — {len(CASES)} case(s)")
    for function in CASES:
        try:
            function()
        except Failure as failure:
            failed += 1
            print(f"  FAIL  {function.__name__}")
            for line in str(failure).splitlines():
                print(f"          {line}")
        except Exception as error:  # noqa: BLE001 — a broken harness names itself
            failed += 1
            print(f"  ERROR {function.__name__}")
            print(f"          {type(error).__name__}: {error}")
        else:
            print(f"  ok    {function.__name__}")

    if failed:
        print(
            f"\n{failed} of {len(CASES)} case(s) FAILED — gen-rust-notices.py no "
            "longer behaves as documented.",
            file=sys.stderr,
        )
        return 1
    print(f"\ngen-rust-notices.py behaves as documented — {len(CASES)} case(s) passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
