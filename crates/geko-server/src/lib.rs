//! What geko serves for every module (docs/MODULES_PLAN.md): `/health`, `/metrics`,
//! `/v1/attribution`, `/openapi.yaml` and `/docs`, API keys, CORS and the rate limit, the bundle
//! that every module loads and keeps watching, and the routes of each active module, mounted under
//! its name (`/geocoder/v1/search`). Only `geko serve` (and `up`) use it, and tokio; the operator mode
//! also serves the dashboard alone (`serve_dashboard`).

mod access;
mod dashboard;
mod metrics;
mod ui;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::{ArcSwap, ArcSwapOption};
use axum::body::Bytes;
use axum::extract::{Query, RawPathParams};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{MethodRouter, get, post};
use axum::{Json, Router};
use geko_module_sdk::Result;
use geko_module_sdk::bundle::{BundleManifest, LicenseNotice, current_version};
use geko_module_sdk::module::{self, BundleView, Description, Method, Module, Request};
use serde_json::json;

pub use access::Access;
pub use dashboard::{ModuleStatus, PodAnswer, Pods, Statuses, serve as serve_dashboard};

/// An active module and what it said it is.
#[derive(Clone)]
pub struct Mounted {
    pub module: Arc<dyn Module>,
    pub description: Description,
}

impl Mounted {
    pub fn new(module: Arc<dyn Module>) -> Self {
        let description = module.describe();
        Self { module, description }
    }

    fn name(&self) -> &str {
        self.description.name.as_str()
    }

    /// The path where its route `path` is served.
    fn path(&self, path: &str) -> String {
        format!("/{}/{path}", self.name())
    }
}

/// Brings a newer version into the bundles directory before each look for one: bundles built
/// on another machine and published where this one pulls them from.
pub type Pull = Arc<dyn Fn() -> Result<()> + Send + Sync>;

#[derive(Clone)]
pub struct BundleSource {
    pub dir: PathBuf,
    /// How often to look for a new `current` version.
    pub reload: Duration,
    pub pull: Option<Pull>,
}

/// Why a module did not take a bundle.
#[derive(Clone)]
enum Skipped {
    /// The bundle has no part of it.
    NoPart,
    Rejected(String),
}

/// How a module stands with the bundles.
#[derive(Clone, Default)]
struct Part {
    /// The version it serves, with the licenses of the sources of that bundle.
    serving: Option<(String, Vec<LicenseNotice>)>,
    /// The last version it did not take, which it does not try again: it goes on with the one it
    /// serves, if any, until there is another.
    skipped: Option<(String, Skipped)>,
}

impl Part {
    fn settled(&self, version: &str) -> bool {
        self.serving.as_ref().is_some_and(|s| s.0 == version) || self.skipped.as_ref().is_some_and(|s| s.0 == version)
    }
}

/// Loads the `current` bundle into every module and keeps watching for new versions. Each module
/// goes on its own: whether it can read its part is its own business (`Module::load`), it serves as
/// soon as it has loaded it, and one that rejects a bundle holds back no other.
pub struct BundleManager {
    source: BundleSource,
    modules: Vec<Mounted>,
    parts: Vec<ArcSwap<Part>>,
    /// Why the last bundle published could not be read at all, for `/health`.
    error: ArcSwapOption<String>,
}

impl BundleManager {
    pub fn new(source: BundleSource, modules: Vec<Mounted>) -> Self {
        let parts = modules.iter().map(|_| ArcSwap::from_pointee(Part::default())).collect();
        Self { source, modules, parts, error: ArcSwapOption::empty() }
    }

    fn part(&self, module: &str) -> Option<Arc<Part>> {
        self.modules.iter().position(|m| m.name() == module).map(|i| self.parts[i].load_full())
    }

    /// Why this server, or only its `module`, cannot answer yet: no bundle, a module without its part
    /// of it, one that rejected it with nothing to go on with, or one that says it is not available.
    /// Empty when it can.
    pub fn waiting_for(&self, module: Option<&str>) -> Vec<String> {
        let mut waiting: Vec<String> = Vec::new();
        for (m, part) in self.modules.iter().zip(&self.parts).filter(|(m, _)| module.is_none_or(|n| n == m.name())) {
            let part = part.load();
            let why = match (&part.serving, &part.skipped) {
                (Some(_), _) => None,
                (None, Some((_, Skipped::NoPart))) => Some(format!("the part of {} in the bundle", m.name())),
                (None, Some((_, Skipped::Rejected(why)))) => Some(format!("{}: {why}", m.name())),
                (None, None) => Some("a bundle".to_owned()),
            };
            waiting.extend(why.filter(|w| !waiting.contains(w)));
            waiting.extend(m.module.unavailable().map(|why| format!("{}: {why}", m.name())));
        }
        waiting
    }

