//! `ProcessModule` (docs/MODULES_PLAN.md): a module that runs as a process of its own, started from
//! its executable, to which every call of `Module` is a message over a Unix socket
//! (`geko_module_sdk::process`). A failure of the module does not take the host down, nor make it
//! wait (docs/MODULES_PLAN.md, «caídas de `ProcessModule`»):
//! - the calls in flight fail, and a new process starts in the background with the bundle the old
//!   one had loaded; meanwhile every request is answered 503 with `Retry-After`;
//! - exits in a row wait longer and longer before the next start, and too many stop it for a while;
//! - a request in flight in several exits is refused for a while, without reaching the module;
//! - a request without an answer in `Limits::timeout` is answered 504, and a process that answered
//!   nothing at all in that time is killed.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::BufReader;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use geko_module_sdk::module::{BundleView, CONTRACT, Context, Description, Request};
use geko_module_sdk::process::SHUTDOWN_GRACE;
use geko_module_sdk::wire::{Call, Reply, read_frame, write_frame};
use geko_module_sdk::{GekoError, Result};

use crate::remote::{Answer, Channel, Remote, decode, refused};

/// How long a module has to connect and greet the host after it starts.
const START: Duration = Duration::from_secs(30);

/// How the host looks after the process of a module.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The wait before starting it again after the second exit in a row (the first starts it at
    /// once); it doubles after each one, up to `max_backoff`.
    pub backoff: Duration,
    pub max_backoff: Duration,
    /// After `exits` exits (or failed starts) within `window`, it is not started again for `pause`.
    pub exits: usize,
    pub window: Duration,
    pub pause: Duration,
    /// A request in flight in `strikes` exits within `window` is refused for `quarantine`.
    pub strikes: usize,
    pub quarantine: Duration,
    /// How long a request or a read of the metrics waits for its answer. If nothing at all came
    /// from the process in that time, it hangs: it is killed and counts as an exit.
    pub timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            exits: 5,
            window: Duration::from_secs(600),
            pause: Duration::from_secs(900),
            strikes: 2,
            quarantine: Duration::from_secs(900),
            timeout: Duration::from_secs(30),
        }
    }
}

/// The callers waiting for an answer, by the id of their call; `None` once the connection is lost.
type Pending = Arc<Mutex<Option<HashMap<u64, mpsc::Sender<Answer>>>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Why a call got no answer.
#[derive(Debug)]
enum Failure {
    /// The process had exited: the call never reached it.
    Gone,
    /// The process exited while the call was in flight.
    Died,
    TimedOut,
}

