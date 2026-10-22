//! The messages of the contract between the host and a module that is not linked into it
//! (docs/MODULES_PLAN.md): a process of its own (`process`) or a library loaded at run time
//! (`dylib`). Both speak the same messages, so that one protocol serves the two.
//!
//! A frame is the length of its header and that of its payload (`u32`, little-endian), the header
//! as JSON and the payload as raw bytes: the bodies of requests and responses, which JSON would
//! inflate. The first message is the module's greeting with its contract, so that a host that does
//! not speak it refuses the module before asking it anything; the first call opens it.

use std::io::{self, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde::{Deserialize, Serialize};

use crate::module::{BundleView, Context, Module, Purge, Request};
use crate::{GekoError, Result};

/// A call of the host: one per method of `Module`, but `describe`, answered once by `Open`.
#[derive(Debug, Serialize, Deserialize)]
pub enum Call {
    Open(Context),
    Prepare,
    Plan,
    Fetch { refresh: bool },
    Ready,
    Build { force: bool, refresh: bool },
    Published,
    NextBuild,
    Purge(Purge),
    Load(BundleView),
    /// Its body travels as the payload of the frame.
    Handle(Request),
    Metrics,
    Progress,
    Shutdown,
    Command { name: String, args: Vec<String> },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Reply {
    Hello { contract: u32 },
    /// What the method returned; the body of a `Response`, as the payload.
    Ok(serde_json::Value),
    Err(WireError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorKind {
    InvalidInput,
    Data,
    Incompatible,
    Config,
    Other,
}

/// A `GekoError` across the boundary, which comes back as the same variant (I/O errors, as `Other`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireError {
    pub kind: ErrorKind,
    pub message: String,
}

impl From<&GekoError> for WireError {
    fn from(e: &GekoError) -> Self {
        let (kind, message) = match e {
            GekoError::InvalidInput(m) => (ErrorKind::InvalidInput, m.clone()),
            GekoError::Data(m) => (ErrorKind::Data, m.clone()),
            GekoError::Incompatible(m) => (ErrorKind::Incompatible, m.clone()),
            GekoError::Config(m) => (ErrorKind::Config, m.clone()),
            GekoError::Io(_) | GekoError::Other(_) => (ErrorKind::Other, e.to_string()),
        };
        Self { kind, message }
    }
}

impl From<WireError> for GekoError {
    fn from(e: WireError) -> Self {
        match e.kind {
            ErrorKind::InvalidInput => GekoError::InvalidInput(e.message),
            ErrorKind::Data => GekoError::Data(e.message),
            ErrorKind::Incompatible => GekoError::Incompatible(e.message),
            ErrorKind::Config => GekoError::Config(e.message),
            ErrorKind::Other => GekoError::Other(e.message.into()),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    id: u64,
    msg: T,
}

pub(crate) fn json<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).expect("the types of the contract are plain data")
}

/// Writes a frame in a single write, so that the threads sharing a socket never interleave.
pub fn write_frame<T: Serialize>(w: &mut impl Write, id: u64, msg: &T, payload: &[u8]) -> io::Result<()> {
    w.write_all(&frame(id, msg, payload)?)
}

pub fn frame<T: Serialize>(id: u64, msg: &T, payload: &[u8]) -> io::Result<Vec<u8>> {
    let header = serde_json::to_vec(&Envelope { id, msg }).map_err(io::Error::other)?;
    let mut frame = Vec::with_capacity(8 + header.len() + payload.len());
    frame.extend_from_slice(&(header.len() as u32).to_le_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Reads a frame: its id, its message and its payload. At the end of the stream, `UnexpectedEof`.
pub fn read_frame<T: for<'de> Deserialize<'de>>(r: &mut impl Read) -> io::Result<(u64, T, Vec<u8>)> {
    let mut lengths = [0u8; 8];
    r.read_exact(&mut lengths)?;
    let header_len = u32::from_le_bytes(lengths[..4].try_into().expect("4 bytes")) as usize;
    let payload_len = u32::from_le_bytes(lengths[4..].try_into().expect("4 bytes")) as usize;
    let mut header = vec![0; header_len];
    r.read_exact(&mut header)?;
    let mut payload = vec![0; payload_len];
    r.read_exact(&mut payload)?;
    let envelope: Envelope<T> = serde_json::from_slice(&header).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok((envelope.id, envelope.msg, payload))
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic.downcast_ref::<&str>().copied().or_else(|| panic.downcast_ref::<String>().map(String::as_str)).unwrap_or("no message")
}

/// Runs `work`, with a panic turned into an error: one call that fails never takes down the module.
pub(crate) fn guarded<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|p| Err(GekoError::Other(format!("the module panicked: {}", panic_message(&*p)).into())))
}

/// What the module answers to a call, and the payload of the answer.
pub(crate) fn answer(module: &dyn Module, call: Call, payload: Vec<u8>) -> (Reply, Vec<u8>) {
    let done = guarded(|| {
        Ok(match call {
            Call::Open(_) => return Err(GekoError::Data("the module is already open".into())),
            Call::Prepare => (json(&module.prepare()?), Vec::new()),
            Call::Plan => (json(&module.plan()?), Vec::new()),
            Call::Fetch { refresh } => (json(&module.fetch(refresh)?), Vec::new()),
            Call::Ready => (json(&module.ready()?), Vec::new()),
            Call::Build { force, refresh } => {
                let _building = crate::progress::Building::start();
                (json(&module.build(force, refresh)?), Vec::new())
            }
            Call::Progress => (json(&module.progress()), Vec::new()),
            Call::Published => (json(&module.published()?), Vec::new()),
            Call::NextBuild => (json(&module.next_build()), Vec::new()),
            Call::Purge(what) => (json(&module.purge(what)?), Vec::new()),
            Call::Load(bundle) => (json(&module.load(&bundle)?), Vec::new()),
            Call::Handle(mut request) => {
                request.body = payload;
                let mut response = module.handle(request);
                let body = std::mem::take(&mut response.body);
                (json(&response), body)
            }
            Call::Metrics => {
                let mut out = String::new();
                module.metrics(&mut out);
                (json(&out), Vec::new())
            }
            Call::Shutdown => (json(&module.shutdown()?), Vec::new()),
            Call::Command { name, args } => (json(&module.command(&name, &args)?), Vec::new()),
        })
    });
    match done {
        Ok((value, payload)) => (Reply::Ok(value), payload),
        Err(e) => (Reply::Err((&e).into()), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_carry_header_and_payload() {
        let mut buf = Vec::new();
        write_frame(&mut buf, 7, &Call::Fetch { refresh: true }, b"\0bytes").unwrap();
        write_frame(&mut buf, 8, &Call::Metrics, &[]).unwrap();
        let mut r = &buf[..];
        let (id, call, payload) = read_frame::<Call>(&mut r).unwrap();
        assert_eq!((id, payload.as_slice()), (7, &b"\0bytes"[..]));
        assert!(matches!(call, Call::Fetch { refresh: true }));
        assert!(matches!(read_frame::<Call>(&mut r).unwrap(), (8, Call::Metrics, p) if p.is_empty()));
        assert_eq!(read_frame::<Call>(&mut r).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn errors_come_back_as_the_same_variant() {
        let back = GekoError::from(WireError::from(&GekoError::Config("bad key".into())));
        assert!(matches!(&back, GekoError::Config(m) if m == "bad key"), "{back}");
        let io = GekoError::from(std::io::Error::other("disk"));
        assert!(matches!(GekoError::from(WireError::from(&io)), GekoError::Other(m) if m.to_string() == "disk"));
    }

    #[test]
    fn a_panic_is_an_error() {
        let e = guarded::<()>(|| panic!("boom")).unwrap_err();
        assert!(e.to_string().contains("panicked: boom"), "{e}");
    }
}
