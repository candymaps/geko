//! The modules of this binary and how the host opens them (docs/MODULES_PLAN.md). Each active module
//! is, in this order: the package of `GEKO_MODULES` (never downloaded); the one linked in
//! (`StaticModule`, behind its Cargo feature), unless `[modules.<module>] version` fixes another
//! version; or the one downloaded from its registry (`store`), the newest of the contract of this
//! geko or the version fixed, but never with `GEKO_MODULES`. A package runs as a process of its own (`ProcessModule`) or is loaded
//! as a library (`DylibModule`), as its `module.toml` says.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use geko_module_sdk::module::{Context, Module, ModuleName, Open};
use geko_module_sdk::{GekoError, Result};

use crate::config::Config;
use crate::store::{Kind, Package, Store, default_registry};

/// The version of the modules linked in: that of the workspace, as every crate of it, compiled with
/// the same `GEKO_MODULE_VERSION`.
pub const LINKED_VERSION: &str = geko_module_sdk::module_version!();

/// The modules linked into this binary, by name.
pub fn linked() -> Vec<(&'static str, Open)> {
    vec![
        #[cfg(feature = "geocoder")]
        (geko_module_geocoder::NAME, geko_module_geocoder::open as Open),
        #[cfg(feature = "tiles")]
        (geko_module_tiles::NAME, geko_module_tiles::open as Open),
    ]
}

/// A module of `GEKO_MODULES`: its package, in `<GEKO_MODULES>/<name>/`.
#[derive(Debug, Clone)]
pub struct Local {
    name: ModuleName,
    dir: PathBuf,
}

/// The modules of `GEKO_MODULES`: each directory in it with a `module.toml`.
pub fn local() -> Result<Vec<Local>> {
    let Some(root) = std::env::var_os("GEKO_MODULES") else { return Ok(Vec::new()) };
    local_in(Path::new(&root))
}

fn local_in(root: &Path) -> Result<Vec<Local>> {
    let entries = std::fs::read_dir(root).map_err(|e| GekoError::Config(format!("GEKO_MODULES: cannot read {}: {e}", root.display())))?;
    let mut found = Vec::new();
    for entry in entries {
        let dir = entry?.path();
        if !dir.join("module.toml").is_file() {
            continue;
        }
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let name = ModuleName::new(name).map_err(|e| GekoError::Config(format!("GEKO_MODULES: {}: {e}", dir.display())))?;
        found.push(Local { name, dir });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// The active modules: those of `[modules]`, or of `--modules`, which replaces it entirely.
pub fn resolve(config: &Config, cli: Option<&[ModuleName]>) -> Result<Vec<ModuleName>> {
    let mut active = match cli {
        Some(list) => list.to_vec(),
        None => config.active_modules(),
    };
    active.sort();
    active.dedup();
    if active.is_empty() {
        return Err(GekoError::Config("at least one module must be active".into()));
    }
    Ok(active)
}

/// Where an open module comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Linked,
    Local(PathBuf),
    Downloaded { how: String, kind: Kind, fixed: bool },
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::Linked => f.write_str("linked in"),
            Origin::Local(dir) => write!(f, "from GEKO_MODULES, {}", dir.display()),
            Origin::Downloaded { how, kind, fixed } => {
                write!(f, "{how}, {}", if *kind == Kind::Dylib { "library" } else { "process" })?;
                if *fixed { f.write_str(", version fixed in geko.toml") } else { Ok(()) }
            }
        }
    }
}

pub struct Opened {
    pub module: Arc<dyn Module>,
    pub origin: Origin,
}

