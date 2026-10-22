//! The modules the host downloads (docs/MODULES_PLAN.md, «Versiones y compatibilidad»): signed
//! packages in a registry, by default `ghcr.io/candymaps/geko-<module>`, each version in its own
//! folder, `<data>/modules/<module>/<version>/`. The host fixes the contract, not the version: it
//! takes the newest module of the contract it speaks (tag `contract-<n>-<platform>`), or the version
//! fixed in `[modules.<module>] version` (tag `<version>-<platform>`), or that of the bundle it
//! serves. Before downloading a version it reads its metadata from the manifest (`geko.*`
//! annotations), and refuses one of another contract or platform; after, the signed `module.toml`.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use geko_artifact::registry::{self, digest_of, partial_of, pull_verified, replace};
use geko_artifact::signature::{self, MODULE, platform};
use geko_fetch::oci::{OciClient, Reference};
use geko_module_sdk::module::{CONTRACT, ModuleName};
use geko_module_sdk::{GekoError, Result};
use serde::Deserialize;

/// The annotations of the manifest of a published module (ci/modules.sh).
pub const VERSION: &str = "geko.version";
pub const CONTRACT_KEY: &str = "geko.contract";
pub const PLATFORM: &str = "geko.platform";

pub fn default_registry(module: &ModuleName) -> String {
    format!("ghcr.io/candymaps/geko-{module}")
}

/// How the host runs a package: always loaded as a library into its process (`DylibModule`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dylib,
}

/// `module.toml`: what a package of a module says of itself. A local one (`GEKO_MODULES`) needs only
/// `kind` and `entry`; a published one says all.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub kind: Kind,
    /// The library to load, relative to the package.
    pub entry: PathBuf,
    pub name: Option<ModuleName>,
    pub version: Option<String>,
    pub contract: Option<u32>,
    pub platform: Option<String>,
}

impl Package {
    pub fn read(dir: &Path) -> Result<Self> {
        let file = dir.join("module.toml");
        let text = std::fs::read_to_string(&file).map_err(|e| GekoError::Config(format!("cannot read {}: {e}", file.display())))?;
        toml::from_str(&text).map_err(|e| GekoError::Config(format!("{}: {e}", file.display())))
    }

    /// Whether it is the module `name`, of `version` if given, for the contract and the platform of
    /// this geko.
    fn check(&self, dir: &Path, name: &ModuleName, version: Option<&str>) -> Result<()> {
        let bad = |what: String| Err(GekoError::Incompatible(format!("{}: {what}", dir.display())));
        if self.name.as_ref() != Some(name) {
            return bad(format!("it is not the module `{name}`"));
        }
        if self.contract != Some(CONTRACT) {
            return bad(format!("it implements contract {}, and this geko speaks contract {CONTRACT}", self.contract.map_or("?".into(), |c| c.to_string())));
        }
        if self.platform.as_deref() != Some(&platform()) {
            return bad(format!("it is for {}, and this geko runs on {}", self.platform.as_deref().unwrap_or("?"), platform()));
        }
        if let Some(v) = version
            && self.version.as_deref() != Some(v)
        {
            return bad(format!("it is version {}, not {v}", self.version.as_deref().unwrap_or("?")));
        }
        Ok(())
    }
}

/// A package of a module, ready to run, and how it was found.
#[derive(Debug, Clone)]
pub struct Found {
    pub dir: PathBuf,
    pub package: Package,
    pub how: String,
}

/// Orders versions as numbers (`1.10.0` after `1.9.2`), and otherwise as text.
fn by_version(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| v.split(['.', '-', '+']).map(|p| p.parse::<u64>().ok()).collect::<Vec<_>>();
    parts(a).cmp(&parts(b)).then_with(|| a.cmp(b))
}

pub struct Store {
    /// `<data>/modules`.
    pub root: PathBuf,
    pub offline: bool,
}

impl Store {
    pub fn new(data_dir: &Path, offline: bool) -> Self {
        Self { root: data_dir.join("modules"), offline }
    }

    fn reference(registry: &str, tag: &str) -> Result<Reference> {
        Reference::parse(&format!("{}:{tag}-{}", registry.trim_end_matches('/'), platform()))
    }

    /// The package of `name`: of `version`, or the newest of the contract of this geko.
    pub fn find(&self, name: &ModuleName, registry: &str, version: Option<&str>) -> Result<Found> {
        match version {
            Some(version) => self.exact(name, registry, version),
            None => self.latest(name, registry),
        }
    }

