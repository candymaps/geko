mod access;
mod metrics;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwapOption;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use geko_core::Result;
use geko_core::bundle::{BundleManifest, LicenseNotice, current_version};
pub use access::Access;
pub use geko_core::module::ModuleName;
use serde_json::json;

#[derive(Debug, Clone)]
pub struct BundleView {
    pub root: PathBuf,
    pub manifest: BundleManifest,
}

/// A functional module (PROJECT.md §2.3). Only active modules are instantiated,
/// so an inactive module allocates nothing and registers no routes.
pub trait Module: Send + Sync {
    fn name(&self) -> ModuleName;
    /// Loads a bundle and switches to it atomically; queries in flight finish on the previous one.
    fn load(&self, bundle: &BundleView) -> Result<()>;
    fn router(&self) -> Router;
    fn shutdown(&self) -> Result<()>;
    /// Appends the module's own counters to `/metrics`, in the Prometheus text format.
    fn metrics(&self, _out: &mut String) {}
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
    modules: Vec<Arc<dyn Module>>,
    loaded: ArcSwapOption<(String, Vec<LicenseNotice>)>,
}

impl BundleManager {
    pub fn new(source: BundleSource, modules: Vec<Arc<dyn Module>>) -> Self {
        Self { source, modules, loaded: ArcSwapOption::empty() }
    }

    pub fn version(&self) -> Option<String> {
        self.loaded.load_full().map(|v| v.0.clone())
    }

    /// Licenses and attributions of the sources of the loaded bundle.
    pub fn licenses(&self) -> Vec<LicenseNotice> {
        self.loaded.load_full().map(|v| v.1.clone()).unwrap_or_default()
    }

