//! Who may call the API and how often (BACKLOG 25): CORS, API keys and a request rate per
//! client address. Everything is off by default.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::ApiError;

/// Clients tracked by the rate limit before idle ones are forgotten.
const MAX_CLIENTS: usize = 100_000;

#[derive(Debug, Clone, Default)]
pub struct Access {
    /// Origins a browser page may call the API from; `*` allows any. Empty: no CORS headers.
    pub cors_origins: Vec<String>,
    /// Keys a request must carry (`Authorization: Bearer`, `X-Api-Key` or `api_key=`). Empty: open.
    pub api_keys: Vec<String>,
    /// Sustained requests per second from one client address, and the burst allowed above it.
    /// 0: no limit.
    pub rate_per_s: f64,
    pub burst: f64,
}

pub struct Guard {
    access: Access,
    /// Paths answered without a key besides those of every geko: the pages of the modules.
    public: std::collections::HashSet<String>,
    buckets: Mutex<HashMap<IpAddr, (f64, Instant)>>,
}

impl Guard {
    pub fn new(access: Access, public: std::collections::HashSet<String>) -> Self {
        Self { access, public, buckets: Mutex::new(HashMap::new()) }
    }

    fn allowed_origin(&self, headers: &HeaderMap) -> Option<HeaderValue> {
        let origin = headers.get(header::ORIGIN)?;
        let any = self.access.cors_origins.iter().any(|o| o == "*");
        (any || self.access.cors_origins.iter().any(|o| o.as_bytes() == origin.as_bytes())).then(|| origin.clone())
    }

    fn authorized(&self, request: &Request) -> bool {
        let path = request.uri().path();
        let public = matches!(path, "/" | "/health" | "/ready") || path.starts_with("/v1/attribution") || self.public.contains(path);
        if self.access.api_keys.is_empty() || public {
            return true;
        }
        let headers = request.headers();
        let bearer = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
        let header_key = headers.get("x-api-key").and_then(|v| v.to_str().ok());
        let query_key = request.uri().query().into_iter().flat_map(|q| q.split('&')).find_map(|kv| kv.strip_prefix("api_key="));
        [bearer, header_key, query_key].into_iter().flatten().any(|k| self.access.api_keys.iter().any(|a| a == k))
    }

    /// Takes one token from the bucket of `ip`; false when it is empty.
    fn take(&self, ip: IpAddr) -> bool {
        if self.access.rate_per_s <= 0.0 {
            return true;
        }
        let capacity = self.access.burst.max(1.0);
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if buckets.len() >= MAX_CLIENTS {
            let rate = self.access.rate_per_s;
            buckets.retain(|_, (tokens, at)| *tokens + now.duration_since(*at).as_secs_f64() * rate < capacity);
        }
        let (tokens, at) = buckets.entry(ip).or_insert((capacity, now));
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64() * self.access.rate_per_s).min(capacity);
        *at = now;
        if *tokens < 1.0 {
            return false;
        }
        *tokens -= 1.0;
        true
    }
}

pub async fn check(State(guard): State<std::sync::Arc<Guard>>, request: Request, next: Next) -> Response {
    let origin = guard.allowed_origin(request.headers());
    let mut response = if request.method() == Method::OPTIONS && origin.is_some() {
        let mut r = StatusCode::NO_CONTENT.into_response();
        let h = r.headers_mut();
        h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, OPTIONS"));
        h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("authorization, x-api-key, content-type"));
        h.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400"));
        r
    } else if !guard.authorized(&request) {
        ApiError::new(StatusCode::UNAUTHORIZED, "a valid API key is required").into_response()
    } else if let Some(ConnectInfo(addr)) = request.extensions().get::<ConnectInfo<SocketAddr>>().copied()
        && !guard.take(addr.ip())
    {
        let mut r = ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too many requests").into_response();
        r.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        r
    } else {
        next.run(request).await
    };
    if let Some(origin) = origin {
        let h = response.headers_mut();
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        h.insert(header::VARY, HeaderValue::from_static("origin"));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bucket_allows_the_burst_then_the_rate() {
        let guard = Guard::new(Access { rate_per_s: 1.0, burst: 3.0, ..Default::default() }, Default::default());
        let ip: IpAddr = [10, 0, 0, 1].into();
        assert!((0..3).all(|_| guard.take(ip)), "the burst");
        assert!(!guard.take(ip), "then empty");
        assert!(guard.take([10, 0, 0, 2].into()), "each client has its own bucket");
    }

    #[test]
    fn no_limit_by_default() {
        let guard = Guard::new(Access::default(), Default::default());
        assert!((0..1000).all(|_| guard.take([10, 0, 0, 1].into())));
    }
}
