//! The internal benchmark structure — the data-side mirror of the component
//! contract: one stable shape, N adapters that produce it.

use std::collections::BTreeMap;

use ragondin_types::{DocId, Document, Query, QueryId};

use crate::error::BenchmarkError;

/// Borrowed when a query has no judgments at all, so that iteration can yield
/// `&BTreeMap` uniformly instead of an `Option` the caller must unwrap.
static NO_JUDGMENTS: BTreeMap<DocId, u8> = BTreeMap::new();

/// The relevance judgments of a benchmark: which documents were assessed for
/// which query, and how relevant each was found to be.
///
/// This crate **owns** this type. `ragondin-metrics` defines none: its
/// functions take a ranked `&[DocId]` and a borrowed `&BTreeMap<DocId, u8>` —
/// the judgments of one query, which is exactly what [`Qrels::for_query`]
/// returns. That is why neither crate depends on the other.
///
/// A grade is a `u8` with **`0` meaning judged and not relevant**. Being judged
/// and being relevant are different things, and a grade-`0` entry must survive
/// parsing: dropping it would silently turn an assessed-irrelevant document
/// into an unassessed one. (Both crates must agree on this and nothing in the
/// type system checks it, which is why it is stated in both.)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Qrels {
    by_query: BTreeMap<QueryId, BTreeMap<DocId, u8>>,
}

impl Qrels {
    /// Creates an empty set of judgments.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `doc` was judged `grade` for `query`.
    ///
    /// A repeated `(query, doc)` pair overwrites: a dataset that judges the
    /// same pair twice is stating the later value, and silently keeping two
    /// would make the judgment count depend on iteration order.
    pub fn insert(&mut self, query: QueryId, doc: DocId, grade: u8) {
        self.by_query.entry(query).or_default().insert(doc, grade);
    }

    /// The judgments of one query, or `None` if it was never judged.
    ///
    /// The return type is `&BTreeMap<DocId, u8>` deliberately: it is the exact
    /// argument `ragondin-metrics` takes, so the harness passes it straight
    /// through with no conversion and no shared type between the crates.
    pub fn for_query(&self, query: &QueryId) -> Option<&BTreeMap<DocId, u8>> {
        self.by_query.get(query)
    }

    /// How many distinct queries carry at least one judgment.
    pub fn judged_query_count(&self) -> usize {
        self.by_query.len()
    }

    /// How many `(query, document)` judgments there are in total.
    pub fn judgment_count(&self) -> usize {
        self.by_query.values().map(BTreeMap::len).sum()
    }

    /// Whether there is no judgment at all.
    pub fn is_empty(&self) -> bool {
        self.by_query.is_empty()
    }
}

/// A loaded benchmark: a corpus, a query set, and the judgments linking them.
///
/// This is the retrieval family of the §5.3 quadruple — corpus + queries +
/// qrels — which needs no judge and yields deterministic metrics. The fourth
/// piece, reference answers, belongs to a later milestone and is deliberately
/// absent; the fields are private and construction goes through
/// [`Benchmark::new`] so that adding it later is additive rather than a
/// breaking change to every construction site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Benchmark {
    corpus: Vec<Document>,
    queries: Vec<Query>,
    qrels: Qrels,
}

impl Benchmark {
    /// Assembles a benchmark from its three pieces.
    ///
    /// Order is preserved as given, and adapters give it in file order: a run
    /// is only reproducible if the corpus is indexed in a fixed order.
    pub fn new(corpus: Vec<Document>, queries: Vec<Query>, qrels: Qrels) -> Self {
        Self {
            corpus,
            queries,
            qrels,
        }
    }

    /// The corpus, for the harness to index before it can retrieve anything.
    pub fn corpus(&self) -> &[Document] {
        &self.corpus
    }

    /// The queries to evaluate.
    pub fn queries(&self) -> &[Query] {
        &self.queries
    }

    /// Every judgment in the benchmark.
    pub fn qrels(&self) -> &Qrels {
        &self.qrels
    }

