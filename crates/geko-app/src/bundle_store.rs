//! Where bundles are published to and served from (docs/OPERATOR_MODE_PLAN.md, «`single` usa el
//! mismo camino que el operador»). A build publishes a bundle into a store and a server looks in it
//! for the newest one and reloads: the same two calls in the single mode, in the operator mode (a
//! shared volume, or a mirror over HTTP) and, later, in an object store.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use geko_module_sdk::Result;
use geko_module_sdk::bundle::{BundleModule, LicenseNotice};
use geko_module_sdk::download::Downloader;
use geko_module_sdk::module::BundlePart;

use crate::bundle::{self, Published};
use crate::config::Config;

/// What a build publishes.
pub struct Build<'a> {
    pub parts: &'a [BundlePart],
    pub modules: Vec<BundleModule>,
    pub licenses: Vec<LicenseNotice>,
    /// Publish even if the current version has the same content.
    pub force: bool,
    /// Keep what the current version has of the modules not in `modules`: the build of one module
    /// (`--module`) does not drop the others'.
    pub keep_others: bool,
}

pub trait BundleStore: Send + Sync {
    /// The directory a server loads bundles from.
    fn dir(&self) -> &Path;

    /// Publishes a new version made of `build.parts`, unless the current one was published from the
    /// same content.
    fn publish(&self, build: Build) -> Result<Published>;

    /// Brings the newest published version to where `dir` reads it: the version that arrived, or
    /// `None` when `dir` already has it.
    fn sync(&self) -> Result<Option<String>>;

    /// Where bundles are, for the logs.
    fn describe(&self) -> String;
}

/// A directory of this machine: the data directory of the single mode, or a volume that the
/// builds and the servers of the operator mode share. A build writes in it and a server reads from
/// it, so there is nothing to bring.
pub struct Local {
    dir: PathBuf,
    keep: usize,
}

impl Local {
    pub fn new(dir: PathBuf, keep: usize) -> Self {
        Self { dir, keep }
    }
}

impl BundleStore for Local {
    fn dir(&self) -> &Path {
        &self.dir
    }

    fn publish(&self, build: Build) -> Result<Published> {
        bundle::publish_incremental(&self.dir, build.parts, build.modules, build.licenses, self.keep, build.force, build.keep_others)
    }

    fn sync(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn describe(&self) -> String {
        format!("the directory {}", self.dir.display())
    }
}

/// Bundles published on another machine, which an HTTP server mirrors: each sync pulls into the
/// local directory the version that is current there, downloading only the artifacts that changed.
pub struct Mirror {
    local: Local,
    url: String,
    downloader: Arc<dyn Downloader>,
}

impl Mirror {
    pub fn new(url: String, local: Local, downloader: Arc<dyn Downloader>) -> Self {
        Self { local, url, downloader }
    }
}

impl BundleStore for Mirror {
    fn dir(&self) -> &Path {
        self.local.dir()
    }

    /// A machine that pulls bundles may still build one of its own, into its local directory.
    fn publish(&self, build: Build) -> Result<Published> {
        self.local.publish(build)
    }

    fn sync(&self) -> Result<Option<String>> {
        bundle::pull(&self.url, self.local.dir(), self.downloader.as_ref(), self.local.keep)
    }

    fn describe(&self) -> String {
        format!("the mirror {} (pulled into {})", self.url, self.local.dir.display())
    }
}

/// The store `[serve] bundles_url` and `[build] keep` ask for, in `[data] dir`.
pub fn open(config: &Config) -> Arc<dyn BundleStore> {
    let local = Local::new(config.bundles_dir(), config.build.keep);
    if config.serve.bundles_url.is_empty() {
        return Arc::new(local);
    }
    // Bundles are not sources: a serving machine pulls them even when it never downloads data.
    let downloader = Arc::new(geko_fetch::HttpDownloader::new(geko_fetch::HttpDownloaderConfig::default()));
    Arc::new(Mirror::new(config.serve.bundles_url.clone(), local, downloader))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use geko_module_sdk::bundle::{BundleManifest, current_version};
    use geko_module_sdk::download::{DownloadOutcome, DownloadRequest};
    use geko_module_sdk::module::ModuleName;

    use super::*;

    /// Serves the files of a local "remote" bundles directory, as an HTTP mirror would.
    struct Remote(PathBuf);

    impl Downloader for Remote {
        fn fetch(&self, req: &DownloadRequest, dest: &Path) -> Result<DownloadOutcome> {
            let rel = req.url.strip_prefix("mirror://root/").expect("mirror url");
            fs::copy(self.0.join(rel), dest)?;
            Ok(DownloadOutcome { downloaded: true, sha256: String::new(), size: fs::metadata(dest)?.len() })
        }
    }

    /// The part of the module `name`, written in `work` under the directory it publishes in (`tiles`
    /// for tiles, `segments` for the geocoder), with `content` in its file.
    fn built(work: &Path, name: &str, content: &[u8]) -> (Vec<BundlePart>, BundleModule) {
        let top = if name == "tiles" { "tiles" } else { "segments" };
        let from = work.join(name);
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("data.bin"), content).unwrap();
        let module = BundleModule { name: ModuleName::new(name).unwrap(), version: "1.0.0".into(), contract: 1, meta: serde_json::Value::Null };
        (vec![BundlePart { from, to: top.into() }], module)
    }

