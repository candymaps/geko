//! What geko serves for every module (docs/MODULES_PLAN.md): `/health`, `/metrics`,
//! `/v1/attribution`, `/openapi.yaml` and `/docs`, API keys, CORS and the rate limit, the bundle
//! that every module loads and keeps watching, and the routes of each active module, mounted under
//! its name (`/geocoder/v1/search`). Only `geko serve` (and `up`) use it, and tokio; the operator mode
//! also serves the dashboard alone (`serve_dashboard`).

mod access;
mod dashboard;
mod metrics;
mod ui;

use std::collections::HashSet;
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
use geko_module_sdk::module::{self, BundleView, Description, Method, Module, ModuleName, Request};
use serde_json::json;

pub use access::Access;
pub use dashboard::{ModuleStatus, Statuses, serve as serve_dashboard};

/// The module behind a mount, which a bundle built by another version of it switches to that
/// version (`Reopen`); requests in flight finish on the one they started on.
struct Slot {
    current: ArcSwap<(Arc<dyn Module>, String)>,
}

impl Slot {
    fn module(&self) -> Arc<dyn Module> {
        Arc::clone(&self.current.load().0)
    }

    fn version(&self) -> String {
        self.current.load().1.clone()
    }

    fn switch(&self, module: Arc<dyn Module>) {
        let version = module.describe().version;
        self.current.store(Arc::new((module, version)));
    }
}

impl Module for Slot {
    fn describe(&self) -> Description {
        self.module().describe()
    }

    fn load(&self, bundle: &BundleView) -> Result<()> {
        self.module().load(bundle)
    }

    fn handle(&self, request: Request) -> module::Response {
        self.module().handle(request)
    }

    fn metrics(&self, out: &mut String) {
        self.module().metrics(out)
    }

    fn shutdown(&self) -> Result<()> {
        self.module().shutdown()
    }

    fn unavailable(&self) -> Option<String> {
        self.module().unavailable()
    }
}

/// An active module and what it said it is.
#[derive(Clone)]
pub struct Mounted {
    pub module: Arc<dyn Module>,
    pub description: Description,
    slot: Arc<Slot>,
}

