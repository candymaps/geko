//! The modules of this binary and how the host opens them (docs/MODULES_PLAN.md). Each active module
//! is, in this order: the package of `GEKO_MODULES`, which in the image of geko are those of
//! `modules.lock`; or the one linked in (`StaticModule`, by the binary that runs the host). A package
//! is loaded as a library into the process of the host (`DylibModule`). The host downloads no module.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use geko_module_sdk::module::{Context, Module, ModuleName, Open};
use geko_module_sdk::{GekoError, Result};

use serde::Deserialize;

use crate::config::Config;

/// A module linked into the binary that runs the host (`geko_app::main`): its name and how it is
/// opened.
#[derive(Clone, Copy)]
pub struct Linked {
    pub name: &'static str,
    pub open: Open,
}

/// How the host runs a package: always loaded as a library into its process (`DylibModule`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dylib,
}

/// `module.toml`: how to run a package. A published one says more of itself, which `ci/modules.sh
/// pull` checks before it goes into the image.
#[derive(Debug, Clone, Deserialize)]
pub struct Package {
    pub kind: Kind,
    /// The library to load, relative to the package.
    pub entry: PathBuf,
}

impl Package {
    pub fn read(dir: &Path) -> Result<Self> {
        let file = dir.join("module.toml");
        let text = std::fs::read_to_string(&file).map_err(|e| GekoError::Config(format!("cannot read {}: {e}", file.display())))?;
        toml::from_str(&text).map_err(|e| GekoError::Config(format!("{}: {e}", file.display())))
    }
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
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::Linked => f.write_str("linked in"),
            Origin::Local(dir) => write!(f, "from GEKO_MODULES, {}", dir.display()),
        }
    }
}

pub struct Opened {
    pub module: Arc<dyn Module>,
    pub origin: Origin,
}

/// What opens the modules: the configuration, the modules linked in and those of `GEKO_MODULES`.
pub struct Opener {
    config: Config,
    geko_toml: PathBuf,
    linked: Vec<Linked>,
    local: Vec<Local>,
}

/// Starts the module of a package.
fn start(dir: &Path, package: &Package, context: Context) -> Result<Box<dyn Module>> {
    let entry = dir.join(&package.entry);
    match package.kind {
        Kind::Dylib => Ok(Box::new(crate::dylib::load(&entry, context)?)),
    }
}

impl Opener {
    pub fn new(config: &Config, geko_toml: &Path, linked: Vec<Linked>, local: Vec<Local>) -> Self {
        Self { config: config.clone(), geko_toml: geko_toml.to_owned(), linked, local }
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

    /// Opens the module `name` as configured.
    pub fn open(&self, name: &ModuleName) -> Result<Opened> {
        let opened = if let Some(local) = self.local.iter().find(|l| l.name == *name) {
            tracing::info!(module = %name, package = %local.dir.display(), "module of GEKO_MODULES");
            let module = start(&local.dir, &Package::read(&local.dir)?, self.context(name)?)?;
            Opened { module: Arc::from(module), origin: Origin::Local(local.dir.clone()) }
        } else if let Some(linked) = self.linked.iter().find(|l| l.name == name.as_str()) {
            Opened { module: Arc::from((linked.open)(self.context(name)?)?), origin: Origin::Linked }
        } else {
            return Err(GekoError::Config(format!(
                "module `{name}` is active, but it is neither in GEKO_MODULES nor linked in: run the image of geko, which carries its modules, build it too (make start-dev: DEV_MODULES=…,{name}) or disable it in [modules]"
            )));
        };
        check(name, &*opened.module)?;
        Ok(opened)
    }

}

fn check(name: &ModuleName, module: &dyn Module) -> Result<()> {
    let described = module.describe();
    if described.name != *name {
        return Err(GekoError::Incompatible(format!("module `{name}` calls itself `{}`", described.name)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use geko_module_sdk::module::Description;

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

    #[test]
    fn a_published_package_says_more_of_itself_than_the_host_reads() {
        let dir = tempfile::tempdir().unwrap();
        let text = "kind = \"dylib\"\nentry = \"libgeko_tiles.so\"\nname = \"tiles\"\nversion = \"1.0.0\"\ncontract = 1\nplatform = \"linux-x86_64\"\n";
        std::fs::write(dir.path().join("module.toml"), text).unwrap();
        assert_eq!(Package::read(dir.path()).unwrap().entry, Path::new("libgeko_tiles.so"));
    }

    struct Named(&'static str);

    impl Module for Named {
        fn describe(&self) -> Description {
            Description {
                name: name(self.0),
                version: "1".into(),
                routes: vec![],
                openapi: None,
                attribution: None,
                commands: vec![],
            }
        }
    }

    fn geocoder(open: Open) -> Linked {
        Linked { name: "geocoder", open }
    }

    fn opener(linked: Vec<Linked>, toml: &str, data: &Path) -> Opener {
        let mut config = Config::parse(toml).unwrap();
        config.data.dir = data.to_owned();
        Opener::new(&config, Path::new("/nonexistent/geko.toml"), linked, vec![])
    }

    #[test]
    fn a_module_that_calls_itself_otherwise_is_refused() {
        let data = tempfile::tempdir().unwrap();
        let other: Open = |_| Ok(Box::new(Named("tiles")));
        let ok: Open = |_| Ok(Box::new(Named("geocoder")));
        let err = opener(vec![geocoder(other)], "", data.path()).open(&name("geocoder")).err().unwrap();
        assert!(err.to_string().contains("calls itself"), "{err}");
        let opened = opener(vec![geocoder(ok)], "", data.path()).open(&name("geocoder")).unwrap();
        assert_eq!(opened.origin, Origin::Linked);
    }

    #[test]
    fn a_module_neither_in_geko_modules_nor_linked_in_is_an_error() {
        let data = tempfile::tempdir().unwrap();
        let err = opener(vec![], "", data.path()).open(&name("tiles")).err().unwrap().to_string();
        assert!(err.contains("neither in GEKO_MODULES nor linked in") && err.contains("DEV_MODULES=…,tiles"), "{err}");
    }
}
