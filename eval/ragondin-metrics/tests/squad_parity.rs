//! Parity against the official SQuAD v1.1 evaluation script.
//!
//! ADR-C30 § 1 makes that script the reference implementation of exact match,
//! token-F1 and the answer normalisation they share, as ADR-10 makes
//! `trec_eval` the reference for retrieval: where a definition admits
//! alternatives, the script's binds. That is a claim about an external program,
//! and the unit tests in `src/generation.rs` cannot check it — they assert
//! hand-computed values, which pin the arithmetic but not the convention.
//!
//! So the expected values here are not this project's opinion. They were
//! produced by running the pinned script itself — its `normalize_answer`,
//! `exact_match_score`, `f1_score` and `metric_max_over_ground_truths` — and
//! frozen. `tests/fixtures/regenerate_squad_parity.py` documents how, verifies
//! the script's SHA-256, and records the Python version it ran under.
//!
//! Adding a case is cheap and worth doing whenever a normalisation question
//! comes up — that is what this file is for.

use ragondin_metrics::{exact_match, normalize_answer, token_f1};

/// Committed rather than generated at build time, for the reason
/// `pytrec_eval_parity.rs` gives: generating it would make the test depend on a
/// Python interpreter and a network download.
const FIXTURE: &str = include_str!("fixtures/squad_parity.tsv");

/// ADR-C30 § 1's tolerance, and the one `pytrec_eval_parity.rs` uses. Exact
/// match is compared exactly, not within it.
const TOLERANCE: f64 = 1e-12;

struct Case<'a> {
    /// The line as written, quoted in a failure so the case can be found in the
    /// fixture without counting lines.
    line: &'a str,
    exact_match: f64,
    token_f1: f64,
    normalised: String,
    answer: String,
    references: Vec<String>,
}

/// Decode one field, as `escape` in the regeneration script encodes it: `\\` is
/// a backslash and `\u{hex}` a character.
fn unescape(field: &str) -> String {
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

fn parse(line: &str) -> Case<'_> {
    let f: Vec<&str> = line.split('\t').collect();
    assert!(f.len() >= 5, "malformed fixture line: {line}");
    let exact_match = match f[0] {
        "0" => 0.0,
        "1" => 1.0,
        other => panic!("exact match is 0 or 1, not {other}: {line}"),
    };
    Case {
        line,
        exact_match,
        token_f1: f[1].parse().expect("token_f1 is an f64"),
        normalised: unescape(f[2]),
        answer: unescape(f[3]),
        references: f[4..].iter().map(|r| unescape(r)).collect(),
    }
}

fn cases() -> Vec<Case<'static>> {
    // Lines are not trimmed: a trailing space is data. The fixture escapes one
    // at either end of a field, so nothing an editor strips is significant.
    FIXTURE
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(parse)
        .collect()
}

#[test]
fn every_case_agrees_with_the_squad_script() {
    let cases = cases();

    // A fixture that silently emptied — a botched regeneration, a truncated
    // checkout — would make this test pass while checking nothing.
    assert!(
        cases.len() > 200,
        "fixture holds only {} cases; it is meant to hold hundreds",
        cases.len()
    );

    for case in &cases {
        let normalised = normalize_answer(&case.answer);
        assert_eq!(
            normalised, case.normalised,
            "normalize_answer disagrees with the script\n  fixture line: {}",
            case.line
        );

        let em = exact_match(&case.answer, &case.references);
        assert_eq!(
            em, case.exact_match,
            "exact_match is {em}, the script says {}\n  fixture line: {}",
            case.exact_match, case.line
        );

        let f1 = token_f1(&case.answer, &case.references);
        assert!(
            (f1 - case.token_f1).abs() < TOLERANCE,
            "token_f1 is {f1}, the script says {}\n  fixture line: {}",
            case.token_f1,
            case.line
        );
    }
}

/// The v1.1 rule that separates this metric from SQuAD v2.0's, called out on
/// its own because the parity loop above hides it among hundreds of cases:
/// where prediction and reference both normalise to empty, exact match is 1
/// and F1 is 0 (ADR-C30 § 1). If this fails, the fix is not to change the
/// number — v2.0's F1 of 1 is a different metric.
#[test]
fn both_normalising_to_empty_is_an_exact_match_with_zero_f1() {
    let case = cases()
        .into_iter()
        .find(|case| case.answer == "." && case.references == ["."])
        .expect("the `both normalise to empty` case is missing from the fixture");

    assert_eq!((case.exact_match, case.token_f1), (1.0, 0.0));
    assert_eq!(exact_match(&case.answer, &case.references), 1.0);
    assert_eq!(token_f1(&case.answer, &case.references), 0.0);
}
