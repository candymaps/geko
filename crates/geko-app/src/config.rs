//! `geko.toml`: what geko (the host) reads. Every module has its own file beside it
//! (`geko-<module>.toml`, docs/MODULES_PLAN.md); this one keeps the active modules, the scope,
//! the data directory and how data are downloaded, the bundles and the HTTP side of the server.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use geko_module_sdk::country::{CountryCode, RegionCode};
use geko_module_sdk::module::{DataPolicy, ModuleName, Refresh};
use geko_module_sdk::scope::Scope;
use geko_module_sdk::{GekoError, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub modules: BTreeMap<String, ModuleEntry>,
    pub scope: Vec<ScopeEntry>,
    pub data: DataConfig,
    pub build: BuildConfig,
    pub serve: ServeConfig,
}

/// `geocoder = true`, or a table with the options of the module.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ModuleEntry {
    Enabled(bool),
    Table(ModuleTable),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleTable {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Its file of configuration, relative to `geko.toml`; `geko-<module>.toml` beside it by default.
    pub config: Option<PathBuf>,
    /// The version to download and build with, instead of the newest of the contract of this geko.
    pub version: Option<String>,
    /// Where it is downloaded from; `ghcr.io/candymaps/geko-<module>` by default.
    pub registry: Option<String>,
}

fn yes() -> bool {
    true
}

impl ModuleEntry {
    pub fn enabled(&self) -> bool {
        match self {
            ModuleEntry::Enabled(on) => *on,
            ModuleEntry::Table(t) => t.enabled,
        }
    }

    fn config(&self) -> Option<&Path> {
        match self {
            ModuleEntry::Enabled(_) => None,
            ModuleEntry::Table(t) => t.config.as_deref(),
        }
    }

