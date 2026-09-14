#!/usr/bin/env python3
"""Regenerate the frozen NFCorpus calibration fixture (ADR-10).

ADR-10 requires that the reproduction of a published leaderboard score leave
behind "a run, its qrels, and the expected scores checked against
`pytrec_eval`" as a permanent CI regression test, and names two cases: SciFact,
with binary qrels, and then NFCorpus, whose graded qrels (1 and 2 on BEIR's
0-2 scale) `docs/system-architecture.md` § 9.8 Calibrating the harness against
a published leaderboard says "alone can expose a linear-versus-exponential gain
bug". This
script is the extraction that produces the graded case's three files from the
store `just calibrate` leaves behind, plus the dataset that calibration ran
over. `nfcorpus_calibration_fixture.rs` beside them is the test that reads them.

The expected values are `pytrec_eval`'s, never `ragondin-metrics`' own: a
fixture computed by the crate under test would pass whatever that crate did.
That matters more here than it did for SciFact — the gain convention this
fixture pins is one of the two the crate had to choose between.

This is a sibling of `regenerate_scifact_calibration.py` rather than a
parameterization of it. The two share a shape, and almost all of what is not
shared is provenance: which run, which store, which terminal node, which
archive, which counts — the part a reader of one fixture needs in front of them
and would have to reassemble from arguments in a shared script.

What it reads
-------------

One run of `bin/ragondin/tests/calibration.rs`, left in the store
`target/tmp/calibration/nfcorpus/` of the worktree the calibration ran in:

  * `5df02792921fe418538358a0c8710bfb683b1b852fecf808c666429388d0fe21`
    — `bin/ragondin/tests/fixtures/calibration/nfcorpus-dense-only.yaml`,
    terminal node `vectors`, `top_k` 10.

From that run's `traces.json` it takes, per query, that terminal node's
`output.chunks.ranked` — the list ADR-C28 has the execution trace carry, in the
order the node produced it — and collapses the chunks to documents by first
occurrence, the rule `ragondin-harness`'s `ranked_documents` applies. (For this
pipeline NFCorpus has one chunk per document, so nothing is ever collapsed; the
script asserts that rather than assuming it.)

The qrels and the query order come from the dataset the calibration ran over:
BEIR NFCorpus, the original `nfcorpus.zip` from the BEIR datasets bucket,
SHA-256 `efe5be03f8c5b86a5870102d0599d227c8c6e2484328e68c6522560385671b0b`,
unpacked as is. `qrels/test.tsv` gives the judgments for the 323 judged queries
— graded, and kept exactly as the file grades them; `queries.jsonl` gives the
order the benchmark walks them in, which is file order filtered to the judged
ones — the order `ragondin-harness` sums its means in, and therefore the order
these files must preserve for the frozen aggregates to reproduce.
`bin/ragondin/ARCHITECTURE.md` § Calibration against a published leaderboard
records the archive hash, the model revision and those aggregates.

What it writes
--------------

Three tab-separated files, beside this script, with the same `#` comment header
convention `pytrec_eval_parity.tsv` uses:

  * `nfcorpus_calibration_qrels.tsv` — `query-id, document-id, grade`, the
    grades verbatim: a 2 written as a 1 is exactly the defect this fixture
    exists to catch.
  * `nfcorpus_calibration_dense_only.run.tsv` — `query-id, rank, document-id,
    score`, one line per ranked document, best first, grouped by query in
    benchmark order. The score is the trace's, widened from the `f32` the
    component returned; no metric reads it, and it is written so the fixture
    records what the node produced rather than only the order it produced it in.
  * `nfcorpus_calibration_dense_only.expected.tsv` — `query-id, ndcg_cut_10,
    recall_10, recip_rank`, in the same order.

The measures are `pytrec_eval`'s `ndcg_cut.10`, `recall.10` and `recip_rank` —
the three `ragondin-harness` reports, with MRR uncut as `trec_eval` computes it.
The run handed to `pytrec_eval` is scored by descending rank rather than by the
pipeline's own scores, so that no tie can make the reference reorder a list this
crate is handed already ordered; `regenerate.py` beside this script does the
same, for the same reason.

The output is deterministic: every file is written in a fixed order and every
float through `repr`, so rerunning without editing this script reproduces all
three byte for byte. A diff on an existing value is a finding, not a refresh.

How to regenerate
-----------------

    export RAGONDIN_CALIBRATION_DATASETS=/path/to/datasets   # holds nfcorpus/
    export RAGONDIN_CALIBRATION_MODELS=/path/to/models
    just calibrate                                          # ~35 min of CPU

    python3 -m venv .venv && .venv/bin/pip install pytrec_eval
    .venv/bin/python eval/ragondin-metrics/tests/fixtures/regenerate_nfcorpus_calibration.py \
        --store target/tmp/calibration/nfcorpus

The committed files were produced with `pytrec_eval` 0.5 on CPython 3.14.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import sys

import pytrec_eval

HERE = pathlib.Path(__file__).resolve().parent

CUTOFF = 10
JUDGED_QUERIES = 323

# (fixture stem, run id, the terminal node whose output is the ranking).
RUNS = [
    (
        "dense_only",
        "5df02792921fe418538358a0c8710bfb683b1b852fecf808c666429388d0fe21",
        "vectors",
    ),
]

BANNER = (
    "# Frozen NFCorpus calibration fixture (ADR-10) — regenerated by\n"
    "# regenerate_nfcorpus_calibration.py, which records where every value came\n"
    "# from. Do not edit a value by hand.\n"
    "#\n"
)


def read_qrels(path):
    """`qrels/<split>.tsv` as `{query: {document: grade}}`.

    The three columns are read by position, and BEIR's `query-id/corpus-id/score`
    header is skipped the way the adapter skips it: only when the score field
    both fails to parse and spells `score`.
    """
    qrels = {}
    with path.open(encoding="utf-8") as handle:
        first = True
        for line in handle:
            if not line.strip():
                continue
            fields = line.rstrip("\n").split("\t")
            query, document, grade = fields[0].strip(), fields[1].strip(), fields[2]
            if first:
                first = False
                if grade.strip() == "score":
                    continue
            qrels.setdefault(query, {})[document] = int(grade)
    return qrels


def read_query_order(path, qrels):
    """The benchmark's query order: `queries.jsonl` order, judged queries only."""
    order = []
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            if not line.strip():
                continue
            query = json.loads(line)["_id"].strip()
            if query in qrels:
                order.append(query)
    return order