    /// A downloaded copy, if it is whole, signed and the right one.
    fn kept(&self, dir: &Path, name: &ModuleName, version: Option<&str>) -> Result<Package> {
        signature::verify_trusted(dir, MODULE)?;
        let package = Package::read(dir)?;
        package.check(dir, name, version)?;
        Ok(package)
    }

    /// The newest downloaded copy that this geko can run.
    fn newest_kept(&self, name: &ModuleName) -> Option<Found> {
        let entries = std::fs::read_dir(self.root.join(name.as_str())).ok()?;
        let mut kept: Vec<(String, PathBuf, Package)> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|dir| !dir.file_name().unwrap_or_default().to_string_lossy().starts_with('.'))
            .filter_map(|dir| {
                let package = self.kept(&dir, name, None).ok()?;
                Some((package.version.clone()?, dir, package))
            })
            .collect();
        kept.sort_by(|a, b| by_version(&a.0, &b.0));
        kept.pop().map(|(_, dir, package)| Found { dir, package, how: String::new() })
    }

    fn exact(&self, name: &ModuleName, registry: &str, version: &str) -> Result<Found> {
        let dir = self.root.join(name.as_str()).join(version);
        if dir.join("module.toml").is_file() {
            // A version, once published, never changes: what is on disk is it.
            let package = self.kept(&dir, name, Some(version))?;
            return Ok(Found { dir, package, how: format!("{version}, kept") });
        }
        if self.offline {
            return Err(GekoError::Config(format!("offline mode: module `{name}` {version} is not in {}", dir.display())));
        }
        let reference = Self::reference(registry, version)?;
        let client = OciClient::new();
        let Some((_, annotations)) = client.annotations(&reference).map_err(|e| GekoError::Data(format!("cannot ask {reference}: {e}")))? else {
            return Err(GekoError::Config(format!(
                "module `{name}` {version} is not published for {} ({reference}): remove `[modules.{name}] version` to use the newest compatible one",
                platform()
            )));
        };
        compatible(&annotations, &reference, name)?;
        self.download(&client, &reference, &dir, name, Some(version))?;
        Ok(Found { package: Package::read(&dir)?, dir, how: format!("{version}, downloaded") })
    }

    fn latest(&self, name: &ModuleName, registry: &str) -> Result<Found> {
        let fallback = |why: String| match self.newest_kept(name) {
            Some(found) => {
                tracing::warn!(module = %name, version = found.package.version.as_deref().unwrap_or("?"), "{why}; using the copy on disk");
                let how = format!("{}, kept ({why})", found.package.version.as_deref().unwrap_or("?"));
                Ok(Found { how, ..found })
            }
            None => Err(GekoError::Config(format!("module `{name}`: {why}, and there is no copy of it in {}", self.root.join(name.as_str()).display()))),
        };
        if self.offline {
            return fallback("offline mode".into());
        }
        let reference = Self::reference(registry, &format!("contract-{CONTRACT}"))?;
        let client = OciClient::new();
        let (digest, annotations) = match client.annotations(&reference) {
            Ok(Some(found)) => found,
            Ok(None) => return fallback(format!("no version is published for contract {CONTRACT} on {} ({reference})", platform())),
            Err(e) => return fallback(format!("cannot ask {reference} ({e})")),
        };
        compatible(&annotations, &reference, name)?;
        let version = annotations.get(VERSION).ok_or_else(|| GekoError::Data(format!("{reference} has no {VERSION}")))?;
        let dir = self.root.join(name.as_str()).join(version);
        if digest_of(&dir).as_deref() == Some(digest.as_str())
            && let Ok(package) = self.kept(&dir, name, Some(version))
        {
            return Ok(Found { dir, package, how: format!("{version}, newest of contract {CONTRACT}, up to date") });
        }
        self.download(&client, &reference, &dir, name, Some(version))?;
        Ok(Found { package: Package::read(&dir)?, dir, how: format!("{version}, newest of contract {CONTRACT}, downloaded") })
    }

    fn download(&self, client: &OciClient, reference: &Reference, dir: &Path, name: &ModuleName, version: Option<&str>) -> Result<()> {
        let partial = partial_of(dir);
        std::fs::create_dir_all(partial.parent().expect("under the store"))?;
        pull_verified(client, reference, MODULE, &partial)?;
        if let Err(e) = Package::read(&partial).and_then(|p| p.check(&partial, name, version)) {
            let _ = std::fs::remove_dir_all(&partial);
            return Err(e);
        }
        replace(&partial, dir)?;
        tracing::info!(module = %name, %reference, "module downloaded");
        Ok(())
    }

    /// Removes the downloaded versions of every module but those `keep` names (module, version).
    pub fn purge(&self, keep: &[(String, String)]) -> Result<Vec<PathBuf>> {
        let mut removed = Vec::new();
        let Ok(modules) = std::fs::read_dir(&self.root) else { return Ok(removed) };
        for module in modules {
            let module = module?.path();
            let name = module.file_name().unwrap_or_default().to_string_lossy().into_owned();
            removed.extend(registry::purge(&module, |dir| {
                let version = dir.file_name().unwrap_or_default().to_string_lossy();
                keep.iter().any(|(m, v)| *m == name && *v == version)
            })?);
        }
        Ok(removed)
    }
}

