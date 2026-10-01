#!/usr/bin/env python3
"""Regenerate the frozen SQuAD generation calibration fixture (ADR-C30 § 4).

ADR-C30 § 4 freezes the generation calibration's subset query by query: for
each question, the ranking the run's retrieval metrics read, the answer **as
text**, the qrels rows and the reference answers needed to re-score it, and the
re-scored exact match, F1 and retrieval values. This script is the extraction
that produces those files from the store `just calibrate-generation` leaves
behind, plus the dataset that calibration ran over.
`squad_generation_calibration_fixture.rs` beside them is the test that reads
them.

The expected values are the reference implementations', never
`ragondin-metrics`' own: exact match and F1 from the official SQuAD v1.1
evaluation script ADR-C30 § 1 pins, the retrieval values from `pytrec_eval`.
A fixture computed by the crate under test would pass whatever that crate did.

What it reads
-------------

One run of `bin/ragondin/tests/calibration_generation.rs`'s generation leg,
left in the store `target/tmp/calibration-generation/squad-generation/` of the
worktree the calibration ran in:

  * `RUN_ID` below — `bin/ragondin/tests/fixtures/calibration/generation/
    squad-generation.yaml` over the first 1 000 questions of SQuAD v1.1 dev.

From the run's `traces.json` it takes, per question, the output entry of the
node `reranked` — the ranking ADR-C30 § 3 finds by walking from the generator
`answer`'s context port to the builder `prompt` and on to its chunks port, in
the committed configuration — and the answer text in the output entry of the
terminal node `answer`. The chunks are
collapsed to documents by first occurrence, the rule `ragondin-metrics`'
`documents_by_first_occurrence` states and the harness applies (SQuAD has one
chunk per paragraph, so nothing collapses; the script asserts that).

The qrels, the reference answers and the question order come from the dataset
the calibration ran over: the official `dev-v1.1.json`, SHA-256
`95aa6a52d5d6a735563366753ca50492a658031da74f301ac5238b03966972c9`, which the
script checks. The order is file order, the order the SQuAD adapter reads and
`ragondin-harness` sums its means in, and the first `QUESTIONS` questions in it
are the subset, exactly as the test derives it.

What it writes
--------------

Four tab-separated files and a licence notice, beside this script, with the
`#` comment header convention the other fixtures here use:

  * `squad_generation_calibration_qrels.tsv` — `question-id, paragraph-id,
    grade`: one row per question, the paragraph it was asked over, grade 1.
  * `squad_generation_calibration.run.tsv` — `question-id, rank,
    paragraph-id, score`, best first, grouped by question in file order. The
    score is the trace's, the cross-encoder's `f32` widened; no metric reads it.
  * `squad_generation_calibration.answers.tsv` — `question-id, answer,
    reference [, reference ...]`, escaped as `squad_parity.tsv` is: `\\\\` a
    backslash, `\\u{hex}` a non-printable character or a space at either end of
    a field, `\\e` an empty field.
  * `squad_generation_calibration.expected.tsv` — `question-id, exact_match,
    token_f1, ndcg_cut_10, recall_10, recip_rank`.
  * `squad_generation_calibration.NOTICE` — the CC BY-SA 4.0 notice the
    reference-answer and question-id text carries (ADR-C30 § 2).

Exact match and F1 are the script's `metric_max_over_ground_truths` over
`exact_match_score` and `f1_score`. The retrieval measures are `pytrec_eval`'s
`ndcg_cut.10`, `recall.10` and `recip_rank`, over a run scored by descending
rank rather than by the pipeline's scores, so that no tie can make the
reference reorder a list this crate is handed already ordered — as
`regenerate_scifact_calibration.py` does, for the same reason.

The SQuAD script is downloaded at run time (the pinned URL first, then the
byte-identical mirror) and refused unless its SHA-256 matches, as
`regenerate_squad_parity.py` does; pass a local copy to work offline.

The output is deterministic: every file is written in a fixed order and every
float through `repr`, so rerunning without editing this script reproduces all
five files byte for byte. A diff on an existing value is a finding, not a
refresh.

How to regenerate
-----------------

    export RAGONDIN_CALIBRATION_DATASETS=/path/to/datasets   # holds squad/
    just calibrate-generation                               # see bin/ragondin/ARCHITECTURE.md

    python3 -m venv .venv && .venv/bin/pip install pytrec_eval numpy
    .venv/bin/python eval/ragondin-metrics/tests/fixtures/regenerate_squad_generation_calibration.py \\
        --store target/tmp/calibration-generation/squad-generation [path/to/evaluate-v1.1.py]

The committed files were produced with `pytrec_eval` 0.5 and `numpy` 2.5.3 on
CPython 3.14.7; the header of `squad_generation_calibration.answers.tsv`
records the Python and Unicode versions, which the SQuAD script's
normalisation reads.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import pathlib
import sys
import unicodedata
import urllib.request

import pytrec_eval

HERE = pathlib.Path(__file__).resolve().parent

RUN_ID = "9e64e3de18d2be0dc14b9c2c4672fad042c55c735d777f5695fee821c865250e"
QUESTIONS = 1000
CUTOFF = 10
RANKING_NODE = "reranked"
ANSWER_NODE = "answer"

DEV_SHA256 = "95aa6a52d5d6a735563366753ca50492a658031da74f301ac5238b03966972c9"
SCRIPT_URLS = [
    "https://worksheets.codalab.org/rest/bundles/0xbcd57bee090b421c982906709c8c27e1/contents/blob/",
    "https://raw.githubusercontent.com/allenai/bi-att-flow/master/squad/evaluate-v1.1.py",
]
SCRIPT_SHA256 = "f5a673dbbd173e29e9ea38f1b2091d883583b77b3a4c17144b223fb0f2f9bd09"

BANNER = (
    "# Frozen SQuAD generation calibration fixture (ADR-C30 § 4) — regenerated\n"
    "# by regenerate_squad_generation_calibration.py, which records where every\n"
    "# value came from. Do not edit a value by hand. The question ids, reference\n"
    "# answers and paragraph ids are from SQuAD v1.1, CC BY-SA 4.0: see\n"
    "# squad_generation_calibration.NOTICE.\n"
)

NOTICE = """\
SQuAD v1.1 — licence notice for the squad_generation_calibration fixture

