#!/usr/bin/env python3
"""Architecture invariant checks — the CI wall for the two load-bearing rules.

A rule in a document is a suggestion; a failing build is a wall. This script is
that wall. It parses `cargo metadata` (no third-party dependency, works on any
runner with Python 3) and enforces:

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

The check is written so it is correct BOTH now (while `components/` is empty) and
the day a component crate appears: it discovers component crates from the
workspace manifest rather than hard-coding a list.

Run via `just check-invariants`. Exit code 0 = all invariants hold; 1 = a
violation (the message names the invariant and explains why the build failed).
"""

from __future__ import annotations

import json
import os
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
    ok = True

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

    if not ok:
        print("\nArchitecture invariants FAILED. See the messages above.", file=sys.stderr)
        return 1
    print("\nAll architecture invariants hold.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
