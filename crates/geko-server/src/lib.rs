//! What geko serves for every module (docs/MODULES_PLAN.md): `/health`, `/metrics`,
//! `/v1/attribution`, `/openapi.yaml` and `/docs`, API keys, CORS and the rate limit, the bundle
//! that every module loads and keeps watching, and the routes of each active module, mounted under
//! its name (`/geocoder/v1/search`). Only `geko serve` (and `up`) use it, and tokio.

mod access;
mod metrics;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;
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

/// An active module and what it said it is.
#[derive(Clone)]
pub struct Mounted {
    pub module: Arc<dyn Module>,
    pub description: Description,
}

impl Mounted {
    pub fn new(module: Arc<dyn Module>) -> Self {
        Self { description: module.describe(), module }
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

/// Loads the `current` bundle into every module and keeps watching for new versions.
pub struct BundleManager {
    source: BundleSource,
    modules: Vec<Mounted>,
    loaded: ArcSwapOption<(String, Vec<LicenseNotice>)>,
}

impl BundleManager {
    pub fn new(source: BundleSource, modules: Vec<Mounted>) -> Self {
        Self { source, modules, loaded: ArcSwapOption::empty() }
    }

    pub fn version(&self) -> Option<String> {
        self.loaded.load_full().map(|v| v.0.clone())
    }

    /// Licenses and attributions of the sources of the loaded bundle.
    pub fn licenses(&self) -> Vec<LicenseNotice> {
        self.loaded.load_full().map(|v| v.1.clone()).unwrap_or_default()
    }

    /// Loads `current` if it differs from the loaded version, into every module that has a part
    /// of it. On error the previous bundle stays in service.
    pub fn refresh(&self) -> Result<bool> {
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
        for m in &self.modules {
            let Some(part) = manifest.module(&m.description.name) else {
                tracing::warn!(module = m.name(), %version, "the bundle has no part of this module: it serves nothing");
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
            if part.version != m.description.version {
                tracing::warn!(module = m.name(), built_by = part.version, serving = m.description.version, "the bundle was built by another version of the module");
            }
            m.module.load(&BundleView { root: root.clone(), manifest: manifest.clone(), module: part.clone() })?;
        }
        tracing::info!(%version, elapsed = ?started.elapsed(), "bundle loaded");
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

/// Licenses and attributions of the served data (PROJECT.md §6.4): the sources of the bundle, whom
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

/// `/docs`: Swagger UI over the reference of geko and those of the modules.
fn docs(modules: &[Mounted]) -> String {
    let mut specs = vec![json!({ "url": "/openapi.yaml", "name": "geko" })];
    specs.extend(modules.iter().filter_map(|m| m.description.openapi.as_ref().map(|p| json!({ "url": m.path(p), "name": m.name() }))));
    let home = modules.iter().any(|m| m.description.home.is_some());
    DOCS.replace("__SPECS__", &serde_json::Value::from(specs).to_string()).replace("__HOME__", if home { "true" } else { "false" })
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
    (status, [(header::CONTENT_TYPE, r.content_type)], r.body).into_response()
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
    if let Some(home) = modules.iter().find_map(|m| m.description.home.as_ref().map(|h| m.path(h))) {
        app = app.route("/", get(move || async move { Redirect::to(&home) }));
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
    let shared_routes = Router::new()
        .route(
            "/v1/attribution",
            get(move || {
                let body = attribution(&attribution_modules, attribution_bundles.as_deref(), contact.as_deref());
                async move { Json(body) }
            }),
        )
        .route("/openapi.yaml", get(|| async { ([(header::CONTENT_TYPE, "application/yaml")], OPENAPI) }))
        .route("/docs", get(move || async move { axum::response::Html(docs) }));
    let metrics = Arc::new(metrics::Metrics::default());
    let version = {
        let bundles = bundles.clone();
        move || bundles.as_ref().and_then(|b| b.version())
    };
    let health_version = version.clone();
    let health = Router::new().route(
        "/health",
        get(move || {
            let names = names.clone();
            let version = health_version();
            async move { Json(json!({ "status": "ok", "modules": names, "bundle": version })) }
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
    let guard = access::Guard::new(access, public_paths(modules));
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
    }

    impl Module for Fake {
        fn describe(&self) -> Description {
            let route = |method, path: &str, pool, public| Route { method, path: path.into(), pool, public };
            Description {
                name: ModuleName::new("fake").unwrap(),
                version: "1.0.0".into(),
                contract: module::CONTRACT,
                routes: vec![
                    route(Method::Get, "", false, true),
                    route(Method::Get, "v1/echo/{id}", true, false),
                    route(Method::Post, "v1/body", false, false),
                ],
                home: Some(String::new()),
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

        let res = app.clone().oneshot(axum::http::Request::get("/").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.headers()[header::LOCATION], "/fake/");
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
        assert!(page.contains("swagger-ui") && page.contains(r#""url":"/fake/openapi.yaml""#), "{page}");
    }

    #[tokio::test]
    async fn the_pages_of_a_module_need_no_key() {
        let app = app(Access { api_keys: vec!["k".into()], ..Default::default() });
        assert_eq!(get_json(&app, "/fake/").await.0, StatusCode::OK);
        assert_eq!(get_json(&app, "/fake/v1/echo/x").await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(get_json(&app, "/fake/v1/echo/x?api_key=k").await.0, StatusCode::OK);
    }

    fn publish(dir: &std::path::Path, version: &str, contract: u32) {
        let root = dir.join(version);
        std::fs::create_dir_all(&root).unwrap();
        let part = BundleModule { name: ModuleName::new("fake").unwrap(), version: "1.0.0".into(), contract, meta: json!({ "n": version }) };
        BundleManifest { format: FORMAT, version: version.into(), created_unix: 0, modules: vec![part], licenses: vec![], artifacts: vec![] }
            .write(&root)
            .unwrap();
        std::fs::write(dir.join("current"), version).unwrap();
    }

    #[test]
    fn manager_loads_only_new_versions_of_a_compatible_contract() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let manager = BundleManager::new(
            BundleSource { dir: dir.path().to_path_buf(), reload: Duration::from_secs(1), pull: None },
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
    }
}
