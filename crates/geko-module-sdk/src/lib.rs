//! What geko (the host) and its modules share (docs/MODULES_PLAN.md): the contract a module
//! implements (`module`), the few types both sides speak (country codes, scope, bundle manifest,
//! downloads) and the helpers every module needs to build incrementally. Nothing here belongs to
//! one module: what only a module uses lives in the module.

pub mod bundle;
pub mod country;
pub mod download;
pub mod dylib;
pub mod error;
pub mod incremental;
pub mod module;
#[cfg(unix)]
pub mod process;
pub mod progress;
pub mod scope;
pub mod stages;
pub mod wire;

pub use error::{GekoError, Result};

#[cfg(feature = "conformance")]
pub mod conformance;