/// Refuses, before downloading it, a module of another contract or platform.
fn compatible(annotations: &HashMap<String, String>, reference: &Reference, name: &ModuleName) -> Result<()> {
    if let Some(contract) = annotations.get(CONTRACT_KEY)
        && contract != &CONTRACT.to_string()
    {
        return Err(GekoError::Incompatible(format!(
            "module `{name}` of {reference} implements contract {contract}, and this geko speaks contract {CONTRACT}: remove `[modules.{name}] version` to use the newest compatible one"
        )));
    }
    if let Some(p) = annotations.get(PLATFORM)
        && *p != platform()
    {
        return Err(GekoError::Incompatible(format!("module `{name}` of {reference} is for {p}, and this geko runs on {}", platform())));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name() -> ModuleName {
        ModuleName::new("geocoder").unwrap()
    }

    fn package(dir: &Path, version: &str, contract: u32) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("module.toml"),
            format!("kind = \"dylib\"\nentry = \"libgeko_geocoder.so\"\nname = \"geocoder\"\nversion = \"{version}\"\ncontract = {contract}\nplatform = \"{}\"\n", platform()),
        )
        .unwrap();
        std::fs::write(dir.join("libgeko_geocoder.so"), "").unwrap();
        std::fs::write(dir.join(".digest"), "sha256:x").unwrap();
    }

    #[test]
    fn versions_order_as_numbers() {
        let mut v = vec!["1.10.0", "1.9.2", "0.1.0", "1.9.10"];
        v.sort_by(|a, b| by_version(a, b));
        assert_eq!(v, ["0.1.0", "1.9.2", "1.9.10", "1.10.0"]);
    }

    #[test]
    fn a_package_of_another_contract_or_version_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        package(tmp.path(), "1.0.0", CONTRACT + 1);
        let p = Package::read(tmp.path()).unwrap();
        assert!(matches!(p.check(tmp.path(), &name(), None), Err(GekoError::Incompatible(e)) if e.contains("contract")));
        package(tmp.path(), "1.0.0", CONTRACT);
        let p = Package::read(tmp.path()).unwrap();
        assert!(p.check(tmp.path(), &name(), Some("1.0.0")).is_ok());
        assert!(p.check(tmp.path(), &name(), Some("1.0.1")).is_err());
        assert!(p.check(tmp.path(), &ModuleName::new("tiles").unwrap(), None).is_err());
    }

    #[test]
    fn offline_without_a_copy_is_an_error_and_an_unsigned_copy_is_not_used() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path(), true);
        let err = store.find(&name(), "ghcr.io/x/geko-geocoder", None).unwrap_err();
        assert!(err.to_string().contains("offline mode"), "{err}");
        package(&tmp.path().join("modules/geocoder/1.0.0"), "1.0.0", CONTRACT);
        assert!(store.find(&name(), "ghcr.io/x/geko-geocoder", None).is_err(), "not signed");
        let err = store.find(&name(), "ghcr.io/x/geko-geocoder", Some("2.0.0")).unwrap_err();
        assert!(err.to_string().contains("offline mode: module `geocoder` 2.0.0"), "{err}");
    }

    #[test]
    fn an_unreachable_registry_without_a_copy_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = Store::new(tmp.path(), false).find(&name(), "127.0.0.1:1/geko-geocoder", None).unwrap_err();
        assert!(err.to_string().contains("cannot ask"), "{err}");
    }

    #[test]
    fn purge_keeps_what_is_named() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path(), true);
        for v in ["1.0.0", "1.1.0", "1.2.0"] {
            package(&store.root.join("geocoder").join(v), v, CONTRACT);
        }
        let removed = store.purge(&[("geocoder".into(), "1.1.0".into())]).unwrap();
        assert_eq!(removed.len(), 2);
        assert!(store.root.join("geocoder/1.1.0").exists());
    }
}