def read_rankings(traces, node):
    """Per query, the documents `node` produced, best first, with their scores.

    Chunks are collapsed to documents by first occurrence — the rule
    `ragondin-harness`'s `ranked_documents` applies, because a metric scores
    documents and a pipeline returns chunks.
    """
    rankings = {}
    for query, trace in traces.items():
        nodes = trace["nodes"]
        terminal = nodes[-1]
        if terminal["node"] != node:
            raise SystemExit(f"{query}: the last node is {terminal['node']}, not {node}")
        ranked = []
        seen = set()
        for hit in terminal["output"]["chunks"]["ranked"]:
            if hit["document"] in seen:
                raise SystemExit(
                    f"{query}: {hit['document']} ranked twice — this dataset has one "
                    "chunk per document, so a collapse here means the extraction is "
                    "reading something other than it thinks"
                )
            seen.add(hit["document"])
            ranked.append((hit["document"], hit["score"]))
        if len(ranked) > CUTOFF:
            raise SystemExit(f"{query}: {len(ranked)} hits above a top_k of {CUTOFF}")
        rankings[query] = ranked
    return rankings


def reference(qrels, ranked):
    """nDCG@10, recall@10 and MRR for one query, straight out of `pytrec_eval`.

    The judgments are handed over graded, which is the whole point of this
    dataset: `trec_eval` reads a grade of 2 as twice the gain of a 1 under the
    linear convention ADR-10 pins, and `recip_rank` and `recall` read any
    positive grade as relevant.
    """
    measures = {f"ndcg_cut.{CUTOFF}", f"recall.{CUTOFF}", "recip_rank"}
    evaluator = pytrec_eval.RelevanceEvaluator({"q": qrels}, measures)
    # Scored by descending rank, never by the pipeline's own score: ties would
    # otherwise be broken by document id inside `trec_eval`, reordering a list
    # this crate is handed already ordered.
    run = {document: float(len(ranked) - i) for i, (document, _) in enumerate(ranked)}
    scored = evaluator.evaluate({"q": run})["q"]
    return [
        scored[f"ndcg_cut_{CUTOFF}"],
        scored[f"recall_{CUTOFF}"],
        scored["recip_rank"],
    ]


