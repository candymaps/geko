//! The modules of this binary and how the host opens them (docs/MODULES_PLAN.md). For now they
//! are linked in (`StaticModule`), each behind its Cargo feature.

use std::path::Path;
use std::sync::Arc;

use geko_module_sdk::module::{Context, Module, ModuleName, Open};
use geko_module_sdk::{GekoError, Result};

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

/// The active modules: those of `[modules]`, or of `--modules`, which replaces it entirely.
pub fn resolve(config: &Config, cli: Option<&[ModuleName]>, linked: &[(&'static str, Open)]) -> Result<Vec<ModuleName>> {
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
    if let Some(missing) = active.iter().find(|m| !linked.iter().any(|(n, _)| *n == m.as_str())) {
        return Err(GekoError::Config(format!("module `{missing}` is active but this binary was built without the `{missing}` feature")));
    }
    Ok(active)
}

/// Opens every active module with its file of configuration (missing: its defaults).
pub fn open(config: &Config, geko_toml: &Path, active: &[ModuleName], linked: &[(&'static str, Open)]) -> Result<Vec<Arc<dyn Module>>> {
    let scope = config.scope()?;
    active
        .iter()
        .map(|name| {
            let (_, open) = linked.iter().find(|(n, _)| *n == name.as_str()).expect("resolved");
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
            let module: Arc<dyn Module> = Arc::from(open(context)?);
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
        assert_eq!(resolve(&cfg, None, &both()).unwrap(), [name("geocoder")]);
        assert_eq!(resolve(&cfg, Some(&[name("tiles")]), &both()).unwrap(), [name("tiles")]);
    }

    #[test]
    fn fails_when_no_module_is_active_or_one_is_not_linked() {
        let cfg = Config::parse("[modules]\ngeocoder = false\ntiles = false\n").unwrap();
        assert!(resolve(&cfg, None, &both()).is_err());
        assert!(resolve(&Config::default(), Some(&[]), &both()).is_err());
        let err = resolve(&Config::default(), None, &both()[..1]).unwrap_err();
        assert!(err.to_string().contains("`tiles` feature"), "{err}");
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
        let err = open(&Config::default(), toml, &geocoder, &[("geocoder", other)]).err().unwrap();
        assert!(err.to_string().contains("contract"), "{err}");
        assert_eq!(open(&Config::default(), toml, &geocoder, &[("geocoder", ok)]).unwrap().len(), 1);
    }
}
