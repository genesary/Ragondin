//! Every route of the `/api` router, listed once, in [`api`], and read two
//! ways: [`Builder`] makes the axum router of it, and [`Declared`] records
//! each route's query and header types for the description. So the
//! description's parameters are the handlers' own extractor types, never a
//! second list kept beside them (ADR-C37 § 5).
//!
//! [`Routes::route`] is also the guard: it takes a handler only when every
//! argument is an [`ApiInput`] — `State`, or one of the crate's own
//! extractors (ADR-C37 § 2). A handler that takes axum's `Path`, `Query`,
//! `Json` (bare, in an `Option` or a `Result`), `Bytes`, `RawQuery`, the
//! `Uri`, the `Request` or a `HeaderMap` does not compile, whatever it is
//! imported as. `clippy.toml` makes it the only way in: it refuses the
//! `Router` and `MethodRouter` methods, and the `axum::routing::*_service`
//! functions, that add a route, a service, a fallback or a layer, outside
//! [`Builder::into_router`] and `router` in `lib.rs`, the one assembly site
//! (`ARCHITECTURE.md` § Request input goes through one extractor module).

use std::collections::BTreeMap;

use axum::handler::Handler;
use axum::routing::{MethodFilter, MethodRouter};
use axum::Router;

use crate::description::Parameters;
use crate::endpoints::{benchmarks, compare, matrix, pipelines, services};
use crate::extract::ApiInputs;
use crate::handlers::{self, AppState};

/// The methods the API serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verb {
    Get,
    Post,
    Put,
    Delete,
}

impl Verb {
    /// The method, lowercase, as OpenAPI and the description spell it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Post => "post",
            Self::Put => "put",
            Self::Delete => "delete",
        }
    }

    fn filter(self) -> MethodFilter {
        match self {
            Self::Get => MethodFilter::GET,
            Self::Post => MethodFilter::POST,
            Self::Put => MethodFilter::PUT,
            Self::Delete => MethodFilter::DELETE,
        }
    }
}

/// What reads the route list.
pub(crate) trait Routes {
    /// `handler` answers `verb` at `path` — under `/api/v1`, in the
    /// description's `{parameter}` syntax — and takes only [`ApiInput`]s.
    ///
    /// [`ApiInput`]: crate::extract::ApiInput
    fn route<H, T>(&mut self, verb: Verb, path: &'static str, handler: H)
    where
        H: Handler<T, AppState>,
        T: ApiInputs + 'static;
}

/// Every route of the API.
pub(crate) fn api(routes: &mut impl Routes) {
    use Verb::{Delete, Get, Post, Put};
    routes.route(Get, "/workspace", handlers::workspace);
    routes.route(Get, "/runs", handlers::runs);
    routes.route(Get, "/runs/{id}", handlers::run);
    routes.route(Get, "/runs/{id}/queries", handlers::queries);
    routes.route(Get, "/runs/{id}/trace/{query}", handlers::trace);
    routes.route(Post, "/compare", compare::compare);
    routes.route(Get, "/pipelines", pipelines::list);
    // A static segment outranks a parameter, so `validate` is never a
    // pipeline's name: the file backend refuses it as one.
    routes.route(Post, "/pipelines/validate", pipelines::validate);
    routes.route(Get, "/pipelines/{name}", pipelines::read);
    routes.route(Put, "/pipelines/{name}", pipelines::write);
    routes.route(Get, "/pipelines/{name}/layout", pipelines::read_layout);
    routes.route(Put, "/pipelines/{name}/layout", pipelines::write_layout);
    routes.route(Get, "/pipelines/{name}/matrix", matrix::matrix);
    routes.route(Get, "/benchmarks", benchmarks::list);
    routes.route(Post, "/benchmarks/import", benchmarks::import);
    routes.route(Get, "/services", services::list);
    routes.route(Put, "/services/{family}/{name}", services::bind);
    routes.route(Delete, "/services/{family}/{name}", services::unbind);
    routes.route(Post, "/services/{family}/{name}/probe", services::probe);
}

/// The routes as an axum router, each path answering a method it does not
/// serve with a problem body (axum still sets `Allow`).
#[derive(Default)]
pub(crate) struct Builder {
    paths: BTreeMap<&'static str, MethodRouter<AppState>>,
}

impl Routes for Builder {
    fn route<H, T>(&mut self, verb: Verb, path: &'static str, handler: H)
    where
        H: Handler<T, AppState>,
        T: ApiInputs + 'static,
    {
        let methods = self.paths.remove(path).unwrap_or_default();
        self.paths.insert(path, methods.on(verb.filter(), handler));
    }
}

impl Builder {
    // The one place an /api route meets `Router::route`, and its method
    // router `MethodRouter::fallback` for `method_not_allowed`: every method
    // router here was built by `Routes::route`, through the guard.
    // `clippy.toml` refuses both everywhere but here and `router` in lib.rs.
    #[allow(clippy::disallowed_methods)]
    pub(crate) fn into_router(self) -> Router<AppState> {
        self.paths
            .into_iter()
            .fold(Router::new(), |router, (path, methods)| {
                router.route(
                    &axum_path(path),
                    methods.fallback(handlers::method_not_allowed),
                )
            })
    }
}

/// A description path as axum 0.7 routes it: under `/v1`, each `{name}`
/// spelled `:name`.
fn axum_path(path: &str) -> String {
    let segments: Vec<String> = path
        .split('/')
        .map(
            |segment| match segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                Some(name) => format!(":{name}"),
                None => segment.to_owned(),
            },
        )
        .collect();
    format!("/v1{}", segments.join("/"))
}

/// One route as the description reads it.
pub(crate) struct Route {
    pub(crate) method: &'static str,
    pub(crate) path: &'static str,
    /// Its handler's `ApiQuery` type.
    pub(crate) query: Option<Parameters>,
    /// Its handler's `ApiHeaders` type, when it reads headers.
    pub(crate) headers: Option<Parameters>,
}

/// The routes, recorded for the description.
#[derive(Default)]
pub(crate) struct Declared(pub(crate) Vec<Route>);

impl Routes for Declared {
    fn route<H, T>(&mut self, verb: Verb, path: &'static str, _: H)
    where
        H: Handler<T, AppState>,
        T: ApiInputs + 'static,
    {
        self.0.push(Route {
            method: verb.name(),
            path,
            query: T::query(),
            headers: T::headers(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared() -> Vec<Route> {
        let mut declared = Declared::default();
        api(&mut declared);
        declared.0
    }

    /// Each route carries the query and header types its handler takes:
    /// every handler an `ApiQuery`, and only the pipeline write headers.
    #[test]
    fn each_route_carries_its_handler_s_query_and_header_types() {
        for route in declared() {
            assert!(route.query.is_some(), "{} {}", route.method, route.path);
            let reads_headers = (route.method, route.path) == ("put", "/pipelines/{name}");
            assert_eq!(
                route.headers.is_some(),
                reads_headers,
                "{} {}",
                route.method,
                route.path
            );
        }
    }

    #[test]
    fn a_description_path_is_routed_in_axum_s_spelling() {
        assert_eq!(axum_path("/workspace"), "/v1/workspace");
        assert_eq!(
            axum_path("/runs/{id}/trace/{query}"),
            "/v1/runs/:id/trace/:query"
        );
    }
}
