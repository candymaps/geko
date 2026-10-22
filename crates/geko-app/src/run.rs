//! What the commands of the host do with the active modules: build each one's part of the bundle,
//! publish it, and write the report of the build; and, for `geko up`, build again when a module
//! asks for it.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use geko_module_sdk::bundle::{Artifact, BundleManifest, BundleModule, LicenseNotice};
use geko_module_sdk::module::{BundlePart, Module};
use geko_module_sdk::progress;
use geko_module_sdk::stages::StageLog;
use geko_module_sdk::{GekoError, Result};
use serde::Serialize;

use crate::bundle::{self, Published};
use crate::config::Config;

/// Time and peak memory of each build stage, and the size of the bundle (BACKLOG_CI 23–24),
/// written beside the data so that a later build of the same data can be compared with it.
pub const REPORT: &str = "build_report.json";

#[derive(Serialize, Default)]
struct BundleSize {
    version: String,
    bytes: u64,
    /// Files under a `hot/` directory: the part pinned in memory (PROJECT.md §9.6).
    hot_bytes: u64,
}

fn bundle_size(version: &str, artifacts: &[Artifact]) -> BundleSize {
    let hot = |path: &str| path.split('/').any(|part| part == "hot");
    BundleSize {
        version: version.to_owned(),
        bytes: artifacts.iter().map(|a| a.size).sum(),
        hot_bytes: artifacts.iter().filter(|a| hot(&a.path)).map(|a| a.size).sum(),
    }
}

/// Logs every few seconds, while the modules build, how far the bundle has gone: the one line of a
/// build without `--verbose`, which with it logs every step instead.
struct Ticker {
    module: Arc<AtomicUsize>,
    stop: Option<mpsc::Sender<()>>,
}

impl Ticker {
    const EVERY: Duration = Duration::from_secs(5);

    fn start(modules: &[Arc<dyn Module>]) -> Self {
        let module = Arc::new(AtomicUsize::new(0));
        if progress::verbose() || modules.is_empty() {
            return Self { module, stop: None };
        }
        let (stop, stopped) = mpsc::channel::<()>();
        let (current, modules) = (Arc::clone(&module), modules.to_vec());
        std::thread::spawn(move || {
            // A step that learns more of what it has to do (a download that starts) goes back a
            // little: the line never does.
            let mut shown = 0.0f32;
            while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(Self::EVERY) {
                let i = current.load(Ordering::Relaxed);
                let name = modules[i].describe().name;
                let p = modules[i].progress();
                shown = shown.max((i as f32 + p.as_ref().map_or(0.0, |p| p.fraction)) / modules.len() as f32);
                let step = p.map_or_else(|| name.to_string(), |p| format!("{name}: {}", p.step));
                tracing::info!(target: progress::TARGET, "building the bundle: {:.0} % ({step})", shown * 100.0);
            }
        });
        Self { module, stop: Some(stop) }
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// Builds the part of every module, then publishes a bundle with them all, unless nothing changed
/// since the current one.
pub fn build(config: &Config, modules: &[Arc<dyn Module>], force: bool, refresh: bool) -> Result<String> {
    let mut out = String::new();
    let (mut stages, mut extra) = (Vec::new(), serde_json::Map::new());
    let (mut entries, mut parts, mut licenses): (Vec<BundleModule>, Vec<BundlePart>, Vec<LicenseNotice>) = Default::default();
    let _building = progress::Building::start();
    let ticker = Ticker::start(modules);
    for (i, m) in modules.iter().enumerate() {
        ticker.module.store(i, Ordering::Relaxed);
        let built = m.build(force, refresh)?;
        out.push_str(&built.report);
        stages.extend(built.stages);
        extra.extend(built.report_extra);
        if built.parts.is_empty() {
            continue;
        }
        let d = m.describe();
        entries.push(BundleModule { name: d.name, version: d.version, contract: d.contract, meta: built.meta });
        parts.extend(built.parts);
        for l in built.licenses {
            if !licenses.iter().any(|k| k.source == l.source) {
                licenses.push(l);
            }
        }
    }
    if entries.is_empty() {
        return Ok(out);
    }
    let mut log = StageLog::new();
    let bundles = config.bundles_dir();
    let published = bundle::publish_incremental(&bundles, &parts, entries, licenses, config.build.keep, force)?;
    let manifest = match &published {
        Published::New(manifest) => {
            writeln!(
                out,
                "published bundle {} ({} files) in {} ms: {}",
                manifest.version,
                manifest.artifacts.len(),
                log.elapsed().as_millis(),
                bundles.join(&manifest.version).display()
            )
            .unwrap();
            manifest.clone()
        }
        Published::UpToDate(version) => {
            writeln!(out, "bundle {version} is up to date: {}", bundles.join(version).display()).unwrap();
            BundleManifest::read(&bundles.join(version))?
        }
    };
    log.done("publish", matches!(published, Published::UpToDate(_)));
    stages.extend(log.into_records());
    for m in modules {
        out.push_str(&m.published()?);
    }
    write_report(&config.data.dir, stages, &bundle_size(&manifest.version, &manifest.artifacts), extra)?;
    Ok(out)
}

fn write_report(dir: &Path, stages: Vec<geko_module_sdk::stages::StageRecord>, bundle: &BundleSize, extra: serde_json::Map<String, serde_json::Value>) -> Result<()> {
    let mut report = serde_json::json!({ "stages": stages, "bundle": bundle });
    for (k, v) in extra {
        report[k] = v;
    }
    std::fs::write(dir.join(REPORT), serde_json::to_vec_pretty(&report).map_err(|e| GekoError::Other(e.into()))?)?;
    Ok(())
}

/// What `geko up` does beside serving: builds now (with `force` and `refresh`, as `geko build`),
/// then again when the first module asks for it (`Module::next_build`: the geocoder, when its
/// sources are due for a check), for as long as the process lives. The server picks up each new
/// bundle with its reload, and keeps the one it serves if a build fails. When no module expects
/// anything to change, it builds once.
pub fn regenerate(config: &Config, modules: &[Arc<dyn Module>], force: bool, refresh: bool) {
    let (mut force, mut refresh) = (force, refresh);
    loop {
        match build(config, modules, force, refresh) {
            Ok(out) if progress::verbose() => print!("{out}"),
            Ok(out) => match out.lines().find(|l| l.starts_with("published bundle ") || l.starts_with("bundle ") && l.contains(" is up to date")) {
                Some(line) => tracing::info!("{line}"),
                None => tracing::info!("built: nothing to publish"),
            },
            Err(e) => tracing::error!(error = %e, "build failed; the bundle being served stays"),
        }
        (force, refresh) = (false, false);
        let Some(wait) = modules.iter().filter_map(|m| m.next_build()).min() else { return };
        tracing::info!(in_hours = wait.as_secs() / 3600, "next build, when a module asks for it (the geocoder: [data] max_age_days)");
        std::thread::sleep(wait.max(Duration::from_secs(1)));
    }
}
