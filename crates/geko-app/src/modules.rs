//! The modules of this binary and how the host opens them (docs/MODULES_PLAN.md): linked in
//! (`StaticModule`), each behind its Cargo feature, or, from `GEKO_MODULES`, run as a process of
//! their own (`ProcessModule`) or loaded as a library (`DylibModule`), which then replace the one
//! linked in.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use geko_module_sdk::module::{Context, Module, ModuleName, Open};
use geko_module_sdk::{GekoError, Result};
use serde::Deserialize;

use crate::config::Config;

/// The modules linked into this binary, by name.
pub fn linked() -> Vec<(&'static str, Open)> {
    vec![
        #[cfg(feature = "geocoder")]
        (geko_module_geocoder::NAME, geko_module_geocoder::open as Open),
        #[cfg(feature = "tiles")]
        (geko_module_tiles::NAME, geko_module_tiles::open as Open),
    ]
}

/// What a package of a module says of itself, in its `module.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    kind: Kind,
    /// The file to run, relative to the package.
    entry: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Process,
    Dylib,
}

/// A module of `GEKO_MODULES`, never downloaded: its package, in `<GEKO_MODULES>/<name>/`.
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
pub fn resolve(config: &Config, cli: Option<&[ModuleName]>, linked: &[(&'static str, Open)], local: &[Local]) -> Result<Vec<ModuleName>> {
    let mut active = match cli {
        Some(list) => list.to_vec(),
        None => config.active_modules(),
    };
    active.sort();
    active.dedup();
    if active.is_empty() {
        let names: Vec<&str> = linked.iter().map(|(n, _)| *n).collect();
        return Err(GekoError::Config(format!("at least one module must be active ({})", names.join(", "))));
    }
    let available = |m: &ModuleName| linked.iter().any(|(n, _)| *n == m.as_str()) || local.iter().any(|l| l.name == *m);
    if let Some(missing) = active.iter().find(|m| !available(m)) {
        return Err(GekoError::Config(format!(
            "module `{missing}` is active but this binary was built without the `{missing}` feature, and GEKO_MODULES has no `{missing}`"
        )));
    }
    Ok(active)
}

/// Starts the module of a package.
fn start(local: &Local, context: Context) -> Result<Box<dyn Module>> {
    let file = local.dir.join("module.toml");
    let text = std::fs::read_to_string(&file)?;
    let package: Package = toml::from_str(&text).map_err(|e| GekoError::Config(format!("{}: {e}", file.display())))?;
    let entry = local.dir.join(&package.entry);
    match package.kind {
        #[cfg(unix)]
        Kind::Process => Ok(Box::new(crate::process::start(entry, context)?)),
        #[cfg(not(unix))]
        Kind::Process => {
            let _ = (entry, context);
            Err(GekoError::Config(format!("{}: a module of kind `process` only runs on Unix", file.display())))
        }
        Kind::Dylib => Ok(Box::new(crate::dylib::load(&entry, context)?)),
    }
}

/// Opens every active module with its file of configuration (missing: its defaults): the one of
/// `GEKO_MODULES`, if there is, or the one linked in.
pub fn open(config: &Config, geko_toml: &Path, active: &[ModuleName], linked: &[(&'static str, Open)], local: &[Local]) -> Result<Vec<Arc<dyn Module>>> {
    let scope = config.scope()?;
    active
        .iter()
        .map(|name| {
            let file = config.module_config(name, geko_toml);
            let text = match std::fs::read_to_string(&file) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(GekoError::Config(format!("cannot read {}: {e}", file.display()))),
            };
            let context = Context {
                config: text,
                config_file: file.display().to_string(),
                scope: scope.clone(),
                data_dir: config.data.dir.clone(),
                data: config.data.policy(),
            };
            let module: Arc<dyn Module> = match local.iter().find(|l| l.name == *name) {
                Some(package) => {
                    tracing::info!(module = %name, package = %package.dir.display(), "module of GEKO_MODULES");
                    Arc::from(start(package, context)?)
                }
                None => {
                    let (_, open) = linked.iter().find(|(n, _)| *n == name.as_str()).expect("resolved");
                    Arc::from(open(context)?)
                }
            };
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
            Ok(module)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use geko_module_sdk::module::Description;

    fn fake(_: Context) -> Result<Box<dyn Module>> {
        unimplemented!()
    }

    fn both() -> Vec<(&'static str, Open)> {
        vec![("geocoder", fake as Open), ("tiles", fake as Open)]
    }

    fn name(n: &str) -> ModuleName {
        ModuleName::new(n).unwrap()
    }

    #[test]
    fn config_selects_modules_and_the_cli_replaces_it() {
        let cfg = Config::parse("[modules]\ntiles = false\n").unwrap();
        assert_eq!(resolve(&cfg, None, &both(), &[]).unwrap(), [name("geocoder")]);
        assert_eq!(resolve(&cfg, Some(&[name("tiles")]), &both(), &[]).unwrap(), [name("tiles")]);
    }

    #[test]
    fn fails_when_no_module_is_active_or_one_is_not_linked() {
        let cfg = Config::parse("[modules]\ngeocoder = false\ntiles = false\n").unwrap();
        assert!(resolve(&cfg, None, &both(), &[]).is_err());
        assert!(resolve(&Config::default(), Some(&[]), &both(), &[]).is_err());
        let err = resolve(&Config::default(), None, &both()[..1], &[]).unwrap_err();
        assert!(err.to_string().contains("`tiles` feature"), "{err}");
    }

    #[test]
    fn a_module_of_geko_modules_needs_no_feature() {
        let dir = tempfile::tempdir().unwrap();
        for module in ["tiles", "notes"] {
            std::fs::create_dir(dir.path().join(module)).unwrap();
        }
        std::fs::write(dir.path().join("tiles/module.toml"), "kind = \"process\"\nentry = \"geko-tiles\"\n").unwrap();
        let local = local_in(dir.path()).unwrap();
        assert_eq!(local.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["tiles"], "only directories with a module.toml");
        assert_eq!(resolve(&Config::default(), None, &both()[..1], &local).unwrap(), [name("geocoder"), name("tiles")]);

        std::fs::write(dir.path().join("tiles/module.toml"), "kind = \"wasm\"\nentry = \"x\"\n").unwrap();
        let err = start(&local[0], Context {
            config: String::new(),
            config_file: String::new(),
            scope: Default::default(),
            data_dir: dir.path().into(),
            data: Default::default(),
        })
        .err()
        .unwrap();
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

    #[test]
    fn a_module_of_another_contract_is_refused() {
        let other: Open = |_| Ok(Box::new(Named("geocoder", geko_module_sdk::module::CONTRACT + 1)));
        let ok: Open = |_| Ok(Box::new(Named("geocoder", geko_module_sdk::module::CONTRACT)));
        let geocoder = [name("geocoder")];
        let toml = Path::new("/nonexistent/geko.toml");
        let err = open(&Config::default(), toml, &geocoder, &[("geocoder", other)], &[]).err().unwrap();
        assert!(err.to_string().contains("contract"), "{err}");
        assert_eq!(open(&Config::default(), toml, &geocoder, &[("geocoder", ok)], &[]).unwrap().len(), 1);
    }
}
