//! A module that is not linked into the host (docs/MODULES_PLAN.md): every method of `Module` is a
//! message of `geko_module_sdk::wire`, carried by a `Channel`: the socket of a process of its own
//! (`process`) or the functions of a library loaded at run time (`dylib`).

use std::time::Duration;

use geko_module_sdk::module::{BundleView, Built, Description, Module, Purge, Request, Response};
use geko_module_sdk::wire::Call;
use geko_module_sdk::{GekoError, Result};
use serde::de::DeserializeOwned;

/// What the module answered to a call, and the payload of the answer.
pub type Answer = Result<(serde_json::Value, Vec<u8>)>;

/// A call that the channel did not carry to the module, and how its request is answered instead.
#[derive(Debug)]
pub struct Refused {
    pub status: u16,
    pub message: String,
    pub retry_after: Option<Duration>,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refused {}

pub fn refused(status: u16, message: String, retry_after: Option<Duration>) -> GekoError {
    GekoError::Other(Box::new(Refused { status, message, retry_after }))
}

/// How the calls reach a module.
pub trait Channel: Send + Sync {
    fn exchange(&self, call: &Call, payload: &[u8]) -> Answer;

    /// Why the module cannot answer now (`Module::unavailable`).
    fn unavailable(&self) -> Option<String> {
        None
    }

    /// After the module loaded `bundle`.
    fn loaded(&self, bundle: &BundleView) {
        let _ = bundle;
    }

    /// After the module answered its shutdown.
    fn shut_down(&self) {}
}

pub fn decode<T: DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| GekoError::Data(format!("unexpected answer of the module: {e}")))
}

pub struct Remote<C> {
    channel: C,
    description: Description,
}

impl<C: Channel> Remote<C> {
    pub fn new(channel: C, description: Description) -> Self {
        Self { channel, description }
    }

    fn ask<T: DeserializeOwned>(&self, call: Call) -> Result<T> {
        decode(self.channel.exchange(&call, &[])?.0)
    }
}

impl<C: Channel> Module for Remote<C> {
    fn describe(&self) -> Description {
        self.description.clone()
    }

    fn prepare(&self) -> Result<String> {
        self.ask(Call::Prepare)
    }

    fn plan(&self) -> Result<String> {
        self.ask(Call::Plan)
    }

    fn fetch(&self, refresh: bool) -> Result<String> {
        self.ask(Call::Fetch { refresh })
    }

    fn ready(&self) -> Result<()> {
        self.ask(Call::Ready)
    }

    fn build(&self, force: bool, refresh: bool) -> Result<Built> {
        self.ask(Call::Build { force, refresh })
    }

    fn published(&self) -> Result<String> {
        self.ask(Call::Published)
    }

    fn next_build(&self) -> Option<Duration> {
        self.ask(Call::NextBuild).unwrap_or_else(|e| {
            tracing::error!(module = %self.description.name, error = %e, "cannot ask the module when to build again; asking again in an hour");
            // Not `None`, which would stop `geko up` from ever building again.
            Some(Duration::from_secs(3600))
        })
    }

    fn purge(&self, what: Purge) -> Result<String> {
        self.ask(Call::Purge(what))
    }

    fn load(&self, bundle: &BundleView) -> Result<()> {
        self.ask::<()>(Call::Load(bundle.clone()))?;
        self.channel.loaded(bundle);
        Ok(())
    }

    fn handle(&self, mut request: Request) -> Response {
        let body = std::mem::take(&mut request.body);
        let answer = self.channel.exchange(&Call::Handle(request), &body);
        match answer.and_then(|(value, body)| Ok(Response { body, ..decode(value)? })) {
            Ok(response) => response,
            Err(GekoError::Other(e)) if e.is::<Refused>() => {
                let r = e.downcast_ref::<Refused>().expect("checked");
                let response = Response::error(r.status, &r.message);
                match r.retry_after {
                    Some(after) => response.with_header("retry-after", after.as_secs().max(1).to_string()),
                    None => response,
                }
            }
            Err(e) => Response::error(500, &e.to_string()),
        }
    }

    fn unavailable(&self) -> Option<String> {
        self.channel.unavailable()
    }

    fn metrics(&self, out: &mut String) {
        match self.ask::<String>(Call::Metrics) {
            Ok(text) => out.push_str(&text),
            Err(e) => tracing::error!(module = %self.description.name, error = %e, "no metrics of the module"),
        }
    }

    fn shutdown(&self) -> Result<()> {
        let done = self.ask::<()>(Call::Shutdown);
        self.channel.shut_down();
        done
    }

    fn command(&self, name: &str, args: &[String]) -> Result<String> {
        self.ask(Call::Command { name: name.to_owned(), args: args.to_vec() })
    }
}

