//! Signed artifacts in an OCI registry (docs/MODULES_PLAN.md, «Descarga y verificación»): the
//! signature of their files (`signature`) and their download, checked before it replaces the copy
//! in use, with the copy on disk when the registry cannot be reached (`registry`). The host uses it
//! for its modules; the geocoder, for its country packs.

pub mod registry;
pub mod signature;
