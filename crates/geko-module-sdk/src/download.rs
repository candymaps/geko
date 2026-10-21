//! The `Downloader` port (PROJECT.md §4): how a module downloads what it needs, with caching,
//! refresh policy and offline mode owned by the downloader, not by whoever asks.

use std::path::Path;

use crate::{GekoError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checksum {
    Md5(String),
    Sha256(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    pub url: String,
    pub checksum: Option<Checksum>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    /// False when the local copy was kept (up to date, refresh policy or offline mode).
    pub downloaded: bool,
    pub sha256: String,
    pub size: u64,
}

/// Owns caching, refresh policy and offline mode, so providers only say what they need.
pub trait Downloader: Send + Sync {
    /// Writes to `dest` atomically: on error, `dest` is left untouched.
    fn fetch(&self, req: &DownloadRequest, dest: &Path) -> Result<DownloadOutcome>;

    /// Whether this build asks the sources for changes to what it already downloaded. When not
    /// (the data are not due for a check, `[data] max_age_days`), a source keeps every download
    /// it has, without contacting anyone, and downloads only what is missing.
    fn checks_for_updates(&self) -> bool {
        true
    }

    /// Whether a download removed after a bundle was published (`[data] keep_downloads = false`),
    /// of which only its record is left, counts as kept. It does, except when a staging that
    /// reads it has to be repeated: then the downloader brings it back.
    fn keeps_removed(&self) -> bool {
        true
    }

    /// Size in bytes of a remote file, for formats read in parts (a Parquet footer and some of
    /// its row groups) rather than whole.
    fn remote_size(&self, url: &str) -> Result<u64> {
        Err(GekoError::Config(format!("this downloader cannot read {url} in parts")))
    }

    /// Bytes `start..start + len` of a remote file. Not cached: the caller keeps what it reads.
    fn fetch_range(&self, url: &str, start: u64, len: u64) -> Result<Vec<u8>> {
        let _ = (start, len);
        Err(GekoError::Config(format!("this downloader cannot read {url} in parts")))
    }
}
