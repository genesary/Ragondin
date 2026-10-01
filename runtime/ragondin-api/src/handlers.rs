//! The handlers of the read endpoints. Each reads its backends, converts
//! what it read into this crate's response types, and answers; every failure
//! is an [`ApiError`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::Json;
use ragondin_benchmarks::identity::CorpusIndex;
use ragondin_experiments::{
    lower_configuration, Run, RunId, RunStore, RunStoreError, Trace, TraceDocument, TraceSummary,
};
use ragondin_pipeline::LogicalPipeline;
use ragondin_types::QueryId;

use crate::backends::{Backends, RunDataset};
use crate::derived::{self, Metrics, Outputs};
use crate::error::ApiError;
use crate::response::{
    QueryScores, QueryTrace, RunDetail, RunListing, RunQueries, SettingsSummary, UnreadableRun,
    Workspace,
};
use crate::{cache, convert, ServerConfig};

/// What every handler reads: the backends and the fixed configuration.
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) backends: Backends,
    pub(crate) config: Arc<ServerConfig>,
}

/// `GET /workspace`.
pub(crate) async fn workspace(State(state): State<AppState>) -> Result<Json<Workspace>, ApiError> {
    let settings = state.backends.settings.read().await?;
    Ok(Json(Workspace {
        path: state.config.workspace.display().to_string(),
        settings: SettingsSummary {
            datasets: settings.datasets.display().to_string(),
            services: settings.services,
        },
        build: state.config.build.clone(),
        capabilities: state.backends.launcher.capabilities(),
    }))
}

/// `GET /runs`: every id the store lists, loaded; a run that does not load is
/// listed as unreadable, with the store's reason, rather than dropped.
pub(crate) async fn runs(State(state): State<AppState>) -> Result<Json<RunListing>, ApiError> {
    let listing = blocking(state.backends.runs, |store| {
        let ids = store.ids().map_err(|error| ApiError::BackendFailed {
            detail: format!("the run store cannot be listed: {error}"),
        })?;
        let mut listing = RunListing {
            runs: Vec::with_capacity(ids.len()),
            unreadable: Vec::new(),
        };
        for id in ids {
            match store.load(&id) {
                Ok(run) => listing.runs.push(convert::summary(&run)),
                Err(error) => listing.unreadable.push(UnreadableRun {
                    id: id.to_string(),
                    reason: error.to_string(),
                }),
            }
        }
        Ok(listing)
    })
    .await?;
    Ok(Json(listing))
}

/// `GET /runs/{id}`. An id that is not a run id names no run, so it is
/// `run_not_found` as an absent one is; a run that is there and does not
/// read is `run_unreadable`.
pub(crate) async fn run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RunDetail>, ApiError> {
    let run = load_run(&state, id).await?;
    Ok(Json(convert::detail(&run)?))
}

