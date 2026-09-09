#!/usr/bin/env python3
"""Build the repository's cross-reference map, with a provenance tier per edge.

The graph this repository forms is already written down -- in `Cargo.toml`, in
trait definitions, in ADR citations, in `#N` references and in `AGENTS.md`'s
invariant tables -- and is visible nowhere. This script extracts it and answers
one question at a time: *what is connected to this?*

**Every edge carries the tier it was learned from, and the tiers are not
equal.** The distinction is `AGENTS.md`'s own, extended by one:

    closure  cargo metadata          complete: no edge can hide from it
    scan     source text            best-effort, with blind spots (below)
    claim    prose: ADRs, docs, `#N` unverified -- an assertion someone wrote

`rust-toolchain.toml` pins the stable channel and `rustdoc`'s JSON output is
nightly-only, so a complete symbol graph is not available to us. Scan is
genuinely best-effort and says so rather than implying otherwise.

**A claim contradicted by code is the point of the tool**, not a side effect:
prose is what makes a conflict visible, and code is what wins it. A map built
from code alone could not report one, because there would be no claim left to
contradict.

Blind spots of the scan tier, stated because a silent one is worse than a
listed one:
  - a call reached through an alias, a macro or a trait object is not seen;
  - test detection is positional -- everything after the first `#[cfg(test)]`
    in a file is treated as test code;
  - `mentions` edges are literal word occurrences, never inferred relations.

Usage:
    scripts/gen-map.py <entity>     one entity's neighbourhood, as text
    scripts/gen-map.py --conflicts  every claim the code contradicts
    scripts/gen-map.py --view       write the interactive viewer
    scripts/gen-map.py --list       every entity the map knows
Options:
    --offline   never call `gh`; use the cached issue metadata
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from collections import defaultdict

CLOSURE, SCAN, CLAIM = "closure", "scan", "claim"

# `Closes #123` is a worked example in both files, and there is no issue 123.
# A tool whose first run invents six edges is not opened twice.
REFERENCE_BLIND = ("CONTRIBUTING.md", ".github/")

SKIP_DIRS = {".git", "target", "node_modules", ".tokensave", "debug"}


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.dirname(here)


def run(args: list[str], cwd: str) -> str | None:
    try:
        out = subprocess.run(args, cwd=cwd, capture_output=True, text=True, check=True)
        return out.stdout
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None


def walk(root: str, exts: tuple[str, ...]) -> list[str]:
    found = []
    for base, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS and not d.startswith(".")]
        for name in files:
            if name.endswith(exts):
                found.append(os.path.join(base, name))
    return sorted(found)


def rel(root: str, path: str) -> str:
    return os.path.relpath(path, root)


class Graph:
    def __init__(self) -> None:
        self.nodes: dict[str, dict] = {}
        self.edges: list[dict] = []
        self.notes: list[str] = []

    def node(self, nid: str, kind: str, **attrs) -> str:
        entry = self.nodes.setdefault(nid, {"id": nid, "kind": kind})
        entry.update({k: v for k, v in attrs.items() if v is not None})
        return nid

    def edge(self, src: str, dst: str, kind: str, tier: str, where: str = "") -> None:
        if src == dst or src not in self.nodes or dst not in self.nodes:
            return
        self.edges.append(
            {"src": src, "dst": dst, "kind": kind, "tier": tier, "where": where}
        )

    def touching(self, nid: str) -> list[dict]:
        return [e for e in self.edges if e["src"] == nid or e["dst"] == nid]


# --------------------------------------------------------------------------
# closure -- cargo metadata. Complete within its scope.
# --------------------------------------------------------------------------


def extract_closure(g: Graph, root: str) -> None:
    raw = run(["cargo", "metadata", "--format-version", "1"], root)
    if raw is None:
        g.notes.append("cargo metadata unavailable: the closure tier is empty")
        return
    md = json.loads(raw)
    members = set(md.get("workspace_members", []))
    by_id = {p["id"]: p for p in md["packages"]}
    names = set()
    for pid in members:
        pkg = by_id.get(pid)
        if pkg:
            names.add(pkg["name"])
            g.node(
                f"crate:{pkg['name']}",
                "crate",
                label=pkg["name"],
                path=rel(root, os.path.dirname(pkg["manifest_path"])),
            )
    for pid in members:
        pkg = by_id.get(pid)
        if not pkg:
            continue
        for dep in pkg.get("dependencies", []):
            if dep["name"] in names:
                g.edge(
                    f"crate:{pkg['name']}",
                    f"crate:{dep['name']}",
                    "depends on",
                    CLOSURE,
                    rel(root, pkg["manifest_path"]),
                )


# --------------------------------------------------------------------------
# scan -- source text. Best-effort, blind spots listed in the module docstring.
# --------------------------------------------------------------------------

TRAIT_DEF = re.compile(r"^\s*pub trait ([A-Z]\w*)")
IMPL_FOR = re.compile(r"^\s*impl(?:<[^>]*>)?\s+([\w:]+)(?:<[^>]*>)?\s+for\s+([\w:]+)")
FN_DEF = re.compile(r"^\s*(?:pub(?:\(crate\))?\s+)?(?:async\s+)?fn\s+(\w+)")
PUB_FN_DEF = re.compile(r"^\s*pub(?:\(crate\))?\s+(?:async\s+)?fn\s+(\w+)")


def crate_of(root: str, path: str) -> str | None:
    """The workspace crate a file belongs to, by walking up to its Cargo.toml."""
    d = os.path.dirname(path)
    while d.startswith(root) and len(d) > len(root):
        manifest = os.path.join(d, "Cargo.toml")
        if os.path.isfile(manifest):
            with open(manifest, encoding="utf-8") as fh:
                for line in fh:
                    m = re.match(r'^\s*name\s*=\s*"([^"]+)"', line)
                    if m:
                        return m.group(1)
        d = os.path.dirname(d)
    return None


def test_offset(text: str) -> int:
    """Where test code starts. Positional, and therefore a blind spot."""
    m = re.search(r"#\[cfg\(test\)\]", text)
    return m.start() if m else len(text) + 1


def extract_scan(g: Graph, root: str) -> dict:
    symbols: dict[str, dict] = {}
    for path in walk(root, (".rs",)):
        crate = crate_of(root, path)
        if crate is None:
            continue
        cnode = g.node(f"crate:{crate}", "crate", label=crate)
        with open(path, encoding="utf-8", errors="replace") as fh:
            text = fh.read()
        tstart = test_offset(text)
        where_base = rel(root, path)
        offset = 0
        for lineno, line in enumerate(text.splitlines(), 1):
            here = f"{where_base}:{lineno}"
            in_test = offset >= tstart
            offset += len(line) + 1

            m = TRAIT_DEF.match(line)
            if m:
                t = g.node(f"trait:{m.group(1)}", "trait", label=m.group(1), where=here)
                g.edge(cnode, t, "defines", SCAN, here)

            m = IMPL_FOR.match(line)
            if m and not in_test:
                trait_name = m.group(1).split("::")[-1]
                type_name = m.group(2).split("::")[-1]
                if trait_name[:1].isupper() and f"trait:{trait_name}" in g.nodes:
                    ty = g.node(f"type:{type_name}", "type", label=type_name)
                    g.edge(ty, f"trait:{trait_name}", "implements", SCAN, here)
                    g.edge(cnode, ty, "defines", SCAN, here)

            m = PUB_FN_DEF.match(line)
            if m:
                name = m.group(1)
                sid = f"symbol:{name}"
                vis = "pub(crate)" if "pub(crate)" in line else "pub"
                g.node(sid, "symbol", label=name, where=here, crate=crate, vis=vis)
                symbols.setdefault(
                    name,
                    {"where": here, "crate": crate, "vis": vis, "calls": [], "test_calls": []},
                )
                g.edge(cnode, sid, "defines", SCAN, here)

    # A second pass, because a call can precede its definition in file order.
    call_re = {name: re.compile(rf"\b{re.escape(name)}\s*\(") for name in symbols}
    for path in walk(root, (".rs",)):
        with open(path, encoding="utf-8", errors="replace") as fh:
            text = fh.read()
        tstart = test_offset(text)
        where_base = rel(root, path)
        offset = 0
        for lineno, line in enumerate(text.splitlines(), 1):
            here = f"{where_base}:{lineno}"
            in_test = offset >= tstart
            offset += len(line) + 1
            if FN_DEF.match(line):
                continue
            for name, pattern in call_re.items():
                if pattern.search(line):
                    bucket = "test_calls" if in_test else "calls"
                    symbols[name][bucket].append(here)
    return symbols


# --------------------------------------------------------------------------
# claim -- prose. Unverified by construction.
# --------------------------------------------------------------------------

ADR_REF = re.compile(r"ADR-(C?)(\d+)")
ISSUE_REF = re.compile(r"(?<![\w/#])#(\d{1,3})\b")
SECTION_REF = re.compile(r"§(\d+(?:\.\d+)?)")
FENCE = re.compile(r"^\s*(```|~~~)")
FRONT_ID = re.compile(r"^id:\s*(\S+)", re.M)
FRONT_TITLE = re.compile(r"^title:\s*(.+)$", re.M)
FRONT_STATUS = re.compile(r"^status:\s*(\S+)", re.M)
FRONT_INV = re.compile(r"^invariants:\s*\[([^\]]*)\]", re.M)
INV_REF = re.compile(r"\bINV-(\d+)\b")


def adr_id(series: str, digits: str) -> str:
    return f"ADR-{series}{int(digits)}"


def index_adrs(g: Graph, root: str) -> dict:
    adrs = {}
    adr_dir = os.path.join(root, "docs", "adr")
    for path in sorted(os.listdir(adr_dir)):
        if not path.startswith("ADR-") or not path.endswith(".md"):
            continue
        full = os.path.join(adr_dir, path)
        with open(full, encoding="utf-8") as fh:
            text = fh.read()
        mid = FRONT_ID.search(text)
        if not mid:
            continue
        m = ADR_REF.match(mid.group(1))
        canonical = adr_id(m.group(1), m.group(2))
        title = FRONT_TITLE.search(text)
        status = FRONT_STATUS.search(text)
        inv_field = FRONT_INV.search(text)
        declared = []
        if inv_field and inv_field.group(1).strip():
            declared = [v.strip() for v in inv_field.group(1).split(",") if v.strip()]
        body = text.split("---", 2)[-1]
        adrs[canonical] = {
            "file": rel(root, full),
            "declared": declared,
            "named": sorted({f"INV-{n}" for n in INV_REF.findall(body)}),
            "status": status.group(1) if status else "?",
        }
        g.node(
            f"adr:{canonical}",
            "adr",
            label=canonical,
            title=title.group(1).strip() if title else "",
            status=adrs[canonical]["status"],
            where=rel(root, full),
        )
    return adrs


def index_invariants(g: Graph, root: str) -> None:
    with open(os.path.join(root, "AGENTS.md"), encoding="utf-8") as fh:
        for lineno, line in enumerate(fh.read().splitlines(), 1):
            if not line.startswith("| **INV-"):
                continue
            inv = INV_REF.search(line)
            if not inv:
                continue
            nid = g.node(f"inv:INV-{inv.group(1)}", "invariant", label=f"INV-{inv.group(1)}")
            for series, digits in ADR_REF.findall(line):
                target = f"adr:{adr_id(series, digits)}"
                if target in g.nodes:
                    g.edge(nid, target, "argued in", CLAIM, f"AGENTS.md:{lineno}")


def load_issues(root: str, offline: bool) -> dict:
    cache_dir = os.path.join(root, "target", "map")
    cache = os.path.join(cache_dir, "issues.json")
    if not offline:
        raw = run(
            ["gh", "issue", "list", "--state", "all", "--limit", "300",
             "--json", "number,title,state,labels,body"],
            root,
        )
        if raw:
            os.makedirs(cache_dir, exist_ok=True)
            with open(cache, "w", encoding="utf-8") as fh:
                fh.write(raw)
            return {str(i["number"]): i for i in json.loads(raw)}
    if os.path.isfile(cache):
        with open(cache, encoding="utf-8") as fh:
            return {str(i["number"]): i for i in json.load(fh)}
    return {}


def extract_claims(g: Graph, root: str, adrs: dict, issues: dict) -> None:
    for num, issue in issues.items():
        g.node(
            f"issue:#{num}",
            "issue",
            label=f"#{num}",
            title=issue.get("title", ""),
            status=issue.get("state", ""),
        )

    mentionable = {
        n["label"]: nid
        for nid, n in list(g.nodes.items())
        if n["kind"] in ("trait", "crate", "invariant", "adr", "type")
        and len(n.get("label", "")) > 3
    }
    mention_re = {
        label: re.compile(rf"(?<![\w-]){re.escape(label)}(?![\w-])")
        for label in mentionable
    }

    for path in walk(root, (".md", ".rs")):
        where_base = rel(root, path)
        if where_base.startswith(REFERENCE_BLIND):
            continue
        source = crate_of(root, path)
        origin = f"crate:{source}" if source and f"crate:{source}" in g.nodes else None
        if where_base.startswith("docs/adr/ADR-"):
            for canonical, meta in adrs.items():
                if meta["file"] == where_base:
                    origin = f"adr:{canonical}"
        if origin is None:
            origin = g.node(f"doc:{where_base}", "doc", label=where_base)

        with open(path, encoding="utf-8", errors="replace") as fh:
            lines = fh.read().splitlines()
        fenced = False
        for lineno, line in enumerate(lines, 1):
            if FENCE.match(line):
                fenced = not fenced
                continue
            here = f"{where_base}:{lineno}"
            for series, digits in ADR_REF.findall(line):
                target = f"adr:{adr_id(series, digits)}"
                if target in g.nodes and target != origin:
                    g.edge(origin, target, "cites", CLAIM, here)
            if not fenced:
                for num in ISSUE_REF.findall(line):
                    target = f"issue:#{num}"
                    if target in g.nodes:
                        g.edge(origin, target, "references", CLAIM, here)
            for num in SECTION_REF.findall(line):
                sid = g.node(f"section:§{num}", "section", label=f"§{num}")
                g.edge(origin, sid, "references", CLAIM, here)
            for label, pattern in mention_re.items():
                if pattern.search(line):
                    target = mentionable[label]
                    if target != origin:
                        g.edge(origin, target, "mentions", CLAIM, here)

    for num, issue in issues.items():
        text = f"{issue.get('title', '')}\n{issue.get('body', '') or ''}"
        origin = f"issue:#{num}"
        for label, pattern in mention_re.items():
            if pattern.search(text):
                g.edge(origin, mentionable[label], "mentions", CLAIM, f"issue #{num}")


# --------------------------------------------------------------------------
# conflicts -- a claim the code does not support
# --------------------------------------------------------------------------


def doc_sections(root: str) -> set:
    found = set()
    for name in ("system-architecture.md", "code-architecture.md"):
        path = os.path.join(root, "docs", name)
        if not os.path.isfile(path):
            continue
        with open(path, encoding="utf-8") as fh:
            for line in fh:
                m = re.match(r"^#{2,4}\s+(\d+(?:\.\d+)?)[.\s]", line)
                if m:
                    found.add(m.group(1))
                    found.add(m.group(1).split(".")[0])
    return found


def find_conflicts(g: Graph, root: str, symbols: dict, adrs: dict, issues: dict) -> list:
    out = []

    # 1. A symbol nothing consumes -- and the prose that says otherwise.
    #
    # Two separate findings share this scan, and the difference is what the code
    # can prove. A `pub(crate)` symbol with no caller in its own crate is dead:
    # nothing outside can reach it. A `pub` one may simply have no caller *yet*
    # -- the composition root is #31 and does not exist -- so it is reported only
    # when prose claims something reaches it, which is a contradiction either way.
    RESOLUTION = re.compile(
        r"resolved through|constructor can reach|reach them|resolves it|is resolved", re.I
    )
    WINDOW = 2
    for name, meta in sorted(symbols.items()):
        if not re.match(r"^(build|register|resolve)_", name):
            continue
        if meta["calls"]:
            continue
        resolves = bool(re.match(r"^(build|resolve)_", name))
        # `build_vector_store` -> `VectorStore`: match the type, never the English
        # word. "a dense retriever is built from" is prose about a *different*
        # component, and matching `retriever` there mis-attributes the claim.
        family = "".join(part.capitalize() for part in name.split("_")[1:])
        claims = []
        for path in walk(root, (".rs", ".md")) if resolves else []:
            with open(path, encoding="utf-8", errors="replace") as fh:
                lines = fh.read().splitlines()
            for lineno, line in enumerate(lines, 1):
                if not RESOLUTION.search(line):
                    continue
                lo = max(0, lineno - 1 - WINDOW)
                window = "\n".join(lines[lo : lineno + WINDOW])
                if re.search(rf"(?<![\w])`?{re.escape(family)}`?(?![\w])", window):
                    claims.append(f"{rel(root, path)}:{lineno}")
        if not claims and meta.get("vis") != "pub(crate)":
            continue
        out.append(
            {
                "rule": "claim contradicted by the call graph"
                if claims
                else "declared, never consumed",
                "severity": "high" if claims else "medium",
                "what": f"`{name}` ({meta.get('vis', 'pub')}) has no call site outside test code"
                + (f" ({len(meta['test_calls'])} in tests)" if meta["test_calls"] else ""),
                "code": meta["where"],
                "claims": claims,
            }
        )

    # 2. An ADR's front-matter against the invariants its own prose names.
    for canonical, meta in sorted(adrs.items()):
        missing = [i for i in meta["named"] if i not in meta["declared"]]
        invented = [i for i in meta["declared"] if i not in meta["named"]]
        if missing or invented:
            out.append(
                {
                    "rule": "ADR front-matter disagrees with its prose",
                    "severity": "medium",
                    "what": (
                        (f"names {', '.join(missing)} in its text but not in `invariants:`" if missing else "")
                        + ("; " if missing and invented else "")
                        + (f"declares {', '.join(invented)} its text never names" if invented else "")
                    ),
                    "code": meta["file"],
                    "claims": [],
                }
            )

    # 3. A `#N` in code pointing at an issue that is closed.
    if issues:
        for e in g.edges:
            if e["kind"] != "references" or not e["where"].endswith(tuple(f":{i}" for i in range(10))):
                continue
            if not e["where"].split(":")[0].endswith(".rs"):
                continue
            num = e["dst"].split("#")[-1]
            issue = issues.get(num)
            if issue and issue.get("state") == "CLOSED":
                out.append(
                    {
                        "rule": "code points at a closed issue",
                        "severity": "low",
                        "what": f"#{num} — {issue.get('title', '')[:70]}",
                        "code": e["where"],
                        "claims": [],
                    }
                )

    # 4. A section reference that resolves in neither architecture document.
    known = doc_sections(root)
    seen = set()
    for e in g.edges:
        if e["kind"] != "references" or not e["dst"].startswith("section:"):
            continue
        num = e["dst"].split("§")[-1]
        if num not in known and (num, e["where"]) not in seen:
            seen.add((num, e["where"]))
            out.append(
                {
                    "rule": "section reference resolves in neither architecture document",
                    "severity": "low",
                    "what": f"§{num}",
                    "code": e["where"],
                    "claims": [],
                }
            )

    order = {"high": 0, "medium": 1, "low": 2}
    return sorted(out, key=lambda c: (order[c["severity"]], c["rule"], c["code"]))


# --------------------------------------------------------------------------
# rendering
# --------------------------------------------------------------------------

TIER_MARK = {CLOSURE: "[closure]", SCAN: "[scan]   ", CLAIM: "[claim]  "}


def resolve(g: Graph, name: str) -> str | None:
    if name in g.nodes:
        return name
    lowered = name.lower()
    for nid, node in g.nodes.items():
        if node.get("label", "").lower() == lowered:
            return nid
    for nid, node in g.nodes.items():
        if lowered in node.get("label", "").lower():
            return nid
    return None


def render_entity(g: Graph, nid: str) -> str:
    node = g.nodes[nid]
    lines = [
        f"{node.get('label', nid)}  ({node['kind']})",
        "=" * 72,
    ]
    for key in ("title", "status", "where", "path"):
        if node.get(key):
            lines.append(f"{key:<10} {node[key]}")
    lines.append("")

    grouped: dict[tuple[str, str], list] = defaultdict(list)
    for e in g.touching(nid):
        outgoing = e["src"] == nid
        other = e["dst"] if outgoing else e["src"]
        arrow = "->" if outgoing else "<-"
        grouped[(e["kind"], arrow)].append((e["tier"], other, e["where"]))

    for (kind, arrow), entries in sorted(grouped.items()):
        seen: dict[str, tuple] = {}
        for tier, other, where in entries:
            if other not in seen:
                seen[other] = (tier, where, 1)
            else:
                t, w, n = seen[other]
                seen[other] = (t, w, n + 1)
        head = f"{arrow} {kind}  ({len(seen)})"
        lines.append(head)
        lines.append("-" * len(head))
        for other, (tier, where, count) in sorted(seen.items()):
            label = g.nodes[other].get("label", other)
            title = g.nodes[other].get("title", "")
            suffix = f"  x{count}" if count > 1 else ""
            note = f"  — {title[:56]}" if title else ""
            lines.append(f"  {TIER_MARK[tier]} {label:<28}{suffix}{note}")
            lines.append(f"  {'':<11} {where}")
        lines.append("")
    return "\n".join(lines)


def render_conflicts(conflicts: list) -> str:
    if not conflicts:
        return "No claim is contradicted by the code."
    lines = [f"{len(conflicts)} finding(s), most severe first", "=" * 72, ""]
    for c in conflicts:
        lines.append(f"[{c['severity']:<6}] {c['rule']}")
        lines.append(f"           {c['what']}")
        lines.append(f"           code:  {c['code']}")
        for claim in c["claims"]:
            lines.append(f"           claim: {claim}")
        lines.append("")
    return "\n".join(lines)


VIEWER = """<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ragondin Reference Map</title>
<style>
/* The three provenance tiers are what this tool is for, so the palette encodes a
   trust gradient rather than decorating: proven, provisional, hearsay. Neutrals
   are biased cool -- the subject is source text. No web font: the viewer must
   open from file:// with no network, so the stack is the system's, and every
   file:line is set in mono because a source location is content here. */
