//! `Launcher`, implemented by the composition root (ADR-C36 § 1).
//!
//! The one path from the UI to the data plane (INV-12): `ragondin-api` holds
//! an `Arc<dyn Launcher>` and never names a component; this module, in the
//! only crate that knows them, answers for it. Everything here reuses what
//! `bench` already runs: the capabilities are [`wiring::carried`], a binding's
//! check is [`binding::check`] — `--remote`'s refusals, in its words — the
//! probe is [`wiring::service_identity`], the read `bench` makes before a run,
//! and a run is [`execution`]'s preparation and execution, the steps `bench`
//! is made of.
//!
//! - **`identity`** prepares the submission and returns the harness's
//!   identity over it: the announced id.
//! - **`execute`** prepares it again — the submission is the authority, and a
//!   job may sit queued for hours while a service swaps its model or a
//!   dataset is rewritten — and runs it. The id the returned run carries is
//!   the harness's over what ran, the decided one, and is never rewritten to
//!   match the announced one: comparing the two is the queue's.
//!
//! Both run on a thread of their own with its own runtime, never on the
//! API's: a preparation loads and digests the benchmark and may read model
//! files, and an execution runs the ONNX Runtime and components that may
//! block (ADR-C25). The API's task awaits the answer over a channel, so the
//! server keeps answering meanwhile.

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use ragondin_api::{
    ApiError, Cancellation, Capabilities, FamilyCapabilities, Launcher, LauncherError, Location,
    NotCarried, QueryProgress, RunObserver, ServiceBinding, ServiceIdentity, Submission,
};
use ragondin_contracts::ComponentError;
use ragondin_engine::ExecError;
use ragondin_experiments::{PrefixOf, Run, RunId, RunProvenance};
use ragondin_harness::HarnessError;
use ragondin_pipeline::LogicalPipeline;

use crate::binding::{self, Binding, Bindings, Family};
use crate::execution::{self, Pipeline, Prepared, Refusal};
use crate::wiring::{self, AtNode};

/// How long a probe waits for its connection: a person is waiting on the
/// answer, and an address that drops packets would otherwise hold them for
/// the operating system's TCP timeout — about 75 s on macOS, two minutes on
/// Linux. Long enough for a service across a slow tunnel.
#[cfg(feature = "remote")]
const PROBE_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The binary's `Launcher`. It holds the datasets directory a submission's
/// benchmark is read under — the one the registry downloads into — and
/// nothing else: the build's capabilities are fixed at compile time, and a
/// probe builds its channel per call.
pub struct BinaryLauncher {
    datasets: PathBuf,
}

impl BinaryLauncher {
    /// A launcher reading benchmarks under `datasets`.
    pub fn new(datasets: impl Into<PathBuf>) -> Self {
        Self {
            datasets: datasets.into(),
        }
    }
}

#[async_trait]
impl Launcher for BinaryLauncher {
    /// Every family `--remote` names, in [`Family::ALL`]'s order, with the
    /// `Local` names this build carries in it — none, for a family whose every
    /// implementation is feature-gated off — those it does not carry with the
    /// features that would, the family's ports, and whether it carries
    /// `remote`.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            families: Family::ALL
                .into_iter()
                .map(|family| FamilyCapabilities {
                    family: family.name().to_owned(),
                    local: wiring::carried()
                        .filter(|(of, _)| *of == family)
                        .map(|(_, name)| name.to_owned())
                        .collect(),
                    ports: ragondin_api::family_ports(family.name()),
                    not_carried: wiring::not_carried()
                        .filter(|(of, _, _)| *of == family)
                        .map(|(_, name, features)| NotCarried {
                            name: name.to_owned(),
                            reason: needs(features),
                        })
                        .collect(),
                })
                .collect(),
            remote: cfg!(feature = "remote"),
        }
    }

    /// `--remote`'s refusals of one argument, as `binding_refused`. A build
    /// without `remote` accepts a well-formed binding here: storing one is
    /// deployment data, and only calling it needs the feature.
    fn check_binding(&self, family: &str, name: &str, uri: &str) -> Result<(), ApiError> {
        binding::check(family, name, uri)
            .map(|_| ())
            .map_err(refused)
    }

    /// `bench`'s key refusals ([`wiring::check_keys`]), with the workspace's
    /// bindings deciding which names are bound: what `bench` would refuse to
    /// read is refused here, in its words, before the document is stored.
    /// The build-specific refusal `bench` adds — an `onnx` embedder in a build
    /// without the feature — is not made: a stored document is not a run.
    ///
    /// Only the bindings a node of the document uses are considered, as
    /// `bench` would only be given those: startup does not check
    /// `workspace.toml`'s bindings, and a malformed one nothing here names
    /// must not block the save of every document. One the document does use
    /// is checked, and refused as `binding_refused` naming it.
    fn check_document(
        &self,
        pipeline: &LogicalPipeline,
        bindings: &[ServiceBinding],
    ) -> Result<(), ApiError> {
        let bindings = bindings
            .iter()
            .filter(|binding| binding::used_by(pipeline, &binding.family, &binding.name))
            .map(|binding| binding::check(&binding.family, &binding.name, &binding.uri))
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(refused)?;
        wiring::check_keys(pipeline, &Bindings::from_checked(bindings)).map_err(|refusal| {
            let node = refusal.node.clone();
            ApiError::PipelineInvalid {
                detail: format!("{:#}", refusal.into_error()),
                location: Location { node, edge: None },
            }
        })
    }

    /// The binding goes through [`check_binding`](Self::check_binding), then
    /// through the identity read `bench` makes before a run, with
    /// `served_model` where the family reports an identity per served model.
    async fn probe(
        &self,
        family: &str,
        name: &str,
        uri: &str,
        served_model: Option<&str>,
    ) -> Result<ServiceIdentity, ApiError> {
        let binding = binding::check(family, name, uri).map_err(refused)?;
        read_identity(binding, served_model).await
    }

    /// The submission prepared — steps 1–3 and the index build, `bench`'s —
    /// and the harness's identity over the evaluation it will run.
    async fn identity(&self, submission: &Submission) -> Result<RunId, LauncherError> {
        let job = Job::of(submission, &self.datasets);
        on_own_thread(move || async move { Ok(job.prepare().await?.identity()) }).await
    }

    /// The submission prepared again, then executed — steps 4–6 — with its
    /// times stamped by this preparation and its launch record named after
    /// the submitted pipeline, and for a prefix run, cut from its parent at
    /// the submitted node and hash. Each query reaches `observer` as it completes,
    /// and `cancel` is read between two queries.
    async fn execute(
        &self,
        submission: &Submission,
        observer: Arc<dyn RunObserver>,
        cancel: Cancellation,
    ) -> Result<Run, LauncherError> {
        let job = Job::of(submission, &self.datasets);
        // A prefix run's submission names its parent, the node it was cut
        // at and the parent's hash: the record says what it was cut from
        // (ADR-C39 § 2). Any other launch records its name alone.
        let provenance = match (&submission.up_to, submission.parent_pipeline_hash) {
            (Some(up_to), Some(parent)) => RunProvenance::prefix(
                &submission.pipeline_name,
                PrefixOf::new(up_to.as_str(), parent),
            ),
            _ => RunProvenance::named(&submission.pipeline_name),
        };
        on_own_thread(move || async move {
            let prepared = job.prepare().await?;
            let flag = cancel.flag();
            prepared
                .execute(
                    Some(provenance),
                    |progress: ragondin_harness::QueryProgress<'_>| {
                        observer.query_done(QueryProgress {
                            position: progress.position as u64,
                            total: progress.total as u64,
                            query: progress.query.clone(),
                            elapsed: progress.elapsed,
                            trace: progress.trace.clone(),
                        })
                    },
                    &flag,
                )
                .await
                .map_err(failed)
        })
        .await
    }
}

