//! `/metrics` in the Prometheus text format: requests, errors and latency of every route, the
//! bundle served and the uptime, so that the service can be operated and alerted on.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;

/// Upper bounds of the latency buckets, in seconds.
const BUCKETS: [f64; 11] = [0.001, 0.002, 0.005, 0.01, 0.015, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0];

#[derive(Default)]
struct Route {
    /// Requests by status class: 2xx, 3xx, 4xx, 5xx.
    by_status: [u64; 4],
    /// Requests at or under each bucket bound; the last is every request.
    buckets: [u64; BUCKETS.len() + 1],
    seconds: f64,
}

pub struct Metrics {
    started: Instant,
    routes: Mutex<BTreeMap<String, Route>>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self { started: Instant::now(), routes: Mutex::new(BTreeMap::new()) }
    }
}

impl Metrics {
    fn record(&self, route: &str, status: u16, seconds: f64) {
        let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        let r = routes.entry(route.to_owned()).or_default();
        r.by_status[usize::from(status.clamp(200, 599) / 100 - 2)] += 1;
        for (i, bound) in BUCKETS.iter().enumerate() {
            if seconds <= *bound {
                r.buckets[i] += 1;
            }
        }
        r.buckets[BUCKETS.len()] += 1;
        r.seconds += seconds;
    }

    pub fn render(&self, bundle: Option<&str>) -> String {
        let mut out = String::new();
        let routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        out.push_str("# HELP geko_requests_total Requests answered, by route and status class.\n# TYPE geko_requests_total counter\n");
        for (route, r) in routes.iter() {
            for (i, n) in r.by_status.iter().enumerate() {
                let _ = writeln!(out, "geko_requests_total{{route=\"{route}\",status=\"{}xx\"}} {n}", i + 2);
            }
        }
        out.push_str("# HELP geko_request_seconds Time to answer a request, by route.\n# TYPE geko_request_seconds histogram\n");
        for (route, r) in routes.iter() {
            for (bound, n) in BUCKETS.iter().zip(&r.buckets) {
                let _ = writeln!(out, "geko_request_seconds_bucket{{route=\"{route}\",le=\"{bound}\"}} {n}");
            }
            let total = r.buckets[BUCKETS.len()];
            let _ = writeln!(out, "geko_request_seconds_bucket{{route=\"{route}\",le=\"+Inf\"}} {total}");
            let _ = writeln!(out, "geko_request_seconds_sum{{route=\"{route}\"}} {}", r.seconds);
            let _ = writeln!(out, "geko_request_seconds_count{{route=\"{route}\"}} {total}");
        }
        out.push_str("# HELP geko_bundle_info Bundle served; no sample before the first one is loaded.\n# TYPE geko_bundle_info gauge\n");
        if let Some(version) = bundle {
            let _ = writeln!(out, "geko_bundle_info{{version=\"{version}\"}} 1");
        }
        let _ = writeln!(
            out,
            "# HELP geko_uptime_seconds Time since the server started.\n# TYPE geko_uptime_seconds gauge\ngeko_uptime_seconds {}",
            self.started.elapsed().as_secs()
        );
        out
    }
}

/// Middleware recording every routed request under its route pattern (`/v1/place/{id}`, not each
/// id), so that the number of series stays bounded.
pub async fn track(metrics: axum::extract::State<std::sync::Arc<Metrics>>, request: Request, next: Next) -> Response {
    let route = request.extensions().get::<MatchedPath>().map(|p| p.as_str().to_owned());
    let started = Instant::now();
    let response = next.run(request).await;
    if let Some(route) = route {
        metrics.record(&route, response.status().as_u16(), started.elapsed().as_secs_f64());
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_counters_and_histograms() {
        let m = Metrics::default();
        m.record("/v1/search", 200, 0.003);
        m.record("/v1/search", 400, 0.0005);
        let text = m.render(Some("v1"));
        assert!(text.contains("geko_requests_total{route=\"/v1/search\",status=\"2xx\"} 1"));
        assert!(text.contains("geko_requests_total{route=\"/v1/search\",status=\"4xx\"} 1"));
        assert!(text.contains("geko_request_seconds_bucket{route=\"/v1/search\",le=\"0.001\"} 1"));
        assert!(text.contains("geko_request_seconds_bucket{route=\"/v1/search\",le=\"0.005\"} 2"));
        assert!(text.contains("geko_request_seconds_count{route=\"/v1/search\"} 2"));
        assert!(text.contains("geko_bundle_info{version=\"v1\"} 1"));
    }
}