impl Mounted {
    pub fn new(module: Arc<dyn Module>) -> Self {
        let description = module.describe();
        let slot = Arc::new(Slot { current: ArcSwap::from_pointee((module, description.version.clone())) });
        Self { module: Arc::clone(&slot) as Arc<dyn Module>, description, slot }
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

/// The module `name` of the version a bundle was built by, to serve that bundle with it; `None`
/// keeps the one in service (a module linked in, or one of `GEKO_MODULES`).
pub type Reopen = Arc<dyn Fn(&ModuleName, &str) -> Result<Option<Arc<dyn Module>>> + Send + Sync>;

#[derive(Clone)]
pub struct BundleSource {
    pub dir: PathBuf,
    /// How often to look for a new `current` version.
    pub reload: Duration,
    pub pull: Option<Pull>,
    pub reopen: Option<Reopen>,
}

/// Loads the `current` bundle into every module and keeps watching for new versions.
pub struct BundleManager {
    source: BundleSource,
    modules: Vec<Mounted>,
    loaded: ArcSwapOption<(String, Vec<LicenseNotice>)>,
    /// Why the last bundle published could not be loaded, for `/health`.
    error: ArcSwapOption<String>,
    /// The modules that the loaded bundle has no part of, which serve nothing: for `/ready`.
    waiting: ArcSwap<Vec<String>>,
}

impl BundleManager {
    pub fn new(source: BundleSource, modules: Vec<Mounted>) -> Self {
        Self { source, modules, loaded: ArcSwapOption::empty(), error: ArcSwapOption::empty(), waiting: ArcSwap::from_pointee(Vec::new()) }
    }

    /// Why this server cannot answer yet, for `/ready`: no bundle loaded, a module without its part of
    /// it, or a module that says it is not available. Empty when it can.
    pub fn waiting_for(&self) -> Vec<String> {
        if self.loaded.load().is_none() {
            return vec!["a bundle".to_owned()];
        }
        let mut waiting: Vec<String> = self.waiting.load().iter().map(|m| format!("the part of {m} in the bundle")).collect();
        waiting.extend(self.modules.iter().filter_map(|m| m.module.unavailable().map(|why| format!("{}: {why}", m.name()))));
        waiting
    }

    pub fn error(&self) -> Option<String> {
        self.error.load_full().map(|e| (*e).clone())
    }

    pub fn version(&self) -> Option<String> {
        self.loaded.load_full().map(|v| v.0.clone())
    }

    /// Licenses and attributions of the sources of the loaded bundle.
    pub fn licenses(&self) -> Vec<LicenseNotice> {
        self.loaded.load_full().map(|v| v.1.clone()).unwrap_or_default()
    }

    /// Loads `current` if it differs from the loaded version, into every module that has a part
    /// of it, switching a module to the version that built its part when that is another. On
    /// error the previous bundle stays in service.
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
        if self.loaded.load().as_ref().is_some_and(|l| l.0 == version) {
            return Ok(false);
        }
        let root = self.source.dir.join(&version);
        let manifest = BundleManifest::read(&root)?;
        let started = Instant::now();
        let mut without_part = Vec::new();
        for m in &self.modules {
            let Some(part) = manifest.module(&m.description.name) else {
                tracing::warn!(module = m.name(), %version, "the bundle has no part of this module: it serves nothing");
                without_part.push(m.name().to_owned());
                continue;
            };
            if part.contract != m.description.contract {
                return Err(geko_module_sdk::GekoError::Incompatible(format!(
                    "bundle {version}: its {} part was built by a module of contract {}, and the module here is of contract {}",
                    m.name(),
                    part.contract,
                    m.description.contract
                )));
            }
            let view = BundleView { root: root.clone(), manifest: manifest.clone(), module: part.clone() };
            let serving = m.slot.version();
            if part.version != serving {
                let reopened = match &self.source.reopen {
                    Some(reopen) => reopen(&m.description.name, &part.version)?,
                    None => None,
                };
                if let Some(module) = reopened {
                    module.load(&view)?;
                    m.slot.switch(module);
                    tracing::info!(module = m.name(), from = serving, to = part.version, "switched to the version of the module that built the bundle");
                    continue;
                }
                tracing::warn!(module = m.name(), built_by = part.version, serving, "the bundle was built by another version of the module");
            }
            m.module.load(&view)?;
        }
        tracing::info!(%version, elapsed = ?started.elapsed(), "bundle loaded");
        self.waiting.store(Arc::new(without_part));
        self.loaded.store(Some(Arc::new((version, manifest.licenses))));
        Ok(true)
    }

    fn watch(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(self.source.reload);
            tick.tick().await;
            loop {
                tick.tick().await;
                let manager = Arc::clone(&self);
                match tokio::task::spawn_blocking(move || manager.refresh()).await {
                    Ok(Err(e)) => tracing::error!(error = %e, "new bundle not loaded; keeping the current one"),
                    Err(e) => tracing::error!(error = %e, "bundle reload task failed"),
                    Ok(Ok(_)) => {}
                }
            }
        });
    }
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