    /// Iterates `(query, judgments-for-that-query)` — the harness's loop.
    ///
    /// A query with no judgments yields an empty map rather than being skipped:
    /// deciding that an unjudged query should not be evaluated is the
    /// adapter's call, made when the query set is built, not a rule this
    /// structure may impose on a hand-built benchmark.
    pub fn iter(&self) -> impl Iterator<Item = (&Query, &BTreeMap<DocId, u8>)> {
        self.queries.iter().map(move |query| {
            (
                query,
                self.qrels.for_query(&query.id).unwrap_or(&NO_JUDGMENTS),
            )
        })
    }
}

/// Normalizes one external benchmark format into the internal [`Benchmark`].
///
/// The data-side mirror of the component contract: one stable interface, N
/// implementations. Adding a dataset format means adding an implementor, never
/// changing [`Benchmark`].
///
/// Synchronous by design. `async_trait` is the frozen decision for *component*
/// traits, which sit on the request hot path; a dataset is read once from local
/// files before a run starts, so an async signature would force a runtime into
/// this crate and buy nothing.
pub trait BenchmarkAdapter {
    /// Loads the benchmark this adapter was configured with.
    fn load(&self) -> Result<Benchmark, BenchmarkError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_document(id: &str) -> Document {
        Document {
            id: DocId::new(id),
            text: format!("the text of {id}"),
            metadata: BTreeMap::new(),
        }
    }

    fn a_query(id: &str) -> Query {
        Query {
            id: QueryId::new(id),
            text: format!("the text of {id}"),
        }
    }

    #[test]
    fn qrels_return_the_relevance_map_of_one_query() {
        let mut qrels = Qrels::new();
        qrels.insert(QueryId::new("q-1"), DocId::new("d-1"), 2);
        qrels.insert(QueryId::new("q-1"), DocId::new("d-2"), 0);
        qrels.insert(QueryId::new("q-2"), DocId::new("d-3"), 1);

        let relevance = qrels
            .for_query(&QueryId::new("q-1"))
            .expect("q-1 is judged");

        assert_eq!(relevance.len(), 2);
        assert_eq!(relevance.get(&DocId::new("d-1")), Some(&2));
        // Grade 0 is *judged and irrelevant*, not absent: it must survive.
        assert_eq!(relevance.get(&DocId::new("d-2")), Some(&0));
        assert_eq!(qrels.judged_query_count(), 2);
        assert_eq!(qrels.judgment_count(), 3);
    }

    #[test]
    fn qrels_report_an_unjudged_query_as_absent() {
        let qrels = Qrels::new();
        assert!(qrels.is_empty());
        assert!(qrels.for_query(&QueryId::new("q-1")).is_none());
    }

    #[test]
    fn iteration_pairs_each_query_with_its_own_relevance_map() {
        let mut qrels = Qrels::new();
        qrels.insert(QueryId::new("q-1"), DocId::new("d-1"), 1);
        qrels.insert(QueryId::new("q-2"), DocId::new("d-2"), 2);

        let benchmark = Benchmark::new(
            vec![a_document("d-1"), a_document("d-2")],
            vec![a_query("q-1"), a_query("q-2")],
            qrels,
        );

        let paired: Vec<(&str, Vec<(&str, u8)>)> = benchmark
            .iter()
            .map(|(query, relevance)| {
                (
                    query.id.as_str(),
                    relevance
                        .iter()
                        .map(|(doc, grade)| (doc.as_str(), *grade))
                        .collect(),
                )
            })
            .collect();

        assert_eq!(
            paired,
            vec![("q-1", vec![("d-1", 1)]), ("q-2", vec![("d-2", 2)]),]
        );
    }

    #[test]
    fn iteration_yields_an_empty_map_for_an_unjudged_query() {
        let benchmark = Benchmark::new(vec![], vec![a_query("q-1")], Qrels::new());

        let (query, relevance) = benchmark.iter().next().expect("one query");

        assert_eq!(query.id, QueryId::new("q-1"));
        assert!(relevance.is_empty());
    }

    #[test]
    fn the_corpus_is_reachable_in_file_order_for_indexing() {
        let benchmark = Benchmark::new(
            vec![a_document("d-2"), a_document("d-1")],
            vec![],
            Qrels::new(),
        );

        let ids: Vec<&str> = benchmark.corpus().iter().map(|d| d.id.as_str()).collect();
        // Insertion order, not sorted: the harness indexes what the file held.
        assert_eq!(ids, vec!["d-2", "d-1"]);
    }
}
