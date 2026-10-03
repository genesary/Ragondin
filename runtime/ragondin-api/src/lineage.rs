//! Which workspace documents a stored run's content belongs to.
//!
//! A run's identity is the canonical hash of what it ran. Its launch record
//! (`Run::provenance`, ADR-C39 § 1) says what it was launched as; this
//! module holds the other fact ADR-C39 § 4 keeps beside it, the content one:
//! the documents under `pipelines/` whose canonical hash is the run's (INV-8:
//! the canonical form, never the text). The two are never resolved into one
//! name: the pipeline matrix and `GET /runs` serve both, side by side.
//! [`pipeline_of`] names the one document a run's hash maps to — none, or
//! several, names none, and a document edited since a run no longer names
//! it — and `POST /compare` reads it to find a run's pairings when the run's
//! record names no pipeline (decided in #402). `GET /runs` and the pipeline matrix's
//! feeding runs read the index itself rather than [`pipeline_of`]: they name
//! *every* document sharing a run's hash, as a list, where a pairing needs
//! exactly one.
//!
//! The structural prefix test, [`is_prefix`], is here too: the other content
//! fact ADR-C39 § 5 asks of every run, by which the pipeline matrix counts a
//! run cut from a pipeline among that pipeline's runs.

use std::collections::{BTreeMap, BTreeSet};

use ragondin_experiments::Run;
use ragondin_pipeline::LogicalPipeline;

use crate::backends::{case_alias, PipelineSource};
use crate::error::ApiError;
use crate::response::NameHeld;
use crate::validation;

/// One listing of the workspace's pipelines, read two ways.
pub(crate) struct Index {
    /// Every pipeline document that validates, by its canonical hash, as
    /// `validation::check` renders it: a hash maps to several names when
    /// several documents are one canonical form.
    pub(crate) by_hash: BTreeMap<String, Vec<String>>,
    /// Every name the listing held, a document that does not validate
    /// included, stored exactly as given.
    pub(crate) names: BTreeSet<String>,
}

/// The workspace's pipelines, listed once: by canonical hash, and every name.
/// Each reader takes both facts from the one listing. `GET /runs` and the
/// pipeline matrix read the hash matches and whether each recorded name is
/// held ([`Index::held`]), so the two are never of two listings; `POST
/// /compare` reads the runs' pipelines and, by [`Index::held`], which of them
/// a pairing is read for. That narrows the window in which a document deleted
/// meanwhile names a run's pipeline and is then missing; it cannot close it,
/// since the document can still go before its pairing is read.
pub(crate) async fn index(source: &dyn PipelineSource) -> Result<Index, ApiError> {
    let mut index = Index {
        by_hash: BTreeMap::new(),
        names: BTreeSet::new(),
    };
    for file in source.list().await? {
        // A document that does not validate has no canonical form, so no
        // run can be a run of it.
        if let Ok(hash) = validation::check(&file.document) {
            index
                .by_hash
                .entry(hash)
                .or_default()
                .push(file.name.clone());
        }
        index.names.insert(file.name);
    }
    Ok(index)
}

impl Index {
    /// Whether the listing holds `name` as the backend would answer a read of
    /// it: a case alias of a stored name first ([`case_alias`]), since the
    /// backend refuses the name then even when it is also stored as given;
    /// then as given; otherwise not at all.
    pub(crate) fn held(&self, name: &str) -> NameHeld {
        if self.names.iter().any(|stored| case_alias(stored, name)) {
            NameHeld::OtherCase
        } else if self.names.contains(name) {
            NameHeld::Exactly
        } else {
            NameHeld::Gone
        }
    }
}

/// The pipeline `run` is a run of, by `index`: the one name its hash maps
/// to, `None` for none or several.
pub(crate) fn pipeline_of(index: &BTreeMap<String, Vec<String>>, run: &Run) -> Option<String> {
    match index.get(&run.inputs.pipeline.to_string()) {
        Some(names) if names.len() == 1 => Some(names[0].clone()),
        _ => None,
    }
}

/// Whether `run` is a prefix of `of` — the structural prefix test ADR-C39
/// § 5 and § 6 keep: `run` declares the same inputs as `of`, and every node
/// of `run` is a node of `of`, equal in its canonical logical form — family,
/// `impl:`, parameters, and its inputs in port order, so its edges too — and
/// `run` has fewer nodes. Its output node is then a node of `of`, and every
/// node it reads is kept, since `run` validated. The whole pipeline is not
/// its own prefix: that is hash equality.
///
/// It compares canonical forms, never text (INV-8), and it says nothing of
/// how `run` was launched: a pipeline written by hand that matches is a
/// prefix as much as one cut by "Run up to this node".
pub(crate) fn is_prefix(run: &LogicalPipeline, of: &LogicalPipeline) -> bool {
    run.inputs() == of.inputs()
        && run.nodes().len() < of.nodes().len()
        && run
            .nodes()
            .iter()
            .all(|node| of.nodes().iter().any(|kept| kept == node))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stages::tests::{lowered, HYBRID, HYBRID_RERANK, HYBRID_RERANK_GEN};

    #[test]
    fn the_pipeline_cut_at_a_node_is_a_prefix_of_it() {
        assert!(is_prefix(
            &lowered(HYBRID_RERANK),
            &lowered(HYBRID_RERANK_GEN)
        ));
        assert!(is_prefix(&lowered(HYBRID), &lowered(HYBRID_RERANK_GEN)));
    }

    #[test]
    fn a_pipeline_is_not_a_prefix_of_itself_nor_of_a_shorter_one() {
        let full = lowered(HYBRID_RERANK_GEN);
        assert!(!is_prefix(&full, &full));
        assert!(!is_prefix(&full, &lowered(HYBRID_RERANK)));
    }

    #[test]
    fn a_subset_with_one_changed_parameter_is_not_a_prefix() {
        let changed = HYBRID_RERANK.replace(
            "      impl: dense\n      inputs: [question]\n",
            "      impl: dense\n      inputs: [question]\n      params: { top_k: 7 }\n",
        );
        assert_ne!(changed, HYBRID_RERANK, "the parameter was added");
        assert!(!is_prefix(&lowered(&changed), &lowered(HYBRID_RERANK_GEN)));
    }

    #[test]
    fn a_subset_with_an_extra_edge_is_not_a_prefix() {
        // The fusion reads the dense leg twice: an edge the parent lacks.
        let extra = HYBRID_RERANK.replace("inputs: [bm25, dense]", "inputs: [bm25, dense, dense]");
        assert_ne!(extra, HYBRID_RERANK, "the edge was added");
        assert!(!is_prefix(&lowered(&extra), &lowered(HYBRID_RERANK_GEN)));
    }
}
