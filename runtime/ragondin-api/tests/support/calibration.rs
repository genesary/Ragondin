//! The `pytrec_eval`-checked fixtures of `ragondin-metrics`, turned into
//! stored runs: the M2 regression fixture (`pytrec_eval_parity.tsv`), the M3
//! calibration fixtures (SciFact dense-only and hybrid-rerank, NFCorpus
//! dense-only) and the SQuAD generation calibration fixture.
//!
//! Each fixture freezes, query by query, a ranking a real run produced and
//! what `pytrec_eval` (and, for SQuAD, the official SQuAD script) scored it.
//! Here each becomes what the harness would have stored: a benchmark holding
//! the fixture's qrels (and reference answers), one trace per query whose last
//! ranking node produced the frozen ranking, and a `metrics.json` holding the
//! reference implementation's means — never a value this crate computed, so
//! the API's reading of the trace is checked against an independent figure.
//!
//! The fixtures are read in place, from `ragondin-metrics`' test data: a
//! second copy would be a second set of frozen values to keep in step.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ragondin_benchmarks::{Benchmark, Qrels, ReferenceAnswers};
use ragondin_experiments::{Run, Trace, TraceChunk};
use ragondin_types::{DocId, Document, Query, QueryId};

use super::runs::{chunk, generation_trace, reranked_trace, run_over, GENERATION, RERANKED};

