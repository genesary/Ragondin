//! [`documents_by_first_occurrence`]: the fold from a ranking of chunks to the
//! ranking of documents every retrieval metric here scores.

use ragondin_types::DocId;

/// The documents of a ranking, each kept at its first occurrence.
///
/// A metric scores documents and a pipeline ranks chunks, so a ranking of
/// chunks is read as the document ids of those chunks, best first, and folded:
/// the best-ranked chunk of a document is where that document enters the
/// ranking, and its later chunks are dropped. Over a list already in
/// descending score order, that is the max-score-per-document rule BEIR
/// evaluations use.
///
/// The one definition of that rule. The harness applies it when it writes a
/// run's metrics, and `ragondin-api` when it recomputes per-query and per-node
/// figures from the stored traces, so a figure read back at a run's ranking
/// node is the one the run recorded. Generic over where the ids come from,
/// because the two read different trace types: the engine's, and the stored
/// document's.
pub fn documents_by_first_occurrence<'a>(ids: impl IntoIterator<Item = &'a DocId>) -> Vec<DocId> {
    let mut documents: Vec<DocId> = Vec::new();
    for id in ids {
        if !documents.contains(id) {
            documents.push(id.clone());
        }
    }
    documents
}

#[cfg(test)]
mod tests {
    use ragondin_types::DocId;

    use super::documents_by_first_occurrence;

    fn ids(names: &[&str]) -> Vec<DocId> {
        names.iter().map(|name| DocId::new(*name)).collect()
    }

    #[test]
    fn a_chunk_ranking_collapses_to_its_documents_by_first_occurrence() {
        // The documents of the chunks `d-1#2, d-2#0, d-1#0, d-3#1`, best first.
        let chunks = ids(&["d-1", "d-2", "d-1", "d-3"]);

        assert_eq!(
            documents_by_first_occurrence(&chunks),
            ids(&["d-1", "d-2", "d-3"]),
            "a document enters the ranking at its best chunk and never twice"
        );
    }

    #[test]
    fn an_empty_ranking_folds_to_no_document() {
        assert_eq!(documents_by_first_occurrence(&[]), Vec::<DocId>::new());
    }

    #[test]
    fn a_ranking_of_distinct_documents_is_kept_in_its_order() {
        let ranking = ids(&["d-3", "d-1", "d-2"]);

        assert_eq!(documents_by_first_occurrence(&ranking), ranking);
    }
}
