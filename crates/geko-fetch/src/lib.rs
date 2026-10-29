//! HTTP implementation of the `Downloader` port (PROJECT.md §7.2).

pub mod oci;
mod record;

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use geko_module_sdk::download::{Checksum, DownloadOutcome, DownloadRequest, Downloader};
use geko_module_sdk::{GekoError, Result};
use md5::Md5;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use record::{RawRecord, sidecar};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPolicy {
    /// Keep any local copy; download only what is missing.
    Never,
    /// Conditional request (ETag / Last-Modified); download only if the remote changed.
    IfChanged,
    /// Always download again.
    Always,
}

#[derive(Debug, Clone)]
pub struct HttpDownloaderConfig {
    pub policy: RefreshPolicy,
    pub offline: bool,
    /// Download again a file removed after publishing (only its record left), rather than
    /// keeping it as it is (`Downloader::keeps_removed`).
    pub restore_removed: bool,
    pub attempts: u32,
    pub backoff: Duration,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
}

impl Default for HttpDownloaderConfig {
    fn default() -> Self {
        Self {
            policy: RefreshPolicy::IfChanged,
            offline: false,
            restore_removed: false,
            attempts: 4,
            backoff: Duration::from_millis(500),
            connect_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(120),
        }
    }
}

pub struct HttpDownloader {
    agent: ureq::Agent,
    config: HttpDownloaderConfig,
}

/// Metadata of an in-progress `.part` file, so an interrupted download can resume safely.
#[derive(Debug, Serialize, Deserialize)]
struct PartMeta {
    url: String,
    etag: Option<String>,
    last_modified: Option<String>,
}

enum Attempt {
    NotModified,
    Complete { etag: Option<String>, last_modified: Option<String> },
}

struct AttemptError {
    error: GekoError,
    retryable: bool,
}

impl AttemptError {
    fn fatal(error: impl Into<GekoError>) -> Self {
        Self { error: error.into(), retryable: false }
    }

    fn retryable(error: impl Into<GekoError>) -> Self {
        Self { error: error.into(), retryable: true }
    }
}

impl HttpDownloader {
    pub fn new(config: HttpDownloaderConfig) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(config.connect_timeout)
            .timeout_read(config.read_timeout)
            .user_agent(concat!("geko/", env!("CARGO_PKG_VERSION")))
            .build();
        Self { agent, config }
    }

    fn attempt(&self, req: &DownloadRequest, dest: &Path, cached: Option<&RawRecord>) -> std::result::Result<Attempt, AttemptError> {
        let part = record::with_suffix(dest, ".part");
        let part_meta_path = record::with_suffix(dest, ".part.meta.json");

        let resume_from = match read_part_meta(&part_meta_path) {
            Some(meta) if meta.url == req.url && (meta.etag.is_some() || meta.last_modified.is_some()) => {
                fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
            }
            _ => 0,
        };
        let resume_meta = (resume_from > 0).then(|| read_part_meta(&part_meta_path)).flatten();

        let mut request = self.agent.get(&req.url);
        if let Some(rec) = cached {
            if let Some(etag) = &rec.etag {
                request = request.set("If-None-Match", etag);
            }
            if let Some(lm) = &rec.last_modified {
                request = request.set("If-Modified-Since", lm);
            }
        }
        if let Some(meta) = &resume_meta {
            request = request.set("Range", &format!("bytes={resume_from}-"));
            if let Some(validator) = meta.etag.as_ref().or(meta.last_modified.as_ref()) {
                request = request.set("If-Range", validator);
            }
        }

        let response = match request.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => {
                let error = GekoError::Data(format!("GET {} returned HTTP {code}", req.url));
                return Err(if code >= 500 || code == 429 { AttemptError::retryable(error) } else { AttemptError::fatal(error) });
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(AttemptError::retryable(GekoError::Data(format!("GET {}: {t}", req.url))));
            }
        };

        let etag = response.header("ETag").map(str::to_owned);
        let last_modified = response.header("Last-Modified").map(str::to_owned);
        let mut file = match response.status() {
            304 if cached.is_some() => return Ok(Attempt::NotModified),
            206 if resume_meta.is_some() => {
                let expected = format!("bytes {resume_from}-");
                if !response.header("Content-Range").is_some_and(|r| r.starts_with(&expected)) {
                    return Err(AttemptError::retryable(discard_part(&part, &part_meta_path, "unexpected Content-Range")));
                }
                let mut f = OpenOptions::new().append(true).open(&part).map_err(AttemptError::fatal)?;
                f.seek(SeekFrom::End(0)).map_err(AttemptError::fatal)?;
                f
            }
            200 => {
                let meta = PartMeta { url: req.url.clone(), etag: etag.clone(), last_modified: last_modified.clone() };
                record::write_json_atomically(&part_meta_path, &meta).map_err(AttemptError::fatal)?;
                File::create(&part).map_err(AttemptError::fatal)?
            }
            other => {
                return Err(AttemptError::fatal(GekoError::Data(format!("GET {} returned unexpected HTTP {other}", req.url))));
            }
        };

        io::copy(&mut response.into_reader(), &mut file)
            .map_err(|e| AttemptError::retryable(GekoError::Data(format!("GET {}: transfer interrupted: {e}", req.url))))?;
        file.sync_all().map_err(AttemptError::fatal)?;
        Ok(Attempt::Complete { etag, last_modified })
    }

    fn finalize(&self, req: &DownloadRequest, dest: &Path, etag: Option<String>, last_modified: Option<String>) -> Result<DownloadOutcome> {
        let part = record::with_suffix(dest, ".part");
        let part_meta_path = record::with_suffix(dest, ".part.meta.json");
        let digests = hash_file(&part)?;
        if let Some(expected) = &req.checksum {
            let (kind, actual, wanted) = match expected {
                Checksum::Md5(w) => ("md5", &digests.md5, w),
                Checksum::Sha256(w) => ("sha256", &digests.sha256, w),
            };
            if !actual.eq_ignore_ascii_case(wanted.trim()) {
                discard_part(&part, &part_meta_path, "checksum mismatch");
                return Err(GekoError::Data(format!(
                    "{kind} mismatch for {}: expected {wanted}, got {actual}",
                    req.url
                )));
            }
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&part, dest)?;
        let _ = fs::remove_file(&part_meta_path);
        let rec = RawRecord {
            url: req.url.clone(),
            etag,
            last_modified,
            sha256: digests.sha256.clone(),
            size: digests.size,
            fetched_at_unix: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        };
        record::write(dest, &rec)?;
        Ok(DownloadOutcome { downloaded: true, sha256: digests.sha256, size: digests.size })
    }
}

