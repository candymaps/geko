//! `ProcessModule` (docs/MODULES_PLAN.md): a module that runs as a process of its own, started from
//! its executable, to which every call of `Module` is a message over a Unix socket
//! (`geko_module_sdk::process`). A failure of the module does not take the host down: the calls in
//! flight fail, and the next one starts it again, with the bundle it had loaded.

use std::collections::HashMap;
use std::io::BufReader;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use geko_module_sdk::module::{BundleView, CONTRACT, Context, Description};
use geko_module_sdk::process::SHUTDOWN_GRACE;
use geko_module_sdk::wire::{Call, Reply, read_frame, write_frame};
use geko_module_sdk::{GekoError, Result};

use crate::remote::{Answer, Channel, Remote, decode};

/// How long a module has to connect and greet the host after it starts.
const START: Duration = Duration::from_secs(30);

/// The callers waiting for an answer, by the id of their call; `None` once the connection is lost.
type Pending = Arc<Mutex<Option<HashMap<u64, mpsc::Sender<Answer>>>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn gone() -> GekoError {
    GekoError::Other("the process of the module exited".into())
}

/// One running process of the module and its socket.
struct Connection {
    writer: Mutex<UnixStream>,
    pending: Pending,
    next: AtomicU64,
    child: Option<Mutex<Child>>,
}

impl Connection {
    /// Starts `executable` and waits for it to connect.
    fn start(executable: &Path, context: &Context) -> Result<(Self, Description)> {
        static STARTED: AtomicU64 = AtomicU64::new(0);
        let socket = std::env::temp_dir().join(format!("geko-module-{}-{}.sock", std::process::id(), STARTED.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        // In a group of its own, so that a Ctrl-C reaches only the host, which shuts the module
        // down after the requests in flight. If the host dies, the module sees its socket close.
        let started = Command::new(executable).env(geko_module_sdk::process::SOCKET, &socket).stdin(Stdio::null()).process_group(0).spawn();
        let accepted = started.map_err(GekoError::from).and_then(|mut child| match accept(&listener, &mut child) {
            Ok(stream) => Ok((stream, child)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(e)
            }
        });
        let _ = std::fs::remove_file(&socket);
        let (stream, child) = accepted.map_err(|e| GekoError::Other(format!("cannot start the module {}: {e}", executable.display()).into()))?;
        Self::over(stream, Some(child), context)
    }

    /// Greets the module on `stream`, checks its contract, opens it with `context` and starts
    /// reading its answers.
    fn over(stream: UnixStream, child: Option<Child>, context: &Context) -> Result<(Self, Description)> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let (_, hello, _) = read_frame::<Reply>(&mut reader)?;
        match hello {
            Reply::Hello { contract } if contract == CONTRACT => {}
            Reply::Hello { contract } => return Err(GekoError::Incompatible(format!("the module implements contract {contract}, and this geko speaks contract {CONTRACT}"))),
            _ => return Err(GekoError::Data("the module did not greet the host".into())),
        }
        let mut writer = stream;
        write_frame(&mut writer, 0, &Call::Open(context.clone()), &[])?;
        let description = match read_frame::<Reply>(&mut reader)?.1 {
            Reply::Ok(value) => decode(value)?,
            Reply::Err(e) => return Err(e.into()),
            Reply::Hello { .. } => return Err(GekoError::Data("the module greeted the host twice".into())),
        };
        let pending: Pending = Arc::new(Mutex::new(Some(HashMap::new())));
        let answers = Arc::clone(&pending);
        std::thread::Builder::new().name("geko-module-reader".into()).spawn(move || read_answers(reader, &answers))?;
        let connection = Self { writer: Mutex::new(writer), pending, next: AtomicU64::new(1), child: child.map(Mutex::new) };
        Ok((connection, description))
    }

    fn alive(&self) -> bool {
        lock(&self.pending).is_some()
    }

    fn call(&self, call: &Call, payload: &[u8]) -> Answer {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).as_mut().ok_or_else(gone)?.insert(id, tx);
        if write_frame(&mut *lock(&self.writer), id, call, payload).is_err() {
            if let Some(waiting) = lock(&self.pending).as_mut() {
                waiting.remove(&id);
            }
            return Err(gone());
        }
        rx.recv().map_err(|_| gone())?
    }

    /// Waits for the process to exit after a shutdown, and kills it if it does not in time.
    fn wait(&self, grace: Duration) {
        let Some(child) = &self.child else { return };
        let mut child = lock(child);
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.wait(Duration::ZERO);
    }
}

