#!/usr/bin/env python3
"""Architecture invariant checks — the CI wall under the load-bearing rules.

A rule in a document is a suggestion; a failing build is a wall. This script is
that wall. It parses `cargo metadata` and scans sources (no third-party
dependency, works on any runner with Python 3) and enforces:

  INV-3 — value types only.
          `ragondin-types` and `ragondin-pipeline` carry NO I/O dependency. A
          value is fully determined by its content; a crate that can open a
          socket holds something else.

  INV-4 — the core stays light.
          `ragondin-types`, `ragondin-pipeline` and `ragondin-contracts` must carry NO heavy
          dependency
          (tantivy, tonic, prost, ort, candle-*, vector-store clients, reqwest,
          hyper, ML runtimes). Someone implementing a component compiles only
          the contracts and the value types — never the whole engine.

  INV-5 — the engine knows only traits.
          `ragondin-engine` must not depend on any crate under `components/`.
          Depending on a concrete component would create a two-tier system in
          which built-ins are privileged over third-party components — the slow
          death of a contribution-driven project.

  INV-6 — no global state.
          `inventory` and `linkme` appear nowhere in the workspace. A static
          global registry makes two `EngineContext`s in one process impossible,
          which is exactly what the evaluation harness needs.

  INV-11 — Tower governs the network envelope only.
          No `impl tower::Service` under `components/`. Domain components are
          heterogeneous traits; a `Service` is uniform, and collapsing the two
          loses the domain signature at the layer that needs it most.

INV-9 is absent because there is nothing to check yet, not because it is
undecided. INV-9 forbids deriving *the wire format* from internal IR types
(ADR-C11: "Never derive the wire format from internal IR types; it is
hand-maintained") — so the checkable property is that the crates reading
configuration deserialize into `ragondin-pipeline::raw::*` and never into
`::node::*`. Those crates are `ragondin-config` and `ragondin-proto`, which
code-architecture.md already names as INV-9's home, and both are empty skeletons
today. The check lands with them.

A scan for `derive(Serialize)` on the IR types would NOT be that check: the
logical node types carry those derives deliberately, for internal round-tripping
rather than for the wire, as `node.rs` and `ARCHITECTURE.md` both state.

The dependency checks were written before the first component crate existed, so
they discover component crates from the workspace manifest rather than
hard-coding a list. `components/` now holds real crates, and the same code walks
them.

Run via `just check-invariants`. Exit code 0 = all invariants hold; 1 = a
violation (the message names the invariant and explains why the build failed).
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys

# --- INV-4 deny-list -------------------------------------------------------
# Heavy dependencies forbidden anywhere in the core's dependency closure.
# Matched by exact crate name, or by prefix for versioned families (candle-*).
# "At minimum" per issue #1 §7; extend as new heavy backends are introduced.
DENY_EXACT = {
    "tantivy",  # sparse retrieval engine
    "tonic",  # gRPC framework
    "prost",  # protobuf runtime
    "ort",  # ONNX Runtime binding
    "onnxruntime",  # ONNX Runtime binding
    "tch",  # libtorch binding
    "torch-sys",
    "reqwest",  # HTTP client
    "hyper",  # HTTP implementation
    "qdrant-client",  # vector store client
    "lance",
    "lancedb",
    "opensearch",
    "elasticsearch",
    "pinecone-sdk",
    "weaviate-client",
    "milvus-sdk",
}
DENY_PREFIX = ("candle",)  # candle-core, candle-nn, candle-transformers, …

# --- INV-3 deny-list -------------------------------------------------------
# INV-3 says `ragondin-types` and `ragondin-pipeline` hold "value types only: no
# global context, no interner, no I/O". This list is the **I/O half**, and every
# entry is a crate this repository already names as I/O machinery — nothing here
# is drawn from general knowledge of the ecosystem, because a deny-list nobody
# can trace is a deny-list nobody can review:
#
#   - `tokio` and `tower`: declared in the root `[workspace.dependencies]` as the
#     async runtime and the serving envelope (INV-11). A value type that needs a
#     runtime to be a value is not a value type.
#   - the transport and store clients INV-4 already denies to the core. The
#     overlap is deliberate: the two invariants protect different properties
#     ("this crate holds values" versus "this crate is cheap to compile
#     against"), so INV-3 must keep holding even if INV-4's list is ever
#     narrowed.
#
# The "no interner" and "no global context" clauses name no crate anywhere in the
# repository, so they are not mechanized here and remain review-enforced.
DENY_IO = {
    "tokio",
    "tower",
    "tonic",
    "reqwest",
    "hyper",
    "qdrant-client",
    "lance",
    "lancedb",
    "opensearch",
    "elasticsearch",
    "pinecone-sdk",
    "weaviate-client",
    "milvus-sdk",
}

# --- INV-6 deny-list -------------------------------------------------------
# Named by the invariant itself: "Never use a static global registry
# (`inventory`, `linkme`, or equivalent)". "Or equivalent" is a judgment a
# reviewer makes; these two are the part a build can decide.
DENY_GLOBAL_REGISTRY = {"inventory", "linkme"}

# The crates INV-3 protects. INV-3 names exactly these two — `ragondin-contracts`
# is deliberately absent, because a trait crate is not a value-type crate.
VALUE_TYPE_CRATES = ("ragondin-types", "ragondin-pipeline")

# The core crates INV-4 protects. `ragondin-pipeline` is included because
# code-architecture.md §4.1 states the whole of `core/` carries no heavy
# dependency, and it is an INV-1 stable boundary (INV-3 value types) just like
# the other two — leaving it unguarded would let the "core is light" rule rot.
CORE_CRATES = ("ragondin-types", "ragondin-pipeline", "ragondin-contracts")


def is_heavy(name: str) -> bool:
    return name in DENY_EXACT or any(name.startswith(p) for p in DENY_PREFIX)


def load_metadata() -> dict:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise SystemExit("cargo metadata failed")
    return json.loads(result.stdout)


def build_indexes(md: dict):
    pkgs_by_id = {p["id"]: p for p in md["packages"]}
    # id -> list of (dependency_id, [kinds]); kind is None for normal deps.
    edges: dict[str, list[tuple[str, list]]] = {}
    for node in md["resolve"]["nodes"]:
        deps = []
        for dep in node["deps"]:
            kinds = [dk.get("kind") for dk in dep.get("dep_kinds", [{}])]
            deps.append((dep["pkg"], kinds))
        edges[node["id"]] = deps
    return pkgs_by_id, edges


def member_id(md: dict, pkgs_by_id: dict, name: str) -> str:
    for mid in md["workspace_members"]:
        if pkgs_by_id[mid]["name"] == name:
            return mid
    raise SystemExit(f"workspace member '{name}' not found — did a crate get renamed?")


def closure(start_id: str, edges: dict) -> set:
    """Transitive dependency closure over normal + build edges (dev excluded).

    Dev-dependencies are excluded because they do not ship to a crate's
    consumers: a heavy dev-dependency does not burden someone implementing a
    contract, so it does not violate "the core stays light".
    """
    seen: set[str] = set()
    stack = [start_id]
    while stack:
        current = stack.pop()
        for dep_id, kinds in edges.get(current, []):
            if kinds and all(k == "dev" for k in kinds):
                continue
            if dep_id not in seen:
                seen.add(dep_id)
                stack.append(dep_id)
    return seen


def check_inv4(md: dict, pkgs_by_id: dict, edges: dict):
    """Return [(crate, [heavy offenders]), …] — empty when the invariant holds."""
    failures = []
    for crate in CORE_CRATES:
        cid = member_id(md, pkgs_by_id, crate)
        # Directly declared deps (catches even an optional/feature-gated heavy
        # dep, which a default-features resolve would miss); dev-deps excluded.
        direct = {
            d["name"]
            for d in pkgs_by_id[cid]["dependencies"]
            if d.get("kind") != "dev"
        }
        # Transitive resolved closure (catches a heavy dep pulled indirectly).
        transitive = {pkgs_by_id[i]["name"] for i in closure(cid, edges)}
        offenders = sorted(n for n in (direct | transitive) if is_heavy(n))
        if offenders:
            failures.append((crate, offenders))
    return failures


def check_inv3(md: dict, pkgs_by_id: dict, edges: dict):
    """Return [(crate, [I/O offenders]), …] — empty when the invariant holds.

    Same shape as `check_inv4`, and deliberately not merged with it: they protect
    different properties over different crate sets, and a single fused check would
    have to report a single reason.
    """
    failures = []
    for crate in VALUE_TYPE_CRATES:
        cid = member_id(md, pkgs_by_id, crate)
        direct = {
            d["name"]
            for d in pkgs_by_id[cid]["dependencies"]
            if d.get("kind") != "dev"
        }
        transitive = {pkgs_by_id[i]["name"] for i in closure(cid, edges)}
        offenders = sorted((direct | transitive) & DENY_IO)
        if offenders:
            failures.append((crate, offenders))
    return failures


def check_inv6(md: dict, pkgs_by_id: dict, edges: dict):
    """Return [(workspace crate, [registry crates it pulls]), …].

    INV-6 is about the whole workspace, not about the core: a global registry
    anywhere in the process defeats the reason the registry lives on an explicit
    `EngineContext` at all — two contexts in one process, which the evaluation
    harness needs.
    """
    failures = []
    for mid in md["workspace_members"]:
        direct = {
            d["name"]
            for d in pkgs_by_id[mid]["dependencies"]
            if d.get("kind") != "dev"
        }
        transitive = {pkgs_by_id[i]["name"] for i in closure(mid, edges)}
        offenders = sorted((direct | transitive) & DENY_GLOBAL_REGISTRY)
        if offenders:
            failures.append((pkgs_by_id[mid]["name"], offenders))
    return sorted(failures)


# --- INV-11 source scan ----------------------------------------------------
# Written before the first component landed, and so written to describe no
# particular crate — the same property `check_inv5` was written for. It now walks
# the real crates under `components/`.

COMMENTS = re.compile(r"//[^\n]*|/\*.*?\*/", re.S)
# An `impl … for` header. Bounded and non-greedy so a stray `impl` cannot swallow
# the rest of the file; `{` and `;` end a header, so neither can appear inside.
IMPL_HEADER = re.compile(r"\bimpl\b[^{;]{0,400}?\bfor\b", re.S)
# `use tower::Service;`, `use tower::{Service, Layer};`, `use tower::service::Service;`
TOWER_SERVICE_IMPORT = re.compile(r"\buse\s+tower\s*::[^;]*\bService\b[^;]*;")


def source_files(root: str, subdirectory: str) -> list[str]:
    """Every `.rs` file under `<root>/<subdirectory>`, repository-relative."""
    found = []
    base = os.path.join(root, subdirectory)
    for directory, _subdirs, names in os.walk(base):
        for name in names:
            if name.endswith(".rs"):
                path = os.path.join(directory, name)
                found.append(os.path.relpath(path, root))
    return sorted(found)


def implemented_trait(header: str) -> str | None:
    """The trait path in an `impl … for` header, or None for an inherent impl."""
    body = header[len("impl"):].strip()
    body = body[: body.rfind("for")].strip()
    if body.startswith("<"):  # `impl<S> Trait for T` — skip the generic params
        depth = 0
        for index, character in enumerate(body):
            depth += (character == "<") - (character == ">")
            if depth == 0:
                body = body[index + 1:].strip()
                break
    if not body:
        return None
    cut = body.find("<")  # drop the trait's own generic arguments
    return "".join((body[:cut] if cut != -1 else body).split())


def check_inv11(root: str):
    """Return [(file, line number, trait path), …] for tower::Service impls."""
    failures = []
    for relative in source_files(root, "components"):
        with open(os.path.join(root, relative), encoding="utf-8") as handle:
            text = handle.read()
        # Blank out comments in place so byte offsets — and therefore line
        # numbers — stay correct.
        source = COMMENTS.sub(lambda m: re.sub(r"[^\n]", " ", m.group(0)), text)
        imported = TOWER_SERVICE_IMPORT.search(source) is not None
        for match in IMPL_HEADER.finditer(source):
            trait = implemented_trait(match.group(0))
            if trait is None:
                continue
            qualified = trait.startswith("tower::") and trait.endswith("::Service")
            bare = trait == "Service" and imported
            if qualified or bare:
                line = source.count("\n", 0, match.start()) + 1
                failures.append((relative, line, trait))
    return failures


def check_inv5(md: dict, pkgs_by_id: dict, edges: dict):
    """Return (component_crate_names, [offenders in ragondin-engine's closure])."""
    components_dir = os.path.join(md["workspace_root"], "components") + os.sep
    component_ids = {
        mid
        for mid in md["workspace_members"]
        if pkgs_by_id[mid]["manifest_path"].startswith(components_dir)
    }
    engine_closure = closure(member_id(md, pkgs_by_id, "ragondin-engine"), edges)
    offenders = sorted(pkgs_by_id[i]["name"] for i in (engine_closure & component_ids))
    component_names = sorted(pkgs_by_id[i]["name"] for i in component_ids)
    return component_names, offenders


def main() -> int:
    md = load_metadata()
    pkgs_by_id, edges = build_indexes(md)
    root = md["workspace_root"]
    ok = True

    # Reported in invariant order rather than in the order they were written:
    # someone reading a red build looks up the number they were given.
    inv3 = check_inv3(md, pkgs_by_id, edges)
    if inv3:
        ok = False
        print("INV-3 VIOLATION — the value-type crates must carry no I/O.")
        print("  ragondin-types and ragondin-pipeline hold value types only: a value")
        print("  is fully determined by its content, and two equal values are")
        print("  indistinguishable. A crate that can open a socket or read a file")
        print("  holds something else, and content addressing (INV-8) stops meaning")
        print("  anything. Put the I/O behind a contract in ragondin-contracts and")
        print("  implement it in a component.")
        for crate, offenders in inv3:
            print(f"    {crate} pulls in I/O dependency: {', '.join(offenders)}")
    else:
        print(
            "INV-3 OK — ragondin-types and ragondin-pipeline carry no I/O dependency."
        )

    inv4 = check_inv4(md, pkgs_by_id, edges)
    if inv4:
        ok = False
        print("INV-4 VIOLATION — the core must stay light.")
        print("  ragondin-types, ragondin-pipeline and ragondin-contracts must carry no heavy dependency, so")
        print("  that")
        print("  someone implementing a component compiles only the contracts and the")
        print("  value types — not the whole engine. A heavy dependency here is an")
        print("  abstraction leak.")
        for crate, offenders in inv4:
            print(f"    {crate} pulls in heavy dependency: {', '.join(offenders)}")
    else:
        print(
            "INV-4 OK — core (ragondin-types, ragondin-pipeline, ragondin-contracts) carries no heavy "
            "dependency."
        )

    component_names, inv5 = check_inv5(md, pkgs_by_id, edges)
    if inv5:
        ok = False
        print("INV-5 VIOLATION — the engine must know only traits.")
        print("  ragondin-engine must not depend on any crate under components/. Depending")
        print("  on a concrete component creates a two-tier system in which built-ins")
        print("  are privileged over third-party components — the slow death of a")
        print("  contribution-driven project.")
        print(f"    ragondin-engine depends on component crate(s): {', '.join(inv5)}")
    else:
        print(
            f"INV-5 OK — ragondin-engine depends on none of the {len(component_names)} "
            "component crate(s) under components/."
        )

    inv6 = check_inv6(md, pkgs_by_id, edges)
    if inv6:
        ok = False
        print("INV-6 VIOLATION — no global state.")
        print("  The component registry lives on an EngineContext passed explicitly")
        print("  as a parameter. A static global registry makes two contexts in one")
        print("  process impossible — which is exactly what the evaluation harness")
        print("  needs, and what lets a benchmark and a served pipeline coexist.")
        print("  Register on the EngineContext instead.")
        for crate, offenders in inv6:
            print(f"    {crate} pulls in global-registry crate: {', '.join(offenders)}")
    else:
        print(
            f"INV-6 OK — none of the {len(md['workspace_members'])} workspace crate(s) "
            "pulls in inventory or linkme."
        )

    inv11 = check_inv11(root)
    if inv11:
        ok = False
        print("INV-11 VIOLATION — Tower governs the network envelope only.")
        print("  A component is a heterogeneous domain trait — a Retriever takes a")
        print("  Query and returns ScoredChunks. A tower::Service is uniform, so")
        print("  making a component one erases the domain signature at the layer")
        print("  that most needs it. Implement the trait from ragondin-contracts;")
        print("  leave Tower to the serving layer in runtime/.")
        for path, line, trait in inv11:
            print(f"    {path}:{line}: impl {trait} for … — a component is not a Service")
    else:
        print("INV-11 OK — no component implements tower::Service.")

    if not ok:
        print("\nArchitecture invariants FAILED. See the messages above.", file=sys.stderr)
        return 1
    print("\nAll architecture invariants hold.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
