//! A module run as a process of its own (docs/MODULES_PLAN.md, `ProcessModule`): the host starts
//! the module's executable, which connects to the Unix socket named by `SOCKET`, and asks it
//! every call of `Module` as a message (`wire`). `serve` is the side of the module: the `main` of
//! its executable. The side of the host is in the host.

use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::module::{CONTRACT, Module, Open};
use crate::wire::{Call, Reply, answer, guarded, json, read_frame, write_frame};
use crate::{GekoError, Result};

/// The variable with the path of the socket the module connects to.
pub const SOCKET: &str = "GEKO_MODULE_SOCKET";

/// What the host waits for a module to shut down before it kills it.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct Writer(Arc<Mutex<UnixStream>>);

impl Writer {
    fn send(&self, id: u64, reply: &Reply, payload: &[u8]) -> io::Result<()> {
        write_frame(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()), id, reply, payload)
    }
}

type Job = (u64, Call, Vec<u8>);

/// The threads that answer requests and metrics, as many as the host may ask at once: one per core
/// on its pool and as many for the routes it answers where they arrive.
fn workers(module: &Arc<dyn Module>, writer: &Writer) -> mpsc::Sender<Job> {
    let (jobs, queue) = mpsc::channel::<Job>();
    let queue = Arc::new(Mutex::new(queue));
    let threads = 2 * std::thread::available_parallelism().map_or(4, |n| n.get());
    for i in 0..threads {
        let (module, writer, queue) = (Arc::clone(module), writer.clone(), Arc::clone(&queue));
        std::thread::Builder::new()
            .name(format!("geko-module-{i}"))
            .spawn(move || {
                loop {
                    let job = queue.lock().unwrap_or_else(|e| e.into_inner()).recv();
                    let Ok((id, call, payload)) = job else { return };
                    let (reply, payload) = answer(&*module, call, payload);
                    // A host that went away is seen by the thread that reads.
                    let _ = writer.send(id, &reply, &payload);
                }
            })
            .expect("a thread");
    }
    jobs
}

/// The side of the module: connects to the host, greets it with the contract, opens the module with
/// `open` and answers the host until it asks it to shut down or goes away.
pub fn serve(open: Open) -> Result<()> {
    let path = std::env::var_os(SOCKET).ok_or_else(|| GekoError::Config(format!("{SOCKET} is not set: this executable is a module of geko, which starts it")))?;
    serve_on(UnixStream::connect(path)?, open)
}

/// `serve` on a socket already connected to the host.
pub fn serve_on(stream: UnixStream, open: Open) -> Result<()> {
    let writer = Writer(Arc::new(Mutex::new(stream.try_clone()?)));
    let mut reader = BufReader::new(stream);
    writer.send(0, &Reply::Hello { contract: CONTRACT }, &[])?;
    let (id, call, _) = read_frame::<Call>(&mut reader)?;
    let Call::Open(context) = call else {
        let e = GekoError::Data("the first call of the host must open the module".into());
        writer.send(id, &Reply::Err((&e).into()), &[])?;
        return Err(e);
    };
    let module: Arc<dyn Module> = match guarded(|| open(context)) {
        Ok(module) => Arc::from(module),
        Err(e) => {
            writer.send(id, &Reply::Err((&e).into()), &[])?;
            return Err(e);
        }
    };
    writer.send(id, &Reply::Ok(json(&module.describe())), &[])?;
    let jobs = workers(&module, &writer);
    loop {
        let (id, call, payload) = match read_frame::<Call>(&mut reader) {
            Ok(frame) => frame,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        match call {
            Call::Handle(_) | Call::Metrics => jobs.send((id, call, payload)).expect("the workers outlive the loop"),
            Call::Shutdown => {
                let (reply, payload) = answer(&*module, call, payload);
                writer.send(id, &reply, &payload)?;
                return Ok(());
            }
            // Builds and the rest are long and few: each on its own thread, so that requests keep
            // being answered meanwhile.
            call => {
                let (module, writer) = (Arc::clone(&module), writer.clone());
                std::thread::spawn(move || {
                    let (reply, payload) = answer(&*module, call, payload);
                    let _ = writer.send(id, &reply, &payload);
                });
            }
        }
    }
    // The host went away without asking: it died, or was killed.
    module.shutdown()
}
