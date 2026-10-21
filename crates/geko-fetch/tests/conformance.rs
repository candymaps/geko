use std::time::Duration;

use geko_fetch::{HttpDownloader, HttpDownloaderConfig, RefreshPolicy};

#[test]
fn conformance() {
    let dl = HttpDownloader::new(HttpDownloaderConfig { backoff: Duration::from_millis(10), ..Default::default() });
    let scratch = tempfile::tempdir().unwrap();
    geko_module_sdk::conformance::download::check(&dl, scratch.path());
}

#[test]
fn removed_downloads() {
    let config = HttpDownloaderConfig { backoff: Duration::from_millis(10), policy: RefreshPolicy::Never, ..Default::default() };
    let keep = HttpDownloader::new(config.clone());
    let restore = HttpDownloader::new(HttpDownloaderConfig { restore_removed: true, ..config });
    let scratch = tempfile::tempdir().unwrap();
    geko_module_sdk::conformance::download::check_removed(&keep, &restore, scratch.path());
}
