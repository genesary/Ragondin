//! The metric catalogue: every metric the harness records, by the name a
//! run's `metrics.json` holds it under, with its family and its direction.
//!
//! **Closed.** [`Metric`] is an enum, and [`Metric::family`] and
//! [`Metric::direction`] are matches with no wildcard arm, so a metric added
//! without a family or a direction does not compile. The harness writes every
//! name through [`Metric`]'s `Display`, and a reader reads one back through
//! [`Metric::parse`]: the name written and the name read have one spelling.
//!
//! **A name the catalogue does not know is unknown, never dropped.** A run's
//! metrics are an open map — a stored run may hold a name this build does not
//! know, written by another build or by hand — and [`Metric::parse`] answers
//! `None` for it. A reader keeps such a value, with no family and no
//! direction, so no reader calls it the best (`ARCHITECTURE.md` § The
//! catalogue).

use std::fmt;

/// A metric the harness records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Metric {
    /// `ndcg@<k>`: nDCG over the first `k` documents ([`crate::ndcg_at_k`]).
    Ndcg {
        /// The rank cutoff.
        k: usize,
    },
    /// `recall@<k>`: recall over the first `k` documents
    /// ([`crate::recall_at_k`]).
    Recall {
        /// The rank cutoff.
        k: usize,
    },
    /// `mrr`: the reciprocal rank, uncut, as `trec_eval`'s `recip_rank`
    /// ([`crate::reciprocal_rank`]).
    Mrr,
    /// `exact_match` ([`crate::exact_match`]).
    ExactMatch,
    /// `token_f1` ([`crate::token_f1`]).
    TokenF1,
}

/// Which ground truth a metric reads (ADR-C30 § 1, ADR-008).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// Scored from qrels: a ranking of documents against relevance judgments.
    Ranking,
    /// Scored from reference answers: a generated answer against them.
    Answers,
}

/// Which way a metric improves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// A higher value is better.
    HigherIsBetter,
    /// A lower value is better.
    LowerIsBetter,
}

impl Metric {
    /// The metric stored under `name`, or `None` for any name the catalogue
    /// does not hold. Exactly the names `Display` writes are read: a cutoff
    /// is a decimal integer with no sign and no leading zero, so a name read
    /// is always the name written back.
    pub fn parse(name: &str) -> Option<Self> {
        let cutoff = |prefix: &str| name.strip_prefix(prefix).and_then(canonical_cutoff);
        match name {
            "mrr" => Some(Self::Mrr),
            "exact_match" => Some(Self::ExactMatch),
            "token_f1" => Some(Self::TokenF1),
            _ => cutoff("ndcg@")
                .map(|k| Self::Ndcg { k })
                .or_else(|| cutoff("recall@").map(|k| Self::Recall { k })),
        }
    }

    /// The ground truth it reads.
    pub fn family(self) -> Family {
        match self {
            Self::Ndcg { .. } | Self::Recall { .. } | Self::Mrr => Family::Ranking,
            Self::ExactMatch | Self::TokenF1 => Family::Answers,
        }
    }

    /// Which way it improves.
    pub fn direction(self) -> Direction {
        match self {
            Self::Ndcg { .. }
            | Self::Recall { .. }
            | Self::Mrr
            | Self::ExactMatch
            | Self::TokenF1 => Direction::HigherIsBetter,
        }
    }
}

impl Direction {
    /// The direction the metric stored under `name` improves in, read from
    /// the catalogue; `None` for a name it does not know, which no reader
    /// ranks.
    pub fn of(name: &str) -> Option<Self> {
        Metric::parse(name).map(Metric::direction)
    }
}

/// `text` as a cutoff when it is spelled as `Display` spells one: digits
/// only, and no leading zero but in `0` itself.
fn canonical_cutoff(text: &str) -> Option<usize> {
    let digits = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let leading_zero = text.len() > 1 && text.starts_with('0');
    if digits && !leading_zero {
        text.parse().ok()
    } else {
        None
    }
}

impl fmt::Display for Metric {
    /// The name the metric is stored under in a run's `metrics.json`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ndcg { k } => write!(f, "ndcg@{k}"),
            Self::Recall { k } => write!(f, "recall@{k}"),
            Self::Mrr => f.write_str("mrr"),
            Self::ExactMatch => f.write_str("exact_match"),
            Self::TokenF1 => f.write_str("token_f1"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_entry() -> Vec<Metric> {
        let mut entries = vec![Metric::Mrr, Metric::ExactMatch, Metric::TokenF1];
        for k in [1, 10, 100] {
            entries.push(Metric::Ndcg { k });
            entries.push(Metric::Recall { k });
        }
        entries
    }

    #[test]
    fn every_catalogue_entry_reads_back_its_own_name() {
        for metric in every_entry() {
            assert_eq!(Metric::parse(&metric.to_string()), Some(metric), "{metric}");
        }
        assert_eq!(Metric::Ndcg { k: 10 }.to_string(), "ndcg@10");
        assert_eq!(Metric::Recall { k: 100 }.to_string(), "recall@100");
        assert_eq!(Metric::Mrr.to_string(), "mrr");
        assert_eq!(Metric::ExactMatch.to_string(), "exact_match");
        assert_eq!(Metric::TokenF1.to_string(), "token_f1");
    }

    #[test]
    fn a_name_the_catalogue_does_not_know_parses_to_nothing() {
        for other in [
            "latency_p50",
            "ndcg@",
            "ndcg@x",
            "ndcg@+10",
            "ndcg@010",
            "ndcg@-1",
            "ndcg@10 ",
            "NDCG@10",
            "precision@10",
            "mrr@10",
            "foo_score",
            "",
        ] {
            assert_eq!(Metric::parse(other), None, "{other:?}");
        }
    }

    #[test]
    fn answers_and_ranking_metrics_carry_their_family_and_direction() {
        let ndcg = Metric::parse("ndcg@10").unwrap();
        assert_eq!(ndcg.family(), Family::Ranking);
        assert_eq!(ndcg.direction(), Direction::HigherIsBetter);
        let f1 = Metric::parse("token_f1").unwrap();
        assert_eq!(f1.family(), Family::Answers);
        assert_eq!(f1.direction(), Direction::HigherIsBetter);

        for metric in [Metric::Recall { k: 10 }, Metric::Mrr] {
            assert_eq!(metric.family(), Family::Ranking, "{metric}");
        }
        assert_eq!(Metric::ExactMatch.family(), Family::Answers);
        for metric in every_entry() {
            assert_eq!(metric.direction(), Direction::HigherIsBetter, "{metric}");
        }
    }

    #[test]
    fn a_direction_is_read_off_a_name_only_through_the_catalogue() {
        assert_eq!(Direction::of("ndcg@10"), Some(Direction::HigherIsBetter));
        assert_eq!(Direction::of("latency_p50"), None);
    }
}
