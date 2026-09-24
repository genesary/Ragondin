//! Exact match and token-F1 of a generated answer, as the official SQuAD v1.1
//! evaluation script defines them. The conventions, and the one known
//! divergence from the script, are stated in the crate documentation under
//! "Generation metrics"; this module is the code they describe.

use std::collections::BTreeMap;

/// The English articles the script removes, `\b(a|an|the)\b`.
const ARTICLES: [&str; 3] = ["a", "an", "the"];

/// Whitespace as Python's `str.split()` sees it. Rust's `char::is_whitespace`
/// is the Unicode `White_Space` property, which leaves out the four ASCII
/// information separators U+001C..=U+001F that Python's `str.isspace` counts;
/// over every other code point Python 3.14 and Rust 1.98 agree.
fn is_python_whitespace(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// A word character for `\b`: Python's `\w` on a `str` pattern is an
/// alphanumeric character or `_`. `char::is_alphanumeric` stands in for
/// Python's `str.isalnum`; the crate documentation names where they differ.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Normalise an answer the way the SQuAD v1.1 script's `normalize_answer`
/// does, so a caller can see what "after normalisation" means.
///
/// In the script's order: lower-case (full Unicode case mapping, a capital
/// sigma lowering to `ς` in final position); remove every ASCII punctuation
/// character — exactly `string.punctuation`, so `’`, `“` and `—` survive;
/// replace each standalone English article (*a*, *an*, *the*, a whole word
/// between `\b` boundaries) by a space; then split on whitespace and join the
/// pieces with one space. Because punctuation goes first, "the-man" becomes
/// "theman", article kept. The articles are English ones: the normalisation
/// assumes English answers.
pub fn normalize_answer(text: &str) -> String {
    let lowered: String = text
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_ascii_punctuation())
        .collect();

    // `re.sub(r'\b(a|an|the)\b', ' ', ...)` replaces exactly the maximal runs
    // of word characters that are an article, so the text is walked run by run
    // rather than through a regular expression.
    let mut unarticled = String::with_capacity(lowered.len());
    let mut rest = lowered.as_str();
    while let Some(first) = rest.chars().next() {
        let word = is_word(first);
        let end = rest
            .find(|c: char| is_word(c) != word)
            .unwrap_or(rest.len());
        let (run, tail) = rest.split_at(end);
        if word && ARTICLES.contains(&run) {
            unarticled.push(' ');
        } else {
            unarticled.push_str(run);
        }
        rest = tail;
    }

    unarticled
        .split(is_python_whitespace)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The tokens `f1_score` counts: the normalised text split on its single
/// spaces.
fn tokens(normalised: &str) -> Vec<&str> {
    normalised.split(' ').filter(|t| !t.is_empty()).collect()
}

/// The script's `f1_score` over already-normalised token lists, in its
/// arithmetic order so the result matches it to the last bit or two.
fn f1_one(answer: &[&str], reference: &[&str]) -> f64 {
    let mut available: BTreeMap<&str, usize> = BTreeMap::new();
    for token in reference {
        *available.entry(token).or_default() += 1;
    }
    // The multiset intersection: each answer token consumes one matching
    // reference token, so a repeated token is credited at most as often as the
    // reference repeats it.
    let same = answer
        .iter()
        .filter(|token| match available.get_mut(*token) {
            Some(count) if *count > 0 => {
                *count -= 1;
                true
            }
            _ => false,
        })
        .count();
    if same == 0 {
        return 0.0;
    }
    let precision = same as f64 / answer.len() as f64;
    let recall = same as f64 / reference.len() as f64;
    (2.0 * precision * recall) / (precision + recall)
}

/// The script leaves an empty reference list undefined — its `max` raises —
/// and ADR-C30 makes such a query unjudged, so no score exists to return.
fn require_references(references: &[String]) {
    assert!(
        !references.is_empty(),
        "exact match and token-F1 need at least one reference; a query with \
         none is unjudged and must not be scored"
    );
}

/// Exact match of one query's `answer` against its `references`: `1.0` when
/// the normalised answer equals some normalised reference, else `0.0` — the
/// script's `exact_match_score` under `metric_max_over_ground_truths`.
///
/// Two empty normalisations are equal, so an answer and a reference that both
/// normalise to empty (an empty answer against the reference ".") score `1.0`.
/// Normalisation is English-specific; see [`normalize_answer`].
///
/// # Panics
///
/// When `references` is empty. The script is undefined there, and a query
/// without a reference answer is unjudged: the harness leaves it out of the
/// mean rather than scoring it.
pub fn exact_match(answer: &str, references: &[String]) -> f64 {
    require_references(references);
    let answer = normalize_answer(answer);
    let hit = references
        .iter()
        .any(|reference| normalize_answer(reference) == answer);
    if hit {
        1.0
    } else {
        0.0
    }
}

/// Token-F1 of one query's `answer` against its `references`: the maximum
/// over references of the F1 between the multisets of normalised tokens — the
/// script's `f1_score` under `metric_max_over_ground_truths`. In `[0, 1]`.
///
/// An overlap of zero scores `0.0`, including when both sides normalise to
/// empty (where [`exact_match`] scores `1.0`): SQuAD v1.1's rule, not v2.0's.
/// The maximum is taken independently of [`exact_match`]'s, so the two may
/// come from different references. Normalisation is English-specific; see
/// [`normalize_answer`].
///
/// # Panics
///
/// When `references` is empty, as [`exact_match`] does.
pub fn token_f1(answer: &str, references: &[String]) -> f64 {
    require_references(references);
    let answer = normalize_answer(answer);
    let answer = tokens(&answer);
    references
        .iter()
        .map(|reference| f1_one(&answer, &tokens(&normalize_answer(reference))))
        .fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(references: &[&str]) -> Vec<String> {
        references.iter().map(|r| (*r).to_owned()).collect()
    }

    #[test]
    fn exact_match_holds_after_normalisation_only() {
        let references = refs(&["denver broncos"]);
        // Case, ASCII punctuation, a leading article and surplus whitespace
        // are all that differ.
        assert_eq!(exact_match("  The DENVER, Broncos!  ", &references), 1.0);
        // A different word is not normalised away.
        assert_eq!(exact_match("Denver Nuggets", &references), 0.0);
    }

    #[test]
    fn token_f1_with_partial_overlap_is_the_hand_computed_value() {
        // "the quick brown fox" -> [quick, brown, fox]
        // "a quick red fox jumps" -> [quick, red, fox, jumps]
        // overlap 2, precision 2/3, recall 2/4, F1 = 2·(2/3)·(1/2) / (2/3 + 1/2)
        //                                          = 4/7.
        let f1 = token_f1("the quick brown fox", &refs(&["a quick red fox jumps"]));
        assert!((f1 - 4.0 / 7.0).abs() < 1e-12, "F1 is {f1}, not 4/7");
    }

    #[test]
    fn token_f1_counts_tokens_as_a_multiset() {
        // [cat, cat] against [cat]: overlap 1, precision 1/2, recall 1 -> 2/3.
        let f1 = token_f1("cat cat", &refs(&["cat"]));
        assert!((f1 - 2.0 / 3.0).abs() < 1e-12, "F1 is {f1}, not 2/3");
    }

    #[test]
    fn several_references_take_the_maximum_of_each_metric_independently() {
        let references = refs(&["blue", "big red dog", "red dog"]);
        // "red dog" is the exact match; "big red dog" gives F1 1 to the
        // prediction "red big dog" — no single reference maximises both.
        assert_eq!(exact_match("red dog", &references), 1.0);
        assert_eq!(exact_match("red big dog", &references), 0.0);
        assert_eq!(token_f1("red big dog", &references), 1.0);
        // The maximum, not the mean or the first: 0.8 from "red dog" beats
        // the 0.0 of "blue" before it.
        let f1 = token_f1("red dog barks", &refs(&["blue", "red dog"]));
        assert!((f1 - 0.8).abs() < 1e-12, "F1 is {f1}, not 0.8");
    }

    #[test]
    fn empty_answer_against_a_non_empty_reference_scores_zero() {
        let references = refs(&["Paris"]);
        assert_eq!(exact_match("", &references), 0.0);
        assert_eq!(token_f1("", &references), 0.0);
    }

    #[test]
    fn identical_strings_score_one() {
        let references = refs(&["Super Bowl 50"]);
        assert_eq!(exact_match("Super Bowl 50", &references), 1.0);
        assert_eq!(token_f1("Super Bowl 50", &references), 1.0);
    }

    #[test]
    fn both_sides_normalising_to_empty_is_an_exact_match_with_zero_f1() {
        let references = refs(&["."]);
        assert_eq!(exact_match("", &references), 1.0);
        assert_eq!(token_f1("", &references), 0.0);
    }

    #[test]
    fn normalisation_follows_the_scripts_order() {
        // Punctuation goes before articles: the hyphen is removed, not
        // replaced, so "the-man" keeps its article.
        assert_eq!(normalize_answer("the-man"), "theman");
        assert_eq!(normalize_answer("Bankman-Fried"), "bankmanfried");
        // Only ASCII punctuation goes; the curly quote stays, and is a word
        // boundary, so the article before it goes.
        assert_eq!(normalize_answer("the\u{2019}s"), "\u{2019}s");
        // Every Unicode whitespace splits, including the information
        // separators Python's `str.split` counts and Rust's `is_whitespace`
        // does not.
        assert_eq!(normalize_answer("new\u{a0}york\u{1f}city"), "new york city");
        // A digit glued to an article makes it part of a word; an underscore
        // does not, because it is ASCII punctuation and goes first.
        assert_eq!(normalize_answer("the1 an_ a"), "the1");
    }

    /// The one divergence from the script this crate knows of, pinned so the
    /// crate documentation that describes it stays true. U+0345, a combining
    /// mark with the `Other_Alphabetic` property, is a word character to
    /// `char::is_alphanumeric` and not to Python's `\w`, so the script removes
    /// the "a" before it and this crate does not.
    #[test]
    fn an_other_alphabetic_mark_beside_an_article_is_the_known_divergence() {
        assert_eq!(normalize_answer("a\u{345}"), "a\u{345}");
        // An ordinary combining mark is no word character on either side, and
        // agrees with the script (which the parity fixture checks).
        assert_eq!(normalize_answer("a\u{301}"), "\u{301}");
    }

    #[test]
    #[should_panic(expected = "at least one reference")]
    fn an_empty_reference_list_is_a_caller_error() {
        exact_match("Paris", &[]);
    }
}