/// `/admin/api/modules`: the active modules, so the dashboard offers a tab for each.
fn module_list(modules: &[Mounted]) -> serde_json::Value {
    modules.iter().map(|m| json!({ "name": m.name(), "version": m.slot.version(), "unavailable": m.module.unavailable() })).collect()
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
            async move { Json(json!({ "status": "ok", "modules": names, "bundle": version, "bundle_error": error, "modules_unavailable": unavailable })) }
        }),
    );
    // `/health` says the process is alive, from the start (the health check of the image, while it
    // builds); `/ready` says it can answer: the probe that keeps a pod of the operator mode out of
    // the balancer until it has loaded its bundle.
    let (ready_bundles, ready_modules) = (bundles.clone(), modules.to_vec());
    let health = health.route(
        "/ready",
        get(move || {
            let waiting: Vec<String> = match &ready_bundles {
                Some(b) => b.waiting_for(),
                None => ready_modules.iter().filter_map(|m| m.module.unavailable().map(|why| format!("{}: {why}", m.name()))).collect(),
            };
            let version = ready_bundles.as_ref().and_then(|b| b.version());
            let status = if waiting.is_empty() { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
            async move { (status, Json(json!({ "ready": status == StatusCode::OK, "bundle": version, "waiting": waiting }))) }
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
    if let Some(m) = &manager {
        let initial = Arc::clone(m);
        match tokio::task::spawn_blocking(move || initial.refresh()).await {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => tracing::warn!("no bundle published yet (geko build, or the one geko up is running): serving 503 until one appears"),
            Ok(Err(e)) => tracing::error!(error = %e, "cannot load the current bundle"),
            Err(e) => tracing::error!(error = %e, "bundle load task failed"),
        }
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
    }

    impl Module for Fake {
        fn describe(&self) -> Description {
            let route = |method, path: &str, pool, public| Route { method, path: path.into(), pool, public };
            Description {
                name: ModuleName::new("fake").unwrap(),
                version: if self.version.is_empty() { "1.0.0" } else { self.version }.into(),
                contract: module::CONTRACT,
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
            BundleSource { dir: dir.path().to_path_buf(), reload: Duration::from_secs(1), pull: None, reopen: None },
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
        assert!(manager.refresh().unwrap());
        let (status, body) = get_json(&app, "/ready").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["waiting"], json!(["the part of fake in the bundle"]));

        publish(dir.path(), "v2", module::CONTRACT);
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

    fn publish(dir: &std::path::Path, version: &str, contract: u32) {
        publish_by(dir, version, contract, "1.0.0");
    }

    /// Publishes `version` with the part of the fake module built by its version `by`.
    fn publish_by(dir: &std::path::Path, version: &str, contract: u32, by: &str) {
        let root = dir.join(version);
        std::fs::create_dir_all(&root).unwrap();
        let part = BundleModule { name: ModuleName::new("fake").unwrap(), version: by.into(), contract, meta: json!({ "n": version }) };
        BundleManifest { format: FORMAT, version: version.into(), created_unix: 0, modules: vec![part], licenses: vec![], artifacts: vec![] }
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
    fn manager_loads_only_new_versions_of_a_compatible_contract() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let manager = BundleManager::new(
            BundleSource { dir: dir.path().to_path_buf(), reload: Duration::from_secs(1), pull: None, reopen: None },
            vec![Mounted::new(fake.clone())],
        );
        assert!(!manager.refresh().unwrap(), "nothing published yet");
        publish(dir.path(), "v1", module::CONTRACT);
        assert!(manager.refresh().unwrap());
        assert!(!manager.refresh().unwrap());
        publish(dir.path(), "v2", module::CONTRACT);
        assert!(manager.refresh().unwrap());
        assert_eq!(*fake.loaded.lock().unwrap(), [r#"v1 "v1""#, r#"v2 "v2""#]);
        assert_eq!(manager.version().as_deref(), Some("v2"));
        publish(dir.path(), "v3", module::CONTRACT + 1);
        let err = manager.refresh().unwrap_err().to_string();
        assert!(err.contains("contract"), "{err}");
        assert_eq!(manager.version().as_deref(), Some("v2"), "the previous one stays");
        assert!(manager.error().is_some_and(|e| e.contains("contract")), "and /health says why");
    }

    #[tokio::test]
    async fn a_bundle_of_another_version_of_the_module_switches_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let newer = Arc::new(Fake { version: "2.0.0", ..Default::default() });
        let given = Arc::clone(&newer);
        let reopen: Reopen = Arc::new(move |name, version| {
            assert_eq!((name.as_str(), version), ("fake", "2.0.0"));
            Ok(Some(Arc::clone(&given) as Arc<dyn Module>))
        });
        let mounted = Mounted::new(Arc::new(Fake::default()));
        let manager = Arc::new(BundleManager::new(
            BundleSource { dir: dir.path().to_path_buf(), reload: Duration::from_secs(1), pull: None, reopen: Some(reopen) },
            vec![mounted.clone()],
        ));
        publish_by(dir.path(), "v1", module::CONTRACT, "2.0.0");
        assert!(manager.refresh().unwrap());
        assert_eq!(*newer.loaded.lock().unwrap(), [r#"v1 "v1""#], "the new version loaded the bundle");
        assert_eq!(mounted.module.describe().version, "2.0.0");
        let app = router(&[mounted], Some(manager), Access::default(), None);
        assert_eq!(get_json(&app, "/fake/v1/echo/x").await.0, StatusCode::OK, "and serves its routes");
    }
}
