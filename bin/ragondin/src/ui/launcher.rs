//! `Launcher`, implemented by the composition root (ADR-C36 § 1).
//!
//! The one path from the UI to the data plane (INV-12): `ragondin-api` holds
//! an `Arc<dyn Launcher>` and never names a component; this module, in the
//! only crate that knows them, answers for it. Everything here reuses what
//! `bench` already runs: the capabilities are [`wiring::carried`], a binding's
//! check is [`binding::check`] — `--remote`'s refusals, in its words — and the
//! probe is [`wiring::service_identity`], the read `bench` makes before a run.
//!
//! `identity` and `execute` answer "not available in this build yet": a run's
//! identity at submission and its execution over `bench`'s path are the
//! launcher's issue (#353), which replaces both.

use async_trait::async_trait;
use ragondin_api::{
    ApiError, Capabilities, FamilyCapabilities, Job, JobState, Launcher, Location, ServiceBinding,
    ServiceIdentity, Submission,
};
use ragondin_experiments::RunId;
use ragondin_pipeline::LogicalPipeline;

use crate::binding::{self, Binding, Bindings, Family};
use crate::wiring;

/// What `identity` and `execute` answer until the launcher's issue fills them.
const NOT_YET: &str = "running a pipeline from the UI is not available in this build yet";

/// How long a probe waits for its connection: a person is waiting on the
/// answer, and an address that drops packets would otherwise hold them for
/// the operating system's TCP timeout — about 75 s on macOS, two minutes on
/// Linux. Long enough for a service across a slow tunnel.
#[cfg(feature = "remote")]
const PROBE_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The binary's `Launcher`. It holds nothing: the build's capabilities are
/// fixed at compile time, and a probe builds its channel per call.
pub struct BinaryLauncher;

#[async_trait]
impl Launcher for BinaryLauncher {
    /// Every family `--remote` names, in [`Family::ALL`]'s order, with the
    /// `Local` names this build carries in it — none, for a family whose every
    /// implementation is feature-gated off — and whether it carries `remote`.
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

    async fn identity(&self, _submission: &Submission) -> Result<RunId, ApiError> {
        Err(ApiError::BackendFailed {
            detail: NOT_YET.to_owned(),
        })
    }

    async fn execute(&self, _job: Job) -> JobState {
        JobState::Failed {
            error: NOT_YET.to_owned(),
            at_node: None,
        }
    }
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
    use ragondin_contracts::ComponentError;

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
    use std::time::SystemTime;

    use ragondin_api::FamilyCapabilities;

    use super::*;

    fn families(capabilities: &Capabilities) -> Vec<(&str, Vec<&str>)> {
        capabilities
            .families
            .iter()
            .map(|FamilyCapabilities { family, local }| {
                (family.as_str(), local.iter().map(String::as_str).collect())
            })
            .collect()
    }

    #[test]
    fn every_family_is_listed_once_in_the_order_bindings_name_them() {
        let capabilities = BinaryLauncher.capabilities();
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
        let capabilities = BinaryLauncher.capabilities();

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
        let capabilities = BinaryLauncher.capabilities();

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
    }

    fn submission() -> Submission {
        Submission {
            pipeline_name: "hybrid".to_owned(),
            pipeline: "pipeline: {}".to_owned(),
            benchmark: "beir/scifact".to_owned(),
            bindings: Vec::new(),
            up_to: None,
        }
    }

    #[tokio::test]
    async fn a_run_s_identity_is_not_available_in_this_build_yet() {
        let error = BinaryLauncher
            .identity(&submission())
            .await
            .expect_err("submission is not wired yet");

        assert!(
            matches!(&error, ApiError::BackendFailed { detail } if detail.contains("not available in this build yet")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn execution_fails_as_not_available_in_this_build_yet() {
        let job = Job {
            id: "job-1".to_owned(),
            run_id: RunId::from_digest([0; 32]),
            submission: submission(),
            created_at: SystemTime::UNIX_EPOCH,
        };

        let state = BinaryLauncher.execute(job).await;

        assert!(
            matches!(&state, JobState::Failed { error, at_node: None } if error.contains("not available in this build yet")),
            "{state:?}"
        );
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
            let error = BinaryLauncher
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
        BinaryLauncher
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

        let error = BinaryLauncher
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

        BinaryLauncher
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

        BinaryLauncher
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

        let error = BinaryLauncher
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

        BinaryLauncher
            .check_document(&document, &bound)
            .expect("a bound reranker reads `top_k` and `served_model`");
        let stray = pipeline(
            "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
             inputs: [question]\n      params: { top_k: 10 }\n\
             \x20   - id: ranked\n      component: reranker\n      impl: bge-reranker\n      \
             inputs: [question, lexical]\n      params: { top_k: 5, served_model: r, model: m }\n",
        );
        let error = BinaryLauncher
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
        let error = BinaryLauncher
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
        let error = BinaryLauncher
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
        async fn a_probe_reads_the_identity_the_service_reports() {
            let service = fakes::serve_context_builder();

            let identity = BinaryLauncher
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
                let identity = BinaryLauncher
                    .probe(family, "served", uri, Some(model))
                    .await
                    .unwrap_or_else(|error| panic!("{family}: {error:?}"));

                assert_eq!(identity.identity, expected, "{family}");
            }
        }

        #[tokio::test]
        async fn a_service_that_refuses_the_served_model_answers_request_invalid() {
            let generator = fakes::serve_generator();

            let error = BinaryLauncher
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

            let error = BinaryLauncher
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

            let error = BinaryLauncher
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
                let error = BinaryLauncher
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
                let error = BinaryLauncher
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
            let error = BinaryLauncher
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
