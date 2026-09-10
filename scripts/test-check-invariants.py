#!/usr/bin/env python3
"""Tests for `scripts/check-invariants.py` — proof that the gate still gates.

`check-invariants.py` is the step `ci.yml` calls "the most important gate", and
its failure mode is silence: a check that resolved a narrower graph, or that
stopped looking at the edge it is supposed to look at, would keep printing
`All architecture invariants hold.` while holding less. That is exactly how it
came to be blind: it resolved `cargo metadata` without `--all-features` while
every heavy backend sat behind a feature by rule (ADR-C14), so no component
crate's real dependencies were ever in the graph it walked, and nothing said so.
That is what #146 decided to close, and what this file exists to keep closed.

So each case here pins a decision the checker makes, not merely the fact that it
runs. The ones that matter most are the ones a refactor is most likely to undo:

  - the graph is resolved with `--all-features` — an optional dependency that no
    default feature enables is still *in* the graph the closure checks walk;
  - INV-6 fires on a **direct** dependency edge from a workspace member to
    `inventory` or `linkme`;
  - INV-6 does **not** fire on a transitive-only path through a third-party
    crate's own internals — the `tantivy` → `typetag` → `inventory` shape;
  - a first-party wrapper does not launder that edge, **whether or not it is a
    workspace member**. `exclude = [...]` takes a crate out of
    `workspace_members` in one line while leaving it in the repository, so
    "ours" is decided by where the manifest lives; both shapes are pinned.

**One property here is deliberately not pinned, and this is the record of it.**
`check_inv6` reads *declared* dependencies rather than resolved edges, and the
docstring leans on that: it is what makes the rule complete over a dependency
whose feature nobody enabled. No case below can tell the two apart. Under
`--all-features` every optional dependency resolves, so declared and resolved
coincide in any fixture this file can build — swapping the manifest read for the
crate's direct resolved edges leaves every case green. Pinning it would need a
fixture whose feature is *off*, which is the one thing `--all-features` rules
out. So the property is argued in `check_inv6`'s docstring and unenforced here;
a reader finding this gap has found a known limit, not an oversight.

**Fixtures are built at run time, never committed.** Each case writes a throwaway
cargo workspace under the system temporary directory and runs the checker with
that directory as its working directory. The stand-ins for third-party crates —
including a crate literally named `inventory` — are path dependencies *outside*
the fixture workspace root, so they sit outside the repository the checker calls
its own and no network or registry is needed to resolve them. `CARGO_HOME` is
redirected into the same throwaway directory and cargo is put offline, so a
developer's own cargo configuration cannot decide what a case resolves.

Standard library only, and no test framework: the repository's Python tooling
carries no dependency by decision, and this file is tooling like the rest.

Run via `just test-check-invariants`. Exit code 0 = every case passed; 1 = at
least one did not (the report names the case, what was expected and what was
produced).
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Callable, Iterable

SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
CHECKER = os.path.join(SCRIPTS_DIR, "check-invariants.py")

# The crates the checker names by hand: INV-3's and INV-4's protected sets, and
# INV-5's subject. A fixture without them fails on `member_id` before reaching
# anything a case is about, so every fixture carries all four.
REQUIRED_MEMBERS = {
    "core/ragondin-types": "ragondin-types",
    "core/ragondin-pipeline": "ragondin-pipeline",
    "core/ragondin-contracts": "ragondin-contracts",
    "engine/ragondin-engine": "ragondin-engine",
}


class Failure(Exception):
    """One expectation a case did not meet."""


class Result:
    def __init__(self, returncode: int, output: str) -> None:
        self.returncode = returncode
        self.output = output

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

    def is_silent_about(self, unexpected: str) -> "Result":
        if unexpected in self.output:
            raise Failure(
                f"expected no mention of {unexpected!r}; output was:\n{self.output}"
            )
        return self


def manifest(
    name: str,
    dependencies: Iterable[str] = (),
    features: Iterable[str] = (),
) -> str:
    """A minimal manifest. `dependencies` and `features` are whole TOML lines."""
    lines = [
        "[package]",
        f'name = "{name}"',
        'version = "0.0.0"',
        'edition = "2021"',
        "",
        "[dependencies]",
        *dependencies,
    ]
    if features:
        lines += ["", "[features]", *features]
    return "\n".join(lines) + "\n"


def write_crate(root: str, directory: str, text: str) -> None:
    path = os.path.join(root, directory)
    os.makedirs(os.path.join(path, "src"), exist_ok=True)
    with open(os.path.join(path, "Cargo.toml"), "w", encoding="utf-8") as handle:
        handle.write(text)
    with open(os.path.join(path, "src", "lib.rs"), "w", encoding="utf-8") as handle:
        handle.write("//! fixture crate\n")


def isolated_environment(cargo_home: str) -> dict[str, str]:
    """The caller's environment with every cargo-steering variable neutralised.

    A working directory is not on its own enough to say what `cargo metadata`
    will resolve. A developer's `$CARGO_HOME/config.toml` is read wherever the
    fixture sits, and a `[patch]` or `[net]` table in it would silently change
    what a case resolves — a suite whose result depends on whose laptop it runs
    on pins nothing. `CARGO_HOME` is redirected into the throwaway directory,
    which also makes the "these fixtures fetch nothing" claim testable rather
    than asserted: offline, with an empty registry, a case that reached for the
    network would fail instead of quietly succeeding.

    `CARGO_BUILD_TARGET` is stripped for the same reason: it decides which
    target-conditional dependencies resolve, and so which graph the closure
    checks walk.
    """
    environment = dict(os.environ)
    for name in ("CARGO_BUILD_TARGET", "CARGO_TARGET_DIR", "RUSTFLAGS"):
        environment.pop(name, None)
    environment["CARGO_HOME"] = cargo_home
    environment["CARGO_NET_OFFLINE"] = "true"
    return environment


def run_checker(
    members: dict[str, str] | None = None,
    excluded: dict[str, str] | None = None,
    outside: dict[str, str] | None = None,
) -> Result:
    """Build a throwaway workspace and run the checker in it.

    Three buckets, because "is this crate ours?" is exactly what INV-6 turns on
    and the three answers have to be expressible separately:

      - `members` — workspace-relative directories listed in `members = [...]`.
        The four crates the checker names by hand are added with empty manifests
        unless a case overrides one.
      - `excluded` — directories **inside** the workspace root that are named in
        `exclude = [...]`, so they are first-party code that is not a workspace
        member. Without the exclusion cargo would make a path dependency living
        here a member automatically.
      - `outside` — directories *beside* the workspace, which is where a stand-in
        for a third-party crate goes: reached by path, so nothing is fetched, but
        outside the repository the checker calls its own.
    """
    members = dict(members or {})
    excluded = dict(excluded or {})
    outside = dict(outside or {})
    for directory, name in REQUIRED_MEMBERS.items():
        members.setdefault(directory, manifest(name))

    base = tempfile.mkdtemp(prefix="check-invariants-fixture-")
    try:
        root = os.path.join(base, "workspace")
        cargo_home = os.path.join(base, "cargo-home")
        os.makedirs(root)
        os.makedirs(cargo_home)
        listed = ",\n".join(f'    "{d}"' for d in sorted(members))
        table = f'[workspace]\nresolver = "2"\nmembers = [\n{listed},\n]\n'
        if excluded:
            skipped = ", ".join(f'"{d}"' for d in sorted(excluded))
            table += f"exclude = [{skipped}]\n"
        with open(os.path.join(root, "Cargo.toml"), "w", encoding="utf-8") as handle:
            handle.write(table)
        for directory, text in {**members, **excluded}.items():
            write_crate(root, directory, text)
        for directory, text in outside.items():
            write_crate(base, directory, text)
        completed = subprocess.run(
            [sys.executable, CHECKER],
            cwd=root,
            env=isolated_environment(cargo_home),
            capture_output=True,
            text=True,
        )
        return Result(completed.returncode, completed.stdout + completed.stderr)
    finally:
        shutil.rmtree(base, ignore_errors=True)


CASES: list[Callable[[], None]] = []


def case(function: Callable[[], None]) -> Callable[[], None]:
    CASES.append(function)
    return function


@case
def a_clean_workspace_passes() -> None:
    """The baseline, and the shape of INV-6's success message.

    The crate count is asserted so that a check which silently stopped walking
    the whole workspace reads as a changed number rather than as a green tick.
    """
    run_checker().exits(0).says(
        "INV-6 OK — none of the 4 first-party crate(s) declares inventory or "
        "linkme as a direct dependency."
    ).says("All architecture invariants hold.")


@case
def a_direct_dependency_on_inventory_is_a_violation() -> None:
    """INV-6's rule, in the form it is written to catch (#146)."""
    run_checker(
        members={
            "engine/ragondin-engine": manifest(
                "ragondin-engine",
                ['inventory = { path = "../../../inventory" }'],
            )
        },
        outside={"inventory": manifest("inventory")},
    ).exits(1).says("INV-6 VIOLATION").says(
        "ragondin-engine declares global-registry crate as a direct "
        "dependency: inventory"
    )


