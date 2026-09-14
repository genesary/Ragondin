//! The SciFact calibration run, frozen query by query — ADR-10's permanent
//! regression test.
//!
//! ADR-10 trusts this platform's numbers only once the harness reproduces a
//! published leaderboard score, and says what that reproduction leaves behind:
//! "the fixtures it freezes (a run, its qrels, and the expected scores checked
//! against `pytrec_eval`) become permanent CI regression tests". The
//! reproduction itself lives in `bin/ragondin/tests/calibration.rs` and needs
//! BEIR SciFact and two exported ONNX models on disk, so it is `#[ignore]` and
//! runs by hand. This test is the part of it that runs in ordinary CI: the two
//! rankings that run produced, checked against `pytrec_eval` without a dataset,
//! a model, a network or a Python interpreter.
//!
//! # What is in the fixture
//!
//! Two runs over BEIR SciFact's `test` split — dense-only, and hybrid with
//! reranking — as `bin/ragondin/ARCHITECTURE.md` § Calibration against a
//! published leaderboard records them. For each of the 300 judged queries, in
//! the order the benchmark walks them: the documents the pipeline's terminal
//! node ranked, best first, with the score it gave each one; the qrels rows for
//! that query; and the nDCG@10, recall@10 and MRR `pytrec_eval` computes over
//! that ranking. The rankings are read out of each stored run's `traces.json`,
//! which carries them because ADR-C28 has every node's output entry in the
//! execution trace name the chunks it produced, in rank order.
//!
//! The expected values are **not** this crate's opinion, which is the whole
//! point of freezing them: they come from `pytrec_eval`, the binding BEIR
//! itself evaluates with. `tests/fixtures/regenerate_scifact_calibration.py`
//! records which run, which store, which node and which measures, and
//! regenerates every file byte for byte.
//!
//! # What this pins, and what it does not
//!
//! It pins the **metrics**: that `ndcg_at_k`, `recall_at_k` and
//! `reciprocal_rank` score 300 real rankings exactly as `trec_eval` scores
//! them, and that the means of those per-query values are the aggregates the
//! calibration recorded. A convention mistake — an exponential nDCG gain, a
//! recall divided by `min(k, R)` — moves a score by percent and fails here on
//! hundreds of queries at once, naming the query it first failed on.
//!
//! It pins nothing about how the ranking was *produced*. The embedder, the
//! reranker, the fusion, the store's search and the BEIR adapter are all
//! upstream of the frozen file; a regression in any of them changes what a
//! fresh `just calibrate` computes and leaves this test green. That is
//! `bin/ragondin/tests/calibration.rs`'s job, and the division is deliberate:
//! this test is the one that can run everywhere, every time.
//!
//! A diff on a committed value is therefore a finding, never a refresh. Either
//! `pytrec_eval` changed or the fixture was edited by hand, and both deserve
//! investigation.

use std::collections::BTreeMap;

use ragondin_metrics::{ndcg_at_k, recall_at_k, reciprocal_rank};
use ragondin_types::DocId;

/// The rank cutoff the calibration ran at, and the cutoff `pytrec_eval` was
/// asked for: `ndcg_cut_10` and `recall_10`.
const CUTOFF: usize = 10;

/// SciFact's `test` split judges 300 queries. A fixture that silently lost
/// most of them would still agree with itself on the few that remained.
const JUDGED_QUERIES: usize = 300;

/// The qrels rows, shared by both runs: they are the dataset's, not a run's.
const QRELS: &str = include_str!("fixtures/scifact_calibration_qrels.tsv");

/// `pytrec_eval` computes in `f64` as this crate does, but sums in its own
/// order and through its own `log`, so agreement is not *guaranteed* to be
/// bit-for-bit on every platform. The same tolerance `pytrec_eval_parity.rs`
/// uses, and for the same reason: it is orders of magnitude tighter than any
/// convention mistake, every one of which shifts a score by percent rather than
/// by ulps. On the machine that froze these files the observed deviation is
/// zero — every one of the 1 800 per-query values, and both runs' three means,
/// agree bit for bit — so the tolerance is headroom for another platform's
/// libm rather than slack this fixture is already using.
const TOLERANCE: f64 = 1e-12;