    /// Why the last bundle could not be read, or why each module that rejected it did.
    pub fn error(&self) -> Option<String> {
        let read = self.error.load_full().map(|e| (*e).clone());
        let rejected = self.modules.iter().filter_map(|m| Some(format!("{}: {}", m.name(), self.error_of(m.name())?)));
        let errors: Vec<String> = read.into_iter().chain(rejected).collect();
        (!errors.is_empty()).then(|| errors.join("; "))
    }

    /// Why `module` rejected the last bundle it looked at.
    pub fn error_of(&self, module: &str) -> Option<String> {
        match self.part(module)?.skipped.as_ref()? {
            (_, Skipped::Rejected(why)) => Some(why.clone()),
            (_, Skipped::NoPart) => None,
        }
    }

    /// The newest version that some module serves.
    pub fn version(&self) -> Option<String> {
        self.parts.iter().filter_map(|p| p.load().serving.as_ref().map(|s| s.0.clone())).max()
    }

    /// The version that `module` serves.
    pub fn version_of(&self, module: &str) -> Option<String> {
        self.part(module)?.serving.as_ref().map(|s| s.0.clone())
    }

    /// Licenses and attributions of the sources of the bundles being served.
    pub fn licenses(&self) -> Vec<LicenseNotice> {
        let mut licenses: Vec<LicenseNotice> = Vec::new();
        for part in &self.parts {
            for l in part.load().serving.iter().flat_map(|s| &s.1) {
                if !licenses.iter().any(|k| k.source == l.source) {
                    licenses.push(l.clone());
                }
            }
        }
        licenses
    }

    /// Offers `current` to every module that has not settled it, all at once: a module that takes
    /// long holds back no other. Whether any took it; an error only if the bundle cannot be read.
    pub fn refresh(&self) -> Result<bool> {
        let refreshed = self.load_current();
        match &refreshed {
            Ok(_) => self.error.store(None),
            Err(e) => self.error.store(Some(Arc::new(e.to_string()))),
        }
        refreshed
    }

    fn load_current(&self) -> Result<bool> {
        if let Some(pull) = &self.source.pull
            && let Err(e) = pull()
        {
            tracing::error!(error = %e, "cannot pull a new bundle; serving the local one");
        }
        let Some(version) = current_version(&self.source.dir) else { return Ok(false) };
        if self.parts.iter().all(|p| p.load().settled(&version)) {
            return Ok(false);
        }
        let root = self.source.dir.join(&version);
        let manifest = BundleManifest::read(&root)?;
        let (root, manifest) = (&root, &manifest);
        Ok(std::thread::scope(|scope| {
            let pending = self.modules.iter().zip(&self.parts).filter(|(_, part)| !part.load().settled(&manifest.version));
            let loading: Vec<_> = pending.map(|(m, part)| scope.spawn(move || offer(m, part, root, manifest))).collect();
            // Every one joined: a module that panics is one that did not take it.
            let taken: Vec<bool> = loading.into_iter().map(|l| l.join().unwrap_or(false)).collect();
            taken.contains(&true)
        }))
    }

    fn watch(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(self.source.reload);
            let mut first = true;
            loop {
                tick.tick().await;
                let manager = Arc::clone(&self);
                match tokio::task::spawn_blocking(move || manager.refresh()).await {
                    Ok(Err(e)) => tracing::error!(error = %e, "cannot read the current bundle; each module keeps the one it serves"),
                    Err(e) => tracing::error!(error = %e, "bundle reload task failed"),
                    Ok(Ok(_)) if first && current_version(&self.source.dir).is_none() => {
                        tracing::warn!("no bundle published yet (geko build, or the one geko up is running): serving 503 until one appears")
                    }
                    Ok(Ok(_)) => {}
                }
                first = false;
            }
        });
    }
}

/// Offers its part of the bundle to a module, and notes how it went: whether it took it.
fn offer(m: &Mounted, part: &ArcSwap<Part>, root: &std::path::Path, manifest: &BundleManifest) -> bool {
    let version = &manifest.version;
    let started = Instant::now();
    let taken = match manifest.module(&m.description.name) {
        None => {
            tracing::warn!(module = m.name(), %version, "the bundle has no part of this module");
            Err(Skipped::NoPart)
        }
        Some(own) => {
            let view = BundleView { root: root.to_path_buf(), manifest: manifest.clone(), module: own.clone() };
            m.module.load(&view).map_err(|e| Skipped::Rejected(e.to_string()))
        }
    };
    let now = part.load();
    let next = match taken {
        Ok(()) => {
            tracing::info!(module = m.name(), %version, elapsed = ?started.elapsed(), "bundle loaded");
            Part { serving: Some((version.clone(), manifest.licenses.clone())), skipped: None }
        }
        Err(skipped) => {
            if let Skipped::Rejected(why) = &skipped {
                let serving = now.serving.as_ref().map(|s| s.0.as_str());
                tracing::error!(module = m.name(), %version, serving, error = why, "the module rejected the bundle; it is not tried again until there is another");
            }
            Part { serving: now.serving.clone(), skipped: Some((version.clone(), skipped)) }
        }
    };
    let took = next.skipped.is_none();
    part.store(Arc::new(next));
    took
}