@case
def a_direct_dependency_on_linkme_is_a_violation() -> None:
    """Both names on the deny-list are decided by the build, not one of them."""
    run_checker(
        members={
            "core/ragondin-contracts": manifest(
                "ragondin-contracts",
                ['linkme = { path = "../../../linkme" }'],
            )
        },
        outside={"linkme": manifest("linkme")},
    ).exits(1).says("INV-6 VIOLATION").says(
        "ragondin-contracts declares global-registry crate as a direct "
        "dependency: linkme"
    )


@case
def inventory_inside_a_third_party_crate_is_not_a_violation() -> None:
    """The `tantivy` → `typetag` → `inventory` shape, which #146 decided is fine.

    The backend is a path dependency outside the workspace, so it is a third
    party to this workspace in exactly the way the rule means: the workspace
    declares the backend, and the backend declares the registry crate.

    The count in the message is asserted too, and it is the other half of the
    case: five first-party crates, not seven. `first_party_ids` follows path
    dependencies, and a rule that decided "ours" by that alone — rather than by
    the manifest being inside the repository — would swallow both stand-ins here
    and report a violation.
    """
    run_checker(
        members={
            "components/backend-user": manifest(
                "backend-user",
                ['third-party-backend = { path = "../../../third-party-backend" }'],
            )
        },
        outside={
            "third-party-backend": manifest(
                "third-party-backend",
                ['inventory = { path = "../inventory" }'],
            ),
            "inventory": manifest("inventory"),
        },
    ).exits(0).says(
        "INV-6 OK — none of the 5 first-party crate(s) declares inventory or "
        "linkme as a direct dependency."
    ).is_silent_about("INV-6 VIOLATION")