impl Downloader for HttpDownloader {
    fn fetch(&self, req: &DownloadRequest, dest: &Path) -> Result<DownloadOutcome> {
        // A file removed after publishing is kept, by its record, unless it is to be restored.
        let removed = || if self.config.restore_removed { None } else { record::read_removed(dest) };
        let cached = record::read(dest).or_else(removed).filter(|r| r.url == req.url);
        let kept = |r: &RawRecord| DownloadOutcome { downloaded: false, sha256: r.sha256.clone(), size: r.size };

        if let Some(rec) = &cached {
            if self.config.offline || self.config.policy == RefreshPolicy::Never {
                return Ok(kept(rec));
            }
        } else if self.config.offline {
            return Err(GekoError::Data(format!(
                "offline mode: {} is missing (source: {})",
                dest.display(),
                req.url
            )));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        let conditional = cached.as_ref().filter(|_| self.config.policy == RefreshPolicy::IfChanged);
        let mut delay = self.config.backoff;
        for attempt in 1..=self.config.attempts.max(1) {
            match self.attempt(req, dest, conditional) {
                Ok(Attempt::NotModified) => {
                    tracing::debug!(url = %req.url, "not modified");
                    return Ok(kept(cached.as_ref().expect("conditional request requires a cached copy")));
                }
                Ok(Attempt::Complete { etag, last_modified }) => {
                    tracing::info!(url = %req.url, dest = %dest.display(), "downloaded");
                    return self.finalize(req, dest, etag, last_modified);
                }
                Err(e) if e.retryable && attempt < self.config.attempts => {
                    tracing::warn!(url = %req.url, attempt, error = %e.error, "download failed, retrying");
                    thread::sleep(delay);
                    delay *= 2;
                }
                Err(e) => return Err(e.error),
            }
        }
        unreachable!("the last attempt always returns")
    }

    fn checks_for_updates(&self) -> bool {
        !self.config.offline && self.config.policy != RefreshPolicy::Never
    }

    fn keeps_removed(&self) -> bool {
        !self.config.restore_removed
    }

    fn remote_size(&self, url: &str) -> Result<u64> {
        self.offline_error(url)?;
        self.retrying(url, || {
            let response = self.agent.head(url).call().map_err(|e| request_error("HEAD", url, e))?;
            response
                .header("Content-Length")
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| AttemptError::fatal(GekoError::Data(format!("HEAD {url}: no Content-Length"))))
        })
    }