/// One of the two calibration runs, as the fixture holds it.
struct Frozen {
    /// What `bench` called the run, so a failure names the run and not a file.
    id: &'static str,
    /// Which configuration produced it, for the same reason.
    configuration: &'static str,
    /// `query-id <TAB> rank <TAB> document-id <TAB> score`, best first.
    run: &'static str,
    /// `query-id <TAB> ndcg_cut_10 <TAB> recall_10 <TAB> recip_rank`.
    expected: &'static str,
    /// The means `bin/ragondin/ARCHITECTURE.md` records, in `METRICS`' order:
    /// nDCG@10, recall@10, MRR.
    recorded: [f64; 3],
}

const RUNS: [Frozen; 2] = [
    Frozen {
        id: "e9f178018e9974f216d6cf81ebd71bd5a7273a281e47e48d016fb1cd265382e7",
        configuration: "dense-only.yaml",
        run: include_str!("fixtures/scifact_calibration_dense_only.run.tsv"),
        expected: include_str!("fixtures/scifact_calibration_dense_only.expected.tsv"),
        recorded: [0.6450816521455768, 0.7833333333333333, 0.6047248677248677],
    },
    Frozen {
        id: "9b0e2d9419a1b5d84ed384f50ce4a100a983c0525b93636f749ac50928456238",
        configuration: "hybrid-rerank.yaml",
        run: include_str!("fixtures/scifact_calibration_hybrid_rerank.run.tsv"),
        expected: include_str!("fixtures/scifact_calibration_hybrid_rerank.expected.tsv"),
        recorded: [0.6886092429213343, 0.8122222222222222, 0.6579272486772487],
    },
];

/// The metric names, in the order every `[f64; 3]` here carries them.
const METRICS: [&str; 3] = ["ndcg@10", "recall@10", "mrr"];

/// The data lines of a fixture file, each split on tabs.
fn rows(fixture: &str) -> impl Iterator<Item = Vec<&str>> {
    fixture
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').collect())
}

/// The qrels, by query: a map from document id to graded relevance, where `0`
/// means judged and **not** relevant — which is not the same as absent.
fn qrels() -> BTreeMap<String, BTreeMap<DocId, u8>> {
    let mut by_query: BTreeMap<String, BTreeMap<DocId, u8>> = BTreeMap::new();
    for row in rows(QRELS) {
        assert_eq!(
            row.len(),
            3,
            "a qrels row is query, document, grade: {row:?}"
        );
        let grade = row[2].parse().expect("a grade is a u8");
        let previous = by_query
            .entry(row[0].to_string())
            .or_default()
            .insert(DocId::new(row[1]), grade);
        assert!(previous.is_none(), "{} judged twice in {}", row[1], row[0]);
    }
    by_query
}

/// The ranked documents of each query, in the benchmark order the run file
/// preserves — which is the order the harness summed its means in.
///
/// Returned as a `Vec` rather than a map on purpose: floating-point addition is
/// not associative, so the query order is part of what makes a mean reproduce,
/// and a `BTreeMap` would quietly re-sort it.
fn rankings(run: &str) -> Vec<(String, Vec<DocId>)> {
    let mut queries: Vec<(String, Vec<DocId>)> = Vec::new();
    for row in rows(run) {
        assert_eq!(
            row.len(),
            4,
            "a run row is query, rank, document, score: {row:?}"
        );
        let rank: usize = row[1].parse().expect("a rank is a usize");
        // A score the metrics never read: the ranking is already ordered, and
        // the field is here so the fixture records what the node produced
        // rather than only the order it produced it in. Parsed so that a
        // corrupted one is a failure rather than a decoration.
        let _: f64 = row[3].parse().expect("a score is an f64");

        if queries.last().map(|(id, _)| id.as_str()) != Some(row[0]) {
            assert!(
                !queries.iter().any(|(id, _)| id == row[0]),
                "query {} appears in two blocks; the run file is not grouped",
                row[0]
            );
            queries.push((row[0].to_string(), Vec::new()));
        }

        let ranked = &mut queries.last_mut().expect("a block was just opened").1;
        assert_eq!(rank, ranked.len() + 1, "ranks are 1..n in {}", row[0]);
        assert!(rank <= CUTOFF, "the calibration ran at top_k {CUTOFF}");
        // `ragondin-harness`'s `ranked_documents` collapses a document's
        // chunks to its best-ranked one, so a document cannot appear twice.
        // The extraction applies that rule; this is where it is checked.
        assert!(
            !ranked.contains(&DocId::new(row[2])),
            "{} ranked twice in {}",
            row[2],
            row[0]
        );
        ranked.push(DocId::new(row[2]));
    }
    queries
}