/// `GET /runs/{id}/queries`: every query the run executed with its scores,
/// and the per-node metrics — read from the trace against the run's own
/// ground truth when the dataset on disk digests to the run's
/// `dataset_version`, listed unscored and flagged otherwise.
/// `?missing_gold_at=<k>` keeps the judged queries with no gold document in
/// the top `k` of the output ranking; that needs the ground truth, so without
/// it the answer is `dataset_absent` or `dataset_differs`.
pub(crate) async fn queries(
    State(state): State<AppState>,
    Path(id): Path<String>,
    uri: Uri,
) -> Result<Json<RunQueries>, ApiError> {
    let mut parameters = parameters(uri.query(), &[MISSING_GOLD_AT])?;
    let filter = parameters
        .remove(MISSING_GOLD_AT)
        .map(|value| {
            value
                .parse::<usize>()
                .ok()
                .filter(|k| *k > 0)
                .ok_or_else(|| ApiError::ParameterInvalid {
                    name: MISSING_GOLD_AT.to_owned(),
                    reason: format!("`{value}` is not a positive integer"),
                })
        })
        .transpose()?;
    let run = load_run(&state, id).await?;
    let pipeline = lower(&run)?;
    let traces = Arc::new(read_traces(&run)?);
    let metrics = Metrics::of(run.metrics.iter().map(|(name, _)| name));
    let outputs = Outputs::of(&pipeline);
    let dataset = state
        .backends
        .registry
        .dataset(&run.inputs.dataset_version)
        .await?;

    let mut cache_error = None;
    let (check, ground) = match dataset {
        RunDataset::Verified { name, benchmark } => {
            let check = convert::ground_verified(&name, &run.inputs);
            let workspace = state.config.workspace.clone();
            let key = cache::Key::of(&state.config.build, &run);
            let (pipeline, traces, metrics, outputs) = (
                pipeline.clone(),
                Arc::clone(&traces),
                metrics.clone(),
                outputs.clone(),
            );
            let (benchmark, figures, failure) = work(move || {
                // A cache that cannot be read is a miss whose reason is kept.
                let (cached, read_failure) = match cache::read(&workspace, &key) {
                    Ok(cached) => (cached, None),
                    Err(failure) => (None, Some(failure)),
                };
                if let Some(figures) = cached {
                    return Ok((benchmark, figures, None));
                }
                let figures = cache::Figures {
                    queries: traces
                        .iter()
                        .map(|(query, trace)| {
                            let scores =
                                derived::query_scores(&metrics, &outputs, &benchmark, query, trace);
                            (query.as_str().to_owned(), scores)
                        })
                        .collect(),
                    nodes: derived::node_figures(&metrics, &pipeline, &traces, Some(&benchmark)),
                };
                // Served whatever the cache did: its failure is reported, and
                // fails nothing (`cache.rs`).
                let failure = read_failure.or(cache::write(&workspace, &key, &figures).err());
                Ok((benchmark, figures, failure))
            })
            .await?;
            cache_error = failure;
            (check, Some((benchmark, figures)))
        }
        other => (convert::unverified(&other, &run.inputs), None),
    };
    let (verified_benchmark, ground) = match ground {
        Some((benchmark, figures)) => (Some(benchmark), Some(figures)),
        None => (None, None),
    };
    if filter.is_some() && ground.is_none() {
        return Err(convert::dataset_error(&check));
    }
    let queries = traces
        .iter()
        .filter(|(query, trace)| match (filter, &verified_benchmark) {
            (Some(k), Some(benchmark)) => {
                derived::gold_missing(&outputs, benchmark, query, trace, k) == Some(true)
            }
            _ => true,
        })
        .map(|(query, trace)| QueryScores {
            id: query.as_str().to_owned(),
            scores: ground
                .as_ref()
                .and_then(|figures| figures.queries.get(query.as_str()).cloned())
                .unwrap_or_default(),
            duration_nanos: duration(trace),
        })
        .collect();
    let nodes = match &ground {
        Some(figures) => figures.nodes.iter().map(convert::node_metrics).collect(),
        None => derived::node_figures(&metrics, &pipeline, &traces, None)
            .iter()
            .map(convert::node_metrics)
            .collect(),
    };
    Ok(Json(RunQueries {
        run: run.id.to_string(),
        ground_truth: check,
        metrics: metrics.names(),
        ranking_node: outputs.ranking.map(|node| node.as_str().to_owned()),
        answer_node: outputs.answer.map(|node| node.as_str().to_owned()),
        queries,
        nodes,
        cache_error,
    }))
}