    fn table(&self) -> Option<&ModuleTable> {
        match self {
            ModuleEntry::Enabled(_) => None,
            ModuleEntry::Table(t) => Some(t),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeEntry {
    pub country: String,
    #[serde(default)]
    pub regions: Vec<String>,
}

/// The configuration file, unless `GEKO_CONFIG` names another one.
pub const CONFIG_PATH: &str = "/etc/geko/geko.toml";
/// Where downloads, work files and bundles live, unless `GEKO_DATA` names another directory.
pub const DATA_DIR: &str = "/var/lib/geko";

pub fn config_path() -> PathBuf {
    std::env::var_os("GEKO_CONFIG").map_or_else(|| PathBuf::from(CONFIG_PATH), PathBuf::from)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DataConfig {
    /// Not read from the file: always `DATA_DIR`, or `GEKO_DATA`.
    #[serde(skip)]
    pub dir: PathBuf,
    pub refresh: Refresh,
    /// With `refresh = "if-changed"`, days between two checks of the sources, all at once: in
    /// between, a build contacts none of them and downloads only what is missing.
    pub max_age_days: u64,
    pub offline: bool,
    /// Keep the downloads after a bundle is published. When not, they are removed, leaving
    /// their records: a staging that is up to date is reused without them, and a build that has
    /// to repeat one downloads again what it reads. Never removed while `offline`.
    pub keep_downloads: bool,
    pub accept_licenses: bool,
    pub parallel_downloads: usize,
}

impl Default for DataConfig {
    fn default() -> Self {
        let policy = DataPolicy::default();
        Self {
            dir: std::env::var_os("GEKO_DATA").map_or_else(|| PathBuf::from(DATA_DIR), PathBuf::from),
            refresh: policy.refresh,
            max_age_days: policy.max_age_days,
            offline: policy.offline,
            keep_downloads: policy.keep_downloads,
            accept_licenses: policy.accept_licenses,
            parallel_downloads: policy.parallel_downloads,
        }
    }
}

impl DataConfig {
    pub fn policy(&self) -> DataPolicy {
        DataPolicy {
            refresh: self.refresh,
            max_age_days: self.max_age_days,
            offline: self.offline,
            keep_downloads: self.keep_downloads,
            accept_licenses: self.accept_licenses,
            parallel_downloads: self.parallel_downloads,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BuildConfig {
    /// Published versions kept, the served one included. One by default: an older version costs
    /// almost a whole segment of disk (bundles share only the files that did not change), and a
    /// server that still has it loaded keeps reading it, since every file is opened at load.
    pub keep: usize,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self { keep: 1 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServeConfig {
    pub listen: SocketAddr,
    /// Seconds between checks for a new bundle version.
    pub reload_secs: u64,
    /// Origins allowed to call the API from a browser (`*`: any); empty sends no CORS headers.
    pub cors_origins: Vec<String>,
    /// API keys accepted; empty leaves the API open. `/`, `/health`, `/v1/attribution` and the
    /// pages of the modules never need one.
    pub api_keys: Vec<String>,
    /// Requests per second allowed from one client address, and the burst above it (0: none).
    pub rate_limit_per_s: f64,
    pub rate_limit_burst: f64,
    /// Where to ask for the code that reproduces the served derivative database (ODbL 4.6), shown in
    /// `/v1/attribution`: an e-mail address or a URL.
    pub attribution_contact: String,
    /// HTTP address of a bundles directory published on another machine (its `current.version`
    /// and versions); each reload pulls a newer version into `bundles`, downloading only the
    /// artifacts that changed. Empty: serve the local directory as it is.
    pub bundles_url: String,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 8080)),
            reload_secs: 30,
            cors_origins: Vec::new(),
            api_keys: Vec::new(),
            rate_limit_per_s: 0.0,
            rate_limit_burst: 0.0,
            attribution_contact: String::new(),
            bundles_url: String::new(),
        }
    }
}

/// Options that `geko.toml` had and are now the geocoder's, in `geko-geocoder.toml`: a `geko.toml`
/// that still has one fails, rather than lose it without a word. `None`: the whole section.
const MOVED_TO_GEOCODER: &[(&str, Option<&str>)] = &[
    ("sources", None),
    ("packs", None),
    ("serve", Some("countries")),
    ("serve", Some("preload")),
    ("serve", Some("search_budget_ms")),
    ("serve", Some("autocomplete_budget_ms")),
    ("serve", Some("slow_query_ms")),
    ("serve", Some("same_entity_m")),
    ("serve", Some("batch_max")),
    ("serve", Some("cache_entries")),
];

/// The modules active by default, without a `[modules]` entry: only those published for every
/// geko (ci/modules), so that a geko.toml that names none of them starts.
const DEFAULT_MODULES: &[&str] = &["geocoder", "tiles"];

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| GekoError::Config(format!("cannot read {}: {e}", path.display())))?;
        Self::parse(&text).map_err(|e| GekoError::Config(format!("{}: {e}", path.display())))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let table: toml::Table = toml::from_str(text).map_err(|e| GekoError::Config(e.to_string()))?;
        for (section, key) in MOVED_TO_GEOCODER {
            let Some(found) = table.get(*section) else { continue };
            let moved = match key {
                None => Some(format!("[{section}]")),
                Some(key) => found.get(*key).map(|_| format!("[{section}] {key}")),
            };
            if let Some(what) = moved {
                return Err(GekoError::Config(format!("{what} is an option of the geocoder now: move it to geko-geocoder.toml, beside geko.toml")));
            }
        }
        let config: Config = toml::Value::Table(table).try_into().map_err(|e: toml::de::Error| GekoError::Config(e.to_string()))?;
        if config.data.parallel_downloads == 0 {
            return Err(GekoError::Config("data.parallel_downloads must be at least 1".into()));
        }
        for name in config.modules.keys() {
            ModuleName::new(name)?;
        }
        Ok(config)
    }

    pub fn scope(&self) -> Result<Scope> {
        let mut scope = Scope::default();
        for entry in &self.scope {
            let country = CountryCode::new(&entry.country).map_err(|e| GekoError::Config(e.to_string()))?;
            let regions = entry
                .regions
                .iter()
                .map(|r| RegionCode::new(r).map_err(|e| GekoError::Config(e.to_string())))
                .collect::<Result<Vec<_>>>()?;
            scope.add(country, regions)?;
        }
        Ok(scope)
    }

    /// The modules `[modules]` makes active, in order of name: those set to true, and those it
    /// does not mention of `DEFAULT_MODULES`.
    pub fn active_modules(&self) -> Vec<ModuleName> {
        let mut names: Vec<&str> = self.modules.iter().filter(|(_, e)| e.enabled()).map(|(n, _)| n.as_str()).collect();
        names.extend(DEFAULT_MODULES.iter().filter(|m| !self.modules.contains_key(**m)));
        names.sort();
        names.dedup();
        names.into_iter().map(|n| ModuleName::new(n).expect("checked when parsed")).collect()
    }

    /// The file of configuration of `module`: `[modules.<module>] config`, relative to `geko.toml`,
    /// or `geko-<module>.toml` beside it.
    pub fn module_config(&self, module: &ModuleName, geko_toml: &Path) -> PathBuf {
        let dir = geko_toml.parent().unwrap_or(Path::new("."));
        match self.modules.get(module.as_str()).and_then(|e| e.config()) {
            Some(path) => dir.join(path),
            None => dir.join(format!("geko-{module}.toml")),
        }
    }

    /// `[modules.<module>] version`.
    pub fn module_version(&self, module: &ModuleName) -> Option<&str> {
        self.modules.get(module.as_str()).and_then(ModuleEntry::table).and_then(|t| t.version.as_deref())
    }

    /// `[modules.<module>] registry`.
    pub fn module_registry(&self, module: &ModuleName) -> Option<&str> {
        self.modules.get(module.as_str()).and_then(ModuleEntry::table).and_then(|t| t.registry.as_deref())
    }

    /// Where bundles are published and served from.
    pub fn bundles_dir(&self) -> PathBuf {
        self.data.dir.join("bundles")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(c: &Config) -> Vec<String> {
        c.active_modules().into_iter().map(|m| m.to_string()).collect()
    }

    #[test]
    fn defaults_enable_both_modules() {
        let c = Config::parse("").unwrap();
        assert_eq!(names(&c), ["geocoder", "tiles"]);
        assert_eq!(c.serve.listen.port(), 8080);
        assert!(!c.data.accept_licenses);
        assert_eq!(c.data.refresh, Refresh::IfChanged);
        assert_eq!(c.data.max_age_days, 30);
        assert!(!c.data.keep_downloads);
        assert_eq!(c.build.keep, 1);
        assert!(c.scope().unwrap().is_empty());
    }

    #[test]
    fn modules_as_booleans_or_tables() {
        let c = Config::parse("[modules]\ntiles = false\n").unwrap();
        assert_eq!(names(&c), ["geocoder"]);
        let c = Config::parse("[modules]\ntiles = true\n").unwrap();
        assert_eq!(names(&c), ["geocoder", "tiles"]);
        let c = Config::parse("[modules]\ngeocoder = false\ntiles = true\n").unwrap();
        assert_eq!(names(&c), ["tiles"]);
        let c = Config::parse("[modules.geocoder]\nconfig = \"exact.toml\"\n\n[modules.tiles]\nenabled = false\n").unwrap();
        assert_eq!(names(&c), ["geocoder"]);
        let geocoder = ModuleName::new("geocoder").unwrap();
        assert_eq!(c.module_config(&geocoder, Path::new("/etc/geko/geko.toml")), Path::new("/etc/geko/exact.toml"));
        let tiles = ModuleName::new("tiles").unwrap();
        assert_eq!(c.module_config(&tiles, Path::new("/etc/geko/geko.toml")), Path::new("/etc/geko/geko-tiles.toml"));
        assert_eq!((c.module_version(&geocoder), c.module_registry(&geocoder)), (None, None));
        let c = Config::parse("[modules.geocoder]\nversion = \"1.4.2\"\nregistry = \"ghcr.io/me/geko-geocoder\"\n").unwrap();
        assert_eq!((c.module_version(&geocoder), c.module_registry(&geocoder)), (Some("1.4.2"), Some("ghcr.io/me/geko-geocoder")));
        assert!(Config::parse("[modules.geocoder]\nconfg = \"x\"\n").is_err());
        assert!(Config::parse("[modules]\nGeocoder = true\n").is_err());
    }

    #[test]
    fn options_of_the_geocoder_name_their_new_file() {
        for text in ["[sources.osm]\nmirror = \"auto\"\n", "[packs]\nregistry = \"x\"\n", "[serve]\npreload = false\n", "[serve]\ncache_entries = 5\n"] {
            let err = Config::parse(text).unwrap_err().to_string();
            assert!(err.contains("geko-geocoder.toml"), "{text}: {err}");
        }
        assert!(Config::parse("[serve]\nlisten = \"0.0.0.0:9000\"\n").is_ok());
    }

    #[test]
    fn ignores_sections_of_other_components() {
        let c = Config::parse("[modules]\ntiles = false\n\n[build]\ngeocode = \"fst\"\n\n[serve]\nlisten = \"0.0.0.0:9000\"\n").unwrap();
        assert_eq!(names(&c), ["geocoder"]);
        assert_eq!(c.serve.listen.port(), 9000);
    }

    #[test]
    fn parses_scope_with_several_countries() {
        let c = Config::parse("[[scope]]\ncountry = \"es\"\nregions = [\"ES-B\", \"es-m\"]\n\n[[scope]]\ncountry = \"PT\"\n").unwrap();
        let scope = c.scope().unwrap();
        let countries: Vec<_> = scope.countries().map(|c| c.to_string()).collect();
        assert_eq!(countries, ["ES", "PT"]);
        assert_eq!(scope.partitions().len(), 3);
    }

    #[test]
    fn rejects_invalid_scope() {
        let wrong_region = Config::parse("[[scope]]\ncountry = \"ES\"\nregions = [\"FR-75\"]\n").unwrap();
        assert!(wrong_region.scope().is_err());
        let wrong_country = Config::parse("[[scope]]\ncountry = \"Spain\"\n").unwrap();
        assert!(wrong_country.scope().is_err());
        assert!(Config::parse("[[scope]]\ncountry = \"ES\"\nregion = [\"ES-B\"]\n").is_err());
    }

    #[test]
    fn parses_data_section() {
        let c = Config::parse("[data]\nrefresh = \"always\"\noffline = true\naccept_licenses = true\nparallel_downloads = 2\n").unwrap();
        assert_eq!(c.data.refresh, Refresh::Always);
        assert!(c.data.offline && c.data.accept_licenses);
        assert!(Config::parse("[data]\nparallel_downloads = 0\n").is_err());
        assert!(Config::parse("[data]\nrefresh = \"sometimes\"\n").is_err());
        assert!(Config::parse("[data]\ndir = \"/srv/geko\"\n").is_err());
    }
}