:root{
  --paper:#f6f7f9; --card:#ffffff; --ink:#191b21; --dim:#646a76; --line:#e2e5ea;
  --closure:#0f6b4a; --scan:#8d6008; --claim:#5546bd;
  --high:#a32626; --medium:#8d6008; --low:#646a76;
  --focus:#2f6fd0;
}
:root:not([data-theme="light"]){}
@media (prefers-color-scheme: dark){:root:not([data-theme="light"]){
  --paper:#14161b; --card:#1c1f26; --ink:#e7e9ee; --dim:#969ca8; --line:#2b2f38;
  --closure:#57c295; --scan:#d9a441; --claim:#a99af0;
  --high:#f28b8b; --medium:#d9a441; --low:#969ca8; --focus:#7aa9f0;
}}
:root[data-theme="dark"]{
  --paper:#14161b; --card:#1c1f26; --ink:#e7e9ee; --dim:#969ca8; --line:#2b2f38;
  --closure:#57c295; --scan:#d9a441; --claim:#a99af0;
  --high:#f28b8b; --medium:#d9a441; --low:#969ca8; --focus:#7aa9f0;
}
*{box-sizing:border-box}
body{margin:0;background:var(--paper);color:var(--ink);
  font:15px/1.55 ui-sans-serif,-apple-system,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;
  -webkit-font-smoothing:antialiased}
