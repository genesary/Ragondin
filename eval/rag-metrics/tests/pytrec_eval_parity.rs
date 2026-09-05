//! Parity against `pytrec_eval`, the library BEIR evaluates with.
//!
//! ADR-10 makes `trec_eval` the reference implementation rather than a source
//! of inspiration: where a metric admits more than one defensible definition,
//! this crate implements the one `trec_eval` implements, because the objective
//! is to replay a *published* MTEB/BEIR evaluation and land on the same number.
//! That is a claim about an external program, and the unit tests in
//! `src/lib.rs` cannot check it — they assert hand-computed values, which pin
//! the arithmetic but not the convention. Hand-computing the *wrong* convention
//! produces a test that passes and a leaderboard figure that does not
//! reproduce.
//!
//! So the expected values here are not this project's opinion. They were
//! produced by running `pytrec_eval` itself, and frozen (ADR-10, code
//! architecture §11.4). `tests/fixtures/regenerate.py` documents how, and is
//! deterministic: rerunning it without editing it reproduces the file byte for
//! byte. A diff on an existing line is a finding, not a refresh.
//!
//! Adding a case is cheap and worth doing whenever a convention question comes
//! up — that is what this file is for.

use std::collections::BTreeMap;

use rag_metrics::{
    average_precision_at_k, ndcg_at_k, precision_at_k, recall_at_k, reciprocal_rank,
};
use rag_types::DocId;

/// The fixture is committed rather than generated at build time: generating it
/// would make the test depend on a Python interpreter and on `pytrec_eval`
/// being installed, which is exactly the dependency freezing it avoids.
const FIXTURE: &str = include_str!("fixtures/pytrec_eval_parity.tsv");

/// `pytrec_eval` computes in f64 as this crate does, but sums in its own order
/// and through its own `log` implementation, so the agreement is not expected
/// to be bit-for-bit. It is nonetheless far tighter than any convention
/// mistake, every one of which shifts a score by percent, not by ulps.
const TOLERANCE: f64 = 1e-12;

struct Case<'a> {
    /// The line as written, quoted verbatim in a failure so the case can be
    /// found in the fixture without counting lines.
    line: &'a str,
    k: usize,
    ranked: Vec<DocId>,
    qrels: BTreeMap<DocId, u8>,
    expected: [f64; 5],
}

const METRICS: [&str; 5] = ["ndcg_cut", "recall", "P", "map_cut", "recip_rank"];

fn parse(line: &str) -> Case<'_> {
    let f: Vec<&str> = line.split('\t').collect();
    assert_eq!(f.len(), 8, "malformed fixture line: {line}");

    // An empty run field is a legitimate case (`trec_eval` scores it 0.0 and
    // counts it), and `"".split(',')` yields one empty element rather than
    // none — hence the explicit guard rather than a `filter`.
    let ranked = if f[1].is_empty() {
        Vec::new()
    } else {
        f[1].split(',').map(DocId::new).collect()
    };

    let qrels = f[2]
        .split(',')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (id, grade) = pair.split_once(':').expect("qrels pair is `id:grade`");
            (DocId::new(id), grade.parse().expect("grade is a u8"))
        })
        .collect();

    let mut expected = [0.0; 5];
    for (slot, field) in expected.iter_mut().zip(&f[3..]) {
        *slot = field.parse().expect("expected value is an f64");
    }

    Case {
        line,
        k: f[0].parse().expect("k is a usize"),
        ranked,
        qrels,
        expected,
    }
}

#[test]
fn every_metric_agrees_with_pytrec_eval() {
    let cases: Vec<Case> = FIXTURE
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(parse)
        .collect();

    // A fixture that silently emptied — a botched regeneration, a truncated
    // checkout — would make this test pass while checking nothing.
    assert!(
        cases.len() > 100,
        "fixture holds only {} cases; it is meant to hold hundreds",
        cases.len()
    );

    for case in &cases {
        let Case {
            k, ranked, qrels, ..
        } = case;
        let got = [
            ndcg_at_k(ranked, qrels, *k),
            recall_at_k(ranked, qrels, *k),
            precision_at_k(ranked, qrels, *k),
            average_precision_at_k(ranked, qrels, *k),
            // Uncut, matching `trec_eval`'s `recip_rank`. The cut variant
            // `reciprocal_rank_at_k` has no `trec_eval` counterpart, so no
            // reference value exists for it; `src/lib.rs` pins it instead.
            reciprocal_rank(ranked, qrels),
        ];

        for ((name, got), want) in METRICS.iter().zip(got).zip(case.expected) {
            assert!(
                (got - want).abs() < TOLERANCE,
                "{name}@{k} is {got}, pytrec_eval says {want}\n  fixture line: {}",
                case.line
            );
        }
    }
}

/// The convention that separates a BEIR MAP@k from the recommender-systems
/// MAP@k, called out on its own because it is the one this crate got wrong
/// once: the divisor is every relevant document, never `min(k, R)`.
///
/// The parity test above already covers it, but only inside a loop over
/// hundreds of cases where a reader cannot see it. If this assertion ever
/// fails, the fix is not to change the number.
#[test]
fn map_at_k_divides_by_every_relevant_document() {
    let case = FIXTURE
        .lines()
        .map(str::trim_end)
        .find(|line| line.starts_with("2\td1,d6\t"))
        .map(parse)
        .expect("the `perfect top-k with k < R` case is missing from the fixture");

    // A perfect top-2 drawn from 3 relevant documents. Under `min(k, R)` this
    // would be 1.0; `pytrec_eval` says 2/3, and so must this crate.
    assert!((case.expected[3] - 2.0 / 3.0).abs() < TOLERANCE);
    let got = average_precision_at_k(&case.ranked, &case.qrels, case.k);
    assert!(
        (got - 2.0 / 3.0).abs() < TOLERANCE,
        "AP@2 is {got}; a perfect top-2 out of 3 relevant documents is 2/3 \
         under trec_eval, and 1.0 only under the min(k, R) convention that \
         does not appear on any leaderboard"
    );
}
