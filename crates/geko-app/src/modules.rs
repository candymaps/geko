use std::collections::BTreeSet;
use std::sync::Arc;

use geko_core::{GekoError, Result};
use geko_server::{Module, ModuleName};

use crate::config::ModulesConfig;

pub fn compiled(name: ModuleName) -> bool {
    match name {
        ModuleName::Geocoder => cfg!(feature = "geocoder"),
        ModuleName::Tiles => cfg!(feature = "tiles"),
    }
}

/// The CLI list, when given, replaces the `[modules]` section entirely.
pub fn resolve(config: &ModulesConfig, cli: Option<&[ModuleName]>, compiled: impl Fn(ModuleName) -> bool) -> Result<Vec<ModuleName>> {
    let requested: BTreeSet<ModuleName> = match cli {
        Some(list) => list.iter().copied().collect(),
        None => ModuleName::ALL
            .into_iter()
            .filter(|m| match m {
                ModuleName::Geocoder => config.geocoder,
                ModuleName::Tiles => config.tiles,
            })
            .collect(),
    };
    if requested.is_empty() {
        return Err(GekoError::Config("at least one module must be active (geocoder, tiles)".into()));
    }
    if let Some(missing) = requested.iter().find(|m| !compiled(**m)) {
        return Err(GekoError::Config(format!(
            "module `{missing}` is active but this binary was built without the `{missing}` feature"
        )));
    }
    Ok(requested.into_iter().collect())
}

pub fn instantiate(name: ModuleName, config: &crate::config::Config) -> Result<Arc<dyn Module>> {
    let _ = config;
    match name {
        #[cfg(feature = "geocoder")]
        ModuleName::Geocoder => Ok(Arc::new(geko_module_geocoder::GeocoderModule::new(
            crate::sources::segment_opener(config.serve.preload),
            geko_module_geocoder::GeocoderConfig {
                countries: config.served_countries()?,
                search_budget: budget(config.serve.search_budget_ms),
                autocomplete_budget: budget(config.serve.autocomplete_budget_ms),
                slow_query: budget(config.serve.slow_query_ms),
                same_entity_m: config.serve.same_entity_m,
                batch_max: config.serve.batch_max,
                cache_entries: config.serve.cache_entries,
                ..Default::default()
            },
        ))),
        #[cfg(feature = "tiles")]
        ModuleName::Tiles => Ok(Arc::new(geko_module_tiles::TilesModule::new())),
        #[allow(unreachable_patterns)]
        other => Err(GekoError::Config(format!("module `{other}` is not compiled into this binary"))),
    }
}

/// A budget in milliseconds from `geko.toml`, 0 meaning none.
#[allow(dead_code)]
fn budget(ms: u64) -> Option<std::time::Duration> {
    (ms > 0).then(|| std::time::Duration::from_millis(ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(_: ModuleName) -> bool {
        true
    }

    #[test]
    fn config_selects_modules() {
        let cfg = ModulesConfig { geocoder: true, tiles: false };
        assert_eq!(resolve(&cfg, None, all).unwrap(), vec![ModuleName::Geocoder]);
    }

    #[test]
    fn cli_overrides_config() {
        let cfg = ModulesConfig { geocoder: true, tiles: false };
        assert_eq!(resolve(&cfg, Some(&[ModuleName::Tiles]), all).unwrap(), vec![ModuleName::Tiles]);
    }

    #[test]
    fn fails_when_no_module_is_active() {
        let cfg = ModulesConfig { geocoder: false, tiles: false };
        assert!(resolve(&cfg, None, all).is_err());
        assert!(resolve(&ModulesConfig::default(), Some(&[]), all).is_err());
    }

    #[test]
    fn fails_when_active_module_is_not_compiled() {
        let err = resolve(&ModulesConfig::default(), None, |m| m == ModuleName::Geocoder).unwrap_err();
        assert!(err.to_string().contains("`tiles` feature"), "{err}");
    }
}
