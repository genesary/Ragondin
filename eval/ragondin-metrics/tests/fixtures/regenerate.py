#!/usr/bin/env python3
"""Regenerate `pytrec_eval_parity.tsv`, the frozen parity fixture (ADR-10).

ADR-10 makes `trec_eval` the reference implementation of every metric in
`ragondin-metrics`, and requires that the fixtures cross-checked against it become
permanent CI regression tests. This script produces those expected values from
`pytrec_eval`, the Python binding BEIR itself evaluates with — so the numbers in
the `.tsv` are *not* hand-computed, and are not this project's opinion.

    python3 -m venv .venv && .venv/bin/pip install pytrec_eval-terrier
    .venv/bin/python regenerate.py > pytrec_eval_parity.tsv

The output is deterministic: the random cases use a fixed seed, and cases are
emitted in a fixed order, so regenerating without changing this script produces
a byte-identical file. Regenerate only to *add* coverage — a diff on an existing
line means either `pytrec_eval` changed or the fixture was tampered with, and
both deserve investigation rather than a refreshed file.

Two conventions this fixture deliberately pins, because both are invisible to a
casual test and both silently break the reproduction of a published score:

  * `map_cut` divides by the total number of relevant documents, never by
    `min(k, R)`. Any case with `k >= R` is blind to this, so cases with `k < R`
    are generated on purpose.
  * `ndcg_cut` truncates the *ideal* ranking at `k` as well as the run.

What is NOT in here: runs containing a duplicate id. `trec_eval` rejects such a
run outright (`form_res_rels.c`), so there is no reference value to compare
against; `ragondin-metrics` absorbs the duplicate instead, and the unit tests in
`src/lib.rs` pin that deliberate divergence.
"""

import random
import sys

import pytrec_eval

SEED = 20260905
N_RANDOM = 200

# The qrels used by the unit tests in `src/lib.rs`: d1 highly relevant, d6
# relevant but retrievable only late, d4 marginal, d2 judged and NOT relevant.
MODULE_QRELS = {"d1": 3, "d2": 0, "d4": 1, "d6": 2}

# Cases chosen by hand, each pinning something a random case would only hit by
# luck. `label` is carried into the .tsv as a comment so a failure names what it
# was protecting.
HAND_PICKED = [
    ("the unit-test ranking", MODULE_QRELS, ["d1", "d2", "d3", "d4", "d5"], 5),
    ("cut below the number of relevant docs", MODULE_QRELS, ["d4", "d6", "d1"], 2),
    ("perfect ranking scores 1.0", MODULE_QRELS, ["d1", "d6", "d4", "d2"], 4),
    ("perfect top-k with k < R (the MAP@k divisor)", MODULE_QRELS, ["d1", "d6"], 2),
    ("perfect top-1 with k < R", MODULE_QRELS, ["d4", "d6", "d1"], 1),
    ("k far past the ranking length", MODULE_QRELS, ["d1", "d2", "d3"], 500),
    ("short run, precision still divides by k", MODULE_QRELS, ["d1"], 5),
    ("first relevant doc at rank 3", MODULE_QRELS, ["d2", "d3", "d1", "d4"], 10),
    ("judged-but-irrelevant doc is not a hit", MODULE_QRELS, ["d2", "d1"], 10),
    ("no relevant document retrieved", MODULE_QRELS, ["d2", "d3", "d5"], 3),
    # Degenerate inputs. `pytrec_eval` scores each of these 0.0 and *counts the
    # query in the mean*; it is not an error and not a NaN. (A query with no
    # qrels line at all is different — it is unjudged, and never evaluated. That
    # case cannot appear here, because it produces no reference value.)
    ("empty run", MODULE_QRELS, [], 10),
    ("qrels judged, nothing relevant", {"d1": 0, "d2": 0}, ["d1", "d2"], 10),
    ("graded-only qrels, no grade-1 document", {"d1": 2, "d5": 3}, ["d1", "d9"], 5),
]


def reference(qrels, run, k):
    """The expected values, straight out of `pytrec_eval`."""
    measures = {f"ndcg_cut.{k}", f"recall.{k}", f"P.{k}", f"map_cut.{k}", "recip_rank"}
    evaluator = pytrec_eval.RelevanceEvaluator({"q": qrels}, measures)
    # Scores are descending and never tied: `trec_eval` breaks ties by docid,
    # which would make the fixture depend on a convention the crate does not
    # have (it is handed an already-ordered list).
    scored = {doc: float(len(run) - i) for i, doc in enumerate(run)}
    r = evaluator.evaluate({"q": scored})["q"]
    return [
        r[f"ndcg_cut_{k}"],
        r[f"recall_{k}"],
        r[f"P_{k}"],
        r[f"map_cut_{k}"],
        r["recip_rank"],
    ]


def random_cases():
    rng = random.Random(SEED)
    for _ in range(N_RANDOM):
        n_docs = rng.randint(1, 12)
        docs = [f"d{i}" for i in range(1, n_docs + 1)]
        judged = rng.sample(docs, k=rng.randint(1, n_docs))
        # Grades skewed towards 0 and 1 so that "judged but irrelevant" and
        # binary-relevance datasets (SciFact) are both well represented, while
        # graded qrels (NFCorpus) still appear often enough to exercise nDCG.
        qrels = {d: rng.choice([0, 0, 1, 1, 2, 3]) for d in judged}
        if not any(qrels.values()):
            continue  # no relevant document: covered by the hand-picked cases
        run = rng.sample(docs, k=rng.randint(0, n_docs))
        yield "", qrels, run, rng.choice([1, 2, 3, 5, 10, 20])


def main():
    out = sys.stdout
    out.write(
        "# Expected values produced by pytrec_eval, the library BEIR evaluates\n"
        "# with. Frozen CI regression fixture (ADR-10) — see regenerate.py, and\n"
        "# do not edit a value by hand.\n"
        "#\n"
        "# k <TAB> run <TAB> qrels <TAB> ndcg_cut_k <TAB> recall_k <TAB> P_k"
        " <TAB> map_cut_k <TAB> recip_rank\n"
        "#\n"
        "# run   = doc ids best-first, comma separated, possibly empty\n"
        "# qrels = id:grade pairs, comma separated; a grade of 0 means judged\n"
        "#         and not relevant, which is not the same as absent\n"
        "# recip_rank is uncut, matching trec_eval; the crate's cut variant\n"
        "# reciprocal_rank_at_k has no trec_eval counterpart to compare to.\n"
    )
    cases = list(HAND_PICKED) + list(random_cases())
    for label, qrels, run, k in cases:
        if label:
            out.write(f"\n# {label}\n")
        expected = reference(qrels, run, k)
        fields = [
            str(k),
            ",".join(run),
            ",".join(f"{d}:{g}" for d, g in sorted(qrels.items())),
            *(repr(v) for v in expected),
        ]
        out.write("\t".join(fields) + "\n")


if __name__ == "__main__":
    main()
