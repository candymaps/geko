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
    /// What it is doing now, if it says so (`doing`).
    doing: Option<String>,
    from: f32,
    to: f32,
}

static STEP: Mutex<Option<Step>> = Mutex::new(None);
static EXPECTED: AtomicU64 = AtomicU64::new(0);
static DONE: AtomicU64 = AtomicU64::new(0);
static BUILDING: AtomicBool = AtomicBool::new(false);

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
    EXPECTED.store(0, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
    *step = Some(Step { name: name.into(), doing: None, from, to });
}

/// What the step is doing now, beside its name; `None` when it is done with it.
pub fn doing(what: Option<String>) {
    if let Some(step) = STEP.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        step.doing = what;
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

/// What the step has done so far.
pub fn done() -> u64 {
    DONE.load(Ordering::Relaxed)
}

/// The step has done at least `amount` (of what it expected so far): what an item that reports
/// only part of its work with `advance` adds up to when it ends.
pub fn reach(amount: u64) {
    DONE.fetch_max(amount, Ordering::Relaxed);
}

/// What `path` takes on disk, with what is under it if a directory: of a sparse download, only what
/// was fetched of it.
pub fn disk_bytes(path: &std::path::Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_dir() {
        return std::fs::read_dir(path).map_or(0, |entries| entries.flatten().map(|e| disk_bytes(&e.path())).sum());
    }
    #[cfg(unix)]
    return std::os::unix::fs::MetadataExt::blocks(&meta) * 512;
    #[cfg(not(unix))]
    meta.len()
}

/// A reader of an input of the step, whose bytes count as done; `passes` times over the input count
/// once.
pub struct Reading<R> {
    inner: R,
    passes: u64,
    read: u64,
}

impl<R> Reading<R> {
    pub fn new(inner: R, passes: u64) -> Self {
        Self { inner, passes: passes.max(1), read: 0 }
    }
}

impl<R: std::io::Read> std::io::Read for Reading<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        let before = self.read / self.passes;
        self.read += n as u64;
        advance(self.read / self.passes - before);
        Ok(n)
    }
}

/// Where the build of this process is; `None` outside a step.
pub fn current() -> Option<Progress> {
    let step = STEP.lock().unwrap_or_else(|e| e.into_inner());
    let step = step.as_ref()?;
    let (expected, done) = (EXPECTED.load(Ordering::Relaxed), DONE.load(Ordering::Relaxed));
    let within = if expected == 0 { 0.0 } else { (done as f32 / expected as f32).min(1.0) };
    let name = match &step.doing {
        Some(doing) => format!("{}: {doing}", step.name),
        None => step.name.clone(),
    };
    Some(Progress { fraction: step.from + (step.to - step.from) * within, step: name })
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
