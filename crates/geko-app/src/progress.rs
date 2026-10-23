//! The progress of a build, as the host logs it: that of the SDK, and the line it logs every few
//! seconds while the modules build. Only reporting: `ci/affected.sh` counts a change to a
//! `progress.rs` as one that cannot move a result.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

pub use geko_module_sdk::progress::*;
use geko_module_sdk::module::Module;

/// Logs every few seconds, while the modules build, how far the bundle has gone: the one line of a
/// build without `--verbose`, which with it logs every step instead.
pub struct Ticker {
    pub module: Arc<AtomicUsize>,
    stop: Option<mpsc::Sender<()>>,
}

impl Ticker {
    const EVERY: Duration = Duration::from_secs(2);

    pub fn start(modules: &[Arc<dyn Module>]) -> Self {
        let module = Arc::new(AtomicUsize::new(0));
        if verbose() || modules.is_empty() {
            return Self { module, stop: None };
        }
        let (stop, stopped) = mpsc::channel::<()>();
        let (current, modules) = (Arc::clone(&module), modules.to_vec());
        std::thread::spawn(move || {
            // Each module and step counts its own: a share of the whole bundle would need to weigh
            // one step against another, which nothing measures.
            while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(Self::EVERY) {
                let i = current.load(Ordering::Relaxed);
                if let Some(p) = modules[i].progress() {
                    log(&p);
                }
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