code,.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
:focus-visible{outline:2px solid var(--focus);outline-offset:2px;border-radius:4px}
header{display:flex;align-items:baseline;gap:20px;flex-wrap:wrap;
  padding:16px 24px;border-bottom:1px solid var(--line);background:var(--card)}
h1{margin:0;font-size:15px;font-weight:650;letter-spacing:-.008em}
#stamp{color:var(--dim);font-size:12.5px;font-variant-numeric:tabular-nums}
.tabs{margin-left:auto;display:flex;gap:6px}
button{font:inherit;font-size:13px;background:none;border:1px solid var(--line);
  color:var(--ink);padding:5px 13px;border-radius:6px;cursor:pointer}
button[aria-selected=true]{background:var(--ink);color:var(--paper);border-color:var(--ink)}
main{display:grid;grid-template-columns:262px minmax(0,1fr);height:calc(100vh - 58px)}
@media(max-width:800px){main{grid-template-columns:1fr;height:auto}
  #side{max-height:44vh;border-right:none;border-bottom:1px solid var(--line)}}
#side{border-right:1px solid var(--line);overflow-y:auto;padding:14px 12px}
input[type=search]{width:100%;font:inherit;font-size:13.5px;padding:7px 10px;
  border:1px solid var(--line);border-radius:6px;background:var(--card);color:var(--ink)}
.grp{color:var(--dim);font-size:10.5px;text-transform:uppercase;letter-spacing:.09em;
  font-weight:650;margin:16px 0 5px;font-variant-numeric:tabular-nums}
.item{padding:3px 7px;border-radius:5px;cursor:pointer;font-size:13.5px;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.item:hover{background:var(--line)}
.item.on{background:var(--ink);color:var(--paper)}
#pane{overflow-y:auto;padding:26px 30px 60px;display:flex;flex-direction:column;gap:22px}
.eyebrow{color:var(--dim);font-size:10.5px;text-transform:uppercase;
  letter-spacing:.1em;font-weight:650}
h2{margin:2px 0 0;font-size:26px;font-weight:660;letter-spacing:-.02em;text-wrap:balance}
.subtitle{color:var(--dim);font-size:14px;max-width:66ch;margin-top:4px}
.facts{display:flex;flex-wrap:wrap;gap:6px 22px;margin-top:8px;font-size:12.5px;
  color:var(--dim)}
.facts .mono{color:var(--ink)}
.filters{display:flex;gap:18px;flex-wrap:wrap;align-items:center;font-size:12.5px;
  color:var(--dim);padding:10px 14px;border:1px solid var(--line);border-radius:8px;
  background:var(--card)}
label{display:flex;gap:6px;align-items:center;cursor:pointer}
svg{width:100%;height:392px;display:block}
.diagram{border:1px solid var(--line);border-radius:10px;background:var(--card);
  overflow:hidden}
svg text{font:11px ui-sans-serif,sans-serif;fill:var(--dim)}
svg text.hub{fill:var(--ink);font-size:13px;font-weight:660}
.rel h3{margin:0 0 2px;font-size:10.5px;text-transform:uppercase;letter-spacing:.09em;
  color:var(--dim);font-weight:650;font-variant-numeric:tabular-nums}
.row{display:flex;gap:10px;align-items:baseline;padding:5px 0;
  border-bottom:1px solid var(--line)}
.row:last-child{border-bottom:none}
.tier{flex:none;width:64px;font-size:10px;letter-spacing:.04em;font-weight:650;
  text-transform:uppercase}
.t-closure{color:var(--closure)}.t-scan{color:var(--scan)}.t-claim{color:var(--claim)}
.name{cursor:pointer;font-weight:520}
.name:hover{text-decoration:underline;text-underline-offset:3px}
.gloss{color:var(--dim);font-size:13px;min-width:0;overflow:hidden;text-overflow:ellipsis;
  white-space:nowrap}
.count{color:var(--dim);font-size:12px;font-variant-numeric:tabular-nums}
.where{margin-left:auto;flex:none;color:var(--dim);font-size:11.5px;max-width:40%;
  overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.c{border:1px solid var(--line);border-left-width:4px;border-radius:8px;
  padding:13px 16px;background:var(--card);display:flex;flex-direction:column;gap:4px}
.c.high{border-left-color:var(--high)}
.c.medium{border-left-color:var(--medium)}
.c.low{border-left-color:var(--low)}
.sev{font-size:10px;text-transform:uppercase;letter-spacing:.09em;font-weight:700}
.sev.high{color:var(--high)}.sev.medium{color:var(--medium)}.sev.low{color:var(--low)}
.c .rule{font-weight:620;font-size:14.5px}
.c .what{font-size:13.5px}
.c .loc{font-size:12px;color:var(--dim)}
.note{color:var(--dim);font-size:13.5px;max-width:68ch}
.stack{display:flex;flex-direction:column;gap:12px}
@media (prefers-reduced-motion:reduce){*{animation:none!important;transition:none!important}}
</style></head><body>
<header>
  <h1>Ragondin Reference Map</h1>
  <span id="stamp"></span>
  <span class="tabs">
    <button id="tab-map" aria-selected="true">Map</button>
    <button id="tab-conf" aria-selected="false">Conflicts</button>
  </span>
</header>
<main>
  <div id="side">
    <input type="search" id="q" placeholder="Filter entities" autocomplete="off"
           aria-label="Filter entities">
    <div id="list"></div>
  </div>
  <div id="pane"></div>
</main>
<script>
const DATA = __DATA__;
const byId = Object.fromEntries(DATA.nodes.map(n => [n.id, n]));
const KIND_ORDER = ["trait","crate","adr","invariant","issue","type","symbol","section","doc"];
const TIER_NOTE = {closure:"complete", scan:"best-effort", claim:"unverified prose"};
const tiers = {closure:true, scan:true, claim:true};
let current = null, tab = "map";

const esc = s => String(s).replace(/[&<>"]/g,
  c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"}[c]));
const edgesOf = id => DATA.edges.filter(e => (e.src===id||e.dst===id) && tiers[e.tier]);

function renderList(){
  const q = document.getElementById("q").value.toLowerCase();
  const groups = {};
  for(const n of DATA.nodes){
    if(q && !n.label.toLowerCase().includes(q)) continue;
    (groups[n.kind] = groups[n.kind] || []).push(n);
  }
  const out = [];
  for(const kind of KIND_ORDER){
    const g = groups[kind]; if(!g) continue;
    g.sort((a,b) => a.label.localeCompare(b.label, undefined, {numeric:true}));
    out.push(`<div class="grp">${kind} <span>${g.length}</span></div>`);
    for(const n of g) out.push(
      `<div class="item${n.id===current?" on":""}" data-id="${esc(n.id)}"
        role="button" tabindex="0">${esc(n.label)}</div>`);
  }
  document.getElementById("list").innerHTML = out.join("");
}

function diagram(id){
  const seen = new Map();
  for(const e of edgesOf(id)){
    const other = e.src===id ? e.dst : e.src;
    if(!seen.has(other)) seen.set(other, e);
  }
  const others = [...seen.keys()].slice(0, 40);
  if(!others.length) return "";
  const W=880, H=392, cx=W/2, cy=H/2;
  const out = [`<div class="diagram"><svg viewBox="0 0 ${W} ${H}" role="img"
    aria-label="Neighbourhood of ${esc(byId[id].label)}">`];
  others.forEach((o,i) => {
    const a = (i/others.length)*Math.PI*2 - Math.PI/2;
    const ring = others.length > 22 ? (i%2 ? 1 : 0.76) : 0.9;
    const x = cx + Math.cos(a)*300*ring, y = cy + Math.sin(a)*152*ring;
    const col = `var(--${seen.get(o).tier})`;
    const right = x >= cx;
    out.push(`<line x1="${cx}" y1="${cy}" x2="${x}" y2="${y}" stroke="${col}"
      stroke-width="1" opacity=".45"/>`);
    out.push(`<circle cx="${x}" cy="${y}" r="3.5" fill="${col}"/>`);
    out.push(`<text x="${x + (right?7:-7)}" y="${y+3.5}"
      text-anchor="${right?"start":"end"}" style="cursor:pointer"
      data-go="${esc(o)}">${esc((byId[o]||{}).label||o)}</text>`);
  });
  out.push(`<circle cx="${cx}" cy="${cy}" r="6" fill="var(--ink)"/>`);
  out.push(`<text class="hub" x="${cx}" y="${cy-15}" text-anchor="middle">
    ${esc(byId[id].label)}</text>`);
  out.push("</svg></div>");
  return out.join("");
}

function filters(){
  return `<div class="filters"><span class="eyebrow">Provenance</span>
    ${["closure","scan","claim"].map(t => `<label><input type="checkbox" data-tier="${t}"
      ${tiers[t]?"checked":""}><span class="t-${t}">${t}</span>
      <span>${TIER_NOTE[t]}</span></label>`).join("")}</div>`;
}

function show(id){
  if(!id || !byId[id]) return;
  current = id; tab = "map"; sync();
  const n = byId[id], groups = {};
  for(const e of edgesOf(id)){
    const out = e.src===id, other = out ? e.dst : e.src;
    const key = (out ? "\u2192 " : "\u2190 ") + e.kind;
    const g = (groups[key] = groups[key] || {});
    if(!g[other]) g[other] = {tier:e.tier, where:e.where, n:0};
    g[other].n++;
  }
  const facts = [];
  for(const k of ["status","vis","where","path","crate"])
    if(n[k]) facts.push(`<span>${k} <span class="mono">${esc(n[k])}</span></span>`);
  const body = [`<div>
      <div class="eyebrow">${esc(n.kind)}</div>
      <h2>${esc(n.label)}</h2>
      ${n.title?`<div class="subtitle">${esc(n.title)}</div>`:""}
      ${facts.length?`<div class="facts">${facts.join("")}</div>`:""}
    </div>`, filters(), diagram(id)];
  for(const key of Object.keys(groups).sort()){
    const rows = Object.entries(groups[key]).sort((a,b) =>
      ((byId[a[0]]||{}).label||"").localeCompare((byId[b[0]]||{}).label||"",
        undefined, {numeric:true}));
    const parts = [`<div class="rel"><h3>${esc(key)} <span>${rows.length}</span></h3>`];
    for(const [other,info] of rows){
      const o = byId[other] || {label:other};
      parts.push(`<div class="row">
        <span class="tier t-${info.tier}">${info.tier}</span>
        <span class="name" data-go="${esc(other)}" role="button" tabindex="0">${esc(o.label)}</span>
        ${o.title?`<span class="gloss">${esc(o.title)}</span>`:""}
        ${info.n>1?`<span class="count">\u00d7${info.n}</span>`:""}
        <span class="where mono">${esc(info.where)}</span></div>`);
    }
    parts.push("</div>");
    body.push(parts.join(""));
  }
  document.getElementById("pane").innerHTML = body.join("");
  renderList();
}

function conflicts(){
  tab = "conf"; sync();
  const cs = DATA.conflicts;
  const body = [`<div><div class="eyebrow">Findings</div><h2>Claims the code contradicts</h2>
    <div class="subtitle">Prose is what makes a contradiction visible; code is what wins
    it. A map built from code alone could report none of these, because there would be
    no claim left to contradict.</div></div>`];
  const stack = [`<div class="stack">`];
  if(!cs.length) stack.push(`<div class="note">Nothing found.</div>`);
  for(const c of cs){
    stack.push(`<div class="c ${c.severity}">
      <span class="sev ${c.severity}">${esc(c.severity)}</span>
      <div class="rule">${esc(c.rule)}</div>
      <div class="what">${esc(c.what)}</div>
      <div class="loc mono">code &nbsp;${esc(c.code)}</div>
      ${c.claims.map(x => `<div class="loc mono">claim ${esc(x)}</div>`).join("")}</div>`);
  }
  stack.push("</div>");
  body.push(stack.join(""));
  document.getElementById("pane").innerHTML = body.join("");
}

function sync(){
  document.getElementById("tab-map").setAttribute("aria-selected", tab==="map");
  document.getElementById("tab-conf").setAttribute("aria-selected", tab==="conf");
}
document.addEventListener("click", ev => {
  const go = ev.target.closest("[data-go]");
  if(go) return show(go.dataset.go);
  const item = ev.target.closest(".item");
  if(item) return show(item.dataset.id);
  if(ev.target.id==="tab-conf") return conflicts();
  if(ev.target.id==="tab-map") return show(current);
});
document.addEventListener("keydown", ev => {
  if(ev.key!=="Enter" && ev.key!==" ") return;
  const t = ev.target.closest("[data-go],.item");
  if(t){ ev.preventDefault(); show(t.dataset.go || t.dataset.id); }
});
document.addEventListener("change", ev => {
  const t = ev.target.dataset && ev.target.dataset.tier;
  if(t){ tiers[t] = ev.target.checked; show(current); }
});
document.getElementById("q").addEventListener("input", renderList);
document.getElementById("stamp").textContent =
  `${DATA.nodes.length} entities \u00b7 ${DATA.edges.length} edges \u00b7 ` +
  `${DATA.conflicts.length} conflicts \u00b7 ${DATA.stamp}`;
show(DATA.start);
</script></body></html>
"""


def build(root: str, offline: bool):
    g = Graph()
    extract_closure(g, root)
    symbols = extract_scan(g, root)
    adrs = index_adrs(g, root)
    index_invariants(g, root)
    issues = load_issues(root, offline)
    extract_claims(g, root, adrs, issues)
    conflicts = find_conflicts(g, root, symbols, adrs, issues)
    return g, symbols, adrs, issues, conflicts


def main() -> int:
    root = repo_root()
    args = [a for a in sys.argv[1:] if a != "--offline"]
    offline = "--offline" in sys.argv

    g, symbols, adrs, issues, conflicts = build(root, offline)
    stamp = f"{len(issues)} issues" + (" (cached)" if offline or not issues else "")

    if not args or args[0] in ("-h", "--help"):
        print(__doc__)
        return 0
    if args[0] == "--list":
        for nid in sorted(g.nodes):
            print(f"{g.nodes[nid]['kind']:<10} {g.nodes[nid].get('label', nid)}")
        return 0
    if args[0] == "--conflicts":
        print(render_conflicts(conflicts))
        return 0
    if args[0] == "--view":
        out_dir = os.path.join(root, "target", "map")
        os.makedirs(out_dir, exist_ok=True)
        start = resolve(g, "Embedder") or sorted(g.nodes)[0]
        payload = {
            "nodes": list(g.nodes.values()),
            "edges": g.edges,
            "conflicts": conflicts,
            "start": start,
            "stamp": stamp,
        }
        html = VIEWER.replace("__DATA__", json.dumps(payload))
        out = os.path.join(out_dir, "index.html")
        with open(out, "w", encoding="utf-8") as fh:
            fh.write(html)
        print(f"wrote {rel(root, out)}  ({len(g.nodes)} entities, {len(g.edges)} edges, "
              f"{len(conflicts)} conflicts)")
        for note in g.notes:
            print(f"note: {note}")
        return 0

    nid = resolve(g, args[0])
    if nid is None:
        print(f"unknown entity: {args[0]}\nTry `--list`.", file=sys.stderr)
        return 1
    print(render_entity(g, nid))
    return 0


if __name__ == "__main__":
    sys.exit(main())
