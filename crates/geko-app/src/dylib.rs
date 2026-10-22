//! `DylibModule` (docs/MODULES_PLAN.md): a module loaded as a library into the process of the host,
//! to which every call of `Module` is a message of `geko_module_sdk::wire` passed through the
//! functions it exports (`geko_module_sdk::dylib`). No hop between processes, but a library cannot
//! be unloaded safely: it stays loaded until the host exits, and a new version needs a restart.

use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::path::Path;

use geko_module_sdk::dylib::{self, Buffer, CallFn, CloseFn, FreeFn, HelloFn, OpenFn};
use geko_module_sdk::module::{CONTRACT, Context, Description};
use geko_module_sdk::wire::{Call, Reply, frame, read_frame};
use geko_module_sdk::{GekoError, Result};

use crate::remote::{Answer, Channel, Remote, decode};

/// The functions a module library exports.
#[derive(Clone, Copy)]
pub struct Functions {
    pub hello: HelloFn,
    pub open: OpenFn,
    pub call: CallFn,
    pub free: FreeFn,
    pub close: CloseFn,
}

pub struct Dylib {
    functions: Functions,
    module: *mut c_void,
}

// The module behind the pointer is a `dyn Module`, which is `Send + Sync`, and `geko_module_call`
// may be called from several threads at once.
unsafe impl Send for Dylib {}
unsafe impl Sync for Dylib {}

pub type DylibModule = Remote<Dylib>;

fn io(e: std::io::Error) -> GekoError {
    GekoError::Data(format!("unexpected answer of the module: {e}"))
}

impl Functions {
    /// Reads what a function of the library wrote to `out`, and gives it back to the library.
    fn take(&self, out: MaybeUninit<Buffer>) -> Result<(Reply, Vec<u8>)> {
        // Safety: every function of the library writes `out` before it returns.
        let out = unsafe { out.assume_init() };
        let read = read_frame::<Reply>(&mut unsafe { out.as_slice() }).map(|(_, reply, payload)| (reply, payload));
        unsafe { (self.free)(out) };
        read.map_err(io)
    }
}

/// Loads the library of a module and opens it with `context`.
pub fn load(path: &Path, context: Context) -> Result<DylibModule> {
    let failed = |e: libloading::Error| GekoError::Other(format!("cannot load the module {}: {e}", path.display()).into());
    // Safety: loading runs no code of the module beyond what a Rust library runs on load; its
    // functions are those of `export_module!`, checked by name, and with the contract right after.
    let library = unsafe { libloading::Library::new(path) }.map_err(failed)?;
    let functions = unsafe {
        Functions {
            hello: *library.get::<HelloFn>(dylib::HELLO).map_err(failed)?,
            open: *library.get::<OpenFn>(dylib::OPEN).map_err(failed)?,
            call: *library.get::<CallFn>(dylib::CALL).map_err(failed)?,
            free: *library.get::<FreeFn>(dylib::FREE).map_err(failed)?,
            close: *library.get::<CloseFn>(dylib::CLOSE).map_err(failed)?,
        }
    };
    // Never unloaded: the functions above, and whatever the module left running, live in it.
    std::mem::forget(library);
    open(functions, context)
}

/// Greets the module through `functions`, checks its contract and opens it with `context`.
pub fn open(functions: Functions, context: Context) -> Result<DylibModule> {
    let mut out = MaybeUninit::uninit();
    unsafe { (functions.hello)(out.as_mut_ptr()) };
    match functions.take(out)?.0 {
        Reply::Hello { contract } if contract == CONTRACT => {}
        Reply::Hello { contract } => return Err(GekoError::Incompatible(format!("the module implements contract {contract}, and this geko speaks contract {CONTRACT}"))),
        _ => return Err(GekoError::Data("the module did not greet the host".into())),
    }
    let input = frame(0, &Call::Open(context), &[]).map_err(io)?;
    let mut out = MaybeUninit::uninit();
    let module = unsafe { (functions.open)(input.as_ptr(), input.len(), out.as_mut_ptr()) };
    let description: Description = match functions.take(out)?.0 {
        Reply::Ok(value) if !module.is_null() => decode(value)?,
        Reply::Err(e) => return Err(e.into()),
        _ => return Err(GekoError::Data("the module did not open".into())),
    };
    Ok(Remote::new(Dylib { functions, module }, description))
}

impl Channel for Dylib {
    fn exchange(&self, call: &Call, payload: &[u8]) -> Answer {
        let input = frame(0, call, payload).map_err(io)?;
        let mut out = MaybeUninit::uninit();
        unsafe { (self.functions.call)(self.module, input.as_ptr(), input.len(), out.as_mut_ptr()) };
        match self.functions.take(out)? {
            (Reply::Ok(value), payload) => Ok((value, payload)),
            (Reply::Err(e), _) => Err(e.into()),
            (Reply::Hello { .. }, _) => Err(GekoError::Data("the module greeted the host again".into())),
        }
    }
}

impl Drop for Dylib {
    fn drop(&mut self) {
        unsafe { (self.functions.close)(self.module) };
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::remote::tests::context;

    mod fake {
        geko_module_sdk::export_module!(crate::remote::tests::open_fake);
    }

    mod refusing {
        fn open(_: geko_module_sdk::module::Context) -> geko_module_sdk::Result<Box<dyn geko_module_sdk::module::Module>> {
            Err(geko_module_sdk::GekoError::Config("bad key in geko-fake.toml".into()))
        }

        pub unsafe extern "C" fn geko_module_open(input: *const u8, len: usize, out: *mut geko_module_sdk::dylib::Buffer) -> *mut std::ffi::c_void {
            unsafe { geko_module_sdk::dylib::open(open, input, len, out) }
        }
    }

    /// The functions of the library, as the host finds them in it, but linked into the test.
    fn functions() -> Functions {
        Functions { hello: fake::geko_module_hello, open: fake::geko_module_open, call: fake::geko_module_call, free: fake::geko_module_free, close: fake::geko_module_close }
    }

    #[test]
    fn every_call_crosses_the_boundary() {
        crate::remote::tests::every_call_crosses(&open(functions(), context()).unwrap());
    }

    #[test]
    fn requests_are_answered_at_once() {
        crate::remote::tests::answered_at_once(Arc::new(open(functions(), context()).unwrap()));
    }

    #[test]
    fn a_panic_answers_500_and_the_module_goes_on() {
        crate::remote::tests::a_panic_answers_500(&open(functions(), context()).unwrap());
    }

    #[test]
    fn a_module_failing_to_open_is_refused() {
        let err = open(Functions { open: refusing::geko_module_open, ..functions() }, context()).err().unwrap();
        assert!(matches!(&err, GekoError::Config(e) if e.contains("bad key")), "{err}");
    }

    #[test]
    fn a_library_that_is_not_a_module_is_refused() {
        let err = load(Path::new("/nonexistent/libgeko_none.so"), context()).err().unwrap();
        assert!(err.to_string().contains("cannot load the module"), "{err}");
    }
}
