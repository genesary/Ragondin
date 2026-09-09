//! The BM25 [`Retriever`], and the in-memory tantivy index behind it.

use async_trait::async_trait;
use ragondin_contracts::{ComponentError, RetrieveParams, Retriever};
use ragondin_types::{Chunk, ChunkId, DocId, Query, ScoredChunk};
use tantivy::{
    collector::TopDocs,
    query::{BooleanQuery, Occur, Query as TantivyQuery, TermQuery},
    schema::{Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED},
    Index, IndexReader, TantivyDocument, Term,
};

/// Building the index over a corpus failed.
///
/// Typed rather than `anyhow`, because a library never imposes `anyhow` on its
/// consumers (ADR-C13). It is *not* a [`ComponentError`]: that type describes a
/// call across the component boundary, and construction happens before there is
/// a component to call.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IndexError {
    /// tantivy could not build or open the index.
    #[error("building the BM25 index failed")]
    Backend(#[source] tantivy::TantivyError),
}

impl From<tantivy::TantivyError> for IndexError {
    fn from(error: tantivy::TantivyError) -> Self {
        Self::Backend(error)
    }
}

/// BM25 retrieval over an in-memory tantivy index.
///
/// # Ranking
///
/// Results honour the ranking contract on [`Retriever`]: descending score,
/// every score finite. tantivy scores only documents that share a term with the
/// query, so a `top_k` larger than the number of matches returns fewer than
/// `top_k` hits — and a query matching nothing returns an empty list rather
/// than an error.
///
/// **Ties are broken by chunk id, before `top_k` truncates.** Two chunks with
/// the same BM25 score are ordered lexicographically, so a corpus can never
/// rank two ways. tantivy's own tie-break is by document address, which is
/// stable for one index but carries no meaning a caller could rely on; a
/// benchmark number that moves because two equal scores swapped is a bug
/// nothing else would report. Ordering the survivors of a `top_k` cut would not
/// be enough — the cut itself would still be made by document address — so
/// every matching document is sorted and the list is truncated afterwards.
pub struct Bm25Retriever {
    reader: IndexReader,
    index: Index,
    fields: Fields,
}

/// The schema, held by field handle so no lookup by name happens per query.
struct Fields {
    chunk_id: Field,
    document_id: Field,
    text: Field,
}

impl Bm25Retriever {
    /// Indexes `chunks` and returns a retriever over them.
    ///
    /// The index is built once, in RAM, and is immutable thereafter: this
    /// component searches a corpus, it does not maintain one. Re-indexing means
    /// constructing another retriever, which is also what makes a run
    /// reproducible — the same corpus yields the same index yields the same
    /// ranking.
    ///
    /// An empty corpus is valid and retrieves nothing.
    pub fn new(chunks: Vec<Chunk>) -> Result<Self, IndexError> {
        let mut schema = Schema::builder();
        // Positions are never used — a `TermQuery` needs frequencies, and this
        // component runs no phrase query — so indexing them would be paid for
        // on every corpus and read by nothing.
        // The analyzer is named rather than inherited: `"default"` is tantivy's
        // SimpleTokenizer + RemoveLongFilter(40) + LowerCaser, and the query is
        // run through this same field's analyzer, so naming it here fixes how
        // both sides are tokenized.
        let text_indexing = TextFieldIndexing::default()
            .set_tokenizer("default")
            .set_index_option(IndexRecordOption::WithFreqs);
        let text = schema.add_text_field(
            "text",
            TextOptions::default()
                .set_indexing_options(text_indexing)
                .set_stored(),
        );
        // Identifiers are carried, not searched: stored and unindexed. A
        // `ScoredChunk` holds the whole chunk (`ragondin-types`), so the corpus
        // must be reconstructible from a hit without a lookup the contract does
        // not offer.
        let chunk_id = schema.add_text_field("chunk_id", STORED);
        let document_id = schema.add_text_field("document_id", STORED);
        let schema = schema.build();

        let index = Index::create_in_ram(schema);
        // One writer thread: the segment layout, and therefore tantivy's own
        // ordering of equally scoring documents, is then a function of the
        // corpus rather than of how the work happened to be split.
        let mut writer = index.writer_with_num_threads(1, INDEX_WRITER_HEAP_BYTES)?;
        for chunk in chunks {
            let mut document = TantivyDocument::new();
            document.add_text(text, &chunk.text);
            document.add_text(chunk_id, chunk.id.as_str());
            document.add_text(document_id, chunk.document_id.as_str());
            writer.add_document(document)?;
        }
        writer.commit()?;

        let reader = index.reader()?;
        Ok(Self {
            reader,
            index,
            fields: Fields {
                chunk_id,
                document_id,
                text,
            },
        })
    }

