//! # ragondin-conformance
//!
//! The behavioural suite **every** component implementation must pass, whatever
//! its nature. It is what makes `Local`/`Remote` equivalence *real* rather than
//! asserted, and it is what operationally enforces "no privilege for built-ins"
//! (INV-7): a built-in and a third-party component plug into exactly the same
//! suite and earn exactly the same guarantee. Without it, INV-7 is a slogan.
//!
//! # Using it
//!
//! One function per trait family, called from the component crate's own tests:
//!
//! ```ignore
//! #[tokio::test]
//! async fn the_reranker_is_conformant() {
//!     assert_reranker_conformance(|| Box::new(MyReranker::new())).await;
//! }
//! ```
//!
//! Each takes a **constructor**, not an instance: a scenario that writes must
//! not decide the next one's outcome, and one shape for all five families
//! means a contributor writes the same call whatever they implement. The
//! `check_*` form returns the failure instead of panicking, for callers that
//! want to assert on it.
//!
//! The functions are `async` and start no runtime of their own, so this crate
//! imposes none on the crates that call it.
//!
//! # What it checks, and what it does not
//!
//! **Contract behaviour, never quality.** A conformant reranker need not be a
//! *good* reranker, only a well-behaved one; retrieval quality is measured by
//! `ragondin-metrics` against a benchmark, not here.
//!
//! The suite knows nothing about the corpus, index or model an implementation
//! was built over, so it asserts only what holds whatever those contain. That
//! is why no check requires a *non-empty* result — except for the vector
//! store, the one family the suite can write to before it reads.
//!
//! See `docs/code-architecture.md` §7.4.

#![warn(missing_docs)]

// Private modules with a flat re-export: a caller writes one path per family
// and never has to know which file a check lives in.
mod checks;
mod embedder;
mod failure;
mod fixtures;
mod fusion;
mod reranker;
mod retriever;
mod vector_store;

pub use embedder::{assert_embedder_conformance, check_embedder_conformance};
pub use failure::ConformanceFailure;
pub use fusion::{assert_fusion_conformance, check_fusion_conformance};
pub use reranker::{assert_reranker_conformance, check_reranker_conformance};
pub use retriever::{assert_retriever_conformance, check_retriever_conformance};
pub use vector_store::{assert_vector_store_conformance, check_vector_store_conformance};
