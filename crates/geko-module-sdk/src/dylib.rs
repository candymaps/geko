//! A module loaded by the host as a library (docs/MODULES_PLAN.md, `DylibModule`): a `cdylib` that
//! exports the functions of `export_module!`, a minimal `extern "C"` boundary across which the
//! messages of `wire` travel as bytes. Each side frees
//! the memory it allocated (`geko_module_free`), and no panic crosses the boundary.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::module::{CONTRACT, Module, Open};
use crate::wire::{Call, Reply, answer, frame, guarded, json, read_frame};
use crate::{GekoError, Result};

/// Bytes allocated by the module, which only `geko_module_free` releases.
#[repr(C)]
pub struct Buffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl Buffer {
    fn new(bytes: Vec<u8>) -> Self {
        let mut bytes = std::mem::ManuallyDrop::new(bytes);
        Self { ptr: bytes.as_mut_ptr(), len: bytes.len(), cap: bytes.capacity() }
    }

    /// # Safety
    /// `self` was made by `Buffer::new` in this same library, and is not used again.
    unsafe fn into_vec(self) -> Vec<u8> {
        unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) }
    }

    /// The bytes, while the buffer lives.
    ///
    /// # Safety
    /// `self` was made by `Buffer::new` and not freed yet.
    pub unsafe fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

pub const HELLO: &[u8] = b"geko_module_hello\0";
pub const OPEN: &[u8] = b"geko_module_open\0";
pub const CALL: &[u8] = b"geko_module_call\0";
pub const FREE: &[u8] = b"geko_module_free\0";
pub const CLOSE: &[u8] = b"geko_module_close\0";

/// Writes the greeting, with the contract, to `out`.
pub type HelloFn = unsafe extern "C" fn(out: *mut Buffer);
/// Opens the module with the frame of `Call::Open`; writes its answer to `out` and returns the
/// module, or null if it did not open.
pub type OpenFn = unsafe extern "C" fn(input: *const u8, len: usize, out: *mut Buffer) -> *mut c_void;
/// Answers the frame of a call to `out`. Safe to call from several threads at once.
pub type CallFn = unsafe extern "C" fn(module: *mut c_void, input: *const u8, len: usize, out: *mut Buffer);
pub type FreeFn = unsafe extern "C" fn(buffer: Buffer);
/// Drops the module; nothing calls it with it afterwards.
pub type CloseFn = unsafe extern "C" fn(module: *mut c_void);

fn reply(reply: &Reply, payload: &[u8]) -> Buffer {
    Buffer::new(frame(0, reply, payload).expect("a reply serializes"))
}

fn error(e: &GekoError) -> Buffer {
    reply(&Reply::Err(e.into()), &[])
}

/// What `geko_module_hello` does.
pub fn hello() -> Buffer {
    reply(&Reply::Hello { contract: CONTRACT }, &[])
}

/// What `geko_module_open` does.
///
/// # Safety
/// `input` points to `len` readable bytes.
pub unsafe fn open(open: Open, input: *const u8, len: usize, out: *mut Buffer) -> *mut c_void {
    let input = unsafe { std::slice::from_raw_parts(input, len) };
    let opened = guarded(|| {
        let (_, call, _) = read_frame::<Call>(&mut &input[..])?;
        let Call::Open(context) = call else { return Err(GekoError::Data("the first call of the host must open the module".into())) };
        open(context)
    });
    let (answer, module) = match opened {
        Ok(module) => (reply(&Reply::Ok(json(&module.describe())), &[]), Box::into_raw(Box::new(module)).cast()),
        Err(e) => (error(&e), std::ptr::null_mut()),
    };
    unsafe { out.write(answer) };
    module
}

/// What `geko_module_call` does.
///
/// # Safety
/// `module` was returned by `open` and not closed; `input` points to `len` readable bytes.
pub unsafe fn call(module: *mut c_void, input: *const u8, len: usize, out: *mut Buffer) {
    let module: &dyn Module = unsafe { &**module.cast::<Box<dyn Module>>() };
    let input = unsafe { std::slice::from_raw_parts(input, len) };
    let answered = catch_unwind(AssertUnwindSafe(|| -> Result<Buffer> {
        let (_, call, payload) = read_frame::<Call>(&mut &input[..])?;
        let (r, payload) = answer(module, call, payload);
        Ok(reply(&r, &payload))
    }));
    let buffer = match answered {
        Ok(Ok(buffer)) => buffer,
        Ok(Err(e)) => error(&e),
        Err(_) => error(&GekoError::Other("the module panicked".into())),
    };
    unsafe { out.write(buffer) };
}

/// What `geko_module_free` does.
///
/// # Safety
/// `buffer` was written by a function of this library and is not used again.
pub unsafe fn free(buffer: Buffer) {
    drop(unsafe { buffer.into_vec() });
}

/// What `geko_module_close` does.
///
/// # Safety
/// `module` was returned by `open` and is not used again.
pub unsafe fn close(module: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(module.cast::<Box<dyn Module>>()) })));
}

/// Exports from a `cdylib` the functions the host looks for, for the module that `$open` opens.
#[macro_export]
macro_rules! export_module {
    ($open:path) => {
        /// # Safety
        /// As `geko_module_sdk::dylib::HelloFn`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn geko_module_hello(out: *mut $crate::dylib::Buffer) {
            unsafe { out.write($crate::dylib::hello()) }
        }

        /// # Safety
        /// As `geko_module_sdk::dylib::OpenFn`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn geko_module_open(input: *const u8, len: usize, out: *mut $crate::dylib::Buffer) -> *mut ::std::ffi::c_void {
            unsafe { $crate::dylib::open($open, input, len, out) }
        }

        /// # Safety
        /// As `geko_module_sdk::dylib::CallFn`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn geko_module_call(module: *mut ::std::ffi::c_void, input: *const u8, len: usize, out: *mut $crate::dylib::Buffer) {
            unsafe { $crate::dylib::call(module, input, len, out) }
        }

        /// # Safety
        /// As `geko_module_sdk::dylib::FreeFn`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn geko_module_free(buffer: $crate::dylib::Buffer) {
            unsafe { $crate::dylib::free(buffer) }
        }

        /// # Safety
        /// As `geko_module_sdk::dylib::CloseFn`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn geko_module_close(module: *mut ::std::ffi::c_void) {
            unsafe { $crate::dylib::close(module) }
        }
    };
}