/// A submission, owned, with the datasets directory its benchmark is read
/// under: what a run's thread takes with it.
struct Job {
    document: String,
    benchmark: String,
    bindings: Vec<ServiceBinding>,
    datasets: PathBuf,
}

impl Job {
    fn of(submission: &Submission, datasets: &std::path::Path) -> Self {
        Self {
            document: submission.pipeline.clone(),
            benchmark: submission.benchmark.clone(),
            bindings: submission.bindings.clone(),
            datasets: datasets.to_path_buf(),
        }
    }

    /// [`execution::prepare`] over the submission, with the bindings `bench`
    /// would be given for it: the workspace's bindings a node of the document
    /// uses, through `--remote`'s refusals. A submission carries every
    /// binding the workspace holds, and one no node uses is not this run's.
    async fn prepare(&self) -> Result<Prepared, LauncherError> {
        let pipeline = ragondin_config::parse_document(&self.document).map_err(|error| {
            LauncherError::PipelineInvalid {
                detail: format!("{:#}", anyhow::Error::from(error)),
                node: None,
            }
        })?;
        let used: Vec<&ServiceBinding> = self
            .bindings
            .iter()
            .filter(|binding| binding::used_by(&pipeline, &binding.family, &binding.name))
            .collect();
        // `--remote`'s refusal in a build without the feature, as the
        // capability it is.
        #[cfg(not(feature = "remote"))]
        if let Some(binding) = used.first() {
            return Err(LauncherError::ImplNotInBuild {
                family: binding.family.clone(),
                implementation: binding.name.clone(),
                feature: Some("remote".to_owned()),
            });
        }
        let arguments: Vec<String> = used
            .iter()
            .map(|binding| format!("{}/{}={}", binding.family, binding.name, binding.uri))
            .collect();
        let bindings =
            Bindings::parse(&arguments).map_err(|error| LauncherError::PipelineInvalid {
                detail: format!("{error:#}"),
                node: None,
            })?;
        execution::prepare(execution::Request {
            pipeline: Pipeline::Document(&self.document),
            benchmark: &self.benchmark,
            datasets: &self.datasets,
            bindings,
        })
        .await
        .map_err(refusal_of)
    }
}

/// Runs `work` to its end on a thread of its own, inside a runtime of its
/// own, and awaits its answer without holding the caller's runtime. A
/// dedicated thread rather than `spawn_blocking`, which would share the
/// API's blocking pool with every file read of every request.
async fn on_own_thread<F, W, T>(work: F) -> Result<T, LauncherError>
where
    F: FnOnce() -> W + Send + 'static,
    W: std::future::Future<Output = Result<T, LauncherError>>,
    T: Send + 'static,
{
    let (answer, answered) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("ragondin-run".to_owned())
        .spawn(move || {
            let result = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(work()),
                Err(error) => Err(execution_failed(format!(
                    "the run's runtime could not start: {error}"
                ))),
            };
            // The caller may have gone; the answer then has no one to reach.
            let _ = answer.send(result);
        })
        .map_err(|error| execution_failed(format!("the run's thread could not start: {error}")))?;
    answered.await.unwrap_or_else(|_| {
        Err(execution_failed(
            "the run's thread stopped without an answer",
        ))
    })
}

fn execution_failed(error: impl Into<String>) -> LauncherError {
    LauncherError::Execution {
        error: error.into(),
        at_node: None,
    }
}

/// A preparation's refusal, as `POST /runs` answers it: an `impl:` this build
/// lacks; a service that did not answer its identity read, at the address its
/// node is bound to; the pipeline, in `bench`'s words, naming the node when
/// one is at fault; and the benchmark, which no other variant names, as an
/// execution failure.
fn refusal_of(refusal: Refusal) -> LauncherError {
    match refusal {
        Refusal::NotInBuild(refusal) => LauncherError::ImplNotInBuild {
            family: refusal.family.name().to_owned(),
            implementation: refusal.name,
            feature: (!refusal.features.is_empty()).then(|| refusal.features.join("` or `")),
        },
        Refusal::Identity(error) => {
            let at = error.downcast_ref::<AtNode>();
            let unreachable = error.chain().any(|cause| {
                matches!(
                    cause.downcast_ref::<ComponentError>(),
                    Some(ComponentError::Unavailable(_))
                )
            });
            match (unreachable, at.and_then(|at| at.service.clone())) {
                (true, Some(uri)) => LauncherError::ServiceUnreachable {
                    uri,
                    reason: format!("{error:#}"),
                },
                _ => LauncherError::PipelineInvalid {
                    node: at.map(|at| at.node.clone()),
                    detail: format!("{error:#}"),
                },
            }
        }
        Refusal::Pipeline(error) => LauncherError::PipelineInvalid {
            detail: format!("{error:#}"),
            node: None,
        },
        Refusal::Benchmark(error) => execution_failed(format!("{error:#}")),
    }
}

/// An execution's failure: the harness's cancellation as `Cancelled`, a
/// query that failed naming its node when a component failed in one, and
/// anything else — the corpus that could not be embedded, a plan refused —
/// in its own words.
fn failed(error: anyhow::Error) -> LauncherError {
    match error.downcast_ref::<HarnessError>() {
        Some(HarnessError::Cancelled { .. }) => LauncherError::Cancelled,
        Some(HarnessError::Execute { source, .. }) => LauncherError::Execution {
            at_node: match source.as_ref() {
                ExecError::Component { node, .. } => Some(node.as_str().to_owned()),
                _ => None,
            },
            error: format!("{error:#}"),
        },
        _ => execution_failed(format!("{error:#}")),
    }
}

