//! What the commands of the host do with the active modules: build each one's part of the bundle,
//! publish it, and write the report of the build; and, for `geko up`, build again when a module
//! asks for it.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use geko_module_sdk::bundle::{Artifact, BundleManifest, BundleModule, LicenseNotice};
use geko_module_sdk::module::{BundlePart, Module, ModuleName};
use geko_module_sdk::stages::StageLog;
use geko_module_sdk::{GekoError, Result};
use serde::Serialize;

use crate::bundle::{Keep, Published};
use crate::bundle_store::{self, Build};
use crate::config::Config;
use crate::progress;

/// Time and peak memory of each build stage, and the size of the bundle (BACKLOG_CI 23–24),
/// written beside the data so that a later build of the same data can be compared with it.
pub const REPORT: &str = "build_report.json";

#[derive(Serialize, Default)]
struct BundleSize {
    version: String,
    bytes: u64,
    /// Files under a `hot/` directory: the part pinned in memory (PROJECT.md §9).
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

/// Builds the part of every module, then publishes a bundle with them all, unless nothing changed
/// since the current one. With `keep_others` (a build of some modules, `--module`), the bundle also
/// keeps what the current one has of the others. A module whose build fails holds back no other:
/// the bundle keeps the part it had, and the build ends in that error once the rest is published.
pub fn build(config: &Config, modules: &[Arc<dyn Module>], force: bool, refresh: bool, keep_others: bool) -> Result<String> {
    let mut out = String::new();
    let (mut stages, mut extra) = (Vec::new(), serde_json::Map::new());
    let (mut entries, mut parts, mut licenses): (Vec<BundleModule>, Vec<BundlePart>, Vec<LicenseNotice>) = Default::default();
    let mut failed: Vec<(ModuleName, GekoError)> = Vec::new();
    let _building = progress::Building::start();
    let ticker = progress::Ticker::start(modules);
    for (i, m) in modules.iter().enumerate() {
        ticker.module.store(i, Ordering::Relaxed);
        progress::module(m.describe().name.to_string());
        let built = match m.build(force, refresh) {
            Ok(built) => built,
            Err(e) => {
                let name = m.describe().name;
                tracing::error!(module = %name, error = %e, "the build of the module failed; the others go on");
                failed.push((name, e));
                continue;
            }
        };
        progress::end();
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
        return if failed.is_empty() { Ok(out) } else { Err(failure(&failed, &out)) };
    }
    let mut log = StageLog::new();
    let store = bundle_store::open(config);
    let bundles = store.dir().to_owned();
    let names: Vec<ModuleName> = failed.iter().map(|(name, _)| name.clone()).collect();
    let keep = match keep_others {
        true => Keep::Others,
        false if names.is_empty() => Keep::Nothing,
        false => Keep::These(&names),
    };
    let published = store.publish(Build { parts: &parts, modules: entries, licenses, force, keep })?;
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
    for m in modules.iter().filter(|m| !names.contains(&m.describe().name)) {
        out.push_str(&m.published()?);
    }
    write_report(&config.data.dir, stages, &bundle_size(&manifest.version, &manifest.artifacts), extra)?;
    if failed.is_empty() { Ok(out) } else { Err(failure(&failed, &out)) }
}

/// The error of a build where some modules failed, with what the others did: an error is all that
/// reaches who ran it.
fn failure(failed: &[(ModuleName, GekoError)], out: &str) -> GekoError {
    let why: Vec<String> = failed.iter().map(|(name, e)| format!("{name}: {e}")).collect();
    GekoError::Other(format!("{out}the build failed for {}", why.join("; ")).into())
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
pub fn regenerate(config: &Config, modules: &[Arc<dyn Module>], force: bool, refresh: bool, keep_others: bool) {
    let (mut force, mut refresh) = (force, refresh);
    loop {
        match build(config, modules, force, refresh, keep_others) {
            Ok(out) if progress::verbose() => print!("{out}"),
            Ok(out) => match out.lines().find(|l| l.starts_with("published bundle ") || l.starts_with("bundle ") && l.contains(" is up to date")) {
                Some(line) => tracing::info!("{line}"),
                None => tracing::info!("built: nothing to publish"),
            },
            Err(e) => tracing::error!(error = %e, "build failed; what did not build stays as it was"),
        }
        (force, refresh) = (false, false);
        let Some(wait) = modules.iter().filter_map(|m| m.next_build()).min() else { return };
        tracing::info!(in_hours = wait.as_secs() / 3600, "next build, when a module asks for it (the geocoder: [data] max_age_days)");
        std::thread::sleep(wait.max(Duration::from_secs(1)));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use geko_module_sdk::bundle::current_version;
    use geko_module_sdk::module::{Built, CONTRACT, Description};

    use super::*;

    /// A module that writes `content` as its part, or fails without one.
    struct Fake {
        name: &'static str,
        content: Option<&'static str>,
        work: PathBuf,
    }

    impl Module for Fake {
        fn describe(&self) -> Description {
            Description {
                name: ModuleName::new(self.name).unwrap(),
                version: "1.0.0".into(),
                contract: CONTRACT,
                routes: vec![],
                openapi: None,
                attribution: None,
                commands: vec![],
            }
        }

        fn build(&self, _force: bool, _refresh: bool) -> Result<Built> {
            let content = self.content.ok_or_else(|| GekoError::Data(format!("{} cannot build", self.name)))?;
            let from = self.work.join(self.name);
            fs::create_dir_all(&from)?;
            fs::write(from.join("data.bin"), content)?;
            Ok(Built { parts: vec![BundlePart { from, to: self.name.into() }], ..Default::default() })
        }
    }

    fn modules(work: &Path, list: &[(&'static str, Option<&'static str>)]) -> Vec<Arc<dyn Module>> {
        list.iter().map(|(name, content)| Arc::new(Fake { name, content: *content, work: work.to_owned() }) as Arc<dyn Module>).collect()
    }

    fn config(dir: &Path) -> Config {
        let mut config = Config::default();
        config.data.dir = dir.to_owned();
        config
    }

    fn published(config: &Config) -> (PathBuf, Vec<String>) {
        let dir = bundle_store::open(config).dir().to_owned();
        let root = dir.join(current_version(&dir).unwrap());
        let manifest = BundleManifest::read(&root).unwrap();
        (root, manifest.modules.iter().map(|m| m.name.to_string()).collect())
    }

    /// Two versions published within a second would have the same name.
    fn next_second() {
        std::thread::sleep(Duration::from_millis(1100));
    }

    #[test]
    fn a_module_that_fails_to_build_holds_back_no_other() {
        let tmp = tempfile::tempdir().unwrap();
        let config = config(tmp.path());
        let work = tmp.path().join("work");
        build(&config, &modules(&work, &[("first", Some("1")), ("second", Some("1")), ("off", Some("1"))]), false, false, false).unwrap();

        // The first fails, the second comes after it and has news, and the last was turned off.
        next_second();
        let err = build(&config, &modules(&work, &[("first", None), ("second", Some("2"))]), false, false, false).unwrap_err().to_string();
        assert!(err.contains("first cannot build") && err.contains("published bundle"), "it fails, and says what it published: {err}");
        let (root, names) = published(&config);
        assert_eq!(names, ["first", "second"], "the part of the one that failed stays, not that of one turned off");
        assert_eq!(fs::read(root.join("second/data.bin")).unwrap(), b"2");
        assert_eq!(fs::read(root.join("first/data.bin")).unwrap(), b"1");
    }

    #[test]
    fn when_every_module_fails_nothing_is_published() {
        let tmp = tempfile::tempdir().unwrap();
        let config = config(tmp.path());
        let work = tmp.path().join("work");
        build(&config, &modules(&work, &[("first", Some("1"))]), false, false, false).unwrap();
        let (before, _) = published(&config);
        next_second();
        assert!(build(&config, &modules(&work, &[("first", None)]), false, false, false).is_err());
        assert_eq!(published(&config).0, before);
    }

    #[test]
    fn a_first_build_publishes_what_built() {
        let tmp = tempfile::tempdir().unwrap();
        let config = config(tmp.path());
        let work = tmp.path().join("work");
        assert!(build(&config, &modules(&work, &[("first", None), ("second", Some("1"))]), false, false, false).is_err());
        assert_eq!(published(&config).1, ["second"]);
    }
}