The files squad_generation_calibration_qrels.tsv,
squad_generation_calibration.run.tsv, squad_generation_calibration.answers.tsv
and squad_generation_calibration.expected.tsv in this directory quote material
from the Stanford Question Answering Dataset (SQuAD) v1.1, development set:
question identifiers, paragraph identifiers derived from article titles, and
the reference answers.

SQuAD is by Pranav Rajpurkar, Jian Zhang, Konstantin Lopyrev and Percy Liang,
"SQuAD: 100,000+ Questions for Machine Comprehension of Text", EMNLP 2016,
https://rajpurkar.github.io/SQuAD-explorer/, and is distributed under the
Creative Commons Attribution-ShareAlike 4.0 International licence (CC BY-SA
4.0), https://creativecommons.org/licenses/by-sa/4.0/.

The quoted material was taken from the file dev-v1.1.json, SHA-256
95aa6a52d5d6a735563366753ca50492a658031da74f301ac5238b03966972c9, and is
arranged here — selected (the first 1 000 questions in file order), reordered
into tab-separated rows and escaped — beside answers a language model gave to
those questions. That adaptation of the quoted material is distributed under
the same licence, CC BY-SA 4.0.
"""


def load_script(path):
    """The pinned SQuAD script as a module, refused unless its SHA-256 matches."""
    if path is None:
        source = None
        for url in SCRIPT_URLS:
            try:
                with urllib.request.urlopen(url, timeout=30) as response:
                    source = response.read()
                break
            except OSError as error:
                print(f"could not fetch {url}: {error}", file=sys.stderr)
        if source is None:
            sys.exit("no copy of the SQuAD script could be downloaded")
    else:
        source = pathlib.Path(path).read_bytes()
    digest = hashlib.sha256(source).hexdigest()
    if digest != SCRIPT_SHA256:
        sys.exit(f"SHA-256 mismatch: got {digest}, ADR-C30 pins {SCRIPT_SHA256}")
    spec = importlib.util.spec_from_loader("squad_v1_1", loader=None)
    module = importlib.util.module_from_spec(spec)
    exec(compile(source, "evaluate-v1.1.py", "exec"), module.__dict__)
    return module


def escape(field):
    """One field, TSV-safe, exactly as `regenerate_squad_parity.py` escapes it."""
    if field == "":
        return "\\e"
    out = []
    last = len(field) - 1
    for i, ch in enumerate(field):
        if ch == "\\":
            out.append("\\\\")
        elif not ch.isprintable() or (ch == " " and i in (0, last)):
            out.append(f"\\u{{{ord(ch):x}}}")
        else:
            out.append(ch)
    return "".join(out)


def read_subset(path):
    """The first `QUESTIONS` questions in file order: (id, paragraph id, references)."""
    raw = path.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != DEV_SHA256:
        raise SystemExit(f"{path}: SHA-256 {digest}, not the recorded {DEV_SHA256}")
    subset = []
    for article in json.loads(raw)["data"]:
        for index, paragraph in enumerate(article["paragraphs"]):
            for question in paragraph["qas"]:
                if len(subset) == QUESTIONS:
                    return subset
                subset.append(
                    (
                        question["id"],
                        f"{article['title']}#{index}",
                        [answer["text"] for answer in question["answers"]],
                    )
                )
    raise SystemExit(f"{path} holds fewer than {QUESTIONS} questions")


def read_trace(query, trace):
    """The ranking the metrics read, with scores, and the answer text."""
    nodes = {node["node"]: node for node in trace["nodes"]}
    if trace["nodes"][-1]["node"] != ANSWER_NODE:
        raise SystemExit(f"{query}: the terminal node is not `{ANSWER_NODE}`")
    ranked = []
    seen = set()
    for hit in nodes[RANKING_NODE]["output"]["chunks"]["ranked"]:
        if hit["document"] in seen:
            raise SystemExit(f"{query}: {hit['document']} ranked twice")
        seen.add(hit["document"])
        ranked.append((hit["document"], hit["score"]))
    if len(ranked) > CUTOFF:
        raise SystemExit(f"{query}: {len(ranked)} hits above a top_k of {CUTOFF}")
    return ranked, nodes[ANSWER_NODE]["output"]["answer"]["text"]


def retrieval(relevant, ranked):
    """nDCG@10, recall@10 and MRR for one question, straight out of `pytrec_eval`."""
    measures = {f"ndcg_cut.{CUTOFF}", f"recall.{CUTOFF}", "recip_rank"}
    evaluator = pytrec_eval.RelevanceEvaluator({"q": {relevant: 1}}, measures)
    run = {document: float(len(ranked) - i) for i, (document, _) in enumerate(ranked)}
    scored = evaluator.evaluate({"q": run})["q"]
    return [scored[f"ndcg_cut_{CUTOFF}"], scored[f"recall_{CUTOFF}"], scored["recip_rank"]]


def write(name, header, lines):
    path = HERE / name
    path.write_text(BANNER + header + "\n".join(lines) + "\n", encoding="utf-8")
    print(f"{path.name}: {len(lines)} lines", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--store", type=pathlib.Path, required=True,
                        help="the run store the generation leg left its run in")
    parser.add_argument("--datasets", type=pathlib.Path,
                        default=os.environ.get("RAGONDIN_CALIBRATION_DATASETS"),
                        help="the directory holding squad/ (default: $RAGONDIN_CALIBRATION_DATASETS)")
    parser.add_argument("script", nargs="?", help="a local copy of evaluate-v1.1.py")
    args = parser.parse_args()
    if args.datasets is None:
        parser.error("--datasets, or RAGONDIN_CALIBRATION_DATASETS, must name squad/")

    squad = load_script(args.script)
    subset = read_subset(pathlib.Path(args.datasets) / "squad" / "dev-v1.1.json")

    traces = json.loads((args.store / RUN_ID / "traces.json").read_text(encoding="utf-8"))
    if len(traces) != QUESTIONS:
        raise SystemExit(f"{RUN_ID}: {len(traces)} traces, not {QUESTIONS}")

    rows = []
    for query, relevant, references in subset:
        if query not in traces:
            raise SystemExit(f"{RUN_ID}: {query} has no trace")
        ranked, answer = read_trace(query, traces[query])
        em = squad.metric_max_over_ground_truths(squad.exact_match_score, answer, references)
        f1 = squad.metric_max_over_ground_truths(squad.f1_score, answer, references)
        rows.append((query, relevant, references, ranked, answer,
                     [1.0 if em else 0.0, float(f1), *retrieval(relevant, ranked)]))

    write(
        "squad_generation_calibration_qrels.tsv",
        "# SQuAD v1.1 dev, the first 1 000 questions in file order:\n"
        "# question-id <TAB> paragraph-id <TAB> grade. One relevant paragraph\n"
        "# per question, the one it was asked over (ADR-C30 § 2).\n",
        [f"{query}\t{relevant}\t1" for query, relevant, *_ in rows],
    )
    write(
        "squad_generation_calibration.run.tsv",
        f"# Run {RUN_ID},\n"
        f"# node `{RANKING_NODE}`: question-id <TAB> rank <TAB> paragraph-id <TAB> score.\n"
        "# Best first, grouped by question, questions in file order.\n",
        [
            f"{query}\t{rank}\t{document}\t{score!r}"
            for query, _, _, ranked, _, _ in rows
            for rank, (document, score) in enumerate(ranked, start=1)
        ],
    )
    write(
        "squad_generation_calibration.answers.tsv",
        f"# Run {RUN_ID}, node `{ANSWER_NODE}`:\n"
        "# question-id <TAB> answer <TAB> reference [<TAB> reference ...].\n"
        "# The answer as the generator returned it; the references in file\n"
        "# order, repeats kept. Fields are escaped: `\\\\` is a backslash,\n"
        "# `\\u{hex}` a non-printable character or a space at either end of a\n"
        "# field, and `\\e` an empty field. No line ends in whitespace.\n"
        f"# Written under Python {sys.version.split()[0]} (Unicode {unicodedata.unidata_version}).\n",
        [
            "\t".join([query, escape(answer), *(escape(r) for r in references)])
            for query, _, references, _, answer, _ in rows
        ],
    )
    write(
        "squad_generation_calibration.expected.tsv",
        f"# What the SQuAD v1.1 script and pytrec_eval score run {RUN_ID}:\n"
        "# question-id <TAB> exact_match <TAB> token_f1 <TAB> ndcg_cut_10\n"
        "# <TAB> recall_10 <TAB> recip_rank. exact_match and token_f1 are\n"
        "# metric_max_over_ground_truths over every reference; recip_rank is\n"
        "# uncut, matching trec_eval and the harness's mrr.\n",
        [
            "\t".join([query, *(repr(v) for v in values)])
            for query, *_, values in rows
        ],
    )
    (HERE / "squad_generation_calibration.NOTICE").write_text(NOTICE, encoding="utf-8")

    means = [sum(row[5][i] for row in rows) / len(rows) for i in range(5)]
    print(f"means (em, f1, ndcg, recall, rr), summed in file order: {means!r}", file=sys.stderr)


if __name__ == "__main__":
    main()
