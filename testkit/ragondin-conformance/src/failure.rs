//! What a conformance check reports when it fails.

use thiserror::Error;

use ragondin_contracts::ComponentError;

/// A single broken conformance check.
///
/// Returned rather than panicked so that a caller can assert on it — which is
/// how this crate's own tests prove the suite has teeth. [`crate`]'s `assert_*`
/// wrappers turn it back into a panic for the ordinary case, where a component
/// crate just wants a test that fails.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{component} conformance failed — {check}: {detail}")]
pub struct ConformanceFailure {
    component: &'static str,
    check: &'static str,
    detail: String,
}

impl ConformanceFailure {
    pub(crate) fn new(
        component: &'static str,
        check: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            component,
            check,
            detail: detail.into(),
        }
    }

    /// A call the contract requires to succeed returned an error instead.
    pub(crate) fn from_call(
        component: &'static str,
        context: &str,
        error: &ComponentError,
    ) -> Self {
        Self::new(
            component,
            "well-formed call succeeds",
            format!("{context} failed with: {error}"),
        )
    }

    /// The trait family whose contract was broken — `"Retriever"`, `"Fusion"`,
    /// `"Reranker"`, `"Embedder"`, `"VectorStore"`.
    pub fn component(&self) -> &'static str {
        self.component
    }

    /// The check that failed. Stable strings: a test may assert on them.
    pub fn check(&self) -> &'static str {
        self.check
    }

    /// What was observed, in the terms the implementer needs to debug it.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}
