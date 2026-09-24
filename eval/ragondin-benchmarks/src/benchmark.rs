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

    /// Iterates `(query, its judgments)` over every judged query.
    ///
    /// The whole judgment set, independent of any query file — which is what
    /// separates it from [`Benchmark::iter`]: that one walks the queries a run
    /// will actually execute, while this one walks what the qrels assert. An
    /// adapter checking its two files against each other needs the latter.
    pub fn iter(&self) -> impl Iterator<Item = (&QueryId, &BTreeMap<DocId, u8>)> {
        self.by_query.iter()
    }

    /// How many distinct queries the qrels file names, judged or not.
    ///
    /// This is **not** the count of *evaluable* queries, and must not be used
    /// as the denominator of a mean over a run. A qrels file can name a query
    /// that `queries.jsonl` never defines — `judged_in_split` filters queries by
    /// qrels, but nothing filters qrels by queries, so this count can exceed
    /// the number of queries a run can actually be scored against. The
    /// correct denominator for a mean over a run is `Benchmark::queries().len()`;
    /// a harness that macro-averages over this count instead would divide by
    /// the wrong number and quietly deflate every mean.
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

/// The reference answers of a benchmark: for each query, the strings a
/// generated answer is scored against.
///
/// The fourth piece of the §5.3 quadruple, held as ADR-C30 § 2 shapes it: a
/// `Vec<String>` per query, in the order the dataset gives them and with
/// repeats kept. Several references per query is the common case — most SQuAD
/// dev questions carry three — and a metric over several takes the maximum, so
/// order changes no score; it is kept anyway because a frozen fixture compares
/// files byte for byte.
///
/// References are labels, not a judge's output: ADR-10 puts label-based
/// metrics beneath any judge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferenceAnswers {
    by_query: BTreeMap<QueryId, Vec<String>>,
}

impl ReferenceAnswers {
    /// Creates a set holding no reference at all.
    pub fn new() -> Self {
        Self::default()
    }

    /// States the references of `query`, replacing any stated before.
    ///
    /// An empty list is the absence of a reference, not a reference: ADR-C30
    /// § 5 says a query carries a reference answer when its list is non-empty.
    /// So it is not stored, and it clears whatever the query held — keeping it
    /// would let `is_empty` and [`Benchmark::carries`] disagree about a set
    /// that holds only empty lists.
    pub fn insert(&mut self, query: QueryId, answers: Vec<String>) {
        if answers.is_empty() {
            self.by_query.remove(&query);
        } else {
            self.by_query.insert(query, answers);
        }
    }

    /// The references of one query, in dataset order, or `None` if it has none.
    pub fn for_query(&self, query: &QueryId) -> Option<&[String]> {
        self.by_query.get(query).map(Vec::as_slice)
    }

    /// Iterates `(query, its references)` over every query that has any.
    pub fn iter(&self) -> impl Iterator<Item = (&QueryId, &[String])> {
        self.by_query
            .iter()
            .map(|(query, answers)| (query, answers.as_slice()))
    }

    /// How many queries have at least one reference.
    ///
    /// Like [`Qrels::judged_query_count`], this is not the denominator of a
    /// mean over a run: it counts the queries this set names, which a
    /// hand-built benchmark need not hold.
    pub fn answered_query_count(&self) -> usize {
        self.by_query.len()
    }

    /// How many reference strings there are in total, repeats included.
    pub fn answer_count(&self) -> usize {
        self.by_query.values().map(Vec::len).sum()
    }

    /// Whether no query has a reference.
    pub fn is_empty(&self) -> bool {
        self.by_query.is_empty()
    }
}

/// Which pieces of the quadruple beyond corpus and queries a [`Benchmark`]
/// carries — ADR-8's regime, read off the value rather than configured.
///
/// A piece is carried when at least one of the benchmark's queries has a
/// non-empty list for it (ADR-C30 § 5): one judgment, grade `0` included, for
/// qrels; one string for reference answers. Exhaustive on purpose: the harness
/// matches on it to pick the metric families, and a fifth case would be a new
/// regime it must not absorb silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarriedPieces {
    /// Neither qrels nor reference answers: nothing can be scored.
    Neither,
    /// Qrels only: the retrieval metrics.
    QrelsOnly,
    /// Reference answers only: the generation metrics against reference.
    ReferenceAnswersOnly,
    /// Both: both families.
    QrelsAndReferenceAnswers,
}