/// `GET /runs/{id}/trace/{query}`: one query's trace, node by node, with each
/// named chunk's passage text only when the dataset on disk digests to the
/// run's `dataset_version` and the chunk set derived from it to its
/// `index_version` (ADR-C36 § 4); otherwise the ids alone, and the flag says
/// why. The scores and per-node metrics need the dataset alone.
pub(crate) async fn trace(
    State(state): State<AppState>,
    Path((id, query)): Path<(String, String)>,
    uri: Uri,
) -> Result<Json<QueryTrace>, ApiError> {
    parameters(uri.query(), &[])?;
    let run = load_run(&state, id).await?;
    let pipeline = lower(&run)?;
    let query_id = QueryId::new(&query);
    let document = run
        .traces
        .get(&query_id)
        .ok_or_else(|| ApiError::QueryNotFound {
            run_id: run.id.to_string(),
            query: query.clone(),
        })?;
    let trace = read_trace(&run, &query_id, document)?;
    let metrics = Metrics::of(run.metrics.iter().map(|(name, _)| name));
    let outputs = Outputs::of(&pipeline);
    let dataset = state
        .backends
        .registry
        .dataset(&run.inputs.dataset_version)
        .await?;

    let (passages, benchmark, texts) = match dataset {
        RunDataset::Verified { name, benchmark } => {
            let inputs = run.inputs.clone();
            let named = named_chunks(&trace);
            work(move || {
                let index = CorpusIndex::build(benchmark.corpus());
                if index.version() != inputs.index_version {
                    let check = convert::index_differs(&name, &inputs, index.version());
                    return Ok((check, Some(benchmark), None));
                }
                let texts: HashMap<String, String> = index
                    .chunks()
                    .iter()
                    .filter(|chunk| named.contains(chunk.id.as_str()))
                    .map(|chunk| (chunk.id.as_str().to_owned(), chunk.text.clone()))
                    .collect();
                let check = convert::passages_verified(&name, &inputs);
                Ok((check, Some(benchmark), Some(texts)))
            })
            .await?
        }
        other => (convert::unverified(&other, &run.inputs), None, None),
    };

    let scores = benchmark
        .as_ref()
        .map(|benchmark| derived::query_scores(&metrics, &outputs, benchmark, &query_id, &trace))
        .unwrap_or_default();
    let nodes = convert::trace_view(&trace, texts.as_ref(), |node| {
        benchmark.as_ref().and_then(|benchmark| {
            derived::node_scores(&metrics, benchmark, &query_id, &trace, node)
        })
    });
    Ok(Json(QueryTrace {
        run: run.id.to_string(),
        query,
        passages,
        scores,
        nodes,
    }))
}

/// The one parameter `GET /runs/{id}/queries` takes.
const MISSING_GOLD_AT: &str = "missing_gold_at";

/// The query string's parameters, each name and value percent-decoded (`+`
/// read as a space, as a form encodes it). A parameter not in `allowed`, one
/// given twice, or one that does not decode to UTF-8 is `parameter_invalid`:
/// refused rather than ignored, since an ignored filter answers a question
/// nobody asked.
fn parameters(query: Option<&str>, allowed: &[&str]) -> Result<BTreeMap<String, String>, ApiError> {
    let mut parameters = BTreeMap::new();
    for pair in query
        .unwrap_or("")
        .split('&')
        .filter(|pair| !pair.is_empty())
    {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = decode(name)?;
        let value = decode(value).map_err(|_| ApiError::ParameterInvalid {
            name: name.clone(),
            reason: "its value is not percent-encoded UTF-8".to_owned(),
        })?;
        if !allowed.contains(&name.as_str()) {
            let takes = match allowed {
                [] => "this endpoint takes no parameter".to_owned(),
                names => format!("this endpoint takes only `{}`", names.join("`, `")),
            };
            return Err(ApiError::ParameterInvalid {
                name,
                reason: takes,
            });
        }
        if parameters.contains_key(&name) {
            return Err(ApiError::ParameterInvalid {
                name,
                reason: "it is given more than once".to_owned(),
            });
        }
        parameters.insert(name, value);
    }
    Ok(parameters)
}

/// Percent-decodes one name or value of a query string.
fn decode(text: &str) -> Result<String, ApiError> {
    let refused = || ApiError::ParameterInvalid {
        name: text.to_owned(),
        reason: "it is not percent-encoded UTF-8".to_owned(),
    };
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => decoded.push(b' '),
            b'%' => {
                let hex = bytes.get(at + 1..at + 3).ok_or_else(refused)?;
                // Two hexadecimal digits, checked here: `from_str_radix`
                // also accepts a leading sign, which would read `%+1` as 1.
                if !hex.iter().all(u8::is_ascii_hexdigit) {
                    return Err(refused());
                }
                let hex = std::str::from_utf8(hex).map_err(|_| refused())?;
                decoded.push(u8::from_str_radix(hex, 16).map_err(|_| refused())?);
                at += 2;
            }
            byte => decoded.push(byte),
        }
        at += 1;
    }
    String::from_utf8(decoded).map_err(|_| refused())
}