/// One stored run, the benchmark it was evaluated on, and what the reference
/// implementation scored each query, in benchmark order.
pub struct FixtureRun {
    pub name: String,
    pub run: Run,
    pub benchmark: Benchmark,
    /// Per query: metric name to the reference value.
    pub expected: Vec<(String, BTreeMap<String, f64>)>,
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../eval/ragondin-metrics/tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The data lines of a fixture file, each split on tabs.
fn rows(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').map(str::to_owned).collect())
        .collect()
}

/// Decodes one field of the SQuAD fixture, as its regeneration script
/// encodes it: `\\` is a backslash, `\u{hex}` a character, and a field that
/// is exactly `\e` the empty string.
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

/// A benchmark over `queries` (in order) and `qrels`, whose corpus holds
/// every document the rankings or the qrels name, in id order.
fn benchmark(
    queries: &[String],
    qrels: &[(String, String, u8)],
    rankings: &BTreeMap<String, Vec<(String, f64)>>,
) -> Benchmark {
    let mut documents: BTreeSet<&str> = qrels.iter().map(|(_, doc, _)| doc.as_str()).collect();
    for ranking in rankings.values() {
        documents.extend(ranking.iter().map(|(doc, _)| doc.as_str()));
    }
    let mut judged = Qrels::new();
    for (query, doc, grade) in qrels {
        judged.insert(QueryId::new(query), DocId::new(doc), *grade);
    }
    Benchmark::new(
        documents
            .into_iter()
            .map(|id| Document {
                id: DocId::new(id),
                text: format!("the passage {id}"),
                metadata: BTreeMap::new(),
            })
            .collect(),
        queries
            .iter()
            .map(|id| Query {
                id: QueryId::new(id),
                text: format!("question {id}"),
            })
            .collect(),
        judged,
    )
}

/// A ranking as the one-chunk-per-document derivation names it.
fn as_chunks(ranking: &[(String, f64)]) -> Vec<TraceChunk> {
    ranking
        .iter()
        .map(|(doc, score)| chunk(doc, doc, *score))
        .collect()
}

/// The retriever's ranking, for the node before the last: the frozen one
/// reversed, so that the two ranking nodes score differently and the test
/// sees which one it read.
fn reversed(ranking: &[(String, f64)]) -> Vec<TraceChunk> {
    let mut chunks = as_chunks(ranking);
    chunks.reverse();
    chunks
}

/// The mean of each metric over `expected`, summed in its order.
fn means(expected: &[(String, BTreeMap<String, f64>)]) -> Vec<(String, f64)> {
    let mut sums: BTreeMap<String, f64> = BTreeMap::new();
    for (_, values) in expected {
        for (name, value) in values {
            *sums.entry(name.clone()).or_default() += value;
        }
    }
    let count = expected.len() as f64;
    sums.into_iter()
        .map(|(name, sum)| (name, sum / count))
        .collect()
}

fn assemble(
    id: u8,
    name: &str,
    config: &str,
    benchmark: Benchmark,
    traces: Vec<(String, Trace)>,
    expected: Vec<(String, BTreeMap<String, f64>)>,
) -> FixtureRun {
    let metrics = means(&expected);
    let metrics: Vec<(&str, f64)> = metrics
        .iter()
        .map(|(name, value)| (name.as_str(), *value))
        .collect();
    let traces = traces
        .iter()
        .map(|(query, trace)| (query.as_str(), trace.clone()))
        .collect();
    let run = run_over(id, config, &benchmark, traces, &metrics);
    FixtureRun {
        name: name.to_owned(),
        run,
        benchmark,
        expected,
    }
}

/// A frozen run file: query, rank, document, score — best first, grouped by
/// query.
fn rankings(text: &str) -> BTreeMap<String, Vec<(String, f64)>> {
    let mut rankings: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    for row in rows(text) {
        assert_eq!(row.len(), 4, "a run row is query, rank, document, score");
        let ranking = rankings.entry(row[0].clone()).or_default();
        assert_eq!(row[1].parse::<usize>().unwrap(), ranking.len() + 1);
        ranking.push((row[2].clone(), row[3].parse().unwrap()));
    }
    rankings
}

fn qrels(text: &str) -> Vec<(String, String, u8)> {
    rows(text)
        .into_iter()
        .map(|row| (row[0].clone(), row[1].clone(), row[2].parse().unwrap()))
        .collect()
}

/// A BEIR calibration fixture — `scifact_calibration_<config>` or
/// `nfcorpus_calibration_<config>` — whose expected file holds, per query,
/// `ndcg_cut_10`, `recall_10` and `recip_rank`.
pub fn beir_calibration(id: u8, dataset: &str, config: &str) -> FixtureRun {
    let rankings = rankings(&fixture(&format!("{dataset}_calibration_{config}.run.tsv")));
    let qrels = qrels(&fixture(&format!("{dataset}_calibration_qrels.tsv")));
    let expected: Vec<(String, BTreeMap<String, f64>)> = rows(&fixture(&format!(
        "{dataset}_calibration_{config}.expected.tsv"
    )))
    .into_iter()
    .map(|row| {
        let values = [
            ("ndcg@10", &row[1]),
            ("recall@10", &row[2]),
            ("mrr", &row[3]),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.parse().unwrap()))
        .collect();
        (row[0].clone(), values)
    })
    .collect();
    let queries: Vec<String> = expected.iter().map(|(query, _)| query.clone()).collect();
    let benchmark = benchmark(&queries, &qrels, &rankings);
    let traces = queries
        .iter()
        .map(|query| {
            let ranking = rankings.get(query).cloned().unwrap_or_default();
            (
                query.clone(),
                reranked_trace(query, reversed(&ranking), as_chunks(&ranking)),
            )
        })
        .collect();
    assemble(
        id,
        &format!("{dataset} {config}"),
        RERANKED,
        benchmark,
        traces,
        expected,
    )
}

