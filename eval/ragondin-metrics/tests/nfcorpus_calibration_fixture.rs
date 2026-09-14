//! The NFCorpus calibration run, frozen query by query over **graded** qrels —
//! the fixture ADR-10 asks for second, and the one that can fail a gain bug.
//!
//! ADR-10 names two calibration cases and says why there are two: SciFact
//! first, then NFCorpus, whose graded relevance
//! `docs/system-architecture.md` § 9.8 Calibrating the harness against a
//! published leaderboard says "alone can expose a linear-versus-exponential
//! gain bug". SciFact's qrels are 0/1, and under binary judgments `rel` and
//! `2^rel - 1` are the same number, so no SciFact fixture — however many
//! queries it freezes — can tell the two nDCG gain formulas apart. NFCorpus's
//! qrels grade 1 and 2, and at grade 2 the two formulas differ by half again
//! the gain. This fixture is where that convention is pinned by a test rather
//! than by a comment.
//!
//! The reproduction itself lives in `bin/ragondin/tests/calibration.rs` and
//! needs BEIR NFCorpus and an exported ONNX model on disk, so it is `#[ignore]`
//! and runs by hand. This test is the part of it that runs in ordinary CI: the
//! ranking that run produced, checked against `pytrec_eval` without a dataset,
//! a model, a network or a Python interpreter.
//!
//! # What is in the fixture
//!
//! One run over BEIR NFCorpus's `test` split — dense-only — as
//! `bin/ragondin/ARCHITECTURE.md` § Calibration against a published leaderboard
//! records it. For each of the 323 judged queries, in the order the benchmark
//! walks them: the documents the pipeline's terminal node ranked, best first,
//! with the score it gave each one; the qrels rows for that query, grades
//! verbatim; and the nDCG@10, recall@10 and MRR `pytrec_eval` computes over
//! that ranking. The ranking is read out of the stored run's `traces.json`,
//! which carries it because ADR-C28 has every node's output entry in the
//! execution trace name the chunks it produced, in rank order.
//!
//! The expected values are **not** this crate's opinion, which is the whole
//! point of freezing them: they come from `pytrec_eval`, the binding BEIR
//! itself evaluates with. `tests/fixtures/regenerate_nfcorpus_calibration.py`
//! records which run, which store, which node and which measures, and
//! regenerates every file byte for byte.
//!
//! # What this pins, and what it does not
//!
//! It pins the **metrics** over graded judgments: that `ndcg_at_k`,
//! `recall_at_k` and `reciprocal_rank` score 323 real rankings exactly as
//! `trec_eval` scores them when the grades are not all 1. The gain convention
//! is the one ADR-10 pins — linear, `rel`, not `2^rel - 1` — and this is the
//! fixture that fails if it changes: 80 of the 323 queries score a different
//! nDCG@10 under the exponential gain, the worst of them by 12.4 points.
//!
//! **And it is the only thing that would fail.** Swapping the gain formula
//! moves this run's mean nDCG@10 from 0.31667 to 0.31727 — six hundredths of a
//! point, well inside the half-point tolerance
//! `bin/ragondin/tests/calibration.rs` holds the reproduction to, and inside
//! the 1e-4 that test allows an aggregate. So the calibration would reproduce
//! the published figure with the wrong gain function and report success. The
//! per-query freeze is what converts a bug the aggregate averages away into a
//! named failing query, which is why ADR-10 asks for the fixture and not only
//! for the reproduction.
//!
//! It pins nothing about how the ranking was *produced*. The embedder, the
//! store's search and the BEIR adapter are all upstream of the frozen file; a
//! regression in any of them changes what a fresh `just calibrate` computes and
//! leaves this test green. That is `bin/ragondin/tests/calibration.rs`'s job,
//! and the division is deliberate: this test is the one that can run
//! everywhere, every time.
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

/// NFCorpus's `test` split judges 323 queries. A fixture that silently lost
/// most of them would still agree with itself on the few that remained.
const JUDGED_QUERIES: usize = 323;

/// The qrels rows: the dataset's judgments, graded, exactly as the archive
/// grades them.
const QRELS: &str = include_str!("fixtures/nfcorpus_calibration_qrels.tsv");

/// `pytrec_eval` computes in `f64` as this crate does, but sums in its own
/// order and through its own `log`, so agreement is not *guaranteed* to be
/// bit-for-bit on every platform. The same tolerance
/// `scifact_calibration_fixture.rs` and `pytrec_eval_parity.rs` use, and for
/// the same reason: it is orders of magnitude tighter than any convention
/// mistake, every one of which shifts a score by percent rather than by ulps.
const TOLERANCE: f64 = 1e-12;

/// What `bench` called the run, so a failure names the run and not a file.
const RUN_ID: &str = "5df02792921fe418538358a0c8710bfb683b1b852fecf808c666429388d0fe21";
/// Which configuration produced it, for the same reason.
const CONFIGURATION: &str = "nfcorpus-dense-only.yaml";

/// `query-id <TAB> rank <TAB> document-id <TAB> score`, best first.
const RUN: &str = include_str!("fixtures/nfcorpus_calibration_dense_only.run.tsv");
/// `query-id <TAB> ndcg_cut_10 <TAB> recall_10 <TAB> recip_rank`.
const EXPECTED: &str = include_str!("fixtures/nfcorpus_calibration_dense_only.expected.tsv");

/// The means `bin/ragondin/ARCHITECTURE.md` records, in `METRICS`' order:
/// nDCG@10, recall@10, MRR.
const RECORDED: [f64; 3] = [0.31667312754717813, 0.15498797328057862, 0.5076539387684899];

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
/// means judged and **not** relevant — which is not the same as absent — and a
/// `2` is worth twice a `1` under the linear gain ADR-10 pins.
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
fn every_per_query_score_of_the_nfcorpus_calibration_agrees_with_pytrec_eval() {
    let qrels = qrels();
    assert_eq!(
        qrels.len(),
        JUDGED_QUERIES,
        "NFCorpus's test split judges {JUDGED_QUERIES} queries"
    );
    // The reason this fixture exists rather than a second binary one: without a
    // grade above 1 every nDCG gain formula agrees, and the test below would
    // pass under either.
    assert!(
        qrels
            .values()
            .flat_map(BTreeMap::values)
            .any(|&grade| grade > 1),
        "the qrels are not graded: this fixture cannot fail a gain bug"
    );

    let which = format!("{CONFIGURATION} (run {RUN_ID})");
    let rankings = rankings(RUN);
    let expected = expected(EXPECTED);
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

    means.assert_recorded(RECORDED, &which);
}