/// The reference of what every module shares, served at `/openapi.yaml`; `/docs` lists it with
/// those of the modules.
const OPENAPI: &str = include_str!("../../../openapi.yaml");
const DOCS: &str = include_str!("docs.html");

/// Licenses and attributions of the served data (PROJECT.md §6): the sources of the bundle, whom
/// to ask for the code that reproduces it, and what each module adds under its name.
fn attribution(modules: &[Mounted], bundles: Option<&BundleManager>, contact: Option<&str>) -> serde_json::Value {
    let licenses = bundles.map(|b| b.licenses()).unwrap_or_default();
    let mut body = json!({
        "bundle": bundles.and_then(|b| b.version()),
        "bundles": modules.iter().map(|m| (m.name().to_owned(), bundles.and_then(|b| b.version_of(m.name())).into())).collect::<serde_json::Map<_, _>>(),
        "contact": contact,
        "sources": licenses.iter().map(|l| json!({ "source": l.source, "license": l.name, "attribution": l.attribution, "url": l.url })).collect::<Vec<_>>(),
    });
    for m in modules {
        if let Some(a) = &m.description.attribution {
            body[m.name()] = a.clone();
        }
    }
    body
}

/// Why this server, or only its module `only`, cannot answer yet.
fn waiting(modules: &[Mounted], bundles: Option<&BundleManager>, only: Option<&str>) -> Vec<String> {
    match bundles {
        Some(b) => b.waiting_for(only),
        None => modules.iter().filter(|m| only.is_none_or(|n| n == m.name())).filter_map(|m| m.module.unavailable().map(|why| format!("{}: {why}", m.name()))).collect(),
    }
}

/// `/ready`: whether this server can answer, with what stops it and how each module stands;
/// with `only`, about that module alone.
fn readiness(modules: &[Mounted], bundles: Option<&BundleManager>, only: Option<&str>) -> (StatusCode, serde_json::Value) {
    if let Some(name) = only.filter(|n| !modules.iter().any(|m| m.name() == *n)) {
        return (StatusCode::NOT_FOUND, json!({ "error": format!("module `{name}` is not served here") }));
    }
    let answers: serde_json::Map<String, serde_json::Value> = modules
        .iter()
        .filter(|m| only.is_none_or(|n| n == m.name()))
        .map(|m| {
            let why = waiting(modules, bundles, Some(m.name()));
            (m.name().to_owned(), json!({ "ready": why.is_empty(), "bundle": bundles.and_then(|b| b.version_of(m.name())), "waiting": why }))
        })
        .collect();
    let why = waiting(modules, bundles, only);
    let version = bundles.and_then(|b| match only {
        Some(name) => b.version_of(name),
        None => b.version(),
    });
    let status = if why.is_empty() { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, json!({ "ready": why.is_empty(), "bundle": version, "waiting": why, "modules": answers }))
}

/// `/admin/api/modules`: the active modules, so the dashboard offers a tab for each.
fn module_list(modules: &[Mounted]) -> serde_json::Value {
    modules.iter().map(|m| json!({ "name": m.name(), "version": m.description.version, "unavailable": m.module.unavailable() })).collect()
}

/// `/docs`: Swagger UI over the reference of geko and those of the modules.
fn docs(modules: &[Mounted]) -> String {
    let mut specs = vec![json!({ "url": "/openapi.yaml", "name": "geko" })];
    specs.extend(modules.iter().filter_map(|m| m.description.openapi.as_ref().map(|p| json!({ "url": m.path(p), "name": m.name() }))));
    DOCS.replace("__SPECS__", &serde_json::Value::from(specs).to_string())
}

/// Runs the CPU-bound work of a request (parsing, search) on the blocking pool, so the async
/// workers keep accepting connections and answering cheap routes meanwhile; at most one such job
/// per core runs at once, the rest wait for a slot without taking a core. Unbounded, 64 parallel
/// searches on 16 cores doubled the p99; on the async workers, a cheap reverse request waited
/// twice as long behind them. A panic answers 500.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> std::result::Result<T, ApiError> {
    static SLOTS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    let slots = SLOTS.get_or_init(|| tokio::sync::Semaphore::new(std::thread::available_parallelism().map_or(4, |n| n.get())));
    let _slot = slots.acquire().await.map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("request failed: {e}")))?;
    tokio::task::spawn_blocking(work).await.map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("request failed: {e}")))
}