/// Why a build does not carry a component, from the features any one of
/// which would: "needs the `onnx` feature", "needs the `onnx` or the
/// `remote` feature".
fn needs(features: &[&str]) -> String {
    let named: Vec<String> = features
        .iter()
        .map(|feature| format!("the `{feature}`"))
        .collect();
    format!("needs {} feature", named.join(" or "))
}

fn refused(error: anyhow::Error) -> ApiError {
    ApiError::BindingRefused {
        detail: format!("{error:#}"),
    }
}

/// The identity of `binding`'s service. A failure is classified by where it
/// arose: a `ComponentError` from the service call — `Unavailable` is the
/// service unreachable, `InvalidRequest` a request the service refused, such
/// as a served model it does not serve — or a refusal before any call, which
/// is the request's to correct: a family whose service reports no identity,
/// or one that needs a served model and was given none.
#[cfg(feature = "remote")]
async fn read_identity(
    binding: Binding,
    served_model: Option<&str>,
) -> Result<ServiceIdentity, ApiError> {
    let uri = binding.uri.clone();
    let result = async {
        let bindings = Bindings::parse(&[format!(
            "{}/{}={}",
            binding.family, binding.name, binding.uri
        )])?;
        let bound = wiring::Bound::with_connect_timeout(bindings, PROBE_CONNECT_TIMEOUT)?;
        wiring::service_identity(&bound, binding.family, &binding.name, served_model).await
    }
    .await;
    let error = match result {
        Ok(identity) => {
            return Ok(ServiceIdentity {
                identity: identity.as_str().to_owned(),
            })
        }
        Err(error) => error,
    };
    let detail = format!("{error:#}");
    Err(
        match error
            .chain()
            .find_map(|cause| cause.downcast_ref::<ComponentError>())
        {
            Some(ComponentError::Unavailable(_)) => ApiError::ServiceUnreachable {
                uri,
                reason: detail,
                last_identity: None,
            },
            Some(ComponentError::InvalidRequest(_)) | None => ApiError::RequestInvalid { detail },
            Some(_) => ApiError::BackendFailed { detail },
        },
    )
}

/// A build without `remote` cannot construct the `Remote` adapter a probe
/// reads through: `impl_not_in_build`, naming the feature.
#[cfg(not(feature = "remote"))]
async fn read_identity(
    binding: Binding,
    _served_model: Option<&str>,
) -> Result<ServiceIdentity, ApiError> {
    Err(ApiError::ImplNotInBuild {
        family: binding.family.name().to_owned(),
        implementation: binding.name,
        feature: Some("remote".to_owned()),
    })
}

#[cfg(test)]
mod tests {

    use ragondin_api::FamilyCapabilities;

    use super::*;

    /// The binary's fixtures, where `beir-mini/` and the other miniature
    /// datasets sit.
    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn launcher() -> BinaryLauncher {
        BinaryLauncher::new(fixtures())
    }

    fn families(capabilities: &Capabilities) -> Vec<(&str, Vec<&str>)> {
        capabilities
            .families
            .iter()
            .map(|FamilyCapabilities { family, local, .. }| {
                (family.as_str(), local.iter().map(String::as_str).collect())
            })
            .collect()
    }

