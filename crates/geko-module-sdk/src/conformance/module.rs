//! Conformance of `Module`: what every module must keep for the host, whatever it does, opened
//! with an empty scope and no bundle loaded. Each module runs it from its own tests, linked in; the
//! host runs it through each adapter, so that the process and the library carry the contract as
//! the module keeps it.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::bundle::{BundleManifest, BundleModule, FORMAT};
use crate::module::{BundleView, CONTRACT, Method, Module, Request, Response};

/// Requests that a client may send to any route, for a module that has loaded nothing: every one
/// must be answered, and none with 500.
fn odd_requests(route: &str, method: Method) -> Vec<Request> {
    let base = Request { route: route.to_owned(), ..Default::default() };
    let long = "x".repeat(10_000);
    let query = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut requests = vec![
        base.clone(),
        Request { query: query(&[("text", "calle mayor 1"), ("size", "5")]), ..base.clone() },
        Request { query: query(&[("text", ""), ("size", "-1"), ("lat", "nan"), ("lon", "1e999")]), ..base.clone() },
        Request { query: query(&[("text", &long), ("text", "twice")]), ..base.clone() },
        Request { query: query(&[("\u{0}", "\u{feff}"), ("emoji", "🦎")]), ..base.clone() },
    ];
    if route.contains('{') {
        requests.push(Request { params: vec![("id".into(), "../../etc/passwd".into())], ..base.clone() });
    }
    if method == Method::Post {
        requests.push(Request { content_type: Some("application/json".into()), body: b"{\"not\": [\"closed\"".to_vec(), ..base.clone() });
        requests.push(Request { content_type: Some("application/octet-stream".into()), body: vec![0xff; 4096], ..base });
    }
    requests
}

