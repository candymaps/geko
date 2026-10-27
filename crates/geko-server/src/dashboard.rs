//! The dashboard with no module behind it, for the operator mode (docs/OPERATOR_MODE_PLAN.md,
//! step 10): the shell and the list of active modules, which the operator answers from the state of
//! the cluster because no process of its own has every module mounted. The routes of the modules
//! (`/geocoder/…`) and the common ones (`/docs`, `/v1/attribution`) never pass through here: the
//! Ingress sends them to the pod of each module.

use std::net::SocketAddr;
use std::sync::Arc;

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

pub fn router(access: Access, statuses: Statuses) -> Router {
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
        );
    let public = ui::paths().map(str::to_owned).collect();
    ui::mount(app)
        .layer(axum::middleware::from_fn_with_state(Arc::new(access::Guard::new(access, public)), access::check))
        .layer(tower_http::catch_panic::CatchPanicLayer::new())
}

pub async fn serve(addr: SocketAddr, access: Access, statuses: Statuses) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "the dashboard listens");
    axum::serve(listener, router(access, statuses).into_make_service_with_connect_info::<SocketAddr>())
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
        router(Access { api_keys: keys.iter().map(|k| k.to_string()).collect(), ..Default::default() }, statuses)
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
    async fn is_alive_and_ready_without_a_key() {
        let app = app(&["k"]);
        assert_eq!(get_json(&app, "/health").await.0, StatusCode::OK);
        assert_eq!(get_json(&app, "/ready").await.0, StatusCode::OK);
    }
}
