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

pub trait BundleStore: Send + Sync {
    /// The directory a server loads bundles from.
    fn dir(&self) -> &Path;

    /// Publishes a new version made of `parts`, unless the current one was published from the same
    /// content (`force` publishes it anyway).
    fn publish(&self, parts: &[BundlePart], modules: Vec<BundleModule>, licenses: Vec<LicenseNotice>, force: bool) -> Result<Published>;

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

    fn publish(&self, parts: &[BundlePart], modules: Vec<BundleModule>, licenses: Vec<LicenseNotice>, force: bool) -> Result<Published> {
        bundle::publish_incremental(&self.dir, parts, modules, licenses, self.keep, force)
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
    fn publish(&self, parts: &[BundlePart], modules: Vec<BundleModule>, licenses: Vec<LicenseNotice>, force: bool) -> Result<Published> {
        self.local.publish(parts, modules, licenses, force)
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

    use geko_module_sdk::bundle::current_version;
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

    fn built(dir: &Path) -> (Vec<BundlePart>, Vec<BundleModule>) {
        fs::create_dir_all(dir.join("tiles")).unwrap();
        fs::write(dir.join("tiles/tiles.bin"), b"abc").unwrap();
        let parts = vec![BundlePart { from: dir.join("tiles"), to: "tiles".into() }];
        let module = BundleModule { name: ModuleName::new("tiles").unwrap(), version: "1.0.0".into(), contract: 1, meta: serde_json::Value::Null };
        (parts, vec![module])
    }

    #[test]
    fn a_local_store_is_published_into_and_read_from_the_same_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Local::new(tmp.path().join("bundles"), 1);
        let (parts, modules) = built(&tmp.path().join("work"));
        let Published::New(manifest) = store.publish(&parts, modules.clone(), vec![], false).unwrap() else { panic!("first build publishes") };
        assert_eq!(current_version(store.dir()), Some(manifest.version.clone()));
        assert_eq!(store.sync().unwrap(), None, "nothing to bring: it is where the server reads");
        assert!(matches!(store.publish(&parts, modules, vec![], false).unwrap(), Published::UpToDate(_)));
    }

    #[test]
    fn a_mirror_brings_what_another_store_published() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = Local::new(tmp.path().join("remote"), 1);
        let (parts, modules) = built(&tmp.path().join("work"));
        let Published::New(manifest) = remote.publish(&parts, modules, vec![], false).unwrap() else { panic!("first build publishes") };

        let mirror = Mirror::new("mirror://root".into(), Local::new(tmp.path().join("served"), 1), Arc::new(Remote(remote.dir().to_owned())));
        assert_eq!(current_version(mirror.dir()), None);
        assert_eq!(mirror.sync().unwrap(), Some(manifest.version.clone()));
        assert_eq!(current_version(mirror.dir()), Some(manifest.version.clone()));
        assert_eq!(fs::read(mirror.dir().join(&manifest.version).join("tiles/tiles.bin")).unwrap(), b"abc");
        assert_eq!(mirror.sync().unwrap(), None, "already current");
    }
}