    /// Loads `current` if it differs from the loaded version. On error the previous bundle
    /// stays in service.
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
        let view = BundleView { manifest: BundleManifest::read(&root)?, root };
        let started = std::time::Instant::now();
        for m in &self.modules {
            m.load(&view)?;
        }
        tracing::info!(%version, elapsed = ?started.elapsed(), "bundle loaded");
        self.loaded.store(Some(Arc::new((version, view.manifest.licenses))));
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

/// The method of making the fused database, offered under ODbL 4.6 to whoever gets results.
const FUSION_METHOD: &str = include_str!("../../../docs/FUSION_METHOD.md");

/// The reference of the API, served at `/openapi.yaml` and browsed at `/docs` (Swagger UI).
const OPENAPI: &str = include_str!("../../../openapi.yaml");
const DOCS: &str = include_str!("docs.html");

/// Licenses and attributions of the served data (PROJECT.md §6.4): the sources of the bundle,
/// and, since fusing them with OpenStreetMap makes a derivative database, where its method is
/// described and whom to ask for the code that reproduces it.
fn attribution(bundles: Option<&BundleManager>, contact: Option<&str>) -> serde_json::Value {
    let licenses = bundles.map(|b| b.licenses()).unwrap_or_default();
    json!({
        "bundle": bundles.and_then(|b| b.version()),
        "sources": licenses.iter().map(|l| json!({ "source": l.source, "license": l.name, "attribution": l.attribution, "url": l.url })).collect::<Vec<_>>(),
        "derivative_database": {
            "license": "ODbL 1.0",
            "notice": "The data served are a derivative database of OpenStreetMap, fused with the other sources listed. As ODbL 4.6 allows, the method of making it is described at /v1/attribution/method, and the code needed to reproduce it is available on request.",
            "method": "/v1/attribution/method",
            "contact": contact,
        },
    })
}

pub fn router(modules: &[Arc<dyn Module>], bundles: Option<Arc<BundleManager>>, access: Access, contact: Option<String>) -> Router {
    let names: Vec<&'static str> = modules.iter().map(|m| m.name().as_str()).collect();
    let attribution_bundles = bundles.clone();
    let attribution_routes = Router::new()
        .route(
            "/v1/attribution",
            get(move || {
                let body = attribution(attribution_bundles.as_deref(), contact.as_deref());
                async move { Json(body) }
            }),
        )
        .route("/v1/attribution/method", get(|| async { ([(axum::http::header::CONTENT_TYPE, "text/markdown; charset=utf-8")], FUSION_METHOD) }))
        .route("/openapi.yaml", get(|| async { ([(axum::http::header::CONTENT_TYPE, "application/yaml")], OPENAPI) }))
        .route("/docs", get(|| async { axum::response::Html(DOCS) }));
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
    let tracked = modules
        .iter()
        .fold(health, |app, m| app.merge(m.router()))
        .route_layer(axum::middleware::from_fn_with_state(Arc::clone(&metrics), metrics::track));
    let exposed = Arc::clone(&metrics);
    let modules = modules.to_vec();
    let metrics_route = Router::new().route(
        "/metrics",
        get(move || {
            let mut text = exposed.render(version().as_deref());
            for m in &modules {
                m.metrics(&mut text);
            }
            async move { ([(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")], text) }
        }),
    );
    // A panic in one request becomes a 500 for that request only.
    tracked
        .merge(metrics_route)
        .merge(attribution_routes)
        .layer(axum::middleware::from_fn_with_state(Arc::new(access::Guard::new(access)), access::check))
        .layer(tower_http::catch_panic::CatchPanicLayer::new())
}

pub async fn serve(
    addr: SocketAddr,
    modules: Vec<Arc<dyn Module>>,
    bundles: Option<BundleSource>,
    access: Access,
    contact: Option<String>,
) -> Result<()> {
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
    tracing::info!(%addr, modules = ?modules.iter().map(|m| m.name()).collect::<Vec<_>>(), "listening");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    for m in &modules {
        m.shutdown()?;
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

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Runs the CPU-bound work of a request (parsing, search) on the blocking pool, so the async
/// workers keep accepting connections and answering cheap routes meanwhile; at most one such job
/// per core runs at once, the rest wait for a slot without taking a core. Unbounded, 64 parallel
/// searches on 16 cores doubled the p99; on the async workers, a cheap reverse request waited
/// twice as long behind them. A panic answers 500.
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> std::result::Result<T, ApiError> + Send + 'static,
) -> std::result::Result<T, ApiError> {
    static SLOTS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    let slots = SLOTS.get_or_init(|| tokio::sync::Semaphore::new(std::thread::available_parallelism().map_or(4, |n| n.get())));
    let _slot = slots.acquire().await.map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("request failed: {e}")))?;
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(e) => Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("request failed: {e}"))),
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
    use axum::http::Request;
    use std::sync::Mutex;
    use tower::ServiceExt;

    #[derive(Default)]
    struct Fake {
        loaded: Mutex<Vec<String>>,
    }

    impl Module for Fake {
        fn name(&self) -> ModuleName {
            ModuleName::Tiles
        }
        fn load(&self, bundle: &BundleView) -> Result<()> {
            self.loaded.lock().unwrap().push(bundle.manifest.version.clone());
            Ok(())
        }
        fn router(&self) -> Router {
            Router::new().route("/fake", get(|| async { "fake" }))
        }
        fn shutdown(&self) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn health_lists_active_modules() {
        let app = router(&[Arc::new(Fake::default())], None, Access::default(), None);
        let res = app.oneshot(Request::get("/health").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(&to_bytes(res.into_body(), 1024).await.unwrap()).unwrap();
        assert_eq!(body["modules"], json!(["tiles"]));
    }

    #[tokio::test]
    async fn serves_the_api_reference_and_its_browser() {
        let app = router(&[Arc::new(Fake::default())], None, Access::default(), None);
        let res = app.clone().oneshot(Request::get("/openapi.yaml").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let spec = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        assert!(spec.starts_with(b"openapi: 3.1"));
        let res = app.oneshot(Request::get("/docs").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let page = String::from_utf8(to_bytes(res.into_body(), 1 << 16).await.unwrap().to_vec()).unwrap();
        assert!(page.contains("swagger-ui") && page.contains("/openapi.yaml"), "{page}");
    }

    fn publish(dir: &std::path::Path, version: &str) {
        let root = dir.join(version);
        std::fs::create_dir_all(&root).unwrap();
        BundleManifest {
            version: version.into(),
            created_unix: 0,
            modules: vec![],
            segments: vec![],
            licenses: vec![],
            artifacts: vec![],
        }
        .write(&root)
        .unwrap();
        std::fs::write(dir.join("current"), version).unwrap();
    }

    #[test]
    fn manager_loads_only_new_versions() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let manager = BundleManager::new(
            BundleSource { dir: dir.path().to_path_buf(), reload: Duration::from_secs(1), pull: None },
            vec![fake.clone() as Arc<dyn Module>],
        );
        assert!(!manager.refresh().unwrap(), "nothing published yet");
        publish(dir.path(), "v1");
        assert!(manager.refresh().unwrap());
        assert!(!manager.refresh().unwrap());
        publish(dir.path(), "v2");
        assert!(manager.refresh().unwrap());
        assert_eq!(*fake.loaded.lock().unwrap(), ["v1", "v2"]);
        assert_eq!(manager.version().as_deref(), Some("v2"));
    }
}
