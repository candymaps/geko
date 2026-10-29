//! The dashboard (`ui/`), embedded in the binary by `build.rs`: serving a file is a copy of its
//! bytes, gzipped when the client accepts it. The hash routes of the dashboard (`#/tiles`) need no
//! fallback for deep links, so only the files themselves are routes.

use axum::Router;
use axum::http::{HeaderMap, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

pub struct Asset {
    pub path: &'static str,
    pub mime: &'static str,
    pub raw: &'static [u8],
    pub gz: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));

const MISSING: &str = "<!doctype html><meta charset=utf-8><title>geko</title>\
<p style=\"font:15px system-ui;margin:2em\">This geko was compiled without its dashboard: build it with <code>make ui</code> and compile again.</p>";

/// The paths of the dashboard, which need no API key: a page that asked for one could not even load.
pub fn paths() -> impl Iterator<Item = &'static str> {
    ASSETS.iter().map(|a| a.path)
}

fn accepts_gzip(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|coding| coding.split(';').next().is_some_and(|c| c.trim() == "gzip")))
}

fn serve(asset: &'static Asset, headers: &HeaderMap) -> Response {
    // The files under /assets/ carry a hash of their content in their name; the page does not.
    let cache = if asset.path.starts_with("/assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    let common = [(header::CONTENT_TYPE, asset.mime), (header::CACHE_CONTROL, cache), (header::VARY, "accept-encoding")];
    if accepts_gzip(headers) {
        (common, [(header::CONTENT_ENCODING, "gzip")], asset.gz).into_response()
    } else {
        (common, asset.raw).into_response()
    }
}

pub fn mount(mut app: Router) -> Router {
    if ASSETS.is_empty() {
        return app.route("/", get(|| async { Html(MISSING) }));
    }
    for asset in ASSETS {
        app = app.route(asset.path, get(move |headers: HeaderMap| async move { serve(asset, &headers) }));
    }
    app
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_is_sent_only_to_a_client_that_accepts_it() {
        let with = |value: &str| {
            let mut h = HeaderMap::new();
            h.insert(header::ACCEPT_ENCODING, value.parse().unwrap());
            h
        };
        assert!(accepts_gzip(&with("br, gzip;q=0.8")));
        assert!(!accepts_gzip(&with("br, deflate")));
        assert!(!accepts_gzip(&HeaderMap::new()));
    }
}