/// The M2 regression fixture, one run per cutoff: each case is a query, its
/// ranking and its qrels, and `pytrec_eval`'s `ndcg_cut_k`, `recall_k` and
/// `recip_rank` for it.
pub fn pytrec_eval_parity(first_id: u8) -> Vec<FixtureRun> {
    let mut by_cutoff: BTreeMap<usize, Vec<Vec<String>>> = BTreeMap::new();
    for row in rows(&fixture("pytrec_eval_parity.tsv")) {
        by_cutoff
            .entry(row[0].parse().unwrap())
            .or_default()
            .push(row);
    }
    by_cutoff
        .into_iter()
        .enumerate()
        .map(|(offset, (cutoff, cases))| {
            let mut queries = Vec::new();
            let mut qrels = Vec::new();
            let mut rankings = BTreeMap::new();
            let mut expected = Vec::new();
            for (index, case) in cases.iter().enumerate() {
                let query = format!("case-{index}");
                let ranking: Vec<(String, f64)> = case[1]
                    .split(',')
                    .filter(|doc| !doc.is_empty())
                    .enumerate()
                    .map(|(rank, doc)| (doc.to_owned(), 1.0 / (rank as f64 + 1.0)))
                    .collect();
                for pair in case[2].split(',') {
                    let (doc, grade) = pair.split_once(':').unwrap();
                    qrels.push((query.clone(), doc.to_owned(), grade.parse().unwrap()));
                }
                let values = [
                    (format!("ndcg@{cutoff}"), &case[3]),
                    (format!("recall@{cutoff}"), &case[4]),
                    ("mrr".to_owned(), &case[7]),
                ]
                .into_iter()
                .map(|(name, value)| (name, value.parse().unwrap()))
                .collect();
                expected.push((query.clone(), values));
                rankings.insert(query.clone(), ranking);
                queries.push(query);
            }
            let benchmark = benchmark(&queries, &qrels, &rankings);
            let traces = queries
                .iter()
                .map(|query| {
                    let ranking = &rankings[query];
                    (
                        query.clone(),
                        reranked_trace(query, reversed(ranking), as_chunks(ranking)),
                    )
                })
                .collect();
            assemble(
                first_id + offset as u8,
                &format!("pytrec_eval parity at k = {cutoff}"),
                RERANKED,
                benchmark,
                traces,
                expected,
            )
        })
        .collect()
}

/// The SQuAD generation calibration fixture: per question its ranking, its
/// answer and references, and the SQuAD script's `exact_match` and
/// `token_f1` beside `pytrec_eval`'s three retrieval values.
pub fn squad_generation(id: u8) -> FixtureRun {
    let rankings = rankings(&fixture("squad_generation_calibration.run.tsv"));
    let qrels = qrels(&fixture("squad_generation_calibration_qrels.tsv"));
    let answers: BTreeMap<String, (String, Vec<String>)> =
        rows(&fixture("squad_generation_calibration.answers.tsv"))
            .into_iter()
            .map(|row| {
                (
                    row[0].clone(),
                    (
                        unescape(&row[1]),
                        row[2..]
                            .iter()
                            .map(|reference| unescape(reference))
                            .collect(),
                    ),
                )
            })
            .collect();
    let expected: Vec<(String, BTreeMap<String, f64>)> =
        rows(&fixture("squad_generation_calibration.expected.tsv"))
            .into_iter()
            .map(|row| {
                let names = ["exact_match", "token_f1", "ndcg@10", "recall@10", "mrr"];
                let values = names
                    .iter()
                    .zip(&row[1..])
                    .map(|(name, value)| ((*name).to_owned(), value.parse().unwrap()))
                    .collect();
                (row[0].clone(), values)
            })
            .collect();
    let queries: Vec<String> = expected.iter().map(|(query, _)| query.clone()).collect();
    let mut references = ReferenceAnswers::new();
    for query in &queries {
        references.insert(QueryId::new(query), answers[query].1.clone());
    }
    let benchmark = benchmark(&queries, &qrels, &rankings).with_reference_answers(references);
    let traces = queries
        .iter()
        .map(|query| {
            let ranking = rankings.get(query).cloned().unwrap_or_default();
            (
                query.clone(),
                generation_trace(
                    query,
                    reversed(&ranking),
                    as_chunks(&ranking),
                    2,
                    &answers[query].0,
                ),
            )
        })
        .collect();
    assemble(
        id,
        "squad generation",
        GENERATION,
        benchmark,
        traces,
        expected,
    )
}
