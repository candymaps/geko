//! How far the build of a module has gone, for the progress line the host logs every few seconds
//! while it builds. A module marks its steps (`step`) and, within one, what it expects and what it
//! has done (`expect`, `advance`: bytes of a download, partitions…); the host asks for it with
//! `Module::progress`. With `--verbose` (`GEKO_VERBOSE`) there is no progress line: every step
//! logs its own lines instead, which without it are left out while a build runs (`shown`).
//!
//! Every process has its own: a module of its own process or library answers with its own, and a
//! module linked into the host shares the host's.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// The environment variable that `geko --verbose` sets, which the processes of the modules inherit.
pub const VERBOSE: &str = "GEKO_VERBOSE";
/// The target of the progress line, the one line at `info` that a build without `--verbose` logs.
pub const TARGET: &str = "geko::progress";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    /// From 0 to 1.
    pub fraction: f32,
    pub step: String,
}

struct Step {
    name: String,
    from: f32,
    to: f32,
}

static STEP: Mutex<Option<Step>> = Mutex::new(None);
static EXPECTED: AtomicU64 = AtomicU64::new(0);
static DONE: AtomicU64 = AtomicU64::new(0);
static BUILDING: AtomicBool = AtomicBool::new(false);
static IN_BYTES: AtomicBool = AtomicBool::new(false);

pub fn verbose() -> bool {
    static VERBOSE_SET: OnceLock<bool> = OnceLock::new();
    *VERBOSE_SET.get_or_init(|| std::env::var_os(VERBOSE).is_some_and(|v| !v.is_empty() && v != "0"))
}

/// While it lives, a build is running in this process: without `--verbose`, its `info` lines are
/// left out.
pub struct Building(());

impl Building {
    pub fn start() -> Self {
        BUILDING.store(true, Ordering::Relaxed);
        Self(())
    }
}

impl Drop for Building {
    fn drop(&mut self) {
        BUILDING.store(false, Ordering::Relaxed);
        *STEP.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// The build is now in `name`, which spans from `from` to `to` (0 to 1) of the whole.
pub fn step(name: impl Into<String>, from: f32, to: f32) {
    let mut step = STEP.lock().unwrap_or_else(|e| e.into_inner());
    IN_BYTES.store(false, Ordering::Relaxed);
    EXPECTED.store(0, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
    *step = Some(Step { name: name.into(), from, to });
}

/// As `step`, for one measured by the bytes downloaded (`expect_bytes`, `advance_bytes`), which
/// the other steps leave out: they may read a little over the network too, counted otherwise.
pub fn step_in_bytes(name: impl Into<String>, from: f32, to: f32) {
    step(name, from, to);
    IN_BYTES.store(true, Ordering::Relaxed);
}

/// A download has `bytes` more to read.
pub fn expect_bytes(bytes: u64) {
    if IN_BYTES.load(Ordering::Relaxed) {
        expect(bytes);
    }
}

/// A download read `bytes` more.
pub fn advance_bytes(bytes: u64) {
    if IN_BYTES.load(Ordering::Relaxed) {
        advance(bytes);
    }
}

/// The step has `amount` more to do.
pub fn expect(amount: u64) {
    EXPECTED.fetch_add(amount, Ordering::Relaxed);
}

/// The step did `amount` more.
pub fn advance(amount: u64) {
    DONE.fetch_add(amount, Ordering::Relaxed);
}

/// Where the build of this process is; `None` outside a step.
pub fn current() -> Option<Progress> {
    let step = STEP.lock().unwrap_or_else(|e| e.into_inner());
    let step = step.as_ref()?;
    let (expected, done) = (EXPECTED.load(Ordering::Relaxed), DONE.load(Ordering::Relaxed));
    let within = if expected == 0 { 0.0 } else { (done as f32 / expected as f32).min(1.0) };
    Some(Progress { fraction: step.from + (step.to - step.from) * within, step: step.name.clone() })
}

/// Whether to log an event: without `--verbose`, while a build runs, only warnings, errors and the
/// progress line.
pub fn shown(meta: &tracing::Metadata<'_>) -> bool {
    *meta.level() <= tracing::Level::WARN || meta.target() == TARGET || verbose() || !BUILDING.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fraction_moves_within_the_step() {
        let _building = Building::start();
        step("download", 0.2, 0.6);
        assert_eq!(current().unwrap().fraction, 0.2);
        expect(100);
        advance(50);
        let p = current().unwrap();
        assert!((p.fraction - 0.4).abs() < 1e-6 && p.step == "download", "{p:?}");
        advance(80);
        assert_eq!(current().unwrap().fraction, 0.6, "never past the step");
        drop(_building);
        assert_eq!(current(), None);
    }
}