/// What opens the modules: the configuration, the modules linked in, those of `GEKO_MODULES` and
/// the store of the downloaded ones.
pub struct Opener {
    config: Config,
    geko_toml: PathBuf,
    linked: Vec<(&'static str, Open)>,
    local: Vec<Local>,
    store: Store,
    /// With `GEKO_MODULES`, a module neither in it nor linked in is an error, not a download: the
    /// modules of a development build are those it was built with.
    downloads: bool,
}

/// Starts the module of a package.
fn start(dir: &Path, package: &Package, context: Context) -> Result<Box<dyn Module>> {
    let entry = dir.join(&package.entry);
    match package.kind {
        #[cfg(unix)]
        Kind::Process => Ok(Box::new(crate::process::start(entry, context)?)),
        #[cfg(not(unix))]
        Kind::Process => {
            let _ = (entry, context);
            Err(GekoError::Config(format!("{}: a module of kind `process` only runs on Unix", dir.display())))
        }
        Kind::Dylib => Ok(Box::new(crate::dylib::load(&entry, context)?)),
    }
}

impl Opener {
    pub fn new(config: &Config, geko_toml: &Path, linked: Vec<(&'static str, Open)>, local: Vec<Local>) -> Self {
        let store = Store::new(&config.data.dir, config.data.offline);
        Self { config: config.clone(), geko_toml: geko_toml.to_owned(), linked, local, store, downloads: true }
    }

    pub fn without_downloads(self) -> Self {
        Self { downloads: false, ..self }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn registry(&self, name: &ModuleName) -> String {
        self.config.module_registry(name).map_or_else(|| default_registry(name), str::to_owned)
    }

    /// What the module is opened with: its file of configuration (missing: its defaults), the scope
    /// and the data directory.
    fn context(&self, name: &ModuleName) -> Result<Context> {
        let file = self.config.module_config(name, &self.geko_toml);
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(GekoError::Config(format!("cannot read {}: {e}", file.display()))),
        };
        Ok(Context {
            config: text,
            config_file: file.display().to_string(),
            scope: self.config.scope()?,
            data_dir: self.config.data.dir.clone(),
            data: self.config.data.policy(),
        })
    }

    fn downloaded(&self, name: &ModuleName, version: Option<&str>) -> Result<Opened> {
        let found = self.store.find(name, &self.registry(name), version)?;
        let module = start(&found.dir, &found.package, self.context(name)?)?;
        Ok(Opened { module: Arc::from(module), origin: Origin::Downloaded { how: found.how, kind: found.package.kind, fixed: version.is_some() } })
    }

    /// Opens the module `name` as configured.
    pub fn open(&self, name: &ModuleName) -> Result<Opened> {
        let fixed = self.config.module_version(name);
        let opened = if let Some(local) = self.local.iter().find(|l| l.name == *name) {
            tracing::info!(module = %name, package = %local.dir.display(), "module of GEKO_MODULES");
            let module = start(&local.dir, &Package::read(&local.dir)?, self.context(name)?)?;
            Opened { module: Arc::from(module), origin: Origin::Local(local.dir.clone()) }
        } else if let Some((_, open)) = self.linked.iter().find(|(n, _)| *n == name.as_str())
            && fixed.is_none_or(|v| v == LINKED_VERSION)
        {
            Opened { module: Arc::from(open(self.context(name)?)?), origin: Origin::Linked }
        } else if !self.downloads {
            return Err(GekoError::Config(format!(
                "module `{name}` is active, but it is neither in GEKO_MODULES nor linked in, and with GEKO_MODULES it is not downloaded: build it too (make start-dev: DEV_MODULES=…,{name}) or disable it in [modules]"
            )));
        } else {
            self.downloaded(name, fixed).map_err(|e| {
                let linked = self.linked.iter().any(|(n, _)| *n == name.as_str());
                let why = if linked { format!("version {} is fixed and the one linked in is {LINKED_VERSION}", fixed.unwrap_or("?")) } else { format!("it is not linked into this geko (feature `{name}`) nor in GEKO_MODULES") };
                GekoError::Config(format!("module `{name}`: {why}, and it cannot be had from {}: {e}", self.registry(name)))
            })?
        };
        check(name, &*opened.module)?;
        Ok(opened)
    }

    /// The module `name` of `version`, the one a bundle was built by, to serve it: `None` when the
    /// module in service is kept (one linked in or of `GEKO_MODULES`, which serve as they are).
    pub fn reopen(&self, name: &ModuleName, version: &str, current: &Origin) -> Result<Option<Arc<dyn Module>>> {
        let Origin::Downloaded { kind, .. } = current else { return Ok(None) };
        if *kind == Kind::Dylib {
            return Err(GekoError::Incompatible(format!(
                "the bundle was built by module `{name}` {version}: a module loaded as a library changes its version only when geko restarts"
            )));
        }
        let opened = self.downloaded(name, Some(version))?;
        check(name, &*opened.module)?;
        Ok(Some(opened.module))
    }
}

fn check(name: &ModuleName, module: &dyn Module) -> Result<()> {
    let described = module.describe();
    if described.name != *name {
        return Err(GekoError::Incompatible(format!("module `{name}` calls itself `{}`", described.name)));
    }
    if described.contract != geko_module_sdk::module::CONTRACT {
        return Err(GekoError::Incompatible(format!(
            "module `{name}` {} implements contract {}, and this geko speaks contract {}",
            described.version,
            described.contract,
            geko_module_sdk::module::CONTRACT
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use geko_module_sdk::module::{CONTRACT, Description};

    fn name(n: &str) -> ModuleName {
        ModuleName::new(n).unwrap()
    }

    #[test]
    fn config_selects_modules_and_the_cli_replaces_it() {
        let cfg = Config::parse("[modules]\ntiles = false\n").unwrap();
        assert_eq!(resolve(&cfg, None).unwrap(), [name("geocoder")]);
        assert_eq!(resolve(&cfg, Some(&[name("tiles")])).unwrap(), [name("tiles")]);
    }

    #[test]
    fn fails_when_no_module_is_active() {
        let cfg = Config::parse("[modules]\ngeocoder = false\ntiles = false\n").unwrap();
        assert!(resolve(&cfg, None).is_err());
        assert!(resolve(&Config::default(), Some(&[])).is_err());
    }

    #[test]
    fn only_directories_with_a_module_toml_are_modules_of_geko_modules() {
        let dir = tempfile::tempdir().unwrap();
        for module in ["tiles", "notes"] {
            std::fs::create_dir(dir.path().join(module)).unwrap();
        }
        std::fs::write(dir.path().join("tiles/module.toml"), "kind = \"wasm\"\nentry = \"x\"\n").unwrap();
        let local = local_in(dir.path()).unwrap();
        assert_eq!(local.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["tiles"]);
        let err = Package::read(&local[0].dir).unwrap_err();
        assert!(err.to_string().contains("module.toml"), "{err}");
    }

    struct Named(&'static str, u32);

    impl Module for Named {
        fn describe(&self) -> Description {
            Description {
                name: name(self.0),
                version: "1".into(),
                contract: self.1,
                routes: vec![],
                home: None,
                openapi: None,
                attribution: None,
                commands: vec![],
            }
        }
    }

    fn opener(linked: Vec<(&'static str, Open)>, toml: &str, data: &Path) -> Opener {
        let mut config = Config::parse(toml).unwrap();
        config.data.dir = data.to_owned();
        config.data.offline = true;
        Opener::new(&config, Path::new("/nonexistent/geko.toml"), linked, vec![])
    }

    #[test]
    fn a_module_of_another_contract_is_refused() {
        let data = tempfile::tempdir().unwrap();
        let other: Open = |_| Ok(Box::new(Named("geocoder", CONTRACT + 1)));
        let ok: Open = |_| Ok(Box::new(Named("geocoder", CONTRACT)));
        let err = opener(vec![("geocoder", other)], "", data.path()).open(&name("geocoder")).err().unwrap();
        assert!(err.to_string().contains("contract"), "{err}");
        let opened = opener(vec![("geocoder", ok)], "", data.path()).open(&name("geocoder")).unwrap();
        assert_eq!(opened.origin, Origin::Linked);
    }

    #[test]
    fn a_fixed_version_other_than_the_linked_one_is_downloaded() {
        let data = tempfile::tempdir().unwrap();
        let ok: Open = |_| Ok(Box::new(Named("geocoder", CONTRACT)));
        let same = format!("[modules.geocoder]\nversion = \"{LINKED_VERSION}\"\n");
        assert_eq!(opener(vec![("geocoder", ok)], &same, data.path()).open(&name("geocoder")).unwrap().origin, Origin::Linked);
        let other = "[modules.geocoder]\nversion = \"99.0.0\"\n";
        let err = opener(vec![("geocoder", ok)], other, data.path()).open(&name("geocoder")).err().unwrap();
        assert!(err.to_string().contains("version 99.0.0 is fixed") && err.to_string().contains("offline mode"), "{err}");
        let err = opener(vec![], "", data.path()).open(&name("tiles")).err().unwrap();
        assert!(err.to_string().contains("feature `tiles`"), "{err}");
    }

    #[test]
    fn with_geko_modules_a_missing_module_is_an_error_not_a_download() {
        let data = tempfile::tempdir().unwrap();
        let ok: Open = |_| Ok(Box::new(Named("geocoder", CONTRACT)));
        let o = opener(vec![("geocoder", ok)], "", data.path()).without_downloads();
        assert_eq!(o.open(&name("geocoder")).unwrap().origin, Origin::Linked);
        let err = o.open(&name("tiles")).err().unwrap().to_string();
        assert!(err.contains("not downloaded") && err.contains("DEV_MODULES=…,tiles"), "{err}");
    }

    #[test]
    fn modules_linked_in_or_local_are_kept_whatever_the_bundle() {
        let data = tempfile::tempdir().unwrap();
        let o = opener(vec![], "", data.path());
        assert!(o.reopen(&name("geocoder"), "2.0.0", &Origin::Linked).unwrap().is_none());
        let library = Origin::Downloaded { how: String::new(), kind: Kind::Dylib, fixed: false };
        assert!(o.reopen(&name("geocoder"), "2.0.0", &library).err().unwrap().to_string().contains("restarts"));
    }
}