/// What the module answers, as an HTTP response.
fn response(r: module::Response) -> Response {
    let status = StatusCode::from_u16(r.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = (status, [(header::CONTENT_TYPE, r.content_type)], r.body).into_response();
    for (name, value) in r.headers {
        match (header::HeaderName::try_from(name), header::HeaderValue::try_from(value)) {
            (Ok(name), Ok(value)) => {
                response.headers_mut().append(name, value);
            }
            _ => tracing::warn!("a module answered an invalid header; left out"),
        }
    }
    response
}

/// Where the client reached geko, from the headers of the request: a client can send anything here,
/// so what is not a plain host and port is no origin. Only that client reads what it makes of it.
fn origin(headers: &HeaderMap) -> Option<String> {
    let first = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).and_then(|v| v.split(',').next()).map(str::trim).filter(|v| !v.is_empty());
    let host = first("x-forwarded-host").or_else(|| first("host"))?;
    let plain = host.len() <= 255 && host.bytes().all(|b| b.is_ascii_alphanumeric() || b"-.:[]_".contains(&b));
    let scheme = first("x-forwarded-proto").filter(|s| matches!(*s, "http" | "https")).unwrap_or("http");
    plain.then(|| format!("{scheme}://{host}"))
}

/// The handler of one route of a module: the request as data, answered on the pool if the route
/// asks for it.
fn handler(module: Arc<dyn Module>, route: &module::Route) -> MethodRouter {
    let (path, pool) = (route.path.clone(), route.pool);
    let handle = move |params: RawPathParams, Query(query): Query<Vec<(String, String)>>, headers: HeaderMap, body: Bytes| async move {
        let received = Instant::now();
        let mut request = Request {
            route: path,
            params: params.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
            query,
            content_type: headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_owned),
            body: body.to_vec(),
            waited_us: 0,
            origin: origin(&headers),
        };
        if !pool {
            return response(module.handle(request));
        }
        match blocking(move || {
            request.waited_us = received.elapsed().as_micros() as u64;
            module.handle(request)
        })
        .await
        {
            Ok(r) => response(r),
            Err(e) => e.into_response(),
        }
    };
    match route.method {
        Method::Get => get(handle),
        Method::Post => post(handle),
    }
}

/// The routes of every module under its name, and `/` leading to the first page of one.
fn module_routes(modules: &[Mounted]) -> Router {
    let mut app = Router::new();
    for m in modules {
        for route in &m.description.routes {
            let path = m.path(&route.path);
            app = app.route(&path, handler(Arc::clone(&m.module), route));
            if route.path.is_empty() {
                // Without the slash, the relative links of the page would leave the module.
                let to = path.clone();
                app = app.route(path.trim_end_matches('/'), get(move || async move { Redirect::permanent(&to) }));
            }
        }
    }
    app
}

/// The paths that never need an API key: the pages the modules declare public.
fn public_paths(modules: &[Mounted]) -> HashSet<String> {
    let mut public = HashSet::new();
    for m in modules {
        for route in m.description.routes.iter().filter(|r| r.public) {
            let path = m.path(&route.path);
            public.insert(path.trim_end_matches('/').to_owned());
            public.insert(path);
        }
    }
    public
}

