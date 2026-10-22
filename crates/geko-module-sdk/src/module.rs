//! The contract between geko (the host) and a module (docs/MODULES_PLAN.md). A module is opened
//! with its `Context` and then asked, through `Module`, for what each command of the host needs:
//! to prepare, plan, fetch and build its part of a bundle, and to load that part and answer the
//! requests of its routes. Every type that crosses it is plain data, so that the same contract
//! holds whether the module is linked into the host or is a library loaded at run time.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::bundle::{BundleManifest, BundleModule, LicenseNotice};
use crate::scope::Scope;
use crate::stages::StageRecord;
use crate::{GekoError, Result};

/// The version of this contract. A host lists the contracts it can speak; a module says which one
/// it implements, and is refused by a host that does not speak it.
pub const CONTRACT: u32 = 1;

/// The version a module says it is (`Description::version`): that of its crate, or
/// `GEKO_MODULE_VERSION` when it is compiled to be published (`ci/modules.sh`: `<version>-<commit>`,
/// or `<version>` in a release), the version of its package in the registry.
#[macro_export]
macro_rules! module_version {
    () => {
        match option_env!("GEKO_MODULE_VERSION") {
            Some(version) => version,
            None => env!("CARGO_PKG_VERSION"),
        }
    };
}

/// The name of a module: lowercase ASCII letters, digits and hyphens, starting with a letter. It
/// names its section of `geko.toml`, its file of configuration and the prefix of its routes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ModuleName(String);

impl ModuleName {
    pub fn new(name: &str) -> Result<Self> {
        let valid = name.len() <= 32
            && name.starts_with(|c: char| c.is_ascii_lowercase())
            && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid {
            return Err(GekoError::Config(format!("invalid module name {name:?}: lowercase letters, digits and hyphens, from a letter")));
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModuleName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ModuleName {
    type Err = GekoError;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s.trim())
    }
}

impl TryFrom<String> for ModuleName {
    type Error = GekoError;

    fn try_from(s: String) -> Result<Self> {
        Self::new(&s)
    }
}

impl From<ModuleName> for String {
    fn from(name: ModuleName) -> Self {
        name.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Method {
    Get,
    Post,
}

/// A route of a module. The host serves it under the name of the module: `v1/search` of the
/// geocoder is `/geocoder/v1/search`, and the empty path is `/geocoder/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub method: Method,
    /// Relative, with `{name}` for a part of the path the request carries (`v1/place/{id}`).
    pub path: String,
    /// CPU-bound: the host answers it on its bounded pool (one request per core at once), so that
    /// cheap routes never wait behind it. Otherwise it is answered where it arrives.
    pub pool: bool,
    /// Answered without an API key (`[serve] api_keys`): a page, not the API.
    pub public: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub about: String,
}

/// What a module is, which the host asks before anything else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Description {
    pub name: ModuleName,
    pub version: String,
    pub contract: u32,
    pub routes: Vec<Route>,
    /// The route (of `routes`) of the reference of its API, which `/docs` lists.
    pub openapi: Option<String>,
    /// What the module adds to `/v1/attribution`, under its name.
    pub attribution: Option<serde_json::Value>,
    /// Commands of its own, run as `geko <module> <command> …`.
    pub commands: Vec<CommandInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Refresh {
    Never,
    IfChanged,
    Always,
}

impl Refresh {
    pub fn as_str(self) -> &'static str {
        match self {
            Refresh::Never => "never",
            Refresh::IfChanged => "if-changed",
            Refresh::Always => "always",
        }
    }
}

/// How the data of the host may be downloaded (`[data]` of `geko.toml`), which every module
/// follows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataPolicy {
    pub refresh: Refresh,
    /// With `refresh = "if-changed"`, days between two checks of the sources, all at once.
    pub max_age_days: u64,
    pub offline: bool,
    /// Keep the downloads after a bundle is published.
    pub keep_downloads: bool,
    pub accept_licenses: bool,
    pub parallel_downloads: usize,
}

impl Default for DataPolicy {
    fn default() -> Self {
        Self { refresh: Refresh::IfChanged, max_age_days: 30, offline: false, keep_downloads: false, accept_licenses: false, parallel_downloads: 4 }
    }
}

/// What a module is opened with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Context {
    /// The text of its file of configuration (`geko-<module>.toml`), empty without one.
    pub config: String,
    /// Where that file is, to name it in errors; empty without one.
    pub config_file: String,
    /// The countries and regions of `geko.toml`, as written: each module splits them as it needs.
    pub scope: Scope,
    /// The data directory of the host (`/var/lib/geko`): downloads, work files and bundles.
    pub data_dir: PathBuf,
    pub data: DataPolicy,
}

/// A directory of a module's work files that goes into the bundle at `to` (relative to its root).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundlePart {
    pub from: PathBuf,
    pub to: String,
}

