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
//! ```
//! use async_trait::async_trait;
//! use ragondin_conformance::assert_reranker_conformance;
//! use ragondin_contracts::{ComponentError, RerankParams, Reranker};
//! use ragondin_types::{Query, ScoredChunk};
//!
//! struct MyReranker;
//!
//! #[async_trait]
//! impl Reranker for MyReranker {
//!     async fn rerank(
//!         &self,
//!         _query: &Query,
//!         mut chunks: Vec<ScoredChunk>,
//!         params: &RerankParams,
//!     ) -> Result<Vec<ScoredChunk>, ComponentError> {
//!         if params.top_k == 0 {
//!             return Err(ComponentError::InvalidRequest("top_k of zero".into()));
//!         }
//!         chunks.sort_by(|a, b| b.score.total_cmp(&a.score));
//!         chunks.truncate(params.top_k);
//!         Ok(chunks)
//!     }
//! }
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! // #[tokio::test]
//! assert_reranker_conformance(|| Box::new(MyReranker)).await;
//! # });
//! ```
//!
//! Each takes a **constructor**, not an instance: a scenario that writes must
//! not decide the next one's outcome, and one shape for all five families
//! means a contributor writes the same call whatever they implement. It must
//! be cheap, infallible and synchronous, and it is called more than once. The
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
//! `ragondin-metrics` against a benchmark, not here. **Conformance is a
//! floor**: a retriever returning nothing, or a reranker rejecting every
//! candidate, is conformant.
//!
//! For a retriever, an embedder or a reranker the suite knows nothing about
//! the corpus, index or model behind the trait object, so it asserts only what
//! holds whatever those contain. A fusion and a vector store are different:
//! the suite owns their whole input — it supplies the lists, and it writes the
//! vectors — so there it says what a correct answer looks like.
//!
//! # The check names
//!
//! [`ConformanceFailure::check`] returns one of these, and they are stable:
//!
//! | Check | Families |
//! |---|---|
//! | `well-formed call succeeds` | all |
//! | `descending scores`, `finite scores` | every family returning a ranked list |
//! | `top_k respected`, `zero top_k rejected` | `Retriever`, `Reranker`, `VectorStore` |
//! | `no fabricated ids`, `no duplicate ids` | `Fusion`, `Reranker` |
//! | `non-empty input yields output`, `order preserved` | `Fusion` |
//! | `one vector per input`, `constant dimensionality`, `finite components` | `Embedder` |
//! | `empty store yields no results`, `nearest neighbour is itself`, `upsert replaces by id`, `dimensionality` | `VectorStore` |
//!
//! See `ARCHITECTURE.md` and `docs/code-architecture.md` §7.4.

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
