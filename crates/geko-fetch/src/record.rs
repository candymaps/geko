use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use geko_core::Result;
use serde::{Deserialize, Serialize};

/// Sidecar `<file>.raw.json` describing a downloaded file (PROJECT.md §7.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawRecord {
    pub url: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub sha256: String,
    pub size: u64,
    pub fetched_at_unix: u64,
}

pub fn sidecar(path: &Path) -> PathBuf {
    with_suffix(path, ".raw.json")
}

pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The record of `file`, if both exist and agree on the size.
pub fn read(file: &Path) -> Option<RawRecord> {
    let size = fs::metadata(file).ok()?.len();
    let record: RawRecord = serde_json::from_slice(&fs::read(sidecar(file)).ok()?).ok()?;
    (record.size == size).then_some(record)
}

/// The record of a file removed after publishing: the record is left, the file is not.
pub fn read_removed(file: &Path) -> Option<RawRecord> {
    if file.exists() {
        return None;
    }
    serde_json::from_slice(&fs::read(sidecar(file)).ok()?).ok()
}

pub fn write(file: &Path, record: &RawRecord) -> Result<()> {
    write_json_atomically(&sidecar(file), record)
}

pub fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = with_suffix(path, ".tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(value).map_err(|e| geko_core::GekoError::Other(e.into()))?)?;
    f.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}
