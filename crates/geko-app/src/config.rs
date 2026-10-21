use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use geko_core::country::{CountryCode, RegionCode};
use geko_core::scope::Scope;
use geko_core::{GekoError, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub modules: ModulesConfig,
    pub scope: Vec<ScopeEntry>,
    pub data: DataConfig,
    /// Each provider parses its own section; only `enabled` is common.
    pub sources: BTreeMap<String, toml::Table>,
    pub build: BuildConfig,
    pub serve: ServeConfig,
    pub packs: PacksConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PacksConfig {
    /// Where the country packs are published: `<registry>/<country>:format-<n>` (PROJECT.md §2.4).
    pub registry: String,
}

impl Default for PacksConfig {
    fn default() -> Self {
        Self { registry: geko_country_pack::registry::DEFAULT_REGISTRY.to_owned() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ModulesConfig {
    pub geocoder: bool,
    pub tiles: bool,
}

impl Default for ModulesConfig {
    fn default() -> Self {
        Self { geocoder: true, tiles: true }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeEntry {
    pub country: String,
    #[serde(default)]
    pub regions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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
        Self {
            dir: std::env::var_os("GEKO_DATA").map_or_else(|| PathBuf::from(DATA_DIR), PathBuf::from),
            refresh: Refresh::IfChanged,
            max_age_days: 30,
            offline: false,
            keep_downloads: false,
            accept_licenses: false,
            parallel_downloads: 4,
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
    /// Countries this node serves; empty means every country of the bundle.
    pub countries: Vec<String>,
    /// Load hot index files into RAM at startup (PROJECT.md §9.6).
    pub preload: bool,
    /// Seconds between checks for a new bundle version.
    pub reload_secs: u64,
    /// Time budget of a search and of an autocomplete request, in milliseconds (0: none). Past
    /// it the engine returns what it found; its likeliest reading always completes.
    pub search_budget_ms: u64,
    pub autocomplete_budget_ms: u64,
    /// Requests slower than this, in milliseconds, are logged with their query (0: none).
    pub slow_query_ms: u64,
    /// Results with the same name closer than this, in metres, are merged in the answer
    /// (PROJECT.md §6.5.6); 0 merges only the same id.
    pub same_entity_m: f64,
    /// Origins allowed to call the API from a browser (`*`: any); empty sends no CORS headers.
    pub cors_origins: Vec<String>,
    /// API keys accepted; empty leaves the API open. `/`, `/health` and `/v1/attribution` never need one.
    pub api_keys: Vec<String>,
    /// Requests per second allowed from one client address, and the burst above it (0: none).
    pub rate_limit_per_s: f64,
    pub rate_limit_burst: f64,
    /// Where to ask for the code that reproduces the served derivative database (ODbL 4.6), shown in
    /// `/v1/attribution`: an e-mail address or a URL.
    pub attribution_contact: String,
    /// Most requests in one `/v1/batch`.
    pub batch_max: usize,
    /// Search and autocomplete answers kept for repeated queries (0: none), cleared when the
    /// bundle changes.
    pub cache_entries: usize,
    /// HTTP address of a bundles directory published on another machine (its `current.version`
    /// and versions); each reload pulls a newer version into `bundles`, downloading only the
    /// artifacts that changed. Empty: serve the local directory as it is.
    pub bundles_url: String,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 8080)),
            countries: Vec::new(),
            preload: true,
            reload_secs: 30,
            search_budget_ms: 15,
            autocomplete_budget_ms: 5,
            slow_query_ms: 50,
            same_entity_m: 100.0,
            cors_origins: Vec::new(),
            api_keys: Vec::new(),
            rate_limit_per_s: 0.0,
            rate_limit_burst: 0.0,
            attribution_contact: String::new(),
            batch_max: 100,
            cache_entries: 0,
            bundles_url: String::new(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| GekoError::Config(format!("cannot read {}: {e}", path.display())))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self> {
        let config: Config = toml::from_str(text).map_err(|e| GekoError::Config(e.to_string()))?;
        if config.data.parallel_downloads == 0 {
            return Err(GekoError::Config("data.parallel_downloads must be at least 1".into()));
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

    /// Where bundles are published and served from.
    pub fn bundles_dir(&self) -> PathBuf {
        self.data.dir.join("bundles")
    }

    #[cfg(feature = "geocoder")]
    pub fn served_countries(&self) -> Result<Vec<CountryCode>> {
        self.serve.countries.iter().map(|c| CountryCode::new(c).map_err(|e| GekoError::Config(e.to_string()))).collect()
    }

    pub fn source_enabled(&self, name: &str) -> Result<bool> {
        match self.sources.get(name).and_then(|t| t.get("enabled")) {
            None => Ok(true),
            Some(toml::Value::Boolean(b)) => Ok(*b),
            Some(other) => Err(GekoError::Config(format!("sources.{name}.enabled must be a boolean, got {other}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_enable_both_modules() {
        let c = Config::parse("").unwrap();
        assert!(c.modules.geocoder && c.modules.tiles);
        assert_eq!(c.serve.listen.port(), 8080);
        assert!(!c.data.accept_licenses);
        assert_eq!(c.data.refresh, Refresh::IfChanged);
        assert_eq!(c.data.max_age_days, 30);
        assert!(!c.data.keep_downloads);
        assert_eq!(c.build.keep, 1);
        assert!(c.scope().unwrap().is_empty());
    }

    #[test]
    fn ignores_sections_of_other_components() {
        let c = Config::parse(
            "[modules]\ntiles = false\n\n[build]\ngeocode = \"fst\"\n\n[serve]\nlisten = \"0.0.0.0:9000\"\npreload = true\n",
        )
        .unwrap();
        assert!(c.modules.geocoder);
        assert!(!c.modules.tiles);
        assert_eq!(c.serve.listen.port(), 9000);
    }

    #[test]
    fn parses_scope_with_several_countries() {
        let c = Config::parse(
            "[[scope]]\ncountry = \"es\"\nregions = [\"ES-B\", \"es-m\"]\n\n[[scope]]\ncountry = \"PT\"\n",
        )
        .unwrap();
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
        let c = Config::parse(
            "[data]\nrefresh = \"always\"\noffline = true\naccept_licenses = true\nparallel_downloads = 2\n",
        )
        .unwrap();
        assert_eq!(c.data.refresh, Refresh::Always);
        assert!(c.data.offline && c.data.accept_licenses);
        assert!(Config::parse("[data]\nparallel_downloads = 0\n").is_err());
        assert!(Config::parse("[data]\nrefresh = \"sometimes\"\n").is_err());
        assert!(Config::parse("[data]\ndir = \"/srv/geko\"\n").is_err());
    }

    #[test]
    fn source_sections() {
        let c = Config::parse("[sources.osm]\nmirror = \"auto\"\n\n[sources.cartociudad]\nenabled = false\n").unwrap();
        assert!(c.source_enabled("osm").unwrap());
        assert!(!c.source_enabled("cartociudad").unwrap());
        let bad = Config::parse("[sources.osm]\nenabled = \"yes\"\n").unwrap();
        assert!(bad.source_enabled("osm").is_err());
    }
}
