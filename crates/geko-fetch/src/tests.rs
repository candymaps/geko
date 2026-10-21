use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tiny_http::{Header, Response, Server};

use super::*;

#[derive(Debug, Clone)]
struct Req {
    headers: HashMap<String, String>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }
}

struct Resp {
    status: u16,
    body: Vec<u8>,
    headers: Vec<(&'static str, String)>,
}

impl Resp {
    fn ok(body: &[u8], etag: &str) -> Self {
        Self { status: 200, body: body.to_vec(), headers: vec![("ETag", etag.to_string())] }
    }

    fn status(status: u16) -> Self {
        Self { status, body: Vec::new(), headers: Vec::new() }
    }
}

struct TestServer {
    url: String,
    requests: Arc<Mutex<Vec<Req>>>,
}

impl TestServer {
    fn requests(&self) -> Vec<Req> {
        self.requests.lock().unwrap().clone()
    }
}

fn serve(handler: impl Fn(&Req, usize) -> Resp + Send + 'static) -> TestServer {
    let server = Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    thread::spawn(move || {
        for request in server.incoming_requests() {
            let req = Req {
                headers: request
                    .headers()
                    .iter()
                    .map(|h| (h.field.as_str().as_str().to_ascii_lowercase(), h.value.as_str().to_string()))
                    .collect(),
            };
            let index = {
                let mut all = seen.lock().unwrap();
                all.push(req.clone());
                all.len() - 1
            };
            let resp = handler(&req, index);
            let mut response = Response::from_data(resp.body).with_status_code(resp.status);
            for (name, value) in resp.headers {
                response = response.with_header(Header::from_bytes(name, value).unwrap());
            }
            let _ = request.respond(response);
        }
    });
    TestServer { url: format!("http://127.0.0.1:{port}/data.bin"), requests }
}

fn downloader(policy: RefreshPolicy, offline: bool) -> HttpDownloader {
    HttpDownloader::new(HttpDownloaderConfig {
        policy,
        offline,
        attempts: 3,
        backoff: Duration::from_millis(1),
        ..Default::default()
    })
}

fn request(server: &TestServer) -> DownloadRequest {
    DownloadRequest { url: server.url.clone(), checksum: None }
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[test]
fn downloads_and_records_the_file() {
    let server = serve(|_, _| Resp::ok(b"hello world", "\"v1\""));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("sub/data.bin");

    let outcome = downloader(RefreshPolicy::IfChanged, false).fetch(&request(&server), &dest).unwrap();

    assert!(outcome.downloaded);
    assert_eq!(outcome.size, 11);
    assert_eq!(outcome.sha256, sha256_hex(b"hello world"));
    assert_eq!(fs::read(&dest).unwrap(), b"hello world");
    let rec = record::read(&dest).unwrap();
    assert_eq!(rec.etag.as_deref(), Some("\"v1\""));
    assert_eq!(rec.url, server.url);
    assert!(!record::with_suffix(&dest, ".part").exists());
}

#[test]
fn unchanged_remote_is_not_downloaded_again() {
    let server = serve(|req, _| match req.header("If-None-Match") {
        Some("\"v1\"") => Resp::status(304),
        _ => Resp::ok(b"hello", "\"v1\""),
    });
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("data.bin");
    let dl = downloader(RefreshPolicy::IfChanged, false);

    dl.fetch(&request(&server), &dest).unwrap();
    let second = dl.fetch(&request(&server), &dest).unwrap();

    assert!(!second.downloaded);
    assert_eq!(second.sha256, sha256_hex(b"hello"));
    assert_eq!(server.requests()[1].header("If-None-Match"), Some("\"v1\""));
}

#[test]
fn changed_remote_is_downloaded_again() {
    let server = serve(|_, i| if i == 0 { Resp::ok(b"old", "\"v1\"") } else { Resp::ok(b"new!", "\"v2\"") });
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("data.bin");
    let dl = downloader(RefreshPolicy::IfChanged, false);

    dl.fetch(&request(&server), &dest).unwrap();
    let second = dl.fetch(&request(&server), &dest).unwrap();

    assert!(second.downloaded);
    assert_eq!(fs::read(&dest).unwrap(), b"new!");
    assert_eq!(record::read(&dest).unwrap().etag.as_deref(), Some("\"v2\""));
}

#[test]
fn never_policy_keeps_local_copy_without_network() {
    let server = serve(|_, _| Resp::ok(b"hello", "\"v1\""));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("data.bin");
    let dl = downloader(RefreshPolicy::Never, false);

    dl.fetch(&request(&server), &dest).unwrap();
    assert!(!dl.fetch(&request(&server), &dest).unwrap().downloaded);
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn always_policy_sends_no_validators() {
    let server = serve(|_, _| Resp::ok(b"hello", "\"v1\""));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("data.bin");
    let dl = downloader(RefreshPolicy::Always, false);

    dl.fetch(&request(&server), &dest).unwrap();
    assert!(dl.fetch(&request(&server), &dest).unwrap().downloaded);
    assert_eq!(server.requests()[1].header("If-None-Match"), None);
}

#[test]
fn offline_mode_never_touches_the_network() {
    let server = serve(|_, _| Resp::ok(b"hello", "\"v1\""));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("data.bin");

    let err = downloader(RefreshPolicy::IfChanged, true).fetch(&request(&server), &dest).unwrap_err();
    assert!(err.to_string().contains("offline mode"), "{err}");
    assert!(server.requests().is_empty());

    downloader(RefreshPolicy::IfChanged, false).fetch(&request(&server), &dest).unwrap();
    assert!(!downloader(RefreshPolicy::Always, true).fetch(&request(&server), &dest).unwrap().downloaded);
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn retries_server_errors() {
    let server = serve(|_, i| if i < 2 { Resp::status(503) } else { Resp::ok(b"finally", "\"v1\"") });
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("data.bin");

    let outcome = downloader(RefreshPolicy::IfChanged, false).fetch(&request(&server), &dest).unwrap();

    assert!(outcome.downloaded);
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn gives_up_after_the_last_attempt() {
    let server = serve(|_, _| Resp::status(503));
    let dir = tempfile::tempdir().unwrap();

    let err = downloader(RefreshPolicy::IfChanged, false).fetch(&request(&server), &dir.path().join("x")).unwrap_err();

    assert!(err.to_string().contains("503"), "{err}");
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn client_errors_are_not_retried() {
    let server = serve(|_, _| Resp::status(404));
    let dir = tempfile::tempdir().unwrap();

    let err = downloader(RefreshPolicy::IfChanged, false).fetch(&request(&server), &dir.path().join("x")).unwrap_err();

    assert!(err.to_string().contains("404"), "{err}");
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn verifies_checksums() {
    let server = serve(|_, _| Resp::ok(b"hello", "\"v1\""));
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(RefreshPolicy::IfChanged, false);

    let bad = DownloadRequest { url: server.url.clone(), checksum: Some(Checksum::Md5("0".repeat(32))) };
    let dest = dir.path().join("bad.bin");
    let err = dl.fetch(&bad, &dest).unwrap_err();
    assert!(err.to_string().contains("md5 mismatch"), "{err}");
    assert!(!dest.exists());
    assert!(!record::with_suffix(&dest, ".part").exists());

    let good_md5 = hex::encode(Md5::digest(b"hello")).to_uppercase();
    let good = DownloadRequest { url: server.url.clone(), checksum: Some(Checksum::Md5(good_md5)) };
    assert!(dl.fetch(&good, &dir.path().join("good.bin")).unwrap().downloaded);

    let sha = DownloadRequest { url: server.url.clone(), checksum: Some(Checksum::Sha256(sha256_hex(b"hello"))) };
    assert!(dl.fetch(&sha, &dir.path().join("sha.bin")).unwrap().downloaded);
}

#[test]
fn resumes_an_interrupted_download() {
    let server = serve(|req, _| match (req.header("Range"), req.header("If-Range")) {
        (Some("bytes=5-"), Some("\"v1\"")) => Resp {
            status: 206,
            body: b" world".to_vec(),
            headers: vec![("ETag", "\"v1\"".into()), ("Content-Range", "bytes 5-10/11".into())],
        },
        _ => Resp::ok(b"hello world", "\"v1\""),
    });
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("data.bin");
    fs::write(record::with_suffix(&dest, ".part"), b"hello").unwrap();
    let meta = PartMeta { url: server.url.clone(), etag: Some("\"v1\"".into()), last_modified: None };
    record::write_json_atomically(&record::with_suffix(&dest, ".part.meta.json"), &meta).unwrap();

    let outcome = downloader(RefreshPolicy::IfChanged, false).fetch(&request(&server), &dest).unwrap();

    assert_eq!(fs::read(&dest).unwrap(), b"hello world");
    assert_eq!(outcome.sha256, sha256_hex(b"hello world"));
    assert_eq!(server.requests()[0].header("Range"), Some("bytes=5-"));
}

#[test]
fn restarts_when_the_remote_changed_during_resume() {
    let server = serve(|_, _| Resp::ok(b"brand new file", "\"v2\""));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("data.bin");
    fs::write(record::with_suffix(&dest, ".part"), b"stale").unwrap();
    let meta = PartMeta { url: server.url.clone(), etag: Some("\"v1\"".into()), last_modified: None };
    record::write_json_atomically(&record::with_suffix(&dest, ".part.meta.json"), &meta).unwrap();

    downloader(RefreshPolicy::IfChanged, false).fetch(&request(&server), &dest).unwrap();

    assert_eq!(fs::read(&dest).unwrap(), b"brand new file");
}

#[test]
fn failed_refresh_leaves_the_existing_copy_untouched() {
    let server = serve(|_, i| if i == 0 { Resp::ok(b"good", "\"v1\"") } else { Resp::status(404) });
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("data.bin");

    downloader(RefreshPolicy::IfChanged, false).fetch(&request(&server), &dest).unwrap();
    assert!(downloader(RefreshPolicy::Always, false).fetch(&request(&server), &dest).is_err());

    assert_eq!(fs::read(&dest).unwrap(), b"good");
    assert_eq!(record::read(&dest).unwrap().etag.as_deref(), Some("\"v1\""));
}