/// What every channel must carry the same: the fake module and the checks of `process` and `dylib`.
#[cfg(test)]
pub mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use super::*;
    use geko_module_sdk::bundle::{BundleManifest, BundleModule, FORMAT};
    use geko_module_sdk::module::{CONTRACT, Context, DataPolicy, Method, ModuleName, Route};
    use geko_module_sdk::scope::Scope;

    #[derive(Default)]
    pub struct Fake {
        loaded: Mutex<Option<String>>,
    }

    impl Module for Fake {
        fn describe(&self) -> Description {
            Description {
                name: ModuleName::new("fake").unwrap(),
                version: "1.0.0".into(),
                contract: CONTRACT,
                routes: vec![Route { method: Method::Post, path: "v1/echo".into(), pool: true, public: false }],
                home: None,
                openapi: None,
                attribution: None,
                commands: vec![],
            }
        }

        fn plan(&self) -> Result<String> {
            Err(GekoError::Config("no scope".into()))
        }

        fn load(&self, bundle: &BundleView) -> Result<()> {
            if bundle.manifest.version == "slow" {
                std::thread::sleep(Duration::from_secs(2));
            }
            *self.loaded.lock().unwrap() = Some(bundle.manifest.version.clone());
            Ok(())
        }

        fn handle(&self, r: Request) -> Response {
            match r.query.first().map(|(k, _)| k.as_str()) {
                Some("panic") => panic!("asked to"),
                Some("exit") => std::process::exit(3),
                Some("hang") => loop {
                    std::thread::sleep(Duration::from_secs(1));
                },
                _ => {}
            }
            let loaded = self.loaded.lock().unwrap().clone().unwrap_or_default();
            let mut body = format!("{loaded}:").into_bytes();
            body.extend_from_slice(&r.body);
            Response::new(200, "application/octet-stream", body)
        }

        fn next_build(&self) -> Option<Duration> {
            Some(Duration::from_millis(1500))
        }

        fn command(&self, name: &str, args: &[String]) -> Result<String> {
            Ok(format!("{name} {}", args.join(" ")))
        }
    }

    pub fn open_fake(_: Context) -> Result<Box<dyn Module>> {
        Ok(Box::new(Fake::default()))
    }

    pub fn context() -> Context {
        Context { config: String::new(), config_file: String::new(), scope: Scope::default(), data_dir: PathBuf::from("/nonexistent"), data: DataPolicy::default() }
    }

    pub fn bundle(version: &str) -> BundleView {
        let module = BundleModule { name: ModuleName::new("fake").unwrap(), version: "1.0.0".into(), contract: CONTRACT, meta: serde_json::Value::Null };
        let manifest = BundleManifest { format: FORMAT, version: version.into(), created_unix: 0, modules: vec![module.clone()], licenses: vec![], artifacts: vec![] };
        BundleView { root: PathBuf::from("/nonexistent"), manifest, module }
    }

    pub fn echo(m: &dyn Module, query: &str, body: &[u8]) -> Response {
        let query = if query.is_empty() { vec![] } else { vec![(query.to_owned(), String::new())] };
        m.handle(Request { route: "v1/echo".into(), query, body: body.to_vec(), ..Default::default() })
    }

    pub fn every_call_crosses(m: &dyn Module) {
        assert_eq!(m.describe().name.as_str(), "fake");
        m.load(&bundle("v1")).unwrap();
        let response = echo(m, "", b"\0\xffbytes");
        assert_eq!((response.status, response.content_type.as_str()), (200, "application/octet-stream"));
        assert_eq!(response.body, b"v1:\0\xffbytes");
        assert_eq!(m.next_build(), Some(Duration::from_millis(1500)));
        assert_eq!(m.command("query", &["calle".into(), "mayor".into()]).unwrap(), "query calle mayor");
        assert!(matches!(m.plan().unwrap_err(), GekoError::Config(e) if e == "no scope"));
        assert_eq!(m.prepare().unwrap(), "", "the defaults of the trait");
    }

    pub fn answered_at_once(m: Arc<dyn Module>) {
        let threads: Vec<_> = (0..16)
            .map(|i| {
                let m = Arc::clone(&m);
                std::thread::spawn(move || {
                    for j in 0..50 {
                        let body = format!("{i}-{j}");
                        assert_eq!(echo(&*m, "", body.as_bytes()).body, format!(":{body}").into_bytes());
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
    }

    pub fn a_panic_answers_500(m: &dyn Module) {
        let response = echo(m, "panic", b"");
        assert_eq!(response.status, 500);
        assert!(String::from_utf8_lossy(&response.body).contains("asked to"));
        assert_eq!(echo(m, "", b"x").status, 200);
    }
}