def write(name, header, lines):
    path = HERE / name
    path.write_text(BANNER + header + "\n".join(lines) + "\n", encoding="utf-8")
    print(f"{path.name}: {len(lines)} lines", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--store",
        type=pathlib.Path,
        required=True,
        help="the run store `just calibrate` left the NFCorpus run in",
    )
    parser.add_argument(
        "--datasets",
        type=pathlib.Path,
        default=os.environ.get("RAGONDIN_CALIBRATION_DATASETS"),
        help="the directory holding nfcorpus/ (default: $RAGONDIN_CALIBRATION_DATASETS)",
    )
    args = parser.parse_args()
    if args.datasets is None:
        parser.error("--datasets, or RAGONDIN_CALIBRATION_DATASETS, must name nfcorpus/")

    nfcorpus = pathlib.Path(args.datasets) / "nfcorpus"
    qrels = read_qrels(nfcorpus / "qrels" / "test.tsv")
    order = read_query_order(nfcorpus / "queries.jsonl", qrels)
    if len(order) != JUDGED_QUERIES:
        raise SystemExit(f"{len(order)} judged queries, not {JUDGED_QUERIES}")
    if not any(grade > 1 for judged in qrels.values() for grade in judged.values()):
        raise SystemExit("no grade above 1: this is not the graded qrels file")

    write(
        "nfcorpus_calibration_qrels.tsv",
        "# BEIR NFCorpus, test split: query-id <TAB> document-id <TAB> grade.\n"
        "# Graded 1/2 on BEIR's 0-2 scale, verbatim from the archive; no test\n"
        "# judgment is 0, and an absent pair reads as 0. Queries in benchmark\n"
        "# order, documents sorted within a query.\n",
        [
            f"{query}\t{document}\t{grade}"
            for query in order
            for document, grade in sorted(qrels[query].items())
        ],
    )

    for stem, run_id, node in RUNS:
        traces = json.loads(
            (args.store / run_id / "traces.json").read_text(encoding="utf-8")
        )
        rankings = read_rankings(traces, node)
        missing = [query for query in order if query not in rankings]
        if missing:
            raise SystemExit(f"{run_id}: {len(missing)} judged queries have no trace")

        write(
            f"nfcorpus_calibration_{stem}.run.tsv",
            f"# Run {run_id},\n"
            f"# node `{node}`: query-id <TAB> rank <TAB> document-id <TAB> score.\n"
            "# Best first, grouped by query, queries in benchmark order.\n",
            [
                f"{query}\t{rank}\t{document}\t{score!r}"
                for query in order
                for rank, (document, score) in enumerate(rankings[query], start=1)
            ],
        )
        write(
            f"nfcorpus_calibration_{stem}.expected.tsv",
            f"# What pytrec_eval scores run {run_id}:\n"
            "# query-id <TAB> ndcg_cut_10 <TAB> recall_10 <TAB> recip_rank.\n"
            "# recip_rank is uncut, matching trec_eval and the harness's mrr.\n",
            [
                "\t".join([query, *(repr(v) for v in reference(qrels[query], rankings[query]))])
                for query in order
            ],
        )


if __name__ == "__main__":
    main()