/// Loads a run by the id a path named: `run_not_found` for an id that is not
/// one or names nothing, `run_unreadable` for a run that does not read.
async fn load_run(state: &AppState, id: String) -> Result<Run, ApiError> {
    let run_id: RunId = id
        .parse()
        .map_err(|_| ApiError::RunNotFound { id: id.clone() })?;
    blocking(Arc::clone(&state.backends.runs), move |store| {
        store.load(&run_id).map_err(|error| match error {
            RunStoreError::NotFound { .. } => ApiError::RunNotFound { id },
            other => ApiError::RunUnreadable {
                run_id: run_id.to_string(),
                reason: other.to_string(),
            },
        })
    })
    .await
}

/// The run's pipeline, lowered from its stored document by
/// `ragondin-experiments`' one lowering path.
fn lower(run: &Run) -> Result<LogicalPipeline, ApiError> {
    lower_configuration(&run.config).map_err(|reason| ApiError::RunUnreadable {
        run_id: run.id.to_string(),
        reason,
    })
}

/// Every trace of the run, typed; one that does not read makes the run
/// `run_unreadable`, naming the query — reported, never repaired.
fn read_traces(run: &Run) -> Result<BTreeMap<QueryId, Trace>, ApiError> {
    run.traces
        .iter()
        .map(|(query, document)| Ok((query.clone(), read_trace(run, query, document)?)))
        .collect()
}

fn read_trace(run: &Run, query: &QueryId, document: &TraceDocument) -> Result<Trace, ApiError> {
    Trace::try_from(document).map_err(|error| ApiError::RunUnreadable {
        run_id: run.id.to_string(),
        reason: format!("the trace of query {}: {error}", query.as_str()),
    })
}

/// Every chunk id the trace names, in a ranking or a context, on either side
/// of a node: the rendering names chunks where they were produced, and the
/// shape allows them on an input port too.
fn named_chunks(trace: &Trace) -> HashSet<String> {
    let mut named = HashSet::new();
    for summary in trace
        .nodes
        .iter()
        .flat_map(|node| node.inputs.iter().chain(&node.output))
    {
        if let TraceSummary::RankedChunks { chunks } | TraceSummary::Context { chunks, .. } =
            summary
        {
            named.extend(chunks.iter().map(|chunk| chunk.chunk.as_str().to_owned()));
        }
    }
    named
}

/// A query's duration: the sum of its nodes' own.
fn duration(trace: &Trace) -> u64 {
    trace
        .nodes
        .iter()
        .fold(0u64, |sum, node| sum.saturating_add(node.duration_nanos))
}

/// Runs derivation work — a dataset's chunk set, the cache files — on a
/// blocking thread, off the async workers.
async fn work<T, F>(call: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
{
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|error| ApiError::BackendFailed {
            detail: format!("deriving the run's data did not complete: {error}"),
        })?
}

/// Where the API is nested. Inside the nest, axum hands a handler the path
/// with this prefix stripped, so the two fallbacks below put it back to name
/// the path as it was requested.
pub(crate) const API_PREFIX: &str = "/api";

/// Any path under `/api` that names no endpoint.
pub(crate) async fn route_not_found(uri: Uri) -> ApiError {
    // A request for the bare prefix reaches the nest as `/`.
    let path = match uri.path() {
        "/" => API_PREFIX.to_owned(),
        below => format!("{API_PREFIX}{below}"),
    };
    ApiError::RouteNotFound { path }
}

/// The prefix itself, routed outside the nest, where the path arrives whole.
pub(crate) async fn prefix_not_found(uri: Uri) -> ApiError {
    ApiError::RouteNotFound {
        path: uri.path().to_owned(),
    }
}

/// An endpoint asked for with a method it does not serve.
pub(crate) async fn method_not_allowed(method: Method, uri: Uri) -> ApiError {
    ApiError::MethodNotAllowed {
        method: method.to_string(),
        path: format!("{API_PREFIX}{}", uri.path()),
    }
}

/// Runs a synchronous store call on a blocking thread: the file backend
/// reads the disk, and an async worker must not wait on it.
async fn blocking<T, F>(store: Arc<dyn RunStore>, call: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&dyn RunStore) -> Result<T, ApiError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || call(store.as_ref()))
        .await
        .map_err(|error| ApiError::BackendFailed {
            detail: format!("the run store call did not complete: {error}"),
        })?
}