@case
def a_first_party_wrapper_does_not_launder_the_edge() -> None:
    """#146 raised this as the rule's weakness. It is not one — the easy half.

    A thin first-party wrapper around `inventory` carries the direct edge
    itself, so the wrapper is what the check reports: nothing slips past, the
    offender is simply named one crate further in than the crate consuming it.
    Here the wrapper is a workspace member, which is the shape that needs no
    thought. The case below removes it from the member list, which is the shape
    that does.
    """
    run_checker(
        members={
            "components/wrapper": manifest(
                "wrapper", ['inventory = { path = "../../../inventory" }']
            ),
            "engine/ragondin-engine": manifest(
                "ragondin-engine", ['wrapper = { path = "../../components/wrapper" }']
            ),
        },
        outside={"inventory": manifest("inventory")},
    ).exits(1).says("INV-6 VIOLATION").says(
        "wrapper declares global-registry crate as a direct dependency: inventory"
    )


@case
def an_excluded_first_party_wrapper_is_still_caught() -> None:
    """The bypass a membership-based rule leaves open, and the reason for none.

    `exclude = [...]` — one line — moves a crate out of `workspace_members`
    while leaving it in the repository, in the graph, and compiled into the same
    process. A thin wrapper parked there is first-party code reaching for a
    global registry, which is the case INV-6 exists for, so "ours" is decided by
    where a crate's manifest lives and not by whether someone remembered to list
    it. A nested `[workspace]` table in the sub-crate does the same thing to
    membership and is caught the same way.
    """
    run_checker(
        members={
            "engine/ragondin-engine": manifest(
                "ragondin-engine",
                ['registry-wrapper = { path = "../../vendor/registry-wrapper" }'],
            )
        },
        excluded={
            "vendor/registry-wrapper": manifest(
                "registry-wrapper", ['inventory = { path = "../../../inventory" }']
            )
        },
        outside={"inventory": manifest("inventory")},
    ).exits(1).says("INV-6 VIOLATION").says(
        "registry-wrapper declares global-registry crate as a direct "
        "dependency: inventory"
    )


