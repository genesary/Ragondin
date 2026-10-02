# ARCHITECTURE — ragondin-metrics

**Status: not an API boundary.** INV-1 names the three crates that are, and this
is not among them.

## What lives here

Deterministic evaluation metrics, each scoring **one query**: the retrieval
metrics nDCG@k, recall@k, precision@k, the reciprocal rank and average
precision, against qrels; the answer metrics exact match and token-F1, against
reference answers; the fold from a ranking of chunks to a ranking of
documents; and **the metric catalogue**, the one definition of the names a run
records its metrics under. The conventions each metric follows — the gain
function, what each cutoff divides by, the degenerate inputs, the SQuAD v1.1
script the answer metrics reproduce — are the crate documentation's
(`src/lib.rs`), beside the code they describe, and are not restated here.

| Piece | Role |
|---|---|
| `ndcg_at_k`, `recall_at_k`, `precision_at_k`, `reciprocal_rank`, `reciprocal_rank_at_k`, `average_precision_at_k` | One query's retrieval metrics, `trec_eval`'s definitions |
| `exact_match`, `token_f1`, `normalize_answer` | One query's answer metrics, the SQuAD v1.1 script's definitions |
| `documents_by_first_occurrence` | The fold from chunks to documents every retrieval metric scores |
| `Metric`, `Family`, `Direction` | The catalogue: every metric a run records, its stored name, its family and its direction |

**Deliberately absent**: the **mean over queries**, which is the harness's —
the order it sums in is part of what makes a run reproducible; **storage** of
a metric, which is `ragondin-experiments`'; and any I/O at all. The crate's
one workspace edge is `ragondin-types`, for `DocId`.

## The catalogue

`src/catalogue.rs`. **A closed enum names every metric the harness records**:
`Ndcg { k }`, `Recall { k }`, `Mrr`, `ExactMatch` and `TokenF1`, the cutoff a
field of the variant rather than part of a string. Each has:

- **its stored name**, written by `Display` — `ndcg@<k>`, `recall@<k>`, `mrr`,
  `exact_match`, `token_f1` — and read back by `Metric::parse`. The harness
  writes every name a run's `metrics.json` holds through `Display`, and a
  reader parses through `parse`, so the name written and the name read have one
  spelling; before the catalogue they had two, a literal in the harness and a
  private parser in `ragondin-api`. `parse` reads **exactly** what `Display`
  writes — a cutoff is a decimal integer with no sign and no leading zero —
  so every name it accepts is the name it would write back;
- **its family**, `Family::Ranking` (scored from qrels) or `Family::Answers`
  (scored from reference answers) — the two pieces of ground truth ADR-C30 § 1
  and ADR-008 distinguish;
- **its direction**, `Direction::HigherIsBetter` or `Direction::LowerIsBetter`.
  Every metric the harness records today is better higher.

`Metric::family` and `Metric::direction` are exhaustive matches **with no
wildcard arm**, so a metric added to the enum without a family or a direction
does not compile. That is the point of an enum over a list of strings: a new
metric's family and direction are a compiler check, not a review comment.

**A name the catalogue does not know is unknown, never dropped.** A run's
metrics are an open map (`ragondin-experiments`' `Metrics`): a stored run can
hold a name this build does not know — written by another build, by an export,
by hand. `Metric::parse` answers `None` for it, and `Direction::of`, which reads
the catalogue, answers `None` too. A reader keeps such a metric with its
stored name and value, and gives it no family and no direction: `GET /runs`
lists it as family `unknown`, and a comparison marks no best value for it. No
reader drops it, and none guesses a family or a direction for it from its
spelling — a substring rule ("a name holding `latency` is better lower") was
how the direction used to be read, and a guess is what this rule replaces.

Choices made here (`AGENTS.md` § Rules of engagement):

- **`Direction` is defined here**, beside the catalogue that answers it, and
  `ragondin-experiments` re-exports it rather than mapping it onto an enum of
  its own: a comparison and the catalogue then name one type, and there is no
  mapping to keep in step.
- **`Direction::of(name)`** is the catalogue read by name, `Metric::parse`
  then `Metric::direction`. It lives on `Direction` so that a reader holding
  only a stored name — a comparison's row — has one call to make.
- **A cutoff of `0` is a name like any other** (`ndcg@0`): the harness takes
  any `usize` cutoff, and a name it could write is one the catalogue reads.
