//! Time and peak memory of each build stage (BACKLOG_CI 23–24), which a module hands back with
//! what it built (`module::Built::stages`) and the host writes to `build_report.json`, so that a
//! later build of the same data can be compared with it.

use std::time::Instant;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageRecord {
    pub stage: String,
    pub elapsed_ms: u64,
    /// Peak resident memory of the process during the stage; None where /proc is missing.
    pub peak_mb: Option<u64>,
    pub reused: bool,
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

impl Default for StageLog {
    fn default() -> Self {
        Self::new()
    }
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

    /// The time of the stage running now, so far.
    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    pub fn into_records(self) -> Vec<StageRecord> {
        self.stages
    }
}