fn accept(listener: &UnixListener, child: &mut Child) -> Result<UnixStream> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + START;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                return Ok(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        if let Some(status) = child.try_wait()? {
            return Err(GekoError::Other(format!("it exited before connecting ({status})").into()));
        }
        if Instant::now() > deadline {
            return Err(GekoError::Other(format!("it did not connect in {} s", START.as_secs()).into()));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Hands each answer to the call waiting for it until the connection is lost; then every call
/// still waiting fails.
fn read_answers(mut reader: BufReader<UnixStream>, pending: &Pending) {
    while let Ok((id, reply, payload)) = read_frame::<Reply>(&mut reader) {
        let answer = match reply {
            Reply::Ok(value) => Ok((value, payload)),
            Reply::Err(e) => Err(e.into()),
            Reply::Hello { .. } => Err(GekoError::Data("the module greeted the host again".into())),
        };
        if let Some(tx) = lock(pending).as_mut().and_then(|waiting| waiting.remove(&id)) {
            let _ = tx.send(answer);
        }
    }
    lock(pending).take();
}

pub struct Process {
    /// What it starts again after a failure; `None` for a module already connected (tests).
    executable: Option<PathBuf>,
    context: Context,
    name: String,
    connection: Mutex<Arc<Connection>>,
    /// The bundle loaded last, which a restarted process loads before answering anything.
    bundle: Mutex<Option<BundleView>>,
}

pub type ProcessModule = Remote<Process>;

/// Starts the executable of a module and opens it with `context`.
pub fn start(executable: PathBuf, context: Context) -> Result<ProcessModule> {
    let (connection, description) = Connection::start(&executable, &context)?;
    Ok(module(Some(executable), context, connection, description))
}

fn module(executable: Option<PathBuf>, context: Context, connection: Connection, description: Description) -> ProcessModule {
    let process = Process {
        executable,
        context,
        name: description.name.to_string(),
        connection: Mutex::new(Arc::new(connection)),
        bundle: Mutex::new(None),
    };
    Remote::new(process, description)
}

impl Process {
    /// The running process; after a failure, a new one, which loads the last bundle again.
    fn connection(&self) -> Result<Arc<Connection>> {
        let mut connection = lock(&self.connection);
        if !connection.alive()
            && let Some(executable) = &self.executable
        {
            tracing::error!(module = %self.name, "the process of the module exited; starting it again");
            let (fresh, _) = Connection::start(executable, &self.context)?;
            if let Some(bundle) = lock(&self.bundle).clone() {
                fresh.call(&Call::Load(bundle), &[])?;
            }
            *connection = Arc::new(fresh);
        }
        Ok(Arc::clone(&connection))
    }
}

impl Channel for Process {
    fn exchange(&self, call: &Call, payload: &[u8]) -> Answer {
        if matches!(call, Call::Shutdown) {
            // Never started again only to be shut down.
            let connection = Arc::clone(&lock(&self.connection));
            return if connection.alive() { connection.call(call, payload) } else { Ok((serde_json::Value::Null, Vec::new())) };
        }
        self.connection()?.call(call, payload)
    }

    fn loaded(&self, bundle: &BundleView) {
        *lock(&self.bundle) = Some(bundle.clone());
    }

    fn shut_down(&self) {
        lock(&self.connection).wait(SHUTDOWN_GRACE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::tests::{bundle, context, echo, open_fake};
    use geko_module_sdk::module::{Module, Open};

    fn connected(open: Open) -> ProcessModule {
        let (host, module) = UnixStream::pair().unwrap();
        std::thread::spawn(move || geko_module_sdk::process::serve_on(module, open));
        let (connection, description) = Connection::over(host, None, &context()).unwrap();
        super::module(None, context(), connection, description)
    }

    #[test]
    fn every_call_crosses_the_socket() {
        crate::remote::tests::every_call_crosses(&connected(open_fake));
    }

    #[test]
    fn requests_are_answered_at_once() {
        crate::remote::tests::answered_at_once(Arc::new(connected(open_fake)));
    }

    #[test]
    fn a_panic_answers_500_and_the_module_goes_on() {
        crate::remote::tests::a_panic_answers_500(&connected(open_fake));
    }

    #[test]
    fn after_shutdown_requests_fail() {
        let m = connected(open_fake);
        m.shutdown().unwrap();
        assert_eq!(echo(&m, "", b"").status, 500);
    }

    #[test]
    fn a_module_of_another_contract_or_failing_to_open_is_refused() {
        let (host, module) = UnixStream::pair().unwrap();
        std::thread::spawn(move || write_frame(&mut &module, 0, &Reply::Hello { contract: CONTRACT + 1 }, &[]));
        let err = Connection::over(host, None, &context()).err().unwrap();
        assert!(matches!(&err, GekoError::Incompatible(e) if e.contains("contract")), "{err}");

        let (host, module) = UnixStream::pair().unwrap();
        std::thread::spawn(move || geko_module_sdk::process::serve_on(module, |_| Err(GekoError::Config("bad key in geko-fake.toml".into()))));
        let err = Connection::over(host, None, &context()).err().unwrap();
        assert!(matches!(&err, GekoError::Config(e) if e.contains("bad key")), "{err}");
    }

    /// The module of `a_module_that_exits_is_started_again`: this same test executable, started as
    /// a module with only this test, which serves `Fake` when the host starts it.
    #[test]
    fn child_module() {
        if std::env::var_os(geko_module_sdk::process::SOCKET).is_some() {
            geko_module_sdk::process::serve(open_fake).unwrap();
            std::process::exit(0);
        }
    }

    #[test]
    fn a_module_that_exits_is_started_again() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("module.sh");
        let me = std::env::current_exe().unwrap();
        std::fs::write(&script, format!("#!/bin/sh\nexec '{}' --exact process::tests::child_module --quiet\n", me.display())).unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let m = start(script, context()).unwrap();
        m.load(&bundle("v7")).unwrap();
        assert_eq!(echo(&m, "", b"a").body, b"v7:a");
        assert_eq!(echo(&m, "exit", b"").status, 500, "the request in flight fails");
        assert_eq!(echo(&m, "", b"b").body, b"v7:b", "started again, with the bundle it had");
        m.shutdown().unwrap();
    }

    #[test]
    fn an_executable_that_never_connects_fails_to_start() {
        let err = start(PathBuf::from("/bin/true"), context()).err().unwrap();
        assert!(err.to_string().contains("exited before connecting"), "{err}");
    }
}