    fn build(parts: &[BundlePart], modules: Vec<BundleModule>, keep_others: bool) -> Build<'_> {
        Build { parts, modules, licenses: vec![], force: false, keep_others }
    }

    /// Versions are named by the second: wait for a new name before a new publication.
    fn next_second() {
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }

    #[test]
    fn a_local_store_is_published_into_and_read_from_the_same_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Local::new(tmp.path().join("bundles"), 1);
        let (parts, module) = built(&tmp.path().join("work"), "tiles", b"abc");
        let Published::New(manifest) = store.publish(build(&parts, vec![module.clone()], false)).unwrap() else { panic!("first build publishes") };
        assert_eq!(current_version(store.dir()), Some(manifest.version.clone()));
        assert_eq!(store.sync().unwrap(), None, "nothing to bring: it is where the server reads");
        assert!(matches!(store.publish(build(&parts, vec![module], false)).unwrap(), Published::UpToDate(_)));
    }

    #[test]
    fn a_mirror_brings_what_another_store_published() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = Local::new(tmp.path().join("remote"), 1);
        let (parts, module) = built(&tmp.path().join("work"), "tiles", b"abc");
        let Published::New(manifest) = remote.publish(build(&parts, vec![module], false)).unwrap() else { panic!("first build publishes") };

        let mirror = Mirror::new("mirror://root".into(), Local::new(tmp.path().join("served"), 1), Arc::new(Remote(remote.dir().to_owned())));
        assert_eq!(current_version(mirror.dir()), None);
        assert_eq!(mirror.sync().unwrap(), Some(manifest.version.clone()));
        assert_eq!(current_version(mirror.dir()), Some(manifest.version.clone()));
        assert_eq!(fs::read(mirror.dir().join(&manifest.version).join("tiles/data.bin")).unwrap(), b"abc");
        assert_eq!(mirror.sync().unwrap(), None, "already current");
    }

    #[test]
    fn the_build_of_one_module_keeps_the_part_of_the_others() {
        let tmp = tempfile::tempdir().unwrap();
        let (work, store) = (tmp.path().join("work"), Local::new(tmp.path().join("bundles"), 5));
        let (geocoder_parts, geocoder) = built(&work, "geocoder", b"streets");
        let (tiles_parts, tiles) = built(&work, "tiles", b"v1");
        let parts = vec![BundlePart { from: geocoder_parts[0].from.clone(), to: "segments".into() }, BundlePart { from: tiles_parts[0].from.clone(), to: "tiles".into() }];
        let Published::New(first) = store.publish(build(&parts, vec![geocoder.clone(), tiles.clone()], false)).unwrap() else { panic!("first build publishes") };

        // Tiles builds alone, with new content: the bundle has it and still the geocoder's.
        next_second();
        built(&work, "tiles", b"v2");
        let Published::New(second) = store.publish(build(&tiles_parts, vec![tiles.clone()], true)).unwrap() else { panic!("tiles changed") };
        assert_ne!(second.version, first.version);
        let names: Vec<&str> = second.modules.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["geocoder", "tiles"]);
        let root = store.dir().join(&second.version);
        assert_eq!(fs::read(root.join("tiles/data.bin")).unwrap(), b"v2");
        assert_eq!(fs::read(root.join("segments/data.bin")).unwrap(), b"streets", "kept, from the version before");
        assert_eq!(BundleManifest::read(&root).unwrap(), second);
        let paths: Vec<&str> = second.artifacts.iter().map(|a| a.path.as_str()).collect();
        assert_eq!(paths.len(), 2, "{paths:?}");

        // Nothing changed for either: not published again.
        assert!(matches!(store.publish(build(&tiles_parts, vec![tiles.clone()], true)).unwrap(), Published::UpToDate(_)));

        // Without keeping the others, as `geko build` with every module active (a module that
        // was turned off in the configuration leaves the bundle).
        next_second();
        built(&work, "tiles", b"v3");
        let Published::New(third) = store.publish(build(&tiles_parts, vec![tiles], false)).unwrap() else { panic!("tiles changed") };
        assert_eq!(third.modules.len(), 1);
        assert!(!store.dir().join(&third.version).join("segments").exists());
    }

    #[test]
    fn the_first_build_of_one_module_has_nothing_to_keep() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Local::new(tmp.path().join("bundles"), 1);
        let (parts, tiles) = built(&tmp.path().join("work"), "tiles", b"abc");
        let Published::New(first) = store.publish(build(&parts, vec![tiles], true)).unwrap() else { panic!("first build publishes") };
        assert_eq!(first.modules.len(), 1);
    }

    #[test]
    fn two_builds_do_not_publish_at_once() {
        let tmp = tempfile::tempdir().unwrap();
        let bundles = tmp.path().join("bundles");
        let held = crate::bundle::PublishLock::acquire(&bundles).unwrap();
        let waiting = {
            let bundles = bundles.clone();
            std::thread::spawn(move || crate::bundle::PublishLock::acquire(&bundles).map(|_| std::time::Instant::now()))
        };
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(!waiting.is_finished(), "waits for the build that holds it");
        let released = std::time::Instant::now();
        drop(held);
        assert!(waiting.join().unwrap().unwrap() >= released, "gets it once it is released");
    }
}