    /// Read by the two capability tests below, which compile in `ui` alone
    /// and in `--all-features` only.
    #[cfg(any(
        not(any(
            feature = "bm25",
            feature = "onnx",
            feature = "remote",
            feature = "stub"
        )),
        all(
            feature = "bm25",
            feature = "onnx",
            feature = "remote",
            feature = "stub"
        )
    ))]
    fn not_carried(capabilities: &Capabilities) -> Vec<(&str, Vec<(&str, &str)>)> {
        capabilities
            .families
            .iter()
            .map(|entry| {
                (
                    entry.family.as_str(),
                    entry
                        .not_carried
                        .iter()
                        .map(|missing| (missing.name.as_str(), missing.reason.as_str()))
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn every_family_is_listed_once_in_the_order_bindings_name_them() {
        let capabilities = launcher().capabilities();
        let names: Vec<&str> = families(&capabilities)
            .into_iter()
            .map(|(family, _)| family)
            .collect();

        assert_eq!(
            names,
            [
                "retriever",
                "fusion",
                "reranker",
                "context_builder",
                "generator",
                "embedder"
            ]
        );
    }

    /// The ports each node family serves, read back against what
    /// `ragondin-pipeline` declares for a node of that family: one node per
    /// family a configuration can name, lowered from a document, so the
    /// family's spelling is the configuration's own.
    #[test]
    fn every_node_family_serves_the_ports_ragondin_pipeline_declares() {
        let document = pipeline(
            "    - { id: r, component: retriever, impl: bm25, inputs: [question] }\n\
             \x20   - { id: f, component: fusion, impl: rrf, inputs: [r, r] }\n\
             \x20   - { id: k, component: reranker, impl: x, inputs: [question, f] }\n\
             \x20   - { id: c, component: context_builder, impl: concat, inputs: [question, k] }\n\
             \x20   - { id: g, component: generator, impl: x, inputs: [question, c] }\n",
        );
        let capabilities = launcher().capabilities();
        let ports = |family: &str| {
            let entry = capabilities
                .families
                .iter()
                .find(|entry| entry.family == family)
                .unwrap_or_else(|| panic!("`{family}` is listed"));
            serde_json::to_value(&entry.ports).expect("serializes")
        };

        let mut seen = Vec::new();
        for node in document.nodes() {
            let family = match node.id().as_str() {
                "r" => "retriever",
                "f" => "fusion",
                "k" => "reranker",
                "c" => "context_builder",
                "g" => "generator",
                other => panic!("no node `{other}` in the fixture"),
            };
            let consumes = match ragondin_pipeline::consumed_kinds(node) {
                ragondin_pipeline::PortSpec::Fixed(kinds) => serde_json::json!({
                    "shape": "fixed",
                    "kinds": kinds.iter().map(ToString::to_string).collect::<Vec<_>>(),
                }),
                ragondin_pipeline::PortSpec::Variadic(kind) => {
                    serde_json::json!({ "shape": "variadic", "kind": kind.to_string() })
                }
                ragondin_pipeline::PortSpec::Unknown => panic!("no configured family is unknown"),
            };
            assert_eq!(
                ports(family),
                serde_json::json!({
                    "produces": ragondin_pipeline::produced_kind(node).to_string(),
                    "consumes": consumes,
                }),
                "{family}"
            );
            seen.push(family);
        }
        seen.sort_unstable();
        assert_eq!(
            seen,
            [
                "context_builder",
                "fusion",
                "generator",
                "reranker",
                "retriever"
            ]
        );
        // No node is an embedder: it has no ports.
        assert_eq!(ports("embedder"), serde_json::Value::Null);
        // Every other family has, including one added to `Family::ALL` that
        // the fixture above does not name yet.
        for family in Family::ALL {
            let entry = capabilities
                .families
                .iter()
                .find(|entry| entry.family == family.name())
                .unwrap_or_else(|| panic!("`{family}` is listed"));
            assert_eq!(
                entry.ports.is_some(),
                family != Family::Embedder,
                "`{family}`'s ports"
            );
        }
    }

    /// `ui` alone: the build CI's per-feature clippy step and the `ui` job's
    /// test step compile.
    #[cfg(not(any(
        feature = "bm25",
        feature = "onnx",
        feature = "remote",
        feature = "stub"
    )))]
    #[test]
    fn with_ui_alone_the_build_carries_only_what_no_feature_gates() {
        let capabilities = launcher().capabilities();

        assert_eq!(
            families(&capabilities),
            [
                ("retriever", vec![]),
                ("fusion", vec!["rrf"]),
                ("reranker", vec![]),
                ("context_builder", vec!["concat"]),
                ("generator", vec![]),
                ("embedder", vec![]),
            ]
        );
        assert!(!capabilities.remote);
        assert_eq!(
            not_carried(&capabilities),
            [
                (
                    "retriever",
                    vec![
                        ("bm25", "needs the `bm25` feature"),
                        ("dense", "needs the `onnx` or the `remote` feature"),
                    ]
                ),
                ("fusion", vec![]),
                (
                    "reranker",
                    vec![("cross_encoder", "needs the `onnx` feature")]
                ),
                ("context_builder", vec![]),
                (
                    "generator",
                    vec![("stub_generator", "needs the `stub` feature")]
                ),
                ("embedder", vec![("onnx", "needs the `onnx` feature")]),
            ]
        );
    }

    /// `--all-features`: the build CI's feature-gated test step compiles.
    #[cfg(all(
        feature = "bm25",
        feature = "onnx",
        feature = "remote",
        feature = "stub"
    ))]
    #[test]
    fn with_every_feature_the_build_carries_every_local_component_and_remote() {
        let capabilities = launcher().capabilities();

        assert_eq!(
            families(&capabilities),
            [
                ("retriever", vec!["bm25", "dense"]),
                ("fusion", vec!["rrf"]),
                ("reranker", vec!["cross_encoder"]),
                ("context_builder", vec!["concat"]),
                ("generator", vec!["stub_generator"]),
                ("embedder", vec!["onnx"]),
            ]
        );
        assert!(capabilities.remote);
        assert!(
            not_carried(&capabilities)
                .iter()
                .all(|(_, names)| names.is_empty()),
            "{capabilities:?}"
        );
    }

    /// A submission of `document`, launched as `hybrid`, over `benchmark`,
    /// with `bindings` as the workspace holds them.
    fn submission(document: &str, benchmark: &str, bindings: Vec<ServiceBinding>) -> Submission {
        Submission {
            pipeline_name: "hybrid".to_owned(),
            pipeline: document.to_owned(),
            benchmark: benchmark.to_owned(),
            bindings,
            up_to: None,
            parent_pipeline_hash: None,
        }
    }

    fn fixture_text(name: &str) -> String {
        std::fs::read_to_string(fixtures().join(name)).expect("the fixture reads")
    }

    /// An observer that drops what it is told.
    struct Unobserved;
    impl RunObserver for Unobserved {
        fn query_done(&self, _: ragondin_api::QueryProgress) {}
    }

    const NOWHERE_RETRIEVER: &str = "pipeline:\n  inputs: [question]\n  nodes:\n    \
        - id: lexical\n      component: retriever\n      impl: nowhere\n      \
        inputs: [question]\n      params: { top_k: 10 }\n";

    #[tokio::test]
    async fn an_impl_the_build_lacks_is_refused_at_identity_before_the_benchmark_loads() {
        // `beir/absent` names no dataset: had the benchmark been loaded first,
        // the refusal would be about it.
        let unknown = submission(NOWHERE_RETRIEVER, "beir/absent", Vec::new());

        let error = launcher()
            .identity(&unknown)
            .await
            .expect_err("no build carries a retriever named `nowhere`");

        assert_eq!(
            error,
            LauncherError::ImplNotInBuild {
                family: "retriever".to_owned(),
                implementation: "nowhere".to_owned(),
                feature: None,
            }
        );
        // Execution prepares again, and refuses in the same words.
        let error = launcher()
            .execute(&unknown, Arc::new(Unobserved), Cancellation::new())
            .await
            .expect_err("the same preparation");
        assert!(
            matches!(&error, LauncherError::ImplNotInBuild { implementation, .. } if implementation == "nowhere"),
            "{error:?}"
        );
    }

    /// A name another build carries is refused naming the feature that would.
    #[cfg(not(feature = "bm25"))]
    #[tokio::test]
    async fn a_local_name_this_build_does_not_carry_names_its_feature() {
        let error = launcher()
            .identity(&submission(
                &fixture_text("lexical-pipeline.yaml"),
                "beir/absent",
                Vec::new(),
            ))
            .await
            .expect_err("`bm25` needs its feature");

        assert_eq!(
            error,
            LauncherError::ImplNotInBuild {
                family: "retriever".to_owned(),
                implementation: "bm25".to_owned(),
                feature: Some("bm25".to_owned()),
            }
        );
    }

    #[cfg(feature = "bm25")]
    mod over_bm25 {
        use std::path::Path;
        use std::sync::Mutex;
        use std::thread::ThreadId;
        use std::time::{Duration, SystemTime};

        use ragondin_api::QueryProgress;
        use ragondin_experiments::{FileSystemRunStore, UnixMillis};
        use ragondin_pipeline::PipelineHash;

        use super::*;

        /// A run store of this test's own, emptied first.
        fn store(test_name: &str) -> PathBuf {
            let root = std::env::temp_dir().join(format!(
                "ragondin-launcher-{}-{test_name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            root
        }

        fn lexical() -> Submission {
            submission(
                &fixture_text("lexical-pipeline.yaml"),
                "beir/beir-mini",
                Vec::new(),
            )
        }

        /// Files the lexical fixture with `bench` into `store`, outside the
        /// workspace convention, so with no launch record.
        async fn bench_lexical(store: &Path) {
            crate::bench::run(&crate::bench::Request {
                config: &fixtures().join("lexical-pipeline.yaml"),
                benchmark: "beir/beir-mini",
                datasets: &fixtures(),
                store,
                remote: &[],
            })
            .await
            .expect("bench runs the lexical fixture");
        }

        #[tokio::test]
        async fn the_announced_identity_equals_the_one_bench_files() {
            let store = store("announced");
            bench_lexical(&store).await;

            let announced = launcher()
                .identity(&lexical())
                .await
                .expect("the lexical fixture is runnable here");

            FileSystemRunStore::new(&store)
                .load(&announced)
                .expect("bench filed the run under the announced id");
        }

        /// Every observer call, with the thread it was made on.
        #[derive(Default)]
        struct Recording {
            calls: Mutex<Vec<(ThreadId, QueryProgress)>>,
            cancel_at: Option<(u64, Cancellation)>,
            /// Waited on by the first call: sent by a task of the caller's
            /// runtime, which can run only if `execute` does not hold it.
            gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
            gate_opened: Mutex<Option<bool>>,
        }

        impl RunObserver for Recording {
            fn query_done(&self, progress: QueryProgress) {
                if let Some(gate) = self.gate.lock().expect("lock").take() {
                    let opened = gate.recv_timeout(Duration::from_secs(20)).is_ok();
                    *self.gate_opened.lock().expect("lock") = Some(opened);
                }
                if let Some((at, cancel)) = &self.cancel_at {
                    if progress.position == *at {
                        cancel.cancel();
                    }
                }
                self.calls
                    .lock()
                    .expect("lock")
                    .push((std::thread::current().id(), progress));
            }
        }

        #[tokio::test]
        async fn execute_runs_off_the_caller_s_thread_and_reports_every_query() {
            let (open, gate) = std::sync::mpsc::channel();
            let observer = Arc::new(Recording {
                gate: Mutex::new(Some(gate)),
                ..Recording::default()
            });
            // On the caller's runtime — a current-thread one, as a handler's
            // task shares its worker with every other request's.
            let concurrent = tokio::spawn(async move { open.send(()).is_ok() });

            let run = launcher()
                .execute(&lexical(), observer.clone(), Cancellation::new())
                .await
                .expect("the lexical fixture runs");

            assert!(concurrent.await.expect("the task ran"));
            assert_eq!(
                *observer.gate_opened.lock().expect("lock"),
                Some(true),
                "a task of the caller's runtime ran while the run was executing"
            );
            let calls = observer.calls.lock().expect("lock");
            let caller = std::thread::current().id();
            assert_eq!(calls.len(), 3, "one call per query of beir-mini");
            for (position, (thread, progress)) in calls.iter().enumerate() {
                assert_ne!(*thread, caller, "observed off the caller's thread");
                assert_eq!(progress.position, position as u64 + 1);
                assert_eq!(progress.total, 3);
                assert_eq!(
                    progress.trace, run.traces[&progress.query],
                    "the trace the run files"
                );
            }
        }

        #[tokio::test]
        async fn cancellation_stops_between_queries_and_reports_how_many_ran() {
            let cancel = Cancellation::new();
            let observer = Arc::new(Recording {
                cancel_at: Some((2, cancel.clone())),
                ..Recording::default()
            });

            let error = launcher()
                .execute(&lexical(), observer.clone(), cancel)
                .await
                .expect_err("cancelled after the second query");

            assert_eq!(error, LauncherError::Cancelled);
            assert_eq!(observer.calls.lock().expect("lock").len(), 2);
        }

        fn now() -> UnixMillis {
            UnixMillis::from_system_time(SystemTime::now()).expect("a clock after the epoch")
        }

        #[tokio::test]
        async fn execute_stamps_the_run_s_times_from_its_own_preparation() {
            // An identity read first, and time spent after it: neither is the
            // run's.
            launcher()
                .identity(&lexical())
                .await
                .expect("the lexical fixture is runnable here");
            tokio::time::sleep(Duration::from_millis(20)).await;
            let before = now();

            let run = launcher()
                .execute(&lexical(), Arc::new(Unobserved), Cancellation::new())
                .await
                .expect("the lexical fixture runs");

            let after = now();
            let times = run.times.expect("a clock after the epoch gives times");
            assert!(before <= times.started(), "{before:?} {times:?}");
            assert!(times.started() <= times.finished(), "{times:?}");
            assert!(times.finished() <= after, "{times:?} {after:?}");
        }

        #[tokio::test]
        async fn execute_stamps_the_submitted_name_and_prefix_of_on_the_run() {
            let store = store("provenance");
            bench_lexical(&store).await;

            let run = launcher()
                .execute(&lexical(), Arc::new(Unobserved), Cancellation::new())
                .await
                .expect("the lexical fixture runs");

            let provenance = run.provenance.as_ref().expect("a launch record");
            assert_eq!(provenance.name(), Some("hybrid"));
            assert_eq!(provenance.prefix_of(), None);
            // The record is outside identity: the same run, filed by `bench`
            // with no record, has the same id.
            let filed = FileSystemRunStore::new(&store)
                .load(&run.id)
                .expect("bench filed the same id");
            assert_eq!(filed.provenance, None);

            // A prefix run: the API hands the cut as the document, the
            // parent's name, the node and the parent's hash. The record says
            // what it was cut from; the run's id is still the document's.
            let parent = PipelineHash::from_digest([0x5e; 32]);
            let prefix = Submission {
                up_to: Some("lexical".to_owned()),
                parent_pipeline_hash: Some(parent),
                ..lexical()
            };
            let cut = launcher()
                .execute(&prefix, Arc::new(Unobserved), Cancellation::new())
                .await
                .expect("the lexical fixture runs");
            assert_eq!(
                cut.provenance,
                Some(RunProvenance::prefix(
                    "hybrid",
                    PrefixOf::new("lexical", parent)
                ))
            );
            assert_eq!(cut.id, run.id, "the record is outside identity");
        }

        #[tokio::test]
        async fn a_benchmark_that_does_not_resolve_or_load_is_an_execution_failure() {
            // No other variant names a benchmark: `POST /runs` answers it as
            // `backend_failed`, in the loader's words.
            for (benchmark, words) in [
                ("beir/absent", "loading the benchmark"),
                ("trec/robust04", "is not a benchmark format"),
            ] {
                let error = launcher()
                    .identity(&submission(
                        &fixture_text("lexical-pipeline.yaml"),
                        benchmark,
                        Vec::new(),
                    ))
                    .await
                    .expect_err("no such benchmark");

                assert!(
                    matches!(&error, LauncherError::Execution { error, at_node: None } if error.contains(words)),
                    "{benchmark}: {error:?}"
                );
            }
        }

        /// Records when each query was observed, and lingers on the last one,
        /// so the run's end is after it by a margin a millisecond clock sees.
        struct Timing(Mutex<Vec<UnixMillis>>);

        impl RunObserver for Timing {
            fn query_done(&self, progress: QueryProgress) {
                if progress.position == progress.total {
                    std::thread::sleep(Duration::from_millis(30));
                }
                self.0.lock().expect("lock").push(now());
            }
        }

        #[tokio::test]
        async fn finished_is_read_once_the_last_query_has_run() {
            let observer = Arc::new(Timing(Mutex::new(Vec::new())));

            let run = launcher()
                .execute(&lexical(), observer.clone(), Cancellation::new())
                .await
                .expect("the lexical fixture runs");

            let last = *observer.0.lock().expect("lock").last().expect("observed");
            let times = run.times.expect("a clock after the epoch gives times");
            assert!(last <= times.finished(), "{last:?} {times:?}");
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn started_is_read_before_the_benchmark_loads() {
            // The corpus is a pipe, written only once the loader has opened
            // it and some time has passed: a `started` read after the load
            // would fall after `opened`.
            let (datasets, corpus) = datasets_with_a_piped_corpus("started");
            let text = std::fs::read(fixtures().join("beir-mini/corpus.jsonl")).expect("reads");
            let writer = tokio::spawn(async move {
                use tokio::io::AsyncWriteExt;
                loop {
                    match tokio::net::unix::pipe::OpenOptions::new().open_sender(&corpus) {
                        Ok(mut sender) => {
                            let opened = now();
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            sender.write_all(&text).await.expect("the corpus writes");
                            return opened;
                        }
                        Err(_) => tokio::time::sleep(Duration::from_millis(5)).await,
                    }
                }
            });

            let run = BinaryLauncher::new(&datasets)
                .execute(&lexical(), Arc::new(Unobserved), Cancellation::new())
                .await
                .expect("the piped benchmark loads and runs");

            let opened = writer.await.expect("the writer ran");
            let times = run.times.expect("a clock after the epoch gives times");
            assert!(times.started() <= opened, "{times:?} {opened:?}");
        }

        /// `beir-mini` copied into a directory of this test's own, its corpus
        /// a named pipe: reading the benchmark blocks until something writes
        /// the corpus into it.
        #[cfg(unix)]
        fn datasets_with_a_piped_corpus(test_name: &str) -> (PathBuf, PathBuf) {
            let root = store(test_name);
            let dataset = root.join("beir-mini");
            std::fs::create_dir_all(dataset.join("qrels")).expect("a writable tmpdir");
            let source = fixtures().join("beir-mini");
            for file in ["queries.jsonl", "qrels/test.tsv"] {
                std::fs::copy(source.join(file), dataset.join(file)).expect("the fixture copies");
            }
            let corpus = dataset.join("corpus.jsonl");
            let made = std::process::Command::new("mkfifo")
                .arg(&corpus)
                .status()
                .expect("mkfifo runs");
            assert!(made.success());
            (root, corpus)
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn a_concurrent_request_is_answered_while_an_identity_is_computed() {
            use std::sync::atomic::{AtomicBool, Ordering};

            let (datasets, corpus) = datasets_with_a_piped_corpus("piped");
            let text = std::fs::read(fixtures().join("beir-mini/corpus.jsonl")).expect("reads");
            // Only if the caller's runtime is free: the corpus written by a
            // task of it. Retried, because the pipe opens for writing only
            // once its reader has opened it.
            let writer = {
                let (corpus, text) = (corpus.clone(), text.clone());
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt;
                    loop {
                        match tokio::net::unix::pipe::OpenOptions::new().open_sender(&corpus) {
                            Ok(mut sender) => {
                                sender.write_all(&text).await.expect("the corpus writes");
                                return;
                            }
                            Err(_) => tokio::time::sleep(Duration::from_millis(5)).await,
                        }
                    }
                })
            };
            // Should the runtime be held, a watchdog writes the corpus after
            // a while, so the test fails rather than hangs.
            let watchdog_fired = Arc::new(AtomicBool::new(false));
            let watchdog = {
                let fired = Arc::clone(&watchdog_fired);
                let done = Arc::new(AtomicBool::new(false));
                let stop = Arc::clone(&done);
                let handle = std::thread::spawn(move || {
                    for _ in 0..200 {
                        if stop.load(Ordering::SeqCst) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    fired.store(true, Ordering::SeqCst);
                    let mut pipe = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&corpus)
                        .expect("the pipe opens");
                    std::io::Write::write_all(&mut pipe, &text).expect("the corpus writes");
                });
                (done, handle)
            };

            let announced = BinaryLauncher::new(&datasets)
                .identity(&lexical())
                .await
                .expect("the piped benchmark loads");

            watchdog.0.store(true, Ordering::SeqCst);
            watchdog.1.join().expect("the watchdog ends");
            assert!(
                !watchdog_fired.load(Ordering::SeqCst),
                "the caller's runtime was held while the identity was computed"
            );
            writer.await.expect("the writer ran");
            assert_eq!(
                announced,
                launcher().identity(&lexical()).await.expect("runnable"),
                "the same benchmark, read through a pipe"
            );
        }
    }

    #[test]
    fn a_binding_is_checked_with_the_words_of_remote() {
        for (family, name, uri, words) in [
            ("store", "qdrant", "http://host", "`store` is not a family"),
            (
                "generator",
                "qwen",
                "https://host",
                "is not an `http://` URI",
            ),
            ("context_builder", "concat", "http://host", "`Local`"),
        ] {
            let error = launcher()
                .check_binding(family, name, uri)
                .expect_err("refused");

            assert!(
                matches!(&error, ApiError::BindingRefused { detail }
                    if detail.contains(&format!("`--remote {family}/{name}={uri}`"))
                        && detail.contains(words)),
                "{error:?}"
            );
        }
        // Whatever the build: storing a binding needs no `remote`.
        launcher()
            .check_binding("generator", "qwen", "http://[::1]:8080")
            .expect("well formed");
    }

    fn pipeline(nodes: &str) -> ragondin_pipeline::LogicalPipeline {
        let yaml = format!("pipeline:\n  inputs: [question]\n  nodes:\n{nodes}");
        ragondin_config::parse_document(&yaml).expect("the fixture loads")
    }

    const CROSS_ENCODER_NODE: &str = "    - id: lexical\n      component: retriever\n      \
         impl: bm25\n      inputs: [question]\n      params: { top_k: 10 }\n\
         \x20   - id: ranked\n      component: reranker\n      impl: cross_encoder\n      \
         inputs: [question, lexical]\n      params: { top_k: 5, model: m.onnx, tokenizer: t.json";

    #[test]
    fn a_document_with_a_key_its_component_does_not_read_is_refused_in_bench_s_words() {
        let document = pipeline(&format!(
            "{CROSS_ENCODER_NODE}, endpoint: \"http://10.0.0.5:50051\" }}\n"
        ));

        let error = launcher()
            .check_document(&document, &[])
            .expect_err("`endpoint` is read by nothing");

        match error {
            ApiError::PipelineInvalid { detail, location } => {
                assert_eq!(
                    detail,
                    "node `ranked`: `endpoint` is not a key the ONNX cross-encoder reads: \
                     refused rather than hashed as inert"
                );
                assert_eq!(location.node.as_deref(), Some("ranked"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_url_valued_parameter_a_component_reads_is_accepted() {
        // A prefix is read by every `dense` node, whatever it holds.
        let document = pipeline(
            "    - id: vectors\n      component: retriever\n      impl: dense\n      \
             inputs: [question]\n      params: { top_k: 10, embedder: onnx, model: m.onnx, \
             tokenizer: t.json, query_prefix: \"http://example.org/q \" }\n",
        );

        launcher()
            .check_document(&document, &[])
            .expect("every key is one the ONNX embedder's node reads");
    }

    fn binding(family: &str, name: &str, uri: &str) -> ragondin_api::ServiceBinding {
        ragondin_api::ServiceBinding {
            family: family.to_owned(),
            name: name.to_owned(),
            uri: uri.to_owned(),
        }
    }

    const BM25_ONLY: &str =
        "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
         inputs: [question]\n      params: { top_k: 10 }\n";

    #[test]
    fn a_bad_binding_no_node_uses_does_not_affect_the_document() {
        // Hand-edited into `workspace.toml`, which startup reads without
        // checking its bindings: a family that is none, a URI that is not
        // `http://`, and a malformed binding of a name no node names.
        let bindings = [
            binding("store", "q", "ftp://h"),
            binding("generator", "qwen", "ftp://h"),
            binding("reranker", "unused", "https://h"),
        ];

        launcher()
            .check_document(&pipeline(BM25_ONLY), &bindings)
            .expect("no node uses any of them");
    }

    #[test]
    fn a_bad_binding_the_document_uses_is_refused_naming_it() {
        let document = pipeline(
            "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
             inputs: [question]\n      params: { top_k: 10 }\n\
             \x20   - id: ranked\n      component: reranker\n      impl: bge-reranker\n      \
             inputs: [question, lexical]\n      params: { top_k: 5, served_model: r }\n",
        );

        let error = launcher()
            .check_document(
                &document,
                &[
                    binding("store", "q", "ftp://h"),
                    binding("reranker", "bge-reranker", "ftp://h"),
                ],
            )
            .expect_err("`ranked` names a binding `--remote` refuses");

        assert!(
            matches!(&error, ApiError::BindingRefused { detail }
                if detail.contains("`--remote reranker/bge-reranker=ftp://h`")),
            "{error:?}"
        );
    }

    #[test]
    fn the_workspace_bindings_decide_what_a_bound_name_may_carry() {
        let document = pipeline(
            "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
             inputs: [question]\n      params: { top_k: 10 }\n\
             \x20   - id: ranked\n      component: reranker\n      impl: bge-reranker\n      \
             inputs: [question, lexical]\n      params: { top_k: 5, served_model: r }\n",
        );
        let bound = [ragondin_api::ServiceBinding {
            family: "reranker".to_owned(),
            name: "bge-reranker".to_owned(),
            uri: "http://127.0.0.1:9000".to_owned(),
        }];

        launcher()
            .check_document(&document, &bound)
            .expect("a bound reranker reads `top_k` and `served_model`");
        let stray = pipeline(
            "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
             inputs: [question]\n      params: { top_k: 10 }\n\
             \x20   - id: ranked\n      component: reranker\n      impl: bge-reranker\n      \
             inputs: [question, lexical]\n      params: { top_k: 5, served_model: r, model: m }\n",
        );
        let error = launcher()
            .check_document(&stray, &bound)
            .expect_err("a bound reranker reads no `model`");
        assert!(
            matches!(&error, ApiError::PipelineInvalid { detail, .. }
                if detail.contains("`model` is not a key the bound reranker `bge-reranker` reads")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_probe_that_is_not_a_well_formed_binding_is_refused_with_the_binding_s_reason() {
        let error = launcher()
            .probe("store", "qdrant", "http://127.0.0.1:1", None)
            .await
            .expect_err("`store` is not a family a name is bound in");

        assert!(
            matches!(&error, ApiError::BindingRefused { detail } if detail.contains("store/qdrant")),
            "{error:?}"
        );
    }

    #[cfg(not(feature = "remote"))]
    #[tokio::test]
    async fn a_build_without_remote_answers_every_probe_with_impl_not_in_build() {
        let error = launcher()
            .probe("context_builder", "lines", "http://127.0.0.1:1", None)
            .await
            .expect_err("this build cannot reach a `Remote` component");

        assert_eq!(
            error,
            ApiError::ImplNotInBuild {
                family: "context_builder".to_owned(),
                implementation: "lines".to_owned(),
                feature: Some("remote".to_owned()),
            }
        );
        assert_eq!(error.code(), "impl_not_in_build");
    }

    #[cfg(feature = "remote")]
    mod remote {
        use std::time::{Duration, Instant};

        use super::*;
        use crate::remote_fakes as fakes;

        #[tokio::test]
        async fn an_unreachable_service_is_refused_at_identity_naming_its_address() {
            // A bound retriever, which reports no identity, and a bound
            // context builder, whose identity is read before the benchmark
            // loads — each at an address of its own, so the one named is the
            // context builder's; and a workspace binding no node uses, which
            // a submission carries and the run ignores.
            let uri = fakes::unreachable_uri();
            let retriever_uri = "http://127.0.0.1:2";
            let document = "pipeline:\n  inputs: [question]\n  nodes:\n    \
                - id: search\n      component: retriever\n      impl: far\n      \
                inputs: [question]\n      params: { top_k: 3 }\n    \
                - id: prompt\n      component: context_builder\n      impl: lines\n      \
                inputs: [question, search]\n      params: { budget: 100 }\n";
            let bindings = vec![
                binding("retriever", "far", retriever_uri),
                binding("context_builder", "lines", &uri),
                binding("generator", "unused", "http://127.0.0.1:2"),
            ];

            let error = launcher()
                .identity(&submission(document, "beir/absent", bindings))
                .await
                .expect_err("nothing listens there");

            assert!(
                matches!(&error, LauncherError::ServiceUnreachable { uri: at, .. } if *at == uri),
                "{error:?}"
            );
        }

        #[tokio::test]
        async fn a_changed_remote_identity_yields_a_decided_id_that_differs_from_the_announced_one()
        {
            let embedder = fakes::serve_embedder(fakes::FakeEmbedder::default());
            let context_builder = fakes::serve_context_builder();
            let (generator, swap) = fakes::serve_switchable_generator();
            let generation = submission(
                &fixture_text("remote-generation.yaml"),
                "beir-qa/qa-mini",
                vec![
                    binding("embedder", "bge", &embedder.uri),
                    binding("context_builder", "lines", &context_builder.uri),
                    binding("generator", "vllm", &generator.uri),
                ],
            );

            let announced = launcher()
                .identity(&generation)
                .await
                .expect("every service answers");
            // The service swaps its model between submission and execution.
            swap.store(true, std::sync::atomic::Ordering::SeqCst);
            let run = launcher()
                .execute(&generation, Arc::new(Unobserved), Cancellation::new())
                .await
                .expect("the run executes");

            assert_ne!(run.id, announced, "the decided id names what ran");
            assert_eq!(
                run.inputs.model_hashes["generator"],
                fakes::CHANGED_GENERATOR_IDENTITY
            );
            assert_eq!(
                run.id,
                launcher()
                    .identity(&generation)
                    .await
                    .expect("every service answers"),
                "the id is the harness's over what ran, never rewritten"
            );
        }

        #[tokio::test]
        async fn a_probe_reads_the_identity_the_service_reports() {
            let service = fakes::serve_context_builder();

            let identity = launcher()
                .probe("context_builder", "lines", &service.uri, None)
                .await
                .expect("the service answers");

            assert_eq!(identity.identity, fakes::CONTEXT_BUILDER_IDENTITY);
        }

        #[tokio::test]
        async fn the_probe_returns_the_identity_bench_would_record_for_a_served_model() {
            let generator = fakes::serve_generator();
            let embedder = fakes::serve_embedder(fakes::FakeEmbedder::default());
            let reranker = fakes::serve_reranker();
            for (family, uri, model, expected) in [
                (
                    "generator",
                    &generator.uri,
                    fakes::GENERATOR_MODEL,
                    fakes::GENERATOR_IDENTITY,
                ),
                (
                    "embedder",
                    &embedder.uri,
                    fakes::EMBEDDER_MODEL,
                    fakes::EMBEDDER_IDENTITY,
                ),
                (
                    "reranker",
                    &reranker.uri,
                    fakes::RERANKER_MODEL,
                    fakes::RERANKER_IDENTITY,
                ),
            ] {
                let identity = launcher()
                    .probe(family, "served", uri, Some(model))
                    .await
                    .unwrap_or_else(|error| panic!("{family}: {error:?}"));

                assert_eq!(identity.identity, expected, "{family}");
            }
        }

        #[tokio::test]
        async fn a_service_that_refuses_the_served_model_answers_request_invalid() {
            let generator = fakes::serve_generator();

            let error = launcher()
                .probe("generator", "served", &generator.uri, Some("not-served"))
                .await
                .expect_err("the fake serves one model");

            assert!(
                matches!(&error, ApiError::RequestInvalid { detail } if detail.contains("not-served")),
                "{error:?}"
            );
        }

        #[tokio::test]
        async fn a_probe_of_a_closed_port_is_service_unreachable() {
            let uri = fakes::unreachable_uri();

            let error = launcher()
                .probe("context_builder", "lines", &uri, None)
                .await
                .expect_err("nothing listens there");

            assert!(
                matches!(&error, ApiError::ServiceUnreachable { uri: at, .. } if *at == uri),
                "{error:?}"
            );
        }

        #[tokio::test]
        async fn a_probe_of_an_address_that_drops_packets_gives_up_after_its_connect_timeout() {
            // A non-routable address: packets to it are dropped, so only the
            // connect timeout ends the attempt. Where the network answers at
            // once that it is unreachable, the test passes sooner.
            let started = Instant::now();

            let error = launcher()
                .probe("context_builder", "lines", "http://10.255.255.1:9", None)
                .await
                .expect_err("nothing answers there");

            assert!(
                matches!(&error, ApiError::ServiceUnreachable { .. }),
                "{error:?}"
            );
            assert!(
                started.elapsed() < PROBE_CONNECT_TIMEOUT + Duration::from_secs(5),
                "took {:?}",
                started.elapsed()
            );
        }

        #[tokio::test]
        async fn a_family_whose_identity_names_a_served_model_cannot_be_probed_without_one() {
            // Refused before any call, rather than reported as the service's
            // failure.
            let service = fakes::serve_generator();
            for family in ["embedder", "reranker", "generator"] {
                let error = launcher()
                    .probe(family, "vllm", &service.uri, None)
                    .await
                    .expect_err("no served model to read the identity for");

                assert!(
                    matches!(&error, ApiError::RequestInvalid { detail } if detail.contains("served model")),
                    "{family}: {error:?}"
                );
            }
        }

        #[tokio::test]
        async fn a_family_whose_service_reports_no_identity_cannot_be_probed() {
            for family in ["retriever", "fusion"] {
                let error = launcher()
                    .probe(family, "remote-one", &fakes::unreachable_uri(), None)
                    .await
                    .expect_err("a retriever or a fusion service reports no identity");

                assert!(
                    matches!(&error, ApiError::RequestInvalid { detail } if detail.contains("reports no identity")),
                    "{family}: {error:?}"
                );
            }
        }

        #[tokio::test]
        async fn a_name_this_build_gives_a_local_component_is_refused() {
            let error = launcher()
                .probe("context_builder", "concat", &fakes::unreachable_uri(), None)
                .await
                .expect_err("`concat` is a `Local` context builder");

            assert!(
                matches!(&error, ApiError::BindingRefused { detail } if detail.contains("`Local`")),
                "{error:?}"
            );
        }
    }
}
