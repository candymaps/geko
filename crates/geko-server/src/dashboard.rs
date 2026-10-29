//! The dashboard with no module behind it, for the operator mode (docs/OPERATOR_MODE_PLAN.md,
//! step 10): the shell and the list of active modules, which the operator answers from the state of
//! the cluster because no process of its own has every module mounted. The routes of the modules
//! (`/geocoder/…`) and the common ones (`/docs`, `/v1/attribution`) never pass through here: the
//! Ingress sends them to the pod of each module.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use geko_module_sdk::Result;
use serde_json::json;

use crate::{Access, access, ui};

/// What the dashboard shows of one active module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleStatus {
    pub name: String,
    /// The version the cluster runs, empty when it runs none yet.
    pub version: String,
    /// Why it answers nothing now, if so.
    pub unavailable: Option<String>,
}

/// The active modules as of now: read at each request, so it is always what the operator last saw.
pub type Statuses = Arc<dyn Fn() -> Vec<ModuleStatus> + Send + Sync>;

/// What one pod of a module answered to a GET of the operator: the JSON, or why there is none.
#[derive(Clone, Debug, PartialEq)]
pub struct PodAnswer {
    pub pod: String,
    pub ready: bool,
    pub body: std::result::Result<serde_json::Value, String>,
}

/// Asks every pod of a module (first argument) for a path of it (second), all at once.
pub type Pods = Arc<dyn Fn(String, String) -> Pin<Box<dyn Future<Output = Vec<PodAnswer>> + Send>> + Send + Sync>;

/// A path the operator may ask of the pods of `module`: one of that module, with no way out of it.
fn askable(module: &str, path: &str) -> bool {
    path.strip_prefix(module).and_then(|rest| rest.strip_prefix('/')).is_some_and(|rest| !rest.is_empty())
        && !path.split('/').any(|part| part.is_empty() || part == "." || part == ".." || part.contains(['\\', '?', '#']))
}

pub fn router(access: Access, statuses: Statuses, pods: Pods) -> Router {
    let known = statuses.clone();
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/ready", get(|| async { (StatusCode::OK, Json(json!({ "ready": true }))) }))
        .route(
            "/admin/api/modules",
            get(move || {
                let body: serde_json::Value =
                    statuses().into_iter().map(|m| json!({ "name": m.name, "version": m.version, "unavailable": m.unavailable })).collect();
                async move { Json(body) }
            }),
        )
        // What each pod of a module answers to the same GET, so that the dashboard can show them
        // apart (the replicas keep their own counters). The operator knows no route of a module:
        // the dashboard names the path, and it must be one of that module.
        .route(
            "/admin/api/modules/{module}/pods/{*path}",
            get(move |Path((module, path)): Path<(String, String)>| {
                let active = known().iter().any(|m| m.name == module);
                let asked = (active && askable(&module, &path)).then(|| pods(module, path));
                async move {
                    let Some(asked) = asked else { return Err(StatusCode::NOT_FOUND) };
                    let body: serde_json::Value = asked
                        .await
                        .into_iter()
                        .map(|a| match a.body {
                            Ok(data) => json!({ "pod": a.pod, "ready": a.ready, "data": data, "error": null }),
                            Err(error) => json!({ "pod": a.pod, "ready": a.ready, "data": null, "error": error }),
                        })
                        .collect();
                    Ok(Json(body))
                }
            }),
        );
    let public = ui::paths().map(str::to_owned).collect();
    ui::mount(app)
        .layer(axum::middleware::from_fn_with_state(Arc::new(access::Guard::new(access, public)), access::check))
        .layer(tower_http::catch_panic::CatchPanicLayer::new())
}

pub async fn serve(addr: SocketAddr, access: Access, statuses: Statuses, pods: Pods) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "the dashboard listens");
    axum::serve(listener, router(access, statuses, pods).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt;

    use super::*;

    fn app(keys: &[&str]) -> Router {
        let statuses: Statuses = Arc::new(|| {
            vec![
                ModuleStatus { name: "geocoder".into(), version: "1.2.0".into(), unavailable: None },
                ModuleStatus { name: "tiles".into(), version: String::new(), unavailable: Some("no pod ready (0/1)".into()) },
            ]
        });
        let pods: Pods = Arc::new(|module, path| {
            Box::pin(async move {
                vec![
                    PodAnswer { pod: format!("{module}-a"), ready: true, body: Ok(json!({ "path": path })) },
                    PodAnswer { pod: format!("{module}-b"), ready: false, body: Err("connection refused".into()) },
                ]
            })
        });
        router(Access { api_keys: keys.iter().map(|k| k.to_string()).collect(), ..Default::default() }, statuses, pods)
    }

    async fn get_json(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let res = app.clone().oneshot(axum::http::Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
        let status = res.status();
        let body = to_bytes(res.into_body(), 1 << 16).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn lists_what_the_cluster_runs_behind_the_key() {
        let app = app(&["k"]);
        assert_eq!(get_json(&app, "/admin/api/modules").await.0, StatusCode::UNAUTHORIZED);
        let (status, body) = get_json(&app, "/admin/api/modules?api_key=k").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!([
                { "name": "geocoder", "version": "1.2.0", "unavailable": null },
                { "name": "tiles", "version": "", "unavailable": "no pod ready (0/1)" },
            ])
        );
    }

    #[tokio::test]
    async fn asks_every_pod_of_an_active_module_for_a_path_of_that_module() {
        let app = app(&["k"]);
        assert_eq!(get_json(&app, "/admin/api/modules/geocoder/pods/geocoder/v1/stats").await.0, StatusCode::UNAUTHORIZED);
        let (status, body) = get_json(&app, "/admin/api/modules/geocoder/pods/geocoder/v1/stats?api_key=k").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!([
                { "pod": "geocoder-a", "ready": true, "data": { "path": "geocoder/v1/stats" }, "error": null },
                { "pod": "geocoder-b", "ready": false, "data": null, "error": "connection refused" },
            ])
        );
    }

    #[tokio::test]
    async fn does_not_ask_for_what_is_not_of_the_module() {
        let app = app(&["k"]);
        for uri in [
            "/admin/api/modules/geocoder/pods/tiles/stats.json",
            "/admin/api/modules/geocoder/pods/geocoder/../tiles/stats.json",
            "/admin/api/modules/geocoder/pods/geocoder",
            "/admin/api/modules/nobody/pods/nobody/v1/stats",
        ] {
            assert_eq!(get_json(&app, &format!("{uri}?api_key=k")).await.0, StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[tokio::test]
    async fn is_alive_and_ready_without_a_key() {
        let app = app(&["k"]);
        assert_eq!(get_json(&app, "/health").await.0, StatusCode::OK);
        assert_eq!(get_json(&app, "/ready").await.0, StatusCode::OK);
    }
}
