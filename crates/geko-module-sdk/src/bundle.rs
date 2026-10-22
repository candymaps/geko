//! Bundle manifest (PROJECT.md §11): what a published, immutable bundle contains. The host
//! publishes it from what every module built; each module keeps in its own entry (`meta`) what
//! it needs to load its part, which the host never reads.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::module::ModuleName;
use crate::{GekoError, Result};

pub const MANIFEST: &str = "manifest.json";
/// Symlink in the bundles directory pointing at the version being served.
pub const CURRENT: &str = "current";
/// File in the bundles directory holding the name of the current version: a symlink cannot be
/// read over HTTP, where another machine pulls bundles from (`geko serve` with `bundles_url`).
pub const CURRENT_VERSION: &str = "current.version";
/// Version of the layout of the manifest. 2: one entry per module, with the version of the
/// module that built its part; 1 listed the segments of the geocoder.
pub const FORMAT: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Relative to the bundle root.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseNotice {
    pub source: String,
    pub name: String,
    pub attribution: String,
    pub url: Option<String>,
}

/// The part of a bundle that one module built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleModule {
    pub name: ModuleName,
    /// The version of the module that built it: the one that serves it.
    pub version: String,
    /// The contract that module implements (`module::CONTRACT`).
    pub contract: u32,
    /// What the module needs to load its part, as it wrote it.
    #[serde(default)]
    pub meta: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub format: u32,
    pub version: String,
    pub created_unix: u64,
    pub modules: Vec<BundleModule>,
    pub licenses: Vec<LicenseNotice>,
    pub artifacts: Vec<Artifact>,
}

/// The version `current` points at, if any: a symlink on Unix, a file holding the name elsewhere.
pub fn current_version(bundles: &Path) -> Option<String> {
    let link = bundles.join(CURRENT);
    if let Ok(target) = std::fs::read_link(&link) {
        return target.file_name().map(|n| n.to_string_lossy().into_owned());
    }
    std::fs::read_to_string(link).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

impl BundleManifest {
    pub fn read(bundle: &Path) -> Result<Self> {
        let path = bundle.join(MANIFEST);
        let bytes = std::fs::read(&path).map_err(|e| GekoError::Data(format!("{}: {e}", path.display())))?;
        // The format first, so that a bundle of another layout says so rather than fail on a field.
        #[derive(Deserialize)]
        struct Format {
            #[serde(default = "first")]
            format: u32,
        }
        fn first() -> u32 {
            1
        }
        let format = serde_json::from_slice::<Format>(&bytes).map_err(|e| GekoError::Data(format!("{}: {e}", path.display())))?.format;
        if format != FORMAT {
            return Err(GekoError::Incompatible(format!(
                "{}: bundle format {format}, and this geko reads format {FORMAT}; build the bundle again with this geko",
                path.display()
            )));
        }
        serde_json::from_slice(&bytes).map_err(|e| GekoError::Data(format!("{}: {e}", path.display())))
    }

    pub fn write(&self, bundle: &Path) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| GekoError::Other(e.into()))?;
        std::fs::write(bundle.join(MANIFEST), bytes)?;
        Ok(())
    }

    /// The entry of the module `name`, if this bundle has a part of it.
    pub fn module(&self, name: &ModuleName) -> Option<&BundleModule> {
        self.modules.iter().find(|m| &m.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_of_another_format_says_so() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MANIFEST), r#"{"version":"v1","created_unix":0,"modules":["geocoder"],"segments":[],"licenses":[],"artifacts":[]}"#).unwrap();
        let err = BundleManifest::read(dir.path()).unwrap_err().to_string();
        assert!(err.contains("bundle format 1") && err.contains("build the bundle again"), "{err}");

        let manifest = BundleManifest {
            format: FORMAT,
            version: "v2".into(),
            created_unix: 1,
            modules: vec![BundleModule { name: ModuleName::new("geocoder").unwrap(), version: "1.0.0".into(), contract: 1, meta: serde_json::json!({"a": 1}) }],
            licenses: vec![],
            artifacts: vec![],
        };
        manifest.write(dir.path()).unwrap();
        let read = BundleManifest::read(dir.path()).unwrap();
        assert_eq!(read, manifest);
        assert_eq!(read.module(&ModuleName::new("geocoder").unwrap()).unwrap().meta["a"], 1);
        assert!(read.module(&ModuleName::new("tiles").unwrap()).is_none());
    }
}