/// The per-query values `pytrec_eval` produced, in the run file's order.
fn expected(fixture: &str) -> Vec<(String, [f64; 3])> {
    rows(fixture)
        .map(|row| {
            assert_eq!(
                row.len(),
                4,
                "an expected row is query, ndcg, recall, recip_rank: {row:?}"
            );
            let mut values = [0.0; 3];
            for (slot, field) in values.iter_mut().zip(&row[1..]) {
                *slot = field.parse().expect("an expected value is an f64");
            }
            (row[0].to_string(), values)
        })
        .collect()
}

/// The running sums, added in the run file's order and divided once at the end
/// — the arithmetic `ragondin-harness`'s `Scores` performs, in the same order,
/// because that order is part of what makes the aggregate reproduce.
#[derive(Default)]
struct Means {
    sums: [f64; 3],
    queries: usize,
}

impl Means {
    fn add(&mut self, values: [f64; 3]) {
        for (sum, value) in self.sums.iter_mut().zip(values) {
            *sum += value;
        }
        self.queries += 1;
    }

    /// Asserts the means are the aggregates the calibration recorded.
    ///
    /// Under `TOLERANCE` rather than by equality: the summation order is the
    /// harness's, and the values being summed are this crate's own, so on the
    /// machine that recorded the run the agreement is exact — but `ndcg_at_k`
    /// reaches `f64::log2`, whose last bit belongs to the platform's libm. A
    /// tolerance of 1e-12 absorbs that and nothing else; the gap a real
    /// regression opens is the fourth decimal at worst.
    fn assert_recorded(&self, recorded: [f64; 3], which: &str) {
        assert_eq!(self.queries, JUDGED_QUERIES, "{which}: queries scored");
        for ((name, sum), want) in METRICS.iter().zip(self.sums).zip(recorded) {
            let got = sum / self.queries as f64;
            assert!(
                (got - want).abs() < TOLERANCE,
                "{which} {name} is {got}, the calibration recorded {want}"
            );
        }
    }
}

#[test]
fn every_per_query_score_of_the_calibration_agrees_with_pytrec_eval() {
    let qrels = qrels();
    assert_eq!(
        qrels.len(),
        JUDGED_QUERIES,
        "SciFact's test split judges {JUDGED_QUERIES} queries"
    );

    for frozen in &RUNS {
        let which = format!("{} (run {})", frozen.configuration, frozen.id);
        let rankings = rankings(frozen.run);
        let expected = expected(frozen.expected);
        assert_eq!(
            rankings.len(),
            JUDGED_QUERIES,
            "{which}: queries in the run file"
        );
        assert_eq!(
            expected.len(),
            JUDGED_QUERIES,
            "{which}: queries in the expected file"
        );

        let mut means = Means::default();
        for ((query, ranked), (scored, want)) in rankings.iter().zip(&expected) {
            assert_eq!(
                query, scored,
                "{which}: the two files disagree on the order"
            );
            let judgments = qrels
                .get(query)
                .unwrap_or_else(|| panic!("{which}: {query} has no qrels row"));

            let got = [
                ndcg_at_k(ranked, judgments, CUTOFF),
                recall_at_k(ranked, judgments, CUTOFF),
                // Uncut, matching `trec_eval`'s `recip_rank` and the harness's
                // own `mrr`, which reports it over the whole ranking.
                reciprocal_rank(ranked, judgments),
            ];

            for ((name, got), want) in METRICS.iter().zip(got).zip(*want) {
                assert!(
                    (got - want).abs() < TOLERANCE,
                    "{which}, query {query}: {name} is {got}, pytrec_eval says {want}"
                );
            }

            means.add(got);
        }

        means.assert_recorded(frozen.recorded, &which);
    }
}