    /// The query, as a BM25-scored disjunction over its terms.
    ///
    /// Deliberately not tantivy's `QueryParser`: a retriever is handed
    /// natural-language questions, and a parser would read `?`, `:` or a quote
    /// in one as syntax — failing the call, or silently changing what was
    /// asked. Running the field's own analyzer instead means the query is
    /// tokenized exactly as the corpus was.
    fn analyze(&self, text: &str) -> Result<BooleanQuery, ComponentError> {
        let mut analyzer = self
            .index
            .tokenizer_for_field(self.fields.text)
            .map_err(backend)?;
        let mut stream = analyzer.token_stream(text);

        let mut clauses: Vec<(Occur, Box<dyn TantivyQuery>)> = Vec::new();
        stream.process(&mut |token| {
            let term = Term::from_field_text(self.fields.text, &token.text);
            let query = TermQuery::new(term, IndexRecordOption::WithFreqs);
            clauses.push((Occur::Should, Box::new(query)));
        });

        Ok(BooleanQuery::new(clauses))
    }

    /// Rebuilds the [`Chunk`] a hit stands for from the stored fields.
    fn chunk_of(&self, document: &TantivyDocument) -> Result<Chunk, ComponentError> {
        Ok(Chunk {
            id: ChunkId::new(self.stored_text(document, self.fields.chunk_id, "chunk_id")?),
            text: self
                .stored_text(document, self.fields.text, "text")?
                .to_string(),
            document_id: DocId::new(self.stored_text(
                document,
                self.fields.document_id,
                "document_id",
            )?),
        })
    }

    fn stored_text<'a>(
        &self,
        document: &'a TantivyDocument,
        field: Field,
        name: &str,
    ) -> Result<&'a str, ComponentError> {
        document
            .get_first(field)
            .and_then(|value| value.as_str())
            // Unreachable while `new` is the only way to populate the index: it
            // writes all three fields as text for every chunk. Reported rather
            // than unwrapped so that a future writer of this index fails with a
            // message instead of a panic in somebody else's crate.
            .ok_or_else(|| {
                ComponentError::Backend(
                    format!("indexed document carries no stored `{name}`").into(),
                )
            })
    }
}

#[async_trait]
impl Retriever for Bm25Retriever {
    /// Searching an in-memory index is CPU work with no await point in it, so
    /// this runs inline. It is not offloaded to a blocking pool: that would
    /// oblige every consumer of this crate to run on one runtime, and a
    /// component crate imposes none (`ragondin-conformance` takes the same
    /// stance).
    async fn retrieve(
        &self,
        query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        if params.top_k == 0 {
            return Err(ComponentError::InvalidRequest(
                "top_k of zero: ask for at least one chunk".to_string(),
            ));
        }

        let analyzed = self.analyze(&query.text)?;
        let searcher = self.reader.searcher();
        // Every matching document, not the first `top_k` of them. `TopDocs`
        // selects at its own limit by document address, so truncating there
        // would let insertion order decide which of several equally scoring
        // chunks survive — and the tie-break below would then only reorder a
        // choice already made. `num_docs` is the ceiling on matches; the clamp
        // is for the empty corpus, where `with_limit(0)` panics.
        let ceiling = usize::try_from(searcher.num_docs()).unwrap_or(usize::MAX);
        let hits = searcher
            .search(
                &analyzed,
                &TopDocs::with_limit(ceiling.max(1)).order_by_score(),
            )
            .map_err(backend)?;

        let mut scored = Vec::with_capacity(hits.len());
        for (score, address) in hits {
            let document: TantivyDocument = searcher.doc(address).map_err(backend)?;
            scored.push(ScoredChunk {
                chunk: self.chunk_of(&document)?,
                // BM25 is a sum of finite per-term contributions, so this is
                // finite; the ranking contract requires it and the conformance
                // suite checks it.
                score,
            });
        }

        // Descending score, ties by chunk id. `total_cmp` rather than
        // `partial_cmp(…).unwrap()`: the latter is the panic the ranking
        // contract exists to prevent.
        scored.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.chunk.id.cmp(&b.chunk.id))
        });
        // Only now: the order is total, so the cut is a function of the corpus
        // and the query alone.
        scored.truncate(params.top_k);
        Ok(scored)
    }
}

fn backend(error: tantivy::TantivyError) -> ComponentError {
    ComponentError::Backend(Box::new(error))
}

/// The writer's in-memory budget. tantivy's documented floor is 15 MB; a corpus
/// larger than the budget is written out in several segments rather than
/// failing, so this bounds memory during construction and nothing else.
const INDEX_WRITER_HEAP_BYTES: usize = 15_000_000;
