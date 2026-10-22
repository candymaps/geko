//! Conformance suites of the ports of the SDK (PROJECT.md §2.1 rule 3, §12): every
//! implementation runs the suite of its port from its own tests (`module`: every module, and the
//! host through each adapter). Behind the `conformance` feature, for dev-dependencies only.

pub mod download;
pub mod module;