    fn fetch_range(&self, url: &str, start: u64, len: u64) -> Result<Vec<u8>> {
        self.offline_error(url)?;
        if len == 0 {
            return Ok(Vec::new());
        }
        self.retrying(url, || {
            let range = format!("bytes={start}-{}", start + len - 1);
            let response = self.agent.get(url).set("Range", &range).call().map_err(|e| request_error("GET", url, e))?;
            if response.status() != 206 {
                return Err(AttemptError::fatal(GekoError::Data(format!("GET {url} with {range}: HTTP {} instead of 206", response.status()))));
            }
            let mut bytes = Vec::with_capacity(len as usize);
            response.into_reader().take(len).read_to_end(&mut bytes).map_err(|e| AttemptError::retryable(GekoError::Io(e)))?;
            if bytes.len() as u64 != len {
                return Err(AttemptError::retryable(GekoError::Data(format!("GET {url} with {range}: {} bytes", bytes.len()))));
            }
            Ok(bytes)
        })
    }
}

impl HttpDownloader {
    fn offline_error(&self, url: &str) -> Result<()> {
        if self.config.offline {
            return Err(GekoError::Data(format!("offline mode: cannot read {url}")));
        }
        Ok(())
    }

    /// Runs `attempt` until it succeeds, fails for good or runs out of attempts.
    fn retrying<T>(&self, url: &str, mut attempt: impl FnMut() -> std::result::Result<T, AttemptError>) -> Result<T> {
        let mut delay = self.config.backoff;
        for n in 1..=self.config.attempts.max(1) {
            match attempt() {
                Ok(v) => return Ok(v),
                Err(e) if e.retryable && n < self.config.attempts => {
                    tracing::warn!(%url, attempt = n, error = %e.error, "request failed, retrying");
                    thread::sleep(delay);
                    delay *= 2;
                }
                Err(e) => return Err(e.error),
            }
        }
        unreachable!("the last attempt always returns")
    }
}

/// What `url` answers to a GET, as text: no retries, no proxy and no more than `max_bytes`, for what
/// the host asks of its own pods, where a slow answer is an answer.
pub fn get_text(url: &str, timeout: Duration, max_bytes: u64) -> Result<String> {
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let response = agent.get(url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => GekoError::Data(format!("GET {url} returned HTTP {code}")),
        ureq::Error::Transport(t) => GekoError::Data(format!("GET {url}: {t}")),
    })?;
    let mut body = String::new();
    response.into_reader().take(max_bytes + 1).read_to_string(&mut body).map_err(|e| GekoError::Data(format!("GET {url}: {e}")))?;
    if body.len() as u64 > max_bytes {
        return Err(GekoError::Data(format!("GET {url} answered more than {max_bytes} bytes")));
    }
    Ok(body)
}

fn request_error(method: &str, url: &str, e: ureq::Error) -> AttemptError {
    match e {
        ureq::Error::Status(code, _) => {
            let error = GekoError::Data(format!("{method} {url} returned HTTP {code}"));
            if code >= 500 || code == 429 { AttemptError::retryable(error) } else { AttemptError::fatal(error) }
        }
        ureq::Error::Transport(t) => AttemptError::retryable(GekoError::Data(format!("{method} {url}: {t}"))),
    }
}

fn read_part_meta(path: &Path) -> Option<PartMeta> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn discard_part(part: &Path, meta: &Path, reason: &str) -> GekoError {
    let _ = fs::remove_file(part);
    let _ = fs::remove_file(meta);
    GekoError::Data(format!("discarded partial download {}: {reason}", part.display()))
}

struct Digests {
    sha256: String,
    md5: String,
    size: u64,
}

fn hash_file(path: &Path) -> Result<Digests> {
    let mut reader = BufReader::with_capacity(1 << 20, File::open(path)?);
    let (mut sha, mut md5) = (Sha256::new(), Md5::new());
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        sha.update(&buf[..n]);
        md5.update(&buf[..n]);
        size += n as u64;
    }
    Ok(Digests { sha256: hex::encode(sha.finalize()), md5: hex::encode(md5.finalize()), size })
}

#[cfg(test)]
mod tests;