fn check_response(what: &str, response: &Response) {
    assert!((200..600).contains(&response.status), "{what}: status {}", response.status);
    assert_ne!(response.status, 500, "{what}: 500 is for a fault of the module, not for a request: {}", String::from_utf8_lossy(&response.body));
    assert!(!response.content_type.is_empty(), "{what}: no content type");
    if response.status >= 400 {
        assert!(response.content_type.starts_with("application/json"), "{what}: an error is JSON, not {}", response.content_type);
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap_or_else(|e| panic!("{what}: an error that is not JSON: {e}"));
        assert!(body.get("error").is_some_and(|e| e.is_string()), "{what}: an error is {{\"error\": message}}, not {body}");
    }
    for (name, value) in &response.headers {
        assert!(!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'), "{what}: header name {name:?}");
        assert!(!value.contains(['\r', '\n']), "{what}: header {name} spans lines");
    }
}

/// A route the host can serve under the name of the module.
fn check_route(path: &str) {
    assert!(!path.starts_with('/') && !path.ends_with('/'), "route {path:?}: relative, without a slash at either end");
    assert!(!path.contains(['?', '#', '\\']) && !path.contains("//"), "route {path:?}");
    for part in path.split('/').filter(|p| !p.is_empty()) {
        assert!(part != "." && part != "..", "route {path:?}: no dot segments");
        if part.contains(['{', '}']) {
            let name = part.strip_prefix('{').and_then(|p| p.strip_suffix('}'));
            assert!(name.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')), "route {path:?}: a parameter is a whole part, `{{name}}`");
        }
    }
}

/// What the module appends to `/metrics`: the Prometheus text format, under `geko_`.
fn check_metrics(text: &str) {
    for line in text.lines() {
        if line.is_empty() || line.starts_with("# HELP ") || line.starts_with("# TYPE ") {
            continue;
        }
        assert!(!line.starts_with('#'), "metrics: comment {line:?}");
        let (series, value) = line.rsplit_once(' ').unwrap_or_else(|| panic!("metrics: no value in {line:?}"));
        let name = series.split('{').next().unwrap_or_default();
        assert!(name.starts_with("geko_") && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b':'), "metrics: name {name:?}");
        assert!(series == name || (series.ends_with('}') && series.matches('{').count() == 1), "metrics: labels of {line:?}");
        assert!(value.parse::<f64>().is_ok(), "metrics: value of {line:?}");
    }
}

/// Checks `module`, opened with an empty scope and without a bundle loaded.
pub fn check(module: &dyn Module) {
    let d = module.describe();
    assert_eq!(d.contract, CONTRACT, "the contract of this SDK");
    assert!(!d.version.is_empty(), "a version");
    assert_eq!(module.describe(), d, "describe is the same every time");

    let mut seen = HashSet::new();
    for r in &d.routes {
        check_route(&r.path);
        assert!(seen.insert((r.method, r.path.as_str())), "route {} declared twice", r.path);
    }
    if let Some(openapi) = &d.openapi {
        assert!(d.routes.iter().any(|r| &r.path == openapi && r.method == Method::Get), "openapi {openapi:?}: one of its GET routes");
    }
    let mut commands = HashSet::new();
    for c in &d.commands {
        assert!(!c.name.is_empty() && !c.name.starts_with('-') && !c.name.contains(char::is_whitespace), "command {:?}", c.name);
        assert!(commands.insert(c.name.as_str()), "command {} declared twice", c.name);
    }
    assert!(module.command("no-such-command", &[]).is_err(), "an unknown command is an error");

    assert_eq!(module.progress(), None, "no progress outside a build");
    let mut metrics = String::new();
    module.metrics(&mut metrics);
    check_metrics(&metrics);

    let every = || d.routes.iter().flat_map(|r| odd_requests(&r.path, r.method).into_iter().map(move |q| (r.path.clone(), q)));
    for (path, request) in every() {
        check_response(&format!("{path} without a bundle"), &module.handle(request));
    }

    // A bundle without its part: loading it may fail, but leaves the module answering as before.
    let name = d.name.clone();
    let entry = BundleModule { name, version: d.version.clone(), contract: CONTRACT, meta: serde_json::Value::Null };
    let manifest = BundleManifest { format: FORMAT, version: "conformance".into(), created_unix: 0, modules: vec![entry.clone()], licenses: vec![], artifacts: vec![] };
    let empty = BundleView { root: PathBuf::from("/nonexistent/geko-conformance"), manifest, module: entry };
    let _ = module.load(&empty);
    for (path, request) in every() {
        check_response(&format!("{path} after a bundle it could not load"), &module.handle(request));
    }

    // At once, from as many threads as a pool of the host has.
    std::thread::scope(|threads| {
        for _ in 0..8 {
            threads.spawn(|| {
                for (path, request) in every() {
                    check_response(&format!("{path}, at once"), &module.handle(request));
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::{CommandInfo, Description, ModuleName, Route};

    struct Minimal;

    impl Module for Minimal {
        fn describe(&self) -> Description {
            Description {
                name: ModuleName::new("minimal").unwrap(),
                version: "1.0.0".into(),
                contract: CONTRACT,
                routes: vec![
                    Route { method: Method::Get, path: "v1/thing/{id}".into(), pool: false, public: false },
                    Route { method: Method::Post, path: "v1/batch".into(), pool: true, public: false },
                ],
                openapi: None,
                attribution: None,
                commands: vec![CommandInfo { name: "query".into(), about: "".into() }],
            }
        }

        fn handle(&self, _: Request) -> Response {
            Response::error(503, "nothing loaded").with_header("retry-after", "1")
        }

        fn metrics(&self, out: &mut String) {
            out.push_str("# TYPE geko_minimal_total counter\ngeko_minimal_total{kind=\"a\"} 3\n");
        }
    }

    #[test]
    fn a_module_that_keeps_the_contract_passes() {
        check(&Minimal);
    }

    #[test]
    #[should_panic(expected = "500")]
    fn a_500_for_a_request_fails() {
        struct Faulty;
        impl Module for Faulty {
            fn describe(&self) -> Description {
                Description { routes: vec![Route { method: Method::Get, path: "v1/x".into(), pool: false, public: false }], ..Minimal.describe() }
            }
            fn handle(&self, _: Request) -> Response {
                Response::error(500, "oops")
            }
        }
        check(&Faulty);
    }

    #[test]
    fn routes_and_metrics_are_checked() {
        for bad in ["/v1/search", "v1/search/", "v1/../x", "v1/{id", "v1/x{id}", "v1//x"] {
            assert!(std::panic::catch_unwind(|| check_route(bad)).is_err(), "{bad}");
        }
        check_route("v1/place/{id}");
        assert!(std::panic::catch_unwind(|| check_metrics("cache_hits 1\n")).is_err(), "without geko_");
        assert!(std::panic::catch_unwind(|| check_metrics("geko_x one\n")).is_err(), "a value that is no number");
    }
}