/// One running process of the module and its socket.
struct Connection {
    writer: Mutex<UnixStream>,
    pending: Pending,
    next: AtomicU64,
    child: Option<Mutex<Child>>,
    /// When the process last sent anything.
    heard: Arc<Mutex<Instant>>,
    /// To close the socket of a module already connected (tests), which has no child to kill.
    socket: UnixStream,
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
        let socket = stream.try_clone()?;
        let mut writer = stream;
        write_frame(&mut writer, 0, &Call::Open(context.clone()), &[])?;
        let description = match read_frame::<Reply>(&mut reader)?.1 {
            Reply::Ok(value) => decode(value)?,
            Reply::Err(e) => return Err(e.into()),
            Reply::Hello { .. } => return Err(GekoError::Data("the module greeted the host twice".into())),
        };
        let pending: Pending = Arc::new(Mutex::new(Some(HashMap::new())));
        let heard = Arc::new(Mutex::new(Instant::now()));
        let (answers, heard_by) = (Arc::clone(&pending), Arc::clone(&heard));
        std::thread::Builder::new().name("geko-module-reader".into()).spawn(move || read_answers(reader, &answers, &heard_by))?;
        let connection = Self { writer: Mutex::new(writer), pending, next: AtomicU64::new(1), child: child.map(Mutex::new), heard, socket };
        Ok((connection, description))
    }

    fn alive(&self) -> bool {
        lock(&self.pending).is_some()
    }

    fn call(&self, call: &Call, payload: &[u8], timeout: Option<Duration>) -> std::result::Result<Answer, Failure> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).as_mut().ok_or(Failure::Gone)?.insert(id, tx);
        if write_frame(&mut *lock(&self.writer), id, call, payload).is_err() {
            if let Some(waiting) = lock(&self.pending).as_mut() {
                waiting.remove(&id);
            }
            return Err(Failure::Gone);
        }
        let answer = match timeout {
            Some(timeout) => rx.recv_timeout(timeout).map_err(|e| match e {
                mpsc::RecvTimeoutError::Timeout => Failure::TimedOut,
                mpsc::RecvTimeoutError::Disconnected => Failure::Died,
            }),
            None => rx.recv().map_err(|_| Failure::Died),
        };
        if matches!(answer, Err(Failure::TimedOut))
            && let Some(waiting) = lock(&self.pending).as_mut()
        {
            waiting.remove(&id);
        }
        answer
    }

    /// How long since the process last sent anything.
    fn silent_for(&self) -> Duration {
        lock(&self.heard).elapsed()
    }

    /// Ends a process that hangs: the calls in flight fail as if it had exited.
    fn kill(&self) {
        if let Some(child) = &self.child {
            let _ = lock(child).kill();
        }
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
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
fn read_answers(mut reader: BufReader<UnixStream>, pending: &Pending, heard: &Mutex<Instant>) {
    while let Ok((id, reply, payload)) = read_frame::<Reply>(&mut reader) {
        *lock(heard) = Instant::now();
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

/// The same request, whatever the time it waited: its route, parts, query and body.
fn fingerprint(request: &Request, body: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    (&request.route, &request.params, &request.query, &request.content_type, body).hash(&mut h);
    h.finish()
}

/// What the host knows of the process of the module.
struct State {
    /// `None` while a new one starts, or after an exit until a request notices it.
    connection: Option<Arc<Connection>>,
    starting: bool,
    /// The exits and failed starts within `Limits::window`.
    exits: VecDeque<Instant>,
    /// No start before this.
    next_start: Instant,
    /// When each request was in flight in an exit.
    strikes: HashMap<u64, Vec<Instant>>,
}

impl State {
    fn exited(&mut self, limits: &Limits, name: &str) {
        let now = Instant::now();
        self.exits.retain(|t| now.duration_since(*t) < limits.window);
        self.exits.push_back(now);
        self.strikes.retain(|_, times| {
            times.retain(|t| now.duration_since(*t) < limits.window.max(limits.quarantine));
            !times.is_empty()
        });
        let n = self.exits.len();
        let wait = if n >= limits.exits {
            tracing::error!(module = name, exits = n, window_s = limits.window.as_secs(), pause_s = limits.pause.as_secs(), "the process of the module exits too often; not started again for a while");
            limits.pause
        } else if n == 1 {
            Duration::ZERO
        } else {
            limits.backoff.saturating_mul(1 << (n - 2).min(16)).min(limits.max_backoff)
        };
        self.next_start = now + wait;
    }

    fn strike(&mut self, print: u64) {
        self.strikes.entry(print).or_default().push(Instant::now());
    }

    /// Whether the request was in flight in `strikes` exits, the last of them within `quarantine`.
    fn quarantined(&self, print: u64, limits: &Limits) -> bool {
        self.strikes.get(&print).is_some_and(|times| {
            let recent = times.iter().filter(|t| t.elapsed() < limits.window).count();
            recent >= limits.strikes && times.last().is_some_and(|t| t.elapsed() < limits.quarantine)
        })
    }
}

struct Shared {
    /// What it starts again after a failure; `None` for a module already connected (tests).
    executable: Option<PathBuf>,
    context: Context,
    name: String,
    limits: Limits,
    state: Mutex<State>,
    /// The bundle loaded last, which a new process loads before answering anything.
    bundle: Mutex<Option<BundleView>>,
}

pub struct Process {
    shared: Arc<Shared>,
}

pub type ProcessModule = Remote<Process>;

/// Starts the executable of a module and opens it with `context`.
pub fn start(executable: PathBuf, context: Context) -> Result<ProcessModule> {
    start_with(executable, context, Limits::default())
}

fn start_with(executable: PathBuf, context: Context, limits: Limits) -> Result<ProcessModule> {
    let (connection, description) = Connection::start(&executable, &context)?;
    Ok(module(Some(executable), context, connection, description, limits))
}

fn module(executable: Option<PathBuf>, context: Context, connection: Connection, description: Description, limits: Limits) -> ProcessModule {
    let state = State { connection: Some(Arc::new(connection)), starting: false, exits: VecDeque::new(), next_start: Instant::now(), strikes: HashMap::new() };
    let shared = Shared { executable, context, name: description.name.to_string(), limits, state: Mutex::new(state), bundle: Mutex::new(None) };
    Remote::new(Process { shared: Arc::new(shared) }, description)
}

fn unavailable(message: &str, retry_after: Duration) -> GekoError {
    refused(503, message.to_owned(), Some(retry_after.max(Duration::from_secs(1))))
}

impl Process {
    /// The running process. After an exit, the first call that notices it starts a new one in the
    /// background, when the wait after the last exits allows it, and every call fails at once
    /// until it is up.
    fn connection(&self) -> Result<Arc<Connection>> {
        let shared = &self.shared;
        let mut state = lock(&shared.state);
        if let Some(c) = &state.connection {
            if c.alive() {
                return Ok(Arc::clone(c));
            }
            state.connection = None;
            tracing::error!(module = %shared.name, "the process of the module exited");
            state.exited(&shared.limits, &shared.name);
        }
        if shared.executable.is_none() {
            return Err(unavailable("the process of the module exited", Duration::ZERO));
        }
        let wait = state.next_start.saturating_duration_since(Instant::now());
        if !state.starting && wait.is_zero() {
            state.starting = true;
            let shared = Arc::clone(shared);
            let spawned = std::thread::Builder::new().name("geko-module-start".into()).spawn(move || restart(&shared));
            if spawned.is_err() {
                state.starting = false;
            }
        }
        Err(unavailable("the module is starting again after its process exited", wait))
    }
}

/// Starts a new process of the module and loads the last bundle into it, apart from the requests,
/// which do not wait for it.
fn restart(shared: &Shared) {
    tracing::warn!(module = %shared.name, "starting the process of the module again");
    let executable = shared.executable.as_deref().expect("only a module with an executable is started again");
    let started = Connection::start(executable, &shared.context).and_then(|(fresh, _)| {
        if let Some(bundle) = lock(&shared.bundle).clone() {
            match fresh.call(&Call::Load(bundle), &[], None) {
                Ok(answer) => answer.map(drop)?,
                Err(_) => return Err(GekoError::Other("it exited while loading the bundle".into())),
            }
        }
        Ok(fresh)
    });
    let mut state = lock(&shared.state);
    state.starting = false;
    match started {
        Ok(fresh) => {
            tracing::info!(module = %shared.name, "the process of the module is up again");
            state.connection = Some(Arc::new(fresh));
        }
        Err(e) => {
            tracing::error!(module = %shared.name, error = %e, "cannot start the process of the module again");
            state.exited(&shared.limits, &shared.name);
        }
    }
}

impl Channel for Process {
    fn exchange(&self, call: &Call, payload: &[u8]) -> Answer {
        let shared = &self.shared;
        if matches!(call, Call::Shutdown) {
            // Never started again only to be shut down.
            let connection = lock(&shared.state).connection.clone();
            return match connection.map(|c| c.call(call, payload, None)) {
                Some(Ok(answer)) => answer,
                _ => Ok((serde_json::Value::Null, Vec::new())),
            };
        }
        let print = match call {
            Call::Handle(request) => Some(fingerprint(request, payload)),
            _ => None,
        };
        if let Some(print) = print
            && lock(&shared.state).quarantined(print, &shared.limits)
        {
            tracing::warn!(module = %shared.name, request = format!("{print:016x}"), "refused a request that was in flight when the process of the module exited, more than once");
            let minutes = shared.limits.quarantine.as_secs().div_ceil(60);
            return Err(refused(500, format!("this request was in flight when the module failed, more than once: refused for up to {minutes} min"), None));
        }
        let connection = self.connection()?;
        let timeout = matches!(call, Call::Handle(_) | Call::Metrics | Call::Progress).then_some(shared.limits.timeout);
        match connection.call(call, payload, timeout) {
            Ok(answer) => answer,
            Err(Failure::Gone) => {
                let _ = self.connection();
                Err(unavailable("the process of the module exited", Duration::ZERO))
            }
            Err(Failure::Died) => {
                if let Some(print) = print {
                    tracing::error!(module = %shared.name, request = format!("{print:016x}"), "a request was in flight when the process of the module exited");
                    lock(&shared.state).strike(print);
                }
                let _ = self.connection();
                Err(unavailable("the process of the module exited while answering", Duration::ZERO))
            }
            Err(Failure::TimedOut) => {
                let secs = shared.limits.timeout.as_secs_f32();
                if connection.silent_for() >= shared.limits.timeout {
                    tracing::error!(module = %shared.name, timeout_s = secs, "the process of the module answered nothing in time; killing it");
                    if let Some(print) = print {
                        lock(&shared.state).strike(print);
                    }
                    connection.kill();
                }
                Err(refused(504, format!("the module did not answer in {secs} s"), None))
            }
        }
    }

    fn loaded(&self, bundle: &BundleView) {
        *lock(&self.shared.bundle) = Some(bundle.clone());
    }

    fn shut_down(&self) {
        if let Some(connection) = lock(&self.shared.state).connection.clone() {
            connection.wait(SHUTDOWN_GRACE);
        }
    }

    fn unavailable(&self) -> Option<String> {
        let state = lock(&self.shared.state);
        if state.connection.as_ref().is_some_and(|c| c.alive()) {
            return None;
        }
        let wait = state.next_start.saturating_duration_since(Instant::now());
        Some(if state.starting {
            "its process exited; starting again".to_owned()
        } else if wait.is_zero() {
            "its process exited".to_owned()
        } else {
            format!("its process exited {} times in a row; starting again in {} s", state.exits.len(), wait.as_secs().max(1))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::tests::{bundle, context, echo, open_fake};
    use geko_module_sdk::module::{Module, Open, Response};

    fn connected(open: Open) -> ProcessModule {
        let (host, module) = UnixStream::pair().unwrap();
        std::thread::spawn(move || geko_module_sdk::process::serve_on(module, open));
        let (connection, description) = Connection::over(host, None, &context()).unwrap();
        super::module(None, context(), connection, description, Limits::default())
    }

    #[test]
    fn every_call_crosses_the_socket() {
        crate::remote::tests::every_call_crosses(&connected(open_fake));
    }

    #[test]
    fn the_contract_crosses_the_socket() {
        geko_module_sdk::conformance::module::check(&connected(open_fake));
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
        assert_eq!(echo(&m, "", b"").status, 503);
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

    /// The module of the tests that start one: this same test executable, started as a module with
    /// only this test, which serves `Fake` when the host starts it.
    #[test]
    fn child_module() {
        if std::env::var_os(geko_module_sdk::process::SOCKET).is_some() {
            geko_module_sdk::process::serve(open_fake).unwrap();
            std::process::exit(0);
        }
    }

    /// The fake module as a process of its own, with `limits`.
    fn child(limits: Limits) -> (ProcessModule, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("module.sh");
        let me = std::env::current_exe().unwrap();
        std::fs::write(&script, format!("#!/bin/sh\nexec '{}' --exact process::tests::child_module --quiet\n", me.display())).unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        // A test that starts a process while this one writes the script hands it the open file, and
        // until that process execs, Linux refuses to run the script (ETXTBSY).
        for _ in 0..20 {
            match start_with(script.clone(), context(), limits) {
                Ok(m) => return (m, dir),
                Err(e) if e.to_string().contains("Text file busy") => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => panic!("{e}"),
            }
        }
        panic!("the script of the module stayed busy");
    }

    /// The first answer to `body` that is not a 503, polling.
    fn when_up(m: &ProcessModule, body: &[u8]) -> Response {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let r = echo(m, "", body);
            if r.status != 503 || Instant::now() > deadline {
                return r;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_module_that_exits_is_started_again() {
        let (m, _dir) = child(Limits::default());
        m.load(&bundle("v7")).unwrap();
        assert_eq!(echo(&m, "", b"a").body, b"v7:a");
        assert_eq!(echo(&m, "exit", b"").status, 503, "the request in flight fails");
        assert_eq!(when_up(&m, b"b").body, b"v7:b", "started again, with the bundle it had");
        assert_eq!(m.unavailable(), None);
        m.shutdown().unwrap();
    }

    #[test]
    fn requests_do_not_wait_while_it_starts_again() {
        let (m, _dir) = child(Limits::default());
        m.load(&bundle("slow")).unwrap();
        echo(&m, "exit", b"");
        let asked = Instant::now();
        let r = echo(&m, "", b"a");
        assert!(asked.elapsed() < Duration::from_millis(500), "answered at once, not after the bundle loads");
        assert_eq!(r.status, 503);
        assert!(r.headers.iter().any(|(k, v)| k == "retry-after" && v == "1"), "{:?}", r.headers);
        assert!(m.unavailable().is_some_and(|why| why.contains("starting again")));
        assert_eq!(when_up(&m, b"b").body, b"slow:b");
        m.shutdown().unwrap();
    }

    #[test]
    fn a_request_in_flight_in_two_exits_is_refused_without_reaching_the_module() {
        let (m, _dir) = child(Limits { backoff: Duration::from_millis(10), ..Limits::default() });
        for _ in 0..2 {
            assert_eq!(echo(&m, "exit", b"same").status, 503);
            assert_eq!(when_up(&m, b"x").status, 200);
        }
        let r = echo(&m, "exit", b"same");
        assert_eq!(r.status, 500);
        assert!(String::from_utf8_lossy(&r.body).contains("refused"));
        assert_eq!(echo(&m, "", b"y").status, 200, "the module did not exit");
        assert_eq!(echo(&m, "exit", b"another").status, 503, "other requests reach it");
        m.shutdown().unwrap();
    }

    #[test]
    fn exits_in_a_row_wait_longer_and_too_many_stop_it() {
        let limits = Limits { backoff: Duration::from_millis(300), exits: 3, pause: Duration::from_secs(60), strikes: 99, ..Limits::default() };
        let (m, _dir) = child(limits);
        echo(&m, "exit", b"");
        assert_eq!(when_up(&m, b"").status, 200, "the first exit starts it again at once");
        echo(&m, "exit", b"");
        let exited = Instant::now();
        assert_eq!(when_up(&m, b"").status, 200);
        assert!(exited.elapsed() >= Duration::from_millis(300), "the second waits");
        echo(&m, "exit", b"");
        std::thread::sleep(Duration::from_millis(500));
        let r = echo(&m, "", b"");
        assert_eq!(r.status, 503, "the third stops it");
        assert!(r.headers.iter().any(|(k, v)| k == "retry-after" && v.parse::<u64>().unwrap() > 50), "{:?}", r.headers);
        assert!(m.unavailable().is_some_and(|why| why.contains("3 times")), "{:?}", m.unavailable());
    }

    #[test]
    fn a_module_that_hangs_is_killed_and_started_again() {
        let (m, _dir) = child(Limits { timeout: Duration::from_millis(300), ..Limits::default() });
        m.load(&bundle("v1")).unwrap();
        let asked = Instant::now();
        assert_eq!(echo(&m, "hang", b"").status, 504);
        assert!(asked.elapsed() < Duration::from_secs(2));
        assert_eq!(when_up(&m, b"a").body, b"v1:a", "killed and started again");
        m.shutdown().unwrap();
    }

    #[test]
    fn an_executable_that_never_connects_fails_to_start() {
        let err = start(PathBuf::from("/bin/true"), context()).err().unwrap();
        assert!(err.to_string().contains("exited before connecting"), "{err}");
    }
}