pub fn router(modules: &[Mounted], bundles: Option<Arc<BundleManager>>, access: Access, contact: Option<String>) -> Router {
    let names: Vec<String> = modules.iter().map(|m| m.name().to_owned()).collect();
    let (attribution_bundles, attribution_modules) = (bundles.clone(), modules.to_vec());
    let docs = docs(modules);
    let listed = modules.to_vec();
    let shared_routes = Router::new()
        .route(
            "/v1/attribution",
            get(move || {
                let body = attribution(&attribution_modules, attribution_bundles.as_deref(), contact.as_deref());
                async move { Json(body) }
            }),
        )
        .route("/openapi.yaml", get(|| async { ([(header::CONTENT_TYPE, "application/yaml")], OPENAPI) }))
        .route("/docs", get(move || async move { axum::response::Html(docs) }))
        .route(
            "/admin/api/modules",
            get(move || {
                let body = module_list(&listed);
                async move { Json(body) }
            }),
        );
    // `/`: the dashboard (ui/), a map with what the active modules offer, through their API.
    let shared_routes = ui::mount(shared_routes);
    let metrics = Arc::new(metrics::Metrics::default());
    let version = {
        let bundles = bundles.clone();
        move || bundles.as_ref().and_then(|b| b.version())
    };
    let health_version = version.clone();
    let health_bundles = bundles.clone();
    let health_modules = modules.to_vec();
    let health = Router::new().route(
        "/health",
        get(move || {
            let names = names.clone();
            let version = health_version();
            let error = health_bundles.as_ref().and_then(|b| b.error());
            let unavailable: serde_json::Map<String, serde_json::Value> =
                health_modules.iter().filter_map(|m| Some((m.name().to_owned(), m.module.unavailable()?.into()))).collect();
            let part = |m: &Mounted| {
                let (bundle, error) = health_bundles.as_ref().map_or((None, None), |b| (b.version_of(m.name()), b.error_of(m.name())));
                (m.name().to_owned(), json!({ "bundle": bundle, "error": error }))
            };
            let parts: serde_json::Map<String, serde_json::Value> = health_modules.iter().map(part).collect();
            async move { Json(json!({ "status": "ok", "modules": names, "bundle": version, "bundle_error": error, "bundles": parts, "modules_unavailable": unavailable })) }
        }),
    );
    // `/health` says the process is alive, from the start (the health check of the image, while it
    // builds); `/ready` says it can answer: the probe that keeps a pod of the operator mode out of
    // the balancer until it has loaded its bundle. `?module=` asks about that module alone, which
    // is the one to look at when one balancer sends each module to the same server.
    let (ready_bundles, ready_modules) = (bundles.clone(), modules.to_vec());
    let health = health.route(
        "/ready",
        get(move |Query(query): Query<HashMap<String, String>>| {
            let (status, body) = readiness(&ready_modules, ready_bundles.as_deref(), query.get("module").map(String::as_str));
            async move { (status, Json(body)) }
        }),
    );
    let tracked = health.merge(module_routes(modules)).route_layer(axum::middleware::from_fn_with_state(Arc::clone(&metrics), metrics::track));
    let exposed = Arc::clone(&metrics);
    let mounted = modules.to_vec();
    let metrics_route = Router::new().route(
        "/metrics",
        get(move || {
            let mut text = exposed.render(version().as_deref());
            for m in &mounted {
                m.module.metrics(&mut text);
            }
            async move { ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], text) }
        }),
    );
    let mut public = public_paths(modules);
    public.extend(ui::paths().map(str::to_owned));
    let guard = access::Guard::new(access, public);
    // A panic in one request becomes a 500 for that request only.
    tracked
        .merge(metrics_route)
        .merge(shared_routes)
        .layer(axum::middleware::from_fn_with_state(Arc::new(guard), access::check))
        .layer(tower_http::catch_panic::CatchPanicLayer::new())
}

pub async fn serve(addr: SocketAddr, modules: Vec<Mounted>, bundles: Option<BundleSource>, access: Access, contact: Option<String>) -> Result<()> {
    let manager = bundles.map(|source| Arc::new(BundleManager::new(source, modules.clone())));
    // In the background: it listens from the start, and each module answers once it has loaded.
    if let Some(m) = &manager {
        Arc::clone(m).watch();
    }
    let app = router(&modules, manager, access, contact);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, modules = ?modules.iter().map(|m| m.name().to_owned()).collect::<Vec<_>>(), "listening");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    for m in &modules {
        m.module.shutdown()?;
    }
    Ok(())
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

#[cfg(test)]
mod origin_tests {
    use super::*;

    fn headers(list: &[(&'static str, &'static str)]) -> HeaderMap {
        list.iter().map(|(k, v)| (axum::http::HeaderName::from_static(k), axum::http::HeaderValue::from_static(v))).collect()
    }