@case
def a_dev_dependency_on_inventory_is_not_a_violation() -> None:
    """Deliberate, and consistent with every other check in the file.

    A dev-dependency is not compiled into anything this workspace ships, so it
    cannot put a global registry in a process alongside two `EngineContext`s.
    Pinned because nothing else records the decision: the exclusion is a single
    `kind != "dev"` clause that a refactor could drop without any case noticing.
    """
    run_checker(
        members={
            "engine/ragondin-engine": manifest("ragondin-engine")
            + '\n[dev-dependencies]\ninventory = { path = "../../../inventory" }\n'
        },
        outside={"inventory": manifest("inventory")},
    ).exits(0).says("INV-6 OK").is_silent_about("INV-6 VIOLATION")


@case
def a_build_dependency_on_inventory_is_a_violation() -> None:
    """The other side of that line, recorded rather than left to be discovered.

    A build-dependency runs at compile time, so it is a weaker case than a normal
    one — but it is code this repository chose to carry, the exclusion above is
    written for `dev` alone, and firing is the conservative direction. Whether
    this should ever be relaxed is a decision; until one is taken, this is the
    behaviour, and it is pinned so that changing it has to be deliberate.
    """
    run_checker(
        members={
            "engine/ragondin-engine": manifest("ragondin-engine")
            + '\n[build-dependencies]\ninventory = { path = "../../../inventory" }\n'
        },
        outside={"inventory": manifest("inventory")},
    ).exits(1).says("INV-6 VIOLATION").says(
        "ragondin-engine declares global-registry crate as a direct "
        "dependency: inventory"
    )


@case
def the_graph_is_resolved_with_all_features() -> None:
    """The other half of #146: the closure checks see feature-gated code.

    INV-5 is the sharp end of it, because it is decided purely by the resolved
    graph and has no declared-dependency fallback: an optional dependency on a
    component crate that no default feature enables is absent from a lean
    resolve entirely. A checker that dropped `--all-features` would print
    `INV-5 OK` here.
    """
    run_checker(
        members={
            "components/heavy-component": manifest("heavy-component"),
            "engine/ragondin-engine": manifest(
                "ragondin-engine",
                [
                    'heavy-component = { path = "../../components/heavy-component", '
                    "optional = true }"
                ],
                features=["default = []", 'heavy = ["dep:heavy-component"]'],
            ),
        }
    ).exits(1).says("INV-5 VIOLATION").says(
        "ragondin-engine depends on component crate(s): heavy-component"
    )


def main() -> int:
    failures = []
    for function in CASES:
        try:
            function()
        except Failure as failure:
            failures.append((function.__name__, str(failure)))
            print(f"FAIL  {function.__name__}")
        else:
            print(f"ok    {function.__name__}")

    if failures:
        print(f"\n{len(failures)} of {len(CASES)} case(s) failed:\n", file=sys.stderr)
        for name, message in failures:
            print(f"  {name}\n    {message}\n", file=sys.stderr)
        print(
            "The invariant check does not behave as documented. Fix the check, or "
            "— if the behaviour changed deliberately — the case that pins it.",
            file=sys.stderr,
        )
        return 1
    print(f"\nAll {len(CASES)} case(s) passed: the invariant check still gates.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
