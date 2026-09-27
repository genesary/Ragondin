//! The SQuAD generation calibration run, frozen query by query (ADR-C30 § 4).
//!
//! `bin/ragondin/tests/calibration_generation.rs` runs the generation chain on
//! real data with a real model: hybrid retrieval with reranking over SQuAD
//! v1.1 dev, the concatenating context builder, and a local open-weights model
//! behind the reference `Remote` generator service. It needs the dataset, two
//! ONNX models, the service and an inference server, so it is `#[ignore]` and
//! runs by hand. This test is the part of it that runs in ordinary CI: the run
//! its generation leg produced over the first 1 000 dev questions, re-scored
//! without a dataset, a model, a network or a Python interpreter.
//!
//! # What is in the fixture
//!
//! One run, `RUN_ID`, as `bin/ragondin/ARCHITECTURE.md` § Calibrating the
//! generation chain records it. For each of its 1 000 questions, in file order
//! — the order the SQuAD adapter reads them and the harness sums its means in:
//! the paragraphs the node `reranked` ranked, best first, with the score it
//! gave each (the ranking ADR-C30 § 3 has the retrieval metrics read, found
//! from the generator through the builder's chunks port); the answer the model
//! gave, **as text**; the qrels row and the reference answers; and the exact
//! match, token F1, nDCG@10, recall@10 and reciprocal rank the reference
//! implementations compute over them — the official SQuAD v1.1 script for the
//! first two (ADR-C30 § 1), `pytrec_eval` for the rest.
//! `tests/fixtures/regenerate_squad_generation_calibration.py` records where
//! every value came from and regenerates every file byte for byte. The
//! question ids, paragraph ids and reference answers are SQuAD's, under CC
//! BY-SA 4.0: `tests/fixtures/squad_generation_calibration.NOTICE` says so
//! beside them.
//!
//! # What this pins, and what it does not
//!
//! It pins the **metrics and their reading of a real run**: that
//! `exact_match` and `token_f1` score a thousand answers a real model gave
//! exactly as the SQuAD script scores them, that the three retrieval metrics
//! score the rankings as `trec_eval` does, and that the means of those values
//! are the aggregates the calibration recorded. A normalisation change — an
//! article kept, a punctuation set widened — moves some of a thousand
//! real-world answers and fails here by question id; so does a change to how
//! an answer is rendered into the text a metric reads.
//!
//! It pins nothing about how the answers were *produced*: the model, the
//! service, the prompt and the retrieval are all upstream of the frozen files.
//! A rerun of the calibration may change some answers — the model is not
//! deterministic, ADR-15 — and is held to its own tolerance there, never here.
//! A diff on a committed value is therefore a finding, never a refresh.

use std::collections::BTreeMap;

use ragondin_metrics::{exact_match, ndcg_at_k, recall_at_k, reciprocal_rank, token_f1};
use ragondin_types::DocId;

/// The run the fixture was frozen from, and the configuration that produced
/// it, so that a failure names both.
const RUN_ID: &str = "9e64e3de18d2be0dc14b9c2c4672fad042c55c735d777f5695fee821c865250e";
const CONFIGURATION: &str = "squad-generation.yaml";

/// The rank cutoff the calibration ran at, and the one `pytrec_eval` was asked
/// for: `ndcg_cut_10` and `recall_10`.
const CUTOFF: usize = 10;

/// ADR-C30 § 4's subset: the first 1 000 dev questions. A fixture that
/// silently lost most of them would still agree with itself on the rest.
const QUESTIONS: usize = 1000;

const QRELS: &str = include_str!("fixtures/squad_generation_calibration_qrels.tsv");
const RUN: &str = include_str!("fixtures/squad_generation_calibration.run.tsv");
const ANSWERS: &str = include_str!("fixtures/squad_generation_calibration.answers.tsv");
const EXPECTED: &str = include_str!("fixtures/squad_generation_calibration.expected.tsv");

/// The metric names, in the order every `[f64; 5]` here carries them.
const METRICS: [&str; 5] = ["exact_match", "token_f1", "ndcg@10", "recall@10", "mrr"];

/// The means `bin/ragondin/ARCHITECTURE.md` records for the run, in `METRICS`'
/// order: what `bench` itself scored, from its `metrics.json`.
const RECORDED: [f64; 5] = [
    0.568,
    0.7146161833448988,
    0.9050833556297418,
    0.988,
    0.8773896825396829,
];

/// The tolerance the metric parity fixtures use, `squad_parity.rs` and
/// `pytrec_eval_parity.rs`, for the same reason: the reference sums and takes
/// logarithms in its own order, so agreement is not guaranteed bit for bit on
/// every platform. Exact match is compared exactly, as ADR-C30 § 1 asks.
const TOLERANCE: f64 = 1e-12;

/// The data lines of a fixture file, each split on tabs.
fn rows(fixture: &str) -> impl Iterator<Item = Vec<&str>> {
    fixture
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').collect())
}

/// Decodes one field, as the regeneration script's `escape` encodes it — the
/// scheme `squad_parity.tsv` uses: `\\` is a backslash, `\u{hex}` a
/// character, and a field that is exactly `\e` the empty string.
fn unescape(field: &str) -> String {
    if field == "\\e" {
        return String::new();
    }
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('u') => {
                assert_eq!(chars.next(), Some('{'), "malformed escape in {field:?}");
                let hex: String = chars.by_ref().take_while(|&c| c != '}').collect();
                let code = u32::from_str_radix(&hex, 16).expect("escape is hexadecimal");
                out.push(char::from_u32(code).expect("escape is a scalar value"));
            }
            other => panic!("unknown escape \\{other:?} in {field:?}"),
        }
    }
    out
}