    #[test]
    fn the_origin_is_where_the_client_reached_geko() {
        assert_eq!(origin(&headers(&[("host", "maps.example.com:8080")])).as_deref(), Some("http://maps.example.com:8080"));
        let proxied = [("host", "geko:8080"), ("x-forwarded-host", "maps.example.com, proxy"), ("x-forwarded-proto", "https")];
        assert_eq!(origin(&headers(&proxied)).as_deref(), Some("https://maps.example.com"));
        assert_eq!(origin(&headers(&[("host", "[::1]:8080"), ("x-forwarded-proto", "gopher")])).as_deref(), Some("http://[::1]:8080"));
        assert_eq!(origin(&headers(&[])), None);
        assert_eq!(origin(&headers(&[("host", "evil.example/path?")])), None);
        assert_eq!(origin(&headers(&[("host", "a b")])), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use geko_module_sdk::bundle::{BundleModule, FORMAT};
    use geko_module_sdk::module::{ModuleName, Route};
    use std::sync::Mutex;
    use tower::ServiceExt;

    #[derive(Default)]
    struct Fake {
        loaded: Mutex<Vec<String>>,
        /// "1.0.0" when empty.
        version: &'static str,
        /// "fake" when empty.
        name: &'static str,
        /// The bundle it says it cannot read.
        rejects: &'static str,
        attempts: Mutex<usize>,
    }

    impl Module for Fake {
        fn describe(&self) -> Description {
            let route = |method, path: &str, pool, public| Route { method, path: path.into(), pool, public };
            Description {
                name: ModuleName::new(if self.name.is_empty() { "fake" } else { self.name }).unwrap(),
                version: if self.version.is_empty() { "1.0.0" } else { self.version }.into(),
                routes: vec![
                    route(Method::Get, "", false, true),
                    route(Method::Get, "v1/echo/{id}", true, false),
                    route(Method::Post, "v1/body", false, false),
                ],
                openapi: Some("openapi.yaml".into()),
                attribution: Some(json!({ "notice": "fake data" })),
                commands: vec![],
            }
        }
        fn load(&self, bundle: &BundleView) -> Result<()> {
            *self.attempts.lock().unwrap() += 1;
            if bundle.manifest.version == self.rejects {
                return Err(geko_module_sdk::GekoError::Data("cannot read it".into()));
            }
            self.loaded.lock().unwrap().push(format!("{} {}", bundle.manifest.version, bundle.module.meta["n"]));
            Ok(())
        }
        fn handle(&self, r: Request) -> module::Response {
            let body = json!({ "route": r.route, "params": r.params, "query": r.query, "body": String::from_utf8_lossy(&r.body) });
            module::Response::new(200, "application/json", body.to_string())
        }
    }

    fn app(access: Access) -> Router {
        router(&[Mounted::new(Arc::new(Fake::default()))], None, access, Some("mail@example.org".into()))
    }

    async fn get_json(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let res = app.clone().oneshot(axum::http::Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn mounts_the_routes_of_a_module_under_its_name() {
        let app = app(Access::default());
        let (status, body) = get_json(&app, "/fake/v1/echo/a%20b?x=1&y=%C3%B1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["route"], "v1/echo/{id}");
        assert_eq!(body["params"], json!([["id", "a b"]]));
        assert_eq!(body["query"], json!([["x", "1"], ["y", "ñ"]]));
        let res = app
            .clone()
            .oneshot(axum::http::Request::post("/fake/v1/body").body(Body::from("payload")).unwrap())
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
        assert_eq!(body["body"], "payload");
        assert_eq!(get_json(&app, "/v1/echo/x").await.0, StatusCode::NOT_FOUND, "only under its name");

        let res = app.oneshot(axum::http::Request::get("/fake").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.headers()[header::LOCATION], "/fake/");
    }

    #[tokio::test]
    async fn health_attribution_and_docs_list_the_modules() {
        let app = app(Access::default());
        assert_eq!(get_json(&app, "/health").await.1["modules"], json!(["fake"]));
        let (_, a) = get_json(&app, "/v1/attribution").await;
        assert_eq!(a["fake"]["notice"], "fake data");
        assert_eq!(a["contact"], "mail@example.org");
        let res = app.clone().oneshot(axum::http::Request::get("/openapi.yaml").body(Body::empty()).unwrap()).await.unwrap();
        assert!(to_bytes(res.into_body(), 1 << 20).await.unwrap().starts_with(b"openapi: 3.1"));
        let res = app.oneshot(axum::http::Request::get("/docs").body(Body::empty()).unwrap()).await.unwrap();
        let page = String::from_utf8(to_bytes(res.into_body(), 1 << 16).await.unwrap().to_vec()).unwrap();
        assert!(page.starts_with("<!doctype html>") && page.contains("swagger-ui") && page.contains(r#""url":"/fake/openapi.yaml""#), "{page}");
    }

    #[tokio::test]
    async fn the_dashboard_is_served_at_the_root_without_a_key() {
        let app = app(Access { api_keys: vec!["k".into()], ..Default::default() });
        let request = |encoding: &str| axum::http::Request::get("/").header(header::ACCEPT_ENCODING, encoding).body(Body::empty()).unwrap();

        let res = app.clone().oneshot(request("identity")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));
        assert!(!res.headers().contains_key(header::CONTENT_ENCODING));
        let page = String::from_utf8(to_bytes(res.into_body(), 1 << 20).await.unwrap().to_vec()).unwrap();
        assert!(page.starts_with("<!doctype html>"), "{page}");

        let res = app.oneshot(request("gzip")).await.unwrap();
        // Without `ui/dist` at compile time the page that says so is small and not compressed.
        if res.headers().contains_key(header::CONTENT_ENCODING) {
            assert_eq!(&to_bytes(res.into_body(), 1 << 20).await.unwrap()[..2], &[0x1f, 0x8b]);
        }
    }

    #[tokio::test]
    async fn the_dashboard_lists_the_active_modules_behind_the_key() {
        let app = app(Access { api_keys: vec!["k".into()], ..Default::default() });
        assert_eq!(get_json(&app, "/admin/api/modules").await.0, StatusCode::UNAUTHORIZED);
        let (status, body) = get_json(&app, "/admin/api/modules?api_key=k").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!([{ "name": "fake", "version": "1.0.0", "unavailable": null }]));
    }

    #[tokio::test]
    async fn ready_only_once_the_bundle_has_the_part_of_every_module() {
        let dir = tempfile::tempdir().unwrap();
        let mounted = Mounted::new(Arc::new(Fake::default()));
        let manager = Arc::new(BundleManager::new(
            BundleSource { dir: dir.path().to_path_buf(), reload: Duration::from_secs(1), pull: None },
            vec![mounted.clone()],
        ));
        let app = router(&[mounted], Some(Arc::clone(&manager)), Access { api_keys: vec!["k".into()], ..Default::default() }, None);
        let (status, body) = get_json(&app, "/ready").await;
        assert_eq!((status, &body["ready"]), (StatusCode::SERVICE_UNAVAILABLE, &json!(false)), "no bundle yet, and no key needed");
        assert_eq!(body["waiting"], json!(["a bundle"]));
        assert_eq!(get_json(&app, "/health").await.0, StatusCode::OK, "alive all the same");

        // A bundle that has no part of the module: loaded, but it would serve nothing.
        let root = dir.path().join("v1");
        std::fs::create_dir_all(&root).unwrap();
        BundleManifest { format: FORMAT, version: "v1".into(), created_unix: 0, modules: vec![], licenses: vec![], artifacts: vec![] }.write(&root).unwrap();
        std::fs::write(dir.path().join("current"), "v1").unwrap();
        assert!(!manager.refresh().unwrap(), "no module took it");
        let (status, body) = get_json(&app, "/ready").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["waiting"], json!(["the part of fake in the bundle"]));

        publish(dir.path(), "v2");
        assert!(manager.refresh().unwrap());
        let (status, body) = get_json(&app, "/ready").await;
        assert_eq!((status, &body["ready"], &body["bundle"]), (StatusCode::OK, &json!(true), &json!("v2")));
    }

    #[tokio::test]
    async fn a_server_without_bundles_is_ready() {
        assert_eq!(get_json(&app(Access::default()), "/ready").await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_pages_of_a_module_need_no_key() {
        let app = app(Access { api_keys: vec!["k".into()], ..Default::default() });
        assert_eq!(get_json(&app, "/fake/").await.0, StatusCode::OK);
        assert_eq!(get_json(&app, "/fake/v1/echo/x").await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(get_json(&app, "/fake/v1/echo/x?api_key=k").await.0, StatusCode::OK);
    }

    fn publish(dir: &std::path::Path, version: &str) {
        publish_by(dir, version, "1.0.0");
    }

    /// Publishes `version` with the part of the fake module built by its version `by`.
    fn publish_by(dir: &std::path::Path, version: &str, by: &str) {
        publish_parts(dir, version, &[("fake", by)]);
    }

    fn publish_parts(dir: &std::path::Path, version: &str, parts: &[(&str, &str)]) {
        let root = dir.join(version);
        std::fs::create_dir_all(&root).unwrap();
        let module = |(name, by): &(&str, &str)| BundleModule {
            name: module::ModuleName::new(name).unwrap(),
            version: (*by).into(),
            meta: json!({ "n": version }),
        };
        let licenses = vec![LicenseNotice { source: version.into(), name: "ODbL".into(), attribution: "OSM".into(), url: None }];
        BundleManifest { format: FORMAT, version: version.into(), created_unix: 0, modules: parts.iter().map(module).collect(), licenses, artifacts: vec![] }
            .write(&root)
            .unwrap();
        std::fs::write(dir.join("current"), version).unwrap();
    }

    #[test]
    fn the_headers_of_a_module_reach_the_client() {
        let r = response(module::Response::error(503, "starting").with_header("retry-after", "3").with_header("bad\nname", "x"));
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(r.headers().get(header::RETRY_AFTER).unwrap(), "3");
        assert_eq!(r.headers().len(), 2, "content type and retry-after, not the invalid one");
    }

    #[test]
    fn manager_loads_only_new_versions() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let manager = BundleManager::new(
            BundleSource { dir: dir.path().to_path_buf(), reload: Duration::from_secs(1), pull: None },
            vec![Mounted::new(fake.clone())],
        );
        assert!(!manager.refresh().unwrap(), "nothing published yet");
        publish(dir.path(), "v1");
        assert!(manager.refresh().unwrap());
        assert!(!manager.refresh().unwrap());
        publish(dir.path(), "v2");
        assert!(manager.refresh().unwrap());
        assert_eq!(*fake.loaded.lock().unwrap(), [r#"v1 "v1""#, r#"v2 "v2""#]);
        assert_eq!(manager.version().as_deref(), Some("v2"));
    }

    #[test]
    fn a_bundle_of_another_version_of_the_module_is_for_the_module_to_judge() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let manager = BundleManager::new(BundleSource { dir: dir.path().to_path_buf(), reload: Duration::from_secs(1), pull: None }, vec![Mounted::new(fake.clone())]);
        publish_by(dir.path(), "v1", "0.9.0");
        assert!(manager.refresh().unwrap());
        assert_eq!(*fake.loaded.lock().unwrap(), [r#"v1 "v1""#]);
        assert_eq!(manager.error(), None);
    }

    fn two_modules(dir: &std::path::Path, rejects: &'static str) -> (Arc<Fake>, Arc<Fake>, Vec<Mounted>, Arc<BundleManager>) {
        let good = Arc::new(Fake { name: "good", ..Default::default() });
        let bad = Arc::new(Fake { name: "bad", rejects, ..Default::default() });
        let mounted = vec![Mounted::new(bad.clone()), Mounted::new(good.clone())];
        let source = BundleSource { dir: dir.to_path_buf(), reload: Duration::from_secs(1), pull: None };
        let manager = Arc::new(BundleManager::new(source, mounted.clone()));
        (good, bad, mounted, manager)
    }

    const BOTH: [(&str, &str); 2] = [("bad", "1.0.0"), ("good", "1.0.0")];

    #[tokio::test]
    async fn a_module_that_rejects_a_bundle_holds_back_no_other() {
        let dir = tempfile::tempdir().unwrap();
        let (good, bad, mounted, manager) = two_modules(dir.path(), "v2");
        publish_parts(dir.path(), "v1", &BOTH);
        assert!(manager.refresh().unwrap());
        publish_parts(dir.path(), "v2", &BOTH);
        assert!(manager.refresh().unwrap(), "one of them took it");
        assert_eq!(*good.loaded.lock().unwrap(), [r#"v1 "v1""#, r#"v2 "v2""#]);
        assert_eq!(manager.version_of("good").as_deref(), Some("v2"));
        assert_eq!(manager.version_of("bad").as_deref(), Some("v1"), "it goes on with the one it had");
        assert_eq!(manager.version().as_deref(), Some("v2"));

        let app = router(&mounted, Some(Arc::clone(&manager)), Access::default(), None);
        let (status, body) = get_json(&app, "/ready").await;
        assert_eq!((status, &body["modules"]["bad"]["bundle"]), (StatusCode::OK, &json!("v1")), "it still serves");
        let (_, body) = get_json(&app, "/health").await;
        assert_eq!(body["bundles"]["good"], json!({ "bundle": "v2", "error": null }));
        assert_eq!(body["bundles"]["bad"]["bundle"], json!("v1"));
        assert!(body["bundles"]["bad"]["error"].as_str().unwrap().contains("cannot read it"), "{body}");
        assert!(body["bundle_error"].as_str().unwrap().starts_with("bad: "), "{body}");
        let sources = get_json(&app, "/v1/attribution").await.1;
        assert_eq!(sources["bundles"], json!({ "bad": "v1", "good": "v2" }));
        let named: Vec<&str> = sources["sources"].as_array().unwrap().iter().map(|s| s["source"].as_str().unwrap()).collect();
        assert_eq!(named, ["v1", "v2"], "the licenses of what each module serves");

        let attempts = *bad.attempts.lock().unwrap();
        for _ in 0..3 {
            assert!(!manager.refresh().unwrap());
        }
        assert_eq!(*bad.attempts.lock().unwrap(), attempts, "a rejected bundle is not tried again");
        assert_eq!(good.loaded.lock().unwrap().len(), 2, "nor one already loaded");

        publish_parts(dir.path(), "v3", &BOTH);
        assert!(manager.refresh().unwrap());
        assert_eq!((manager.version_of("bad").as_deref(), manager.error()), (Some("v3"), None), "the next one is tried");
    }

    #[tokio::test]
    async fn a_module_with_nothing_to_serve_does_not_stop_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let (_, _, mounted, manager) = two_modules(dir.path(), "v1");
        publish_parts(dir.path(), "v1", &BOTH);
        assert!(manager.refresh().unwrap());
        let app = router(&mounted, Some(manager), Access::default(), None);

        let (status, body) = get_json(&app, "/ready").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "not all of it can answer");
        assert!(body["waiting"][0].as_str().unwrap().starts_with("bad: "), "{body}");
        assert_eq!(body["modules"]["good"]["ready"], json!(true));
        let (status, body) = get_json(&app, "/ready?module=good").await;
        assert_eq!((status, &body["bundle"]), (StatusCode::OK, &json!("v1")));
        assert_eq!(get_json(&app, "/ready?module=bad").await.0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(get_json(&app, "/ready?module=other").await.0, StatusCode::NOT_FOUND);
    }
}