/// A loaded benchmark: a corpus, a query set, the judgments linking them, and
/// the reference answers to the queries.
///
/// The §5.3 quadruple. Qrels feed the retrieval metrics and reference answers
/// the generation metrics; which of the two a benchmark actually carries is
/// what [`Benchmark::carries`] reports. The fields are private and
/// construction goes through [`Benchmark::new`], which builds a benchmark with
/// no reference answer; [`Benchmark::with_reference_answers`] adds them. That
/// split is why the fourth piece arrived without touching any call to `new`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Benchmark {
    corpus: Vec<Document>,
    queries: Vec<Query>,
    qrels: Qrels,
    reference_answers: ReferenceAnswers,
}

impl Benchmark {
    /// Assembles a benchmark from its three retrieval pieces, with no
    /// reference answer.
    ///
    /// Order is preserved as given, and adapters give it in file order: a run
    /// is only reproducible if the corpus is indexed in a fixed order.
    pub fn new(corpus: Vec<Document>, queries: Vec<Query>, qrels: Qrels) -> Self {
        Self {
            corpus,
            queries,
            qrels,
            reference_answers: ReferenceAnswers::new(),
        }
    }

    /// Gives the benchmark its fourth piece, replacing any it held.
    pub fn with_reference_answers(mut self, reference_answers: ReferenceAnswers) -> Self {
        self.reference_answers = reference_answers;
        self
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

    /// Every reference answer in the benchmark.
    pub fn reference_answers(&self) -> &ReferenceAnswers {
        &self.reference_answers
    }

    /// Which pieces the benchmark carries, judged over its own queries.
    ///
    /// Over [`Benchmark::queries`], not over the qrels or the references as
    /// sets: ADR-C30 § 5 counts a piece as carried when *one of the
    /// benchmark's queries* has a non-empty list for it, and a judgment naming
    /// a query the benchmark does not hold is nothing a run can score.
    pub fn carries(&self) -> CarriedPieces {
        let qrels = self
            .queries
            .iter()
            .any(|query| self.qrels.for_query(&query.id).is_some());
        let references = self
            .queries
            .iter()
            .any(|query| self.reference_answers.for_query(&query.id).is_some());
        match (qrels, references) {
            (false, false) => CarriedPieces::Neither,
            (true, false) => CarriedPieces::QrelsOnly,
            (false, true) => CarriedPieces::ReferenceAnswersOnly,
            (true, true) => CarriedPieces::QrelsAndReferenceAnswers,
        }
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

    /// Iterates `(query, its judgments, its references)`, in query order.
    ///
    /// [`Benchmark::iter`] with the fourth piece beside the third. A query with
    /// no reference yields an empty slice, exactly as an unjudged one yields an
    /// empty map: skipping it is a scoring decision, not this structure's.
    pub fn iter_with_references(
        &self,
    ) -> impl Iterator<Item = (&Query, &BTreeMap<DocId, u8>, &[String])> {
        self.iter().map(move |(query, judgments)| {
            (
                query,
                judgments,
                self.reference_answers
                    .for_query(&query.id)
                    .unwrap_or_default(),
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
    fn qrels_insert_lets_a_repeated_pair_overwrite_rather_than_accumulate() {
        // `insert`'s doc comment promises last-wins on a repeated (query, doc)
        // pair. Nothing exercised that before this test, so a regression to
        // first-wins or to summing grades would have passed the whole suite.
        let mut qrels = Qrels::new();
        qrels.insert(QueryId::new("q-1"), DocId::new("d-1"), 1);
        qrels.insert(QueryId::new("q-1"), DocId::new("d-1"), 3);

        let relevance = qrels
            .for_query(&QueryId::new("q-1"))
            .expect("q-1 is judged");
        assert_eq!(relevance.get(&DocId::new("d-1")), Some(&3));
        assert_eq!(qrels.judgment_count(), 1);
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

    fn references(pairs: &[(&str, &[&str])]) -> ReferenceAnswers {
        let mut references = ReferenceAnswers::new();
        for (query, answers) in pairs {
            references.insert(
                QueryId::new(*query),
                answers.iter().map(|answer| answer.to_string()).collect(),
            );
        }
        references
    }

    #[test]
    fn a_benchmark_built_by_new_alone_carries_qrels_only_and_no_references() {
        let mut qrels = Qrels::new();
        qrels.insert(QueryId::new("q-1"), DocId::new("d-1"), 1);
        let benchmark = Benchmark::new(vec![a_document("d-1")], vec![a_query("q-1")], qrels);

        assert_eq!(benchmark.carries(), CarriedPieces::QrelsOnly);
        assert!(benchmark.reference_answers().is_empty());

        let walked: Vec<(&str, usize, usize)> = benchmark
            .iter_with_references()
            .map(|(query, relevance, references)| {
                (query.id.as_str(), relevance.len(), references.len())
            })
            .collect();
        assert_eq!(walked, vec![("q-1", 1, 0)]);
    }

    #[test]
    fn a_grade_zero_judgment_is_enough_to_carry_qrels() {
        let mut qrels = Qrels::new();
        qrels.insert(QueryId::new("q-1"), DocId::new("d-1"), 0);
        let benchmark = Benchmark::new(vec![], vec![a_query("q-1")], qrels);

        assert_eq!(benchmark.carries(), CarriedPieces::QrelsOnly);
    }

    #[test]
    fn references_without_judgments_carry_reference_answers_only() {
        let benchmark = Benchmark::new(vec![], vec![a_query("q-1")], Qrels::new())
            .with_reference_answers(references(&[("q-1", &["an answer"])]));

        assert_eq!(benchmark.carries(), CarriedPieces::ReferenceAnswersOnly);
    }

    #[test]
    fn both_pieces_are_reported_together() {
        let mut qrels = Qrels::new();
        qrels.insert(QueryId::new("q-1"), DocId::new("d-1"), 1);
        let benchmark = Benchmark::new(vec![], vec![a_query("q-1"), a_query("q-2")], qrels)
            .with_reference_answers(references(&[("q-2", &["an answer"])]));

        assert_eq!(benchmark.carries(), CarriedPieces::QrelsAndReferenceAnswers);
    }

    #[test]
    fn nothing_carried_is_reported_as_neither() {
        let benchmark = Benchmark::new(vec![], vec![a_query("q-1")], Qrels::new())
            .with_reference_answers(ReferenceAnswers::new());

        assert_eq!(benchmark.carries(), CarriedPieces::Neither);
    }

    #[test]
    fn a_piece_is_carried_only_through_a_query_of_the_benchmark() {
        // "At least one of its queries": a judgment or a reference naming a
        // query the benchmark does not hold is not something a run can score.
        let mut qrels = Qrels::new();
        qrels.insert(QueryId::new("q-elsewhere"), DocId::new("d-1"), 1);
        let benchmark = Benchmark::new(vec![], vec![a_query("q-1")], qrels)
            .with_reference_answers(references(&[("q-elsewhere", &["an answer"])]));

        assert_eq!(benchmark.carries(), CarriedPieces::Neither);
    }

    #[test]
    fn an_empty_reference_list_is_the_absence_of_a_reference() {
        let answers = references(&[("q-1", &[])]);

        assert!(answers.is_empty());
        assert_eq!(answers.for_query(&QueryId::new("q-1")), None);
        let benchmark = Benchmark::new(vec![], vec![a_query("q-1")], Qrels::new())
            .with_reference_answers(answers);
        assert_eq!(benchmark.carries(), CarriedPieces::Neither);
    }

    #[test]
    fn references_keep_their_order_and_their_repeats() {
        let answers = references(&[("q-1", &["b", "a", "b"])]);

        assert_eq!(
            answers.for_query(&QueryId::new("q-1")),
            Some(&["b".to_string(), "a".to_string(), "b".to_string()][..])
        );
        assert_eq!(answers.answered_query_count(), 1);
        assert_eq!(answers.answer_count(), 3);
    }

    #[test]
    fn iteration_pairs_each_query_with_its_judgments_and_its_references() {
        let mut qrels = Qrels::new();
        qrels.insert(QueryId::new("q-1"), DocId::new("d-1"), 1);
        let benchmark = Benchmark::new(vec![], vec![a_query("q-1"), a_query("q-2")], qrels)
            .with_reference_answers(references(&[("q-2", &["x", "y"])]));

        let walked: Vec<(&str, usize, Vec<&str>)> = benchmark
            .iter_with_references()
            .map(|(query, relevance, references)| {
                (
                    query.id.as_str(),
                    relevance.len(),
                    references.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(walked, vec![("q-1", 1, vec![]), ("q-2", 0, vec!["x", "y"])]);
    }
}