/// One frozen question: everything needed to re-score it, and what the
/// reference implementations scored.
struct Question {
    id: String,
    judgments: BTreeMap<DocId, u8>,
    ranked: Vec<DocId>,
    answer: String,
    references: Vec<String>,
    expected: [f64; 5],
}

/// The four files joined by question, in the file order they all preserve —
/// a `Vec`, never a map, because the order is part of what makes a mean
/// reproduce: floating-point addition is not associative.
fn questions() -> Vec<Question> {
    let mut judgments: BTreeMap<&str, BTreeMap<DocId, u8>> = BTreeMap::new();
    for row in rows(QRELS) {
        assert_eq!(
            row.len(),
            3,
            "a qrels row is question, paragraph, grade: {row:?}"
        );
        let grade = row[2].parse().expect("a grade is a u8");
        let previous = judgments
            .entry(row[0])
            .or_default()
            .insert(DocId::new(row[1]), grade);
        assert!(previous.is_none(), "{} judged twice for {}", row[1], row[0]);
    }

    let mut rankings: Vec<(&str, Vec<DocId>)> = Vec::new();
    for row in rows(RUN) {
        assert_eq!(
            row.len(),
            4,
            "a run row is question, rank, paragraph, score: {row:?}"
        );
        let rank: usize = row[1].parse().expect("a rank is a usize");
        // A score no metric reads, parsed so a corrupted one fails.
        let _: f64 = row[3].parse().expect("a score is an f64");
        if rankings.last().map(|(id, _)| *id) != Some(row[0]) {
            assert!(
                !rankings.iter().any(|(id, _)| *id == row[0]),
                "{} appears in two blocks; the run file is not grouped",
                row[0]
            );
            rankings.push((row[0], Vec::new()));
        }
        let ranked = &mut rankings.last_mut().expect("a block was just opened").1;
        assert_eq!(rank, ranked.len() + 1, "ranks are 1..n for {}", row[0]);
        assert!(rank <= CUTOFF, "the calibration reranked to {CUTOFF}");
        assert!(
            !ranked.contains(&DocId::new(row[2])),
            "{} ranked twice for {}",
            row[2],
            row[0]
        );
        ranked.push(DocId::new(row[2]));
    }

    let answers: Vec<Vec<&str>> = rows(ANSWERS).collect();
    let expected: Vec<Vec<&str>> = rows(EXPECTED).collect();
    assert_eq!(
        answers.len(),
        rankings.len(),
        "questions in the answers file"
    );
    assert_eq!(
        expected.len(),
        rankings.len(),
        "questions in the expected file"
    );

    rankings
        .into_iter()
        .zip(answers)
        .zip(expected)
        .map(|(((id, ranked), answer), expected)| {
            assert!(
                answer.len() >= 3,
                "an answers row is question, answer, reference...: {answer:?}"
            );
            assert_eq!(
                expected.len(),
                6,
                "an expected row is question and five values"
            );
            assert_eq!(answer[0], id, "the answers file is in another order");
            assert_eq!(expected[0], id, "the expected file is in another order");
            let mut values = [0.0; 5];
            for (slot, field) in values.iter_mut().zip(&expected[1..]) {
                *slot = field.parse().expect("an expected value is an f64");
            }
            Question {
                id: id.to_owned(),
                judgments: judgments
                    .remove(id)
                    .unwrap_or_else(|| panic!("{id} has no qrels row")),
                ranked,
                answer: unescape(answer[1]),
                references: answer[2..].iter().map(|r| unescape(r)).collect(),
                expected: values,
            }
        })
        .collect()
}

#[test]
fn every_frozen_answer_and_ranking_rescores_to_the_reference_values() {
    let which = format!("{CONFIGURATION} (run {RUN_ID})");
    let questions = questions();
    assert_eq!(
        questions.len(),
        QUESTIONS,
        "{which}: questions in the fixture"
    );

    let mut sums = [0.0; 5];
    for question in &questions {
        let got = [
            exact_match(&question.answer, &question.references),
            token_f1(&question.answer, &question.references),
            ndcg_at_k(&question.ranked, &question.judgments, CUTOFF),
            recall_at_k(&question.ranked, &question.judgments, CUTOFF),
            // Uncut, as `trec_eval`'s `recip_rank` and the harness's `mrr` are.
            reciprocal_rank(&question.ranked, &question.judgments),
        ];
        assert_eq!(
            got[0], question.expected[0],
            "{which}, question {}: exact_match is {}, the SQuAD script says {}\n  answer: {:?}\n  references: {:?}",
            question.id, got[0], question.expected[0], question.answer, question.references
        );
        for ((name, got), want) in METRICS.iter().zip(got).zip(question.expected).skip(1) {
            assert!(
                (got - want).abs() < TOLERANCE,
                "{which}, question {}: {name} is {got}, the reference says {want}",
                question.id
            );
        }
        for (sum, value) in sums.iter_mut().zip(got) {
            *sum += value;
        }
    }

    // The aggregates the calibration recorded, summed in the harness's order
    // and divided once, as `ragondin-harness`'s `Scores` does.
    for ((name, sum), want) in METRICS.iter().zip(sums).zip(RECORDED) {
        let got = sum / questions.len() as f64;
        assert!(
            (got - want).abs() < TOLERANCE,
            "{which}: the mean {name} is {got}, the calibration recorded {want}"
        );
    }
}
