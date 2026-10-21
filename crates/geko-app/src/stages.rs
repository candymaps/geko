//! Time and peak memory of each build stage, and the size of the bundle (BACKLOG_CI 23–24),
//! written beside the data as `build_report.json` so that a later build of the same data can be
//! compared with it.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use geko_core::Result;
use serde::Serialize;

pub const REPORT: &str = "build_report.json";

#[derive(Serialize)]
struct StageRecord {
    stage: String,
    elapsed_ms: u64,
    /// Peak resident memory of the process during the stage; None where /proc is missing.
    peak_mb: Option<u64>,
    reused: bool,
}

#[derive(Serialize, Default)]
pub struct BundleSize {
    pub version: String,
    pub bytes: u64,
    /// Files under a `hot/` directory: the part pinned in memory (PROJECT.md §9.6).
    pub hot_bytes: u64,
}

pub struct StageLog {
    stages: Vec<StageRecord>,
    started: Instant,
}

/// Peak resident memory of this process since the last reset, in MB (Linux).
fn peak_mb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    Some(line.split_whitespace().nth(1)?.parse::<u64>().ok()? / 1024)
}

/// Resets the peak to the current resident memory, so the next peak is the stage's own.
fn reset_peak() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

impl StageLog {
    pub fn new() -> Self {
        reset_peak();
        Self { stages: Vec::new(), started: Instant::now() }
    }

    /// Records the stage that ran since the previous one, and starts the next.
    pub fn done(&mut self, stage: impl Into<String>, reused: bool) {
        self.stages.push(StageRecord {
            stage: stage.into(),
            elapsed_ms: self.started.elapsed().as_millis() as u64,
            peak_mb: peak_mb(),
            reused,
        });
        reset_peak();
        self.started = Instant::now();
    }

    /// Writes the report, with the version of the pack of each country it was built with.
    pub fn write(&self, dir: &Path, bundle: &BundleSize, packs: &BTreeMap<String, u32>) -> Result<()> {
        let report = serde_json::json!({ "stages": self.stages, "bundle": bundle, "packs": packs });
        std::fs::write(dir.join(REPORT), serde_json::to_vec_pretty(&report).map_err(|e| geko_core::GekoError::Other(e.into()))?)?;
        Ok(())
    }
}

pub fn bundle_size(version: &str, artifacts: &[geko_core::bundle::Artifact]) -> BundleSize {
    let hot = |path: &str| path.split('/').any(|part| part == "hot");
    BundleSize {
        version: version.to_owned(),
        bytes: artifacts.iter().map(|a| a.size).sum(),
        hot_bytes: artifacts.iter().filter(|a| hot(&a.path)).map(|a| a.size).sum(),
    }
}
