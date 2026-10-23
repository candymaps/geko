//! Download of signed artifacts from the registry where they are published (PROJECT.md §7.1): the
//! country packs of the geocoder and the modules of the host. Each one is downloaded into a folder of
//! its own, checked against its signature before it replaces the previous copy, and kept with the
//! digest of its manifest (`.digest`), so that it is downloaded again only when it changes.
//!
//! A copy made by hand (without `.digest`) is used as it is: geko never replaces what it did not
//! download, nor removes it.

use std::path::{Path, PathBuf};

use geko_fetch::oci::{OciClient, Reference};
use geko_module_sdk::{GekoError, Result};

use crate::signature::{self, Kind};

/// Beside the files of a downloaded artifact: the digest of its manifest in the registry.
pub const DIGEST_FILE: &str = ".digest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Downloaded,
    UpToDate,
    /// The local copy is used: copied by hand, offline, or the registry could not be asked.
    Local(String),
}

/// Whether the artifact in `dir` was downloaded by geko.
pub fn downloaded(dir: &Path) -> bool {
    dir.join(DIGEST_FILE).is_file()
}

/// The digest of the artifact downloaded into `dir`.
pub fn digest_of(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(DIGEST_FILE)).ok().map(|d| d.trim().to_owned())
}

/// Downloads `reference` into `partial` (emptied first) and checks its signature as `kind`; leaves
/// it with its `.digest`, ready to `replace` the copy in use. On error, nothing is left.
pub fn pull_verified(client: &OciClient, reference: &Reference, kind: Kind, partial: &Path) -> Result<String> {
    if partial.exists() {
        std::fs::remove_dir_all(partial)?;
    }
    let artifact = client.pull(reference)?.ok_or_else(|| GekoError::Data(format!("{reference} disappeared while it was downloaded")))?;
    let written = (|| {
        for (path, bytes) in &artifact.files {
            let file = partial.join(path);
            std::fs::create_dir_all(file.parent().expect("a file has a folder"))?;
            std::fs::write(file, bytes)?;
        }
        signature::verify_trusted(partial, kind).map_err(|e| GekoError::Data(format!("{reference} is rejected: {e}")))?;
        std::fs::write(partial.join(DIGEST_FILE), &artifact.digest)?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(partial);
        return Err(e);
    }
    Ok(artifact.digest)
}

/// The folder beside `dir` where its next copy is downloaded.
pub fn partial_of(dir: &Path) -> PathBuf {
    dir.with_file_name(format!(".{}.partial", dir.file_name().unwrap_or_default().to_string_lossy()))
}

/// Swaps `new` into `dir`, keeping the old copy until the new one is in place.
pub fn replace(new: &Path, dir: &Path) -> Result<()> {
    let old: PathBuf = dir.with_file_name(format!(".{}.old", dir.file_name().unwrap_or_default().to_string_lossy()));
    if old.exists() {
        std::fs::remove_dir_all(&old)?;
    }
    if dir.exists() {
        std::fs::rename(dir, &old)?;
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(new, dir)?;
    if old.exists() {
        std::fs::remove_dir_all(&old)?;
    }
    Ok(())
}

/// Makes sure that `dir` holds the artifact published at `reference`, or a copy of it made by hand
/// (one with `marker`, its main file, and without `.digest`). `what` names it in errors ("country
/// pack for ES").
pub fn ensure(dir: &Path, reference: &Reference, kind: Kind, marker: &str, offline: bool, what: &str) -> Result<Outcome> {
    let local = dir.join(marker).is_file();
    if local && !downloaded(dir) {
        return Ok(Outcome::Local(format!("{} was copied by hand", dir.display())));
    }
    if offline {
        return match local {
            true => Ok(Outcome::Local("offline".into())),
            false => Err(GekoError::Config(format!("offline mode: no {what} in {}", dir.display()))),
        };
    }
    let client = OciClient::new();
    let digest = match client.digest(reference) {
        Ok(Some(digest)) => digest,
        Ok(None) if local => {
            tracing::warn!(%reference, "the {what} is no longer published; using the local copy");
            return Ok(Outcome::Local("not published".into()));
        }
        Ok(None) => return Err(GekoError::Config(format!("no {what}: {reference} is not published, or not public"))),
        Err(e) if local => {
            tracing::warn!(%reference, error = %e, "cannot reach the registry of the {what}; using the local copy");
            return Ok(Outcome::Local("registry unreachable".into()));
        }
        Err(e) => return Err(GekoError::Data(format!("the {what} cannot be downloaded: {e}"))),
    };
    if local && digest_of(dir).is_some_and(|d| d == digest) {
        return Ok(Outcome::UpToDate);
    }
    let partial = partial_of(dir);
    pull_verified(&client, reference, kind, &partial)?;
    replace(&partial, dir)?;
    tracing::info!(%reference, "{what} downloaded");
    Ok(Outcome::Downloaded)
}

/// Removes the downloaded artifacts in the folders of `root` that `keep` does not keep. Copies made
/// by hand stay. Returns the folders removed.
pub fn purge(root: &Path, keep: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return Ok(removed) };
    let mut dirs: Vec<PathBuf> = entries.map(|e| e.map(|e| e.path())).collect::<std::io::Result<_>>()?;
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if name.starts_with('.') || !downloaded(&dir) || keep(&dir) {
            continue;
        }
        std::fs::remove_dir_all(&dir)?;
        removed.push(dir);
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACK: Kind = Kind { domain: "geko-country-pack", format: 2, name: "country pack" };

    fn reference(r: &str) -> Reference {
        Reference::parse(r).unwrap()
    }

    #[test]
    fn a_copy_made_by_hand_is_used_as_it_is_and_never_purged() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("es")).unwrap();
        std::fs::write(tmp.path().join("es/pack.toml"), "").unwrap();
        let outcome = ensure(&tmp.path().join("es"), &reference("127.0.0.1:1/none:x"), PACK, "pack.toml", false, "country pack for ES").unwrap();
        assert!(matches!(outcome, Outcome::Local(_)), "{outcome:?}");
        for dir in ["lu", "fr"] {
            std::fs::create_dir_all(tmp.path().join(dir)).unwrap();
            std::fs::write(tmp.path().join(dir).join(DIGEST_FILE), "sha256:x").unwrap();
        }
        let removed = purge(tmp.path(), |d| d.ends_with("lu")).unwrap();
        assert_eq!(removed, [tmp.path().join("fr")]);
        assert_eq!(purge(tmp.path(), |_| false).unwrap(), [tmp.path().join("lu")]);
        assert!(tmp.path().join("es/pack.toml").exists());
    }

    #[test]
    fn offline_without_a_copy_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = ensure(&tmp.path().join("fr"), &reference("ghcr.io/x/fr:format-2"), PACK, "pack.toml", true, "country pack for FR").unwrap_err();
        assert!(err.to_string().contains("offline mode: no country pack for FR"), "{err}");
    }

    #[test]
    fn an_unreachable_registry_without_a_copy_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = ensure(&tmp.path().join("fr"), &reference("127.0.0.1:1/countries/fr:format-2"), PACK, "pack.toml", false, "country pack for FR").unwrap_err();
        assert!(err.to_string().contains("cannot be downloaded"), "{err}");
    }

    #[test]
    fn replace_swaps_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, new) = (tmp.path().join("a/1.0.0"), tmp.path().join("new"));
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("x"), "new").unwrap();
        replace(&new, &dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("x")).unwrap(), "new");
        assert!(!new.exists());
    }
}
