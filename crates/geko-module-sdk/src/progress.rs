//! How far the build of a module has gone, for the progress line the host logs every few seconds
//! while it builds. A module marks its steps (`step`) and, within one, what it expects and what it
//! has done (`expect`, `advance`: the downloads of every source, the partitions…), and what it
//! is working on now (`working_on`: the sources being downloaded); the host asks for it with
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

/// What the count of a step is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    /// Things that tell something by number ("7 of 20"): downloads, partitions.
    Items,
    /// Only the share means something (bytes, entities of several passes): shown as a percentage.
    Share,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// The step, with what it is working on: "downloading OSM, Overture".
    pub step: String,
    /// Which step of how many, from 1.
    pub number: u32,
    pub steps: u32,
    pub done: u64,
    /// 0 while the step does not know yet how much it has to do.
    pub expected: u64,
    pub unit: Unit,
}

impl std::fmt::Display for Progress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "step {} of {} ({})", self.number, self.steps, self.step)?;
        if self.expected == 0 {
            return Ok(());
        }
        let done = self.done.min(self.expected);
        let percent = done * 100 / self.expected;
        match self.unit {
            Unit::Items => write!(f, ": {done} of {} ({percent} %)", self.expected),
            Unit::Share => write!(f, ": {percent} %"),
        }
    }
}

struct Step {
    name: String,
    number: u32,
    steps: u32,
    unit: Unit,
    /// What it is working on now (`working_on`), in the order it started.
    on: Vec<(u64, String)>,
}

static STEP: Mutex<Option<Step>> = Mutex::new(None);
static EXPECTED: AtomicU64 = AtomicU64::new(0);
static DONE: AtomicU64 = AtomicU64::new(0);
static BUILDING: AtomicBool = AtomicBool::new(false);
static NEXT_WORK: AtomicU64 = AtomicU64::new(0);
/// The module that builds, which the progress line names.
static MODULE: Mutex<String> = Mutex::new(String::new());

fn locked() -> std::sync::MutexGuard<'static, Option<Step>> {
    STEP.lock().unwrap_or_else(|e| e.into_inner())
}

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
        // A build that failed: its step never reached 100 %.
        *locked() = None;
    }
}

/// The module that builds from now on, which the progress line names.
pub fn module(name: impl Into<String>) {
    *MODULE.lock().unwrap_or_else(|e| e.into_inner()) = name.into();
}

/// Logs the progress line of `progress`, without `--verbose`.
pub fn log(progress: &Progress) {
    if !verbose() {
        let module = MODULE.lock().unwrap_or_else(|e| e.into_inner()).clone();
        tracing::info!(target: TARGET, "building the bundle: {module}, {progress}");
    }
}

/// The build is now in step `number` of `steps`, `name`, counted in `unit`. The step before, if
/// any, ends: each one logs its line at 0 % when it starts and at 100 % when it ends, however
/// short, beside those of the host every few seconds.
pub fn step(name: impl Into<String>, number: u32, steps: u32, unit: Unit) {
    let mut step = locked();
    if let Some(before) = step.as_ref() {
        log(&ended(before));
    }
    EXPECTED.store(0, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
    let started = Step { name: name.into(), number, steps, unit, on: Vec::new() };
    log(&Progress { done: 0, expected: 1, unit: Unit::Share, ..snapshot(&started) });
    *step = Some(started);
}

/// The last step ends: the build of the module is over.
pub fn end() {
    let last = locked().take();
    if let Some(last) = last {
        log(&ended(&last));
    }
}

/// A step at its end: all it expected done, or 100 % if it counted nothing.
fn ended(step: &Step) -> Progress {
    let p = snapshot(step);
    match p.expected {
        0 => Progress { done: 1, expected: 1, unit: Unit::Share, ..p },
        expected => Progress { done: expected, ..p },
    }
}

/// While it lives, the step is working on `what` (a source being downloaded), which the line names.
pub struct WorkingOn(Option<u64>);

pub fn working_on(what: impl Into<String>) -> WorkingOn {
    let id = NEXT_WORK.fetch_add(1, Ordering::Relaxed);
    match locked().as_mut() {
        Some(step) => {
            step.on.push((id, what.into()));
            WorkingOn(Some(id))
        }
        None => WorkingOn(None),
    }
}

impl Drop for WorkingOn {
    fn drop(&mut self) {
        if let (Some(id), Some(step)) = (self.0, locked().as_mut()) {
            step.on.retain(|(i, _)| *i != id);
        }
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
    locked().as_ref().map(snapshot)
}

fn snapshot(step: &Step) -> Progress {
    let name = if step.on.is_empty() {
        step.name.clone()
    } else {
        format!("{} {}", step.name, step.on.iter().map(|(_, w)| w.as_str()).collect::<Vec<_>>().join(", "))
    };
    Progress {
        step: name,
        number: step.number,
        steps: step.steps,
        done: DONE.load(Ordering::Relaxed),
        expected: EXPECTED.load(Ordering::Relaxed),
        unit: step.unit,
    }
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
    fn a_step_counts_what_it_does_and_names_what_it_works_on() {
        let building = Building::start();
        step("downloading", 1, 5, Unit::Items);
        assert_eq!(current().unwrap().to_string(), "step 1 of 5 (downloading)");
        expect(20);
        advance(7);
        let osm = working_on("OSM");
        let overture = working_on("Overture");
        assert_eq!(current().unwrap().to_string(), "step 1 of 5 (downloading OSM, Overture): 7 of 20 (35 %)");
        drop(osm);
        advance(30);
        assert_eq!(current().unwrap().to_string(), "step 1 of 5 (downloading Overture): 20 of 20 (100 %)", "never past the end");
        drop(overture);
        step("indexing LU", 4, 5, Unit::Share);
        expect(200);
        advance(50);
        assert_eq!(current().unwrap().to_string(), "step 4 of 5 (indexing LU): 25 %");
        drop(building);
        assert_eq!(current(), None);
    }
}
