//! `GET /services`, `PUT`/`DELETE /services/{family}/{name}` and
//! `POST /services/{family}/{name}/probe`.
//!
//! The bindings are the workspace's settings, read through
//! `WorkspaceSettings` and changed by its per-key operations, each applied
//! whole by the backend — no handler reads the settings to write them back.
//! Whether one is acceptable is the composition root's to say, through
//! `Launcher::check_binding`, in the words `--remote` uses.
//! What a probe learnt is this server's memory, not the workspace's: it is
//! kept per binding and address while the server runs, and an address
//! changed is a binding never probed.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::State;
use axum::Json;

use crate::backends::Settings;
use crate::error::ApiError;
use crate::extract::{ApiJson, ApiPath, ApiQuery, NoParameters};
use crate::handlers::AppState;
use crate::request::{ProbeRequest, ServiceAddress};
use crate::response::{ProbeResult, ServiceBinding, ServiceListing, ServiceStatus};

/// What this server last learnt by probing one binding.
#[derive(Clone, Debug, Default)]
pub(crate) struct Probed {
    /// The address last probed: `connected` and the listing's identity hold
    /// for this address only.
    uri: String,
    /// Whether that probe read an identity.
    connected: bool,
    /// The identity last read, and the address it was read at — kept when
    /// the address changes, so an unreachable service can still say what
    /// answered under its name before.
    identity: Option<(String, String)>,
}

/// Every binding's [`Probed`], by family and name.
pub(crate) type Probes = Arc<Mutex<BTreeMap<(String, String), Probed>>>;

/// The memory, whatever a panicking holder left: it records observations, and
/// one interrupted write leaves an observation stale, never unsafe.
fn memory(probes: &Probes) -> MutexGuard<'_, BTreeMap<(String, String), Probed>> {
    probes
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The bindings `settings` holds, with what the server remembers of each.
pub(crate) fn listing(settings: &Settings, probes: &Probes) -> ServiceListing {
    let memory = memory(probes);
    ServiceListing {
        services: settings
            .services
            .iter()
            .map(|binding| {
                let probed = memory
                    .get(&(binding.family.clone(), binding.name.clone()))
                    .filter(|probed| probed.uri == binding.uri);
                ServiceStatus {
                    family: binding.family.clone(),
                    name: binding.name.clone(),
                    uri: binding.uri.clone(),
                    connected: probed.is_some_and(|probed| probed.connected),
                    identity: probed
                        .and_then(|probed| probed.identity.as_ref())
                        .filter(|(_, at)| *at == binding.uri)
                        .map(|(identity, _)| identity.clone()),
                }
            })
            .collect(),
    }
}

/// `GET /services`.
pub(crate) async fn list(
    State(state): State<AppState>,
    _: ApiQuery<NoParameters>,
) -> Result<Json<ServiceListing>, ApiError> {
    let settings = state.backends.settings.read().await?;
    Ok(Json(listing(&settings, &state.probes)))
}

/// `PUT /services/{family}/{name}`: binds the name to the address, replacing
/// any address it had.
pub(crate) async fn bind(
    State(state): State<AppState>,
    ApiPath((family, name)): ApiPath<(String, String)>,
    _: ApiQuery<NoParameters>,
    ApiJson(ServiceAddress { uri }): ApiJson<ServiceAddress>,
) -> Result<Json<ServiceListing>, ApiError> {
    state
        .backends
        .launcher
        .check_binding(&family, &name, &uri)?;
    let settings = state
        .backends
        .settings
        .bind(ServiceBinding { family, name, uri })
        .await?;
    Ok(Json(listing(&settings, &state.probes)))
}

/// `DELETE /services/{family}/{name}`.
pub(crate) async fn unbind(
    State(state): State<AppState>,
    ApiPath((family, name)): ApiPath<(String, String)>,
    _: ApiQuery<NoParameters>,
) -> Result<Json<ServiceListing>, ApiError> {
    let Some(settings) = state.backends.settings.unbind(&family, &name).await? else {
        return Err(ApiError::ServiceNotFound { family, name });
    };
    memory(&state.probes).remove(&(family, name));
    Ok(Json(listing(&settings, &state.probes)))
}

/// `POST /services/{family}/{name}/probe`: the identity read, through the
/// launcher, at the address the workspace binds the name to.
pub(crate) async fn probe(
    State(state): State<AppState>,
    ApiPath((family, name)): ApiPath<(String, String)>,
    _: ApiQuery<NoParameters>,
    // A context builder needs nothing but the binding, so no body at all is
    // the request with no served model.
    ApiJson(request): ApiJson<Option<ProbeRequest>>,
) -> Result<Json<ProbeResult>, ApiError> {
    let ProbeRequest { served_model } = request.unwrap_or_default();
    let settings = state.backends.settings.read().await?;
    let uri = settings
        .services
        .iter()
        .find(|binding| binding.family == family && binding.name == name)
        .map(|binding| binding.uri.clone())
        .ok_or_else(|| ApiError::ServiceNotFound {
            family: family.clone(),
            name: name.clone(),
        })?;
    let result = state
        .backends
        .launcher
        .probe(&family, &name, &uri, served_model.as_deref())
        .await;
    let key = (family, name);
    let mut memory = memory(&state.probes);
    let probed = memory.entry(key).or_default();
    probed.uri.clone_from(&uri);
    match result {
        Ok(identity) => {
            probed.connected = true;
            probed.identity = Some((identity.identity.clone(), uri));
            Ok(Json(ProbeResult {
                identity: identity.identity,
            }))
        }
        Err(ApiError::ServiceUnreachable { uri, reason, .. }) => {
            probed.connected = false;
            Err(ApiError::ServiceUnreachable {
                uri,
                reason,
                last_identity: probed.identity.as_ref().map(|(identity, at)| {
                    if *at == probed.uri {
                        identity.clone()
                    } else {
                        format!("{identity} (at {at})")
                    }
                }),
            })
        }
        Err(other) => {
            probed.connected = false;
            Err(other)
        }
    }
}

/// The identity this server last read at `uri`, under any binding — what a
/// submission whose service did not answer reports beside the failure.
pub(crate) fn last_read_at(probes: &Probes, uri: &str) -> Option<String> {
    memory(probes)
        .values()
        .filter_map(|probed| probed.identity.as_ref())
        .find(|(_, at)| at == uri)
        .map(|(identity, _)| identity.clone())
}