/// What a build leaves for the bundle; without parts, nothing (the module has no part of it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Built {
    /// What the build did, printed as it is.
    pub report: String,
    pub parts: Vec<BundlePart>,
    /// Kept in the entry of the module in the manifest, and handed back to it with the bundle.
    pub meta: serde_json::Value,
    pub licenses: Vec<LicenseNotice>,
    /// The stages it ran, with their time and memory, for `build_report.json`.
    pub stages: Vec<StageRecord>,
    /// Anything else for `build_report.json`, by key.
    pub report_extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Purge {
    /// Downloads and work files the configuration no longer asks for.
    Data,
    /// Downloaded data packages (the country packs of the geocoder) outside the scope; with
    /// `all`, every one.
    Packages { all: bool },
}

/// A bundle to load: where it is, its manifest and the entry of the module in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleView {
    pub root: PathBuf,
    pub manifest: BundleManifest,
    pub module: BundleModule,
}

/// A request to a route of the module.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Request {
    /// The path of the route, as the module declared it (`v1/place/{id}`).
    pub route: String,
    /// The parts of the path named in the route, decoded.
    pub params: Vec<(String, String)>,
    /// The query string, decoded, in order.
    pub query: Vec<(String, String)>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    /// Microseconds it waited in the host before reaching the module (for a free core, on the
    /// pool): part of its time, which the module counts in what it reports.
    pub waited_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    /// Other headers, such as `Retry-After`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self { status, content_type: content_type.to_owned(), body: body.into(), headers: Vec::new() }
    }

    pub fn with_header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    /// `{"error": message}`, as every route of geko answers an error.
    pub fn error(status: u16, message: &str) -> Self {
        Self::new(status, "application/json", serde_json::json!({ "error": message }).to_string())
    }
}

/// A module, as the host sees it. Each method answers one step of a command of the host; the
/// defaults are those of a module with nothing to do in that step. A module is opened once per
/// process of the host, so it keeps what one step prepares for the next.
pub trait Module: Send + Sync {
    fn describe(&self) -> Description;

    /// Before `plan`, `fetch`, `build` and `purge`: what the module needs for the scope, such as
    /// its data packages, downloaded or checked. Returns what it did.
    fn prepare(&self) -> Result<String> {
        Ok(String::new())
    }

    /// What a build would download and generate, without doing it.
    fn plan(&self) -> Result<String> {
        Ok(String::new())
    }

    /// Downloads what a build needs. `refresh`: check the sources for changes now.
    fn fetch(&self, refresh: bool) -> Result<String> {
        let _ = refresh;
        Ok(String::new())
    }

    /// Whether a build could start now (the licenses of its sources accepted…): `geko up` asks
    /// before it serves, rather than serve nothing while every build fails.
    fn ready(&self) -> Result<()> {
        Ok(())
    }

    /// Builds the part of the bundle of the module. `force`: every stage, even those whose code
    /// and inputs did not change; `refresh`: check the sources for changes now.
    fn build(&self, force: bool, refresh: bool) -> Result<Built> {
        let _ = (force, refresh);
        Ok(Built::default())
    }

    /// After the host published a bundle with what `build` left: e.g. removing downloads that are
    /// no longer needed.
    fn published(&self) -> Result<String> {
        Ok(String::new())
    }

    /// How long until `geko up` should build again; `None`: never (nothing can change).
    fn next_build(&self) -> Option<Duration> {
        None
    }

    fn purge(&self, what: Purge) -> Result<String> {
        let _ = what;
        Ok(String::new())
    }

    /// Loads its part of a bundle and switches to it atomically; requests in flight finish on the
    /// previous one.
    fn load(&self, bundle: &BundleView) -> Result<()> {
        let _ = bundle;
        Ok(())
    }

    fn handle(&self, request: Request) -> Response {
        Response::error(404, &format!("no route {}", request.route))
    }

    /// Appends its own counters to `/metrics`, in the Prometheus text format.
    fn metrics(&self, out: &mut String) {
        let _ = out;
    }

    fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    /// How far its `build` has gone, asked while it runs (`progress`).
    fn progress(&self) -> Option<crate::progress::Progress> {
        crate::progress::current()
    }

    /// Why the module cannot answer now, if it cannot, which `/health` reports: the host's side of
    /// a module, never asked of the module itself. No adapter reports one today.
    fn unavailable(&self) -> Option<String> {
        None
    }

    /// Runs one of its commands (`Description::commands`) and returns what it prints.
    fn command(&self, name: &str, args: &[String]) -> Result<String> {
        let _ = args;
        Err(GekoError::Config(format!("unknown command `{name}`")))
    }
}

/// How a module linked into the host is opened.
pub type Open = fn(Context) -> Result<Box<dyn Module>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_names() {
        assert_eq!(" geocoder ".parse::<ModuleName>().unwrap().as_str(), "geocoder");
        for bad in ["", "Geocoder", "1tiles", "tiles/x", "a b"] {
            assert!(ModuleName::new(bad).is_err(), "{bad:?}");
        }
        assert!(serde_json::from_str::<ModuleName>("\"../x\"").is_err());
    }
}
