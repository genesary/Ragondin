//! `Launcher`, implemented by the composition root (ADR-C36 § 1).
//!
//! The one path from the UI to the data plane (INV-12): `ragondin-api` holds
//! an `Arc<dyn Launcher>` and never names a component; this module, in the
//! only crate that knows them, answers for it. Everything here reuses what
//! `bench` already runs: the capabilities are [`wiring::carried`], the probe is
//! [`Bindings::parse`] and [`wiring::service_identity`].
//!
//! `identity` and `execute` answer "not available in this build yet": a run's
//! identity at submission and its execution over `bench`'s path are the
//! launcher's issue (#353), which replaces both.

use async_trait::async_trait;
use ragondin_api::{
    ApiError, Capabilities, FamilyCapabilities, Job, JobState, Launcher, ServiceIdentity,
    Submission,
};
use ragondin_experiments::RunId;

use crate::binding::{Bindings, Family};
use crate::wiring;

/// What `identity` and `execute` answer until the launcher's issue fills them.
const NOT_YET: &str = "running a pipeline from the UI is not available in this build yet";

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

    /// The binding `family/name=uri` goes through every refusal `--remote`
    /// applies to an argument, and then through the identity read `bench`
    /// makes before a run. A service that could not be reached is
    /// `service_unreachable`; any other refusal — a malformed binding, a
    /// family whose identity needs a served model or has none, a build
    /// without `remote` — is `backend_failed` with the reason, since the API
    /// has no code for a request the build cannot honour.
    async fn probe(
        &self,
        family: &str,
        name: &str,
        uri: &str,
    ) -> Result<ServiceIdentity, ApiError> {
        let refused = |error: anyhow::Error| ApiError::BackendFailed {
            detail: format!("{error:#}"),
        };
        let bindings = Bindings::parse(&[format!("{family}/{name}={uri}")]).map_err(refused)?;
        read_identity(bindings, uri).await
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

/// The identity of the one binding in `bindings`, served at `uri`.
#[cfg(feature = "remote")]
async fn read_identity(bindings: Bindings, uri: &str) -> Result<ServiceIdentity, ApiError> {
    use ragondin_contracts::ComponentError;

    let binding = bindings
        .iter()
        .next()
        .cloned()
        .expect("one argument parsed is one binding");
    let result = async {
        let bound = wiring::Bound::new(bindings)?;
        wiring::service_identity(&bound, binding.family, &binding.name, None).await
    }
    .await;
    match result {
        Ok(identity) => Ok(ServiceIdentity {
            identity: identity.as_str().to_owned(),
        }),
        Err(error)
            if error.chain().any(|cause| {
                matches!(
                    cause.downcast_ref::<ComponentError>(),
                    Some(ComponentError::Unavailable(_))
                )
            }) =>
        {
            Err(ApiError::ServiceUnreachable {
                uri: uri.to_owned(),
                reason: format!("{error:#}"),
            })
        }
        Err(error) => Err(ApiError::BackendFailed {
            detail: format!("{error:#}"),
        }),
    }
}

/// A build without `remote` refuses every binding when it is parsed, so
/// [`BinaryLauncher::probe`] does not get here; answered rather than
/// asserted, since a panic would take the server down with it.
#[cfg(not(feature = "remote"))]
async fn read_identity(_bindings: Bindings, _uri: &str) -> Result<ServiceIdentity, ApiError> {
    Err(ApiError::BackendFailed {
        detail: "this build cannot construct a `Remote` component; rebuild with the `remote` \
                 feature"
            .to_owned(),
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

    #[tokio::test]
    async fn a_probe_that_is_not_a_well_formed_binding_is_refused_with_the_binding_s_reason() {
        let error = BinaryLauncher
            .probe("store", "qdrant", "http://127.0.0.1:1")
            .await
            .expect_err("`store` is not a family a name is bound in");

        assert!(
            matches!(&error, ApiError::BackendFailed { detail } if detail.contains("store/qdrant")),
            "{error:?}"
        );
    }

    #[cfg(not(feature = "remote"))]
    #[tokio::test]
    async fn a_build_without_remote_refuses_every_probe_naming_the_feature() {
        let error = BinaryLauncher
            .probe("context_builder", "lines", "http://127.0.0.1:1")
            .await
            .expect_err("this build cannot reach a `Remote` component");

        assert!(
            matches!(&error, ApiError::BackendFailed { detail } if detail.contains("`remote` feature")),
            "{error:?}"
        );
    }

    #[cfg(feature = "remote")]
    mod remote {
        use super::*;
        use crate::remote_fakes as fakes;

        #[tokio::test]
        async fn a_probe_reads_the_identity_the_service_reports() {
            let service = fakes::serve_context_builder();

            let identity = BinaryLauncher
                .probe("context_builder", "lines", &service.uri)
                .await
                .expect("the service answers");

            assert_eq!(identity.identity, fakes::CONTEXT_BUILDER_IDENTITY);
        }

        #[tokio::test]
        async fn a_probe_of_a_closed_port_is_service_unreachable() {
            let uri = fakes::unreachable_uri();

            let error = BinaryLauncher
                .probe("context_builder", "lines", &uri)
                .await
                .expect_err("nothing listens there");

            assert!(
                matches!(&error, ApiError::ServiceUnreachable { uri: at, .. } if *at == uri),
                "{error:?}"
            );
        }

        #[tokio::test]
        async fn a_family_whose_identity_names_a_served_model_cannot_be_probed_without_one() {
            // The probe carries no served model, and an embedder, a reranker
            // or a generator reports an identity only for one: refused before
            // any call, rather than reported as the service's failure.
            let service = fakes::serve_generator();
            for family in ["embedder", "reranker", "generator"] {
                let error = BinaryLauncher
                    .probe(family, "vllm", &service.uri)
                    .await
                    .expect_err("no served model to read the identity for");

                assert!(
                    matches!(&error, ApiError::BackendFailed { detail } if detail.contains("served model")),
                    "{family}: {error:?}"
                );
            }
        }

        #[tokio::test]
        async fn a_family_whose_service_reports_no_identity_cannot_be_probed() {
            for family in ["retriever", "fusion"] {
                let error = BinaryLauncher
                    .probe(family, "remote-one", &fakes::unreachable_uri())
                    .await
                    .expect_err("a retriever or a fusion service reports no identity");

                assert!(
                    matches!(&error, ApiError::BackendFailed { detail } if detail.contains("reports no identity")),
                    "{family}: {error:?}"
                );
            }
        }

        #[tokio::test]
        async fn a_name_this_build_gives_a_local_component_is_refused() {
            let error = BinaryLauncher
                .probe("context_builder", "concat", &fakes::unreachable_uri())
                .await
                .expect_err("`concat` is a `Local` context builder");

            assert!(
                matches!(&error, ApiError::BackendFailed { detail } if detail.contains("`Local`")),
                "{error:?}"
            );
        }
    }
}
