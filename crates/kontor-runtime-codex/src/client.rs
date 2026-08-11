//! The Codex transport: one seam, one live implementation, bounded and quiet.
//!
//! The adapter never spawns a process itself. It builds a [`CodexCommand`] and a
//! [`PreparedCommand`], hands both to a [`CodexTransport`], and reads back JSON
//! Lines. That seam earns its keep three times over, exactly as AO's and Paseo's
//! do:
//!
//! * the contract suite can prove a refusal produced **zero** dispatches, which
//!   is a claim about the process table that no amount of return-value checking
//!   can make;
//! * "one admitted run starts one process" becomes a count over a recorded
//!   ledger instead of an inference;
//! * a process ending can be scripted in every shape — EOF, a zero exit, a
//!   non-zero exit, a signal, a deadline, a kill — which is the whole set of
//!   things this adapter must refuse to read as a verdict.
//!
//! # No shell, ever
//!
//! Every invocation is an argv array. There is no string a prompt or a path is
//! interpolated into, so a hostile prompt is an argument and never a second
//! command. There is no PTY, no terminal size, no stdin conversation: Codex is
//! started once, and its stdout is read.
//!
//! # Where the secret is, and where it is not
//!
//! The resolved `CODEX_HOME` lives in exactly one place: the environment block of
//! the [`PreparedCommand`] the adapter is about to spawn, written there by
//! [`kontor_accounts::ResolvedAccountEnvironment::apply`]. It is not a field of
//! [`CodexCommand`], so it cannot reach a ledger, a checkpoint, an error payload
//! or a fixture — not because every call site remembers to redact it, but
//! because no call site is ever handed it.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsStr;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use kontor_core::DomainError;
use kontor_core::id::ExternalId;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex as AsyncMutex;

use crate::wire::{CodexEnding, MAX_FRAME_BYTES, MAX_FRAMES_PER_DRAIN, REDACTED};

/// The subcommand and flags this adapter dispatches, and the whole of them.
const EXEC_ARGV: &[&str] = &["exec", "--json"];

/// The redacted ledger key every Codex dispatch is counted under.
pub const EXEC_ROUTE: &str = "codex exec --json";

/// How many produced-but-undrained lines one process may buffer.
///
/// A bound rather than an unbounded queue because the reader task outruns the
/// control plane's drains. Overflow is *reported* — see [`CodexDrained::dropped`]
/// — and becomes a typed timeline break rather than a hole nobody sees.
pub const MAX_BUFFERED_LINES: usize = 8192;

// ---------------------------------------------------------------------------
// The invocation
// ---------------------------------------------------------------------------

/// One `codex exec --json <prompt>` invocation, as an argv array.
///
/// The constructor below is the only way to build one, so no call site can
/// invent a subcommand, drop `--json`, or reach a Codex mode this adapter has not
/// verified.
///
/// # Why `Debug` is written out rather than derived
///
/// This type holds the two values the whole adapter is arranged to keep out of
/// every artefact: the prompt, which is the operator's actual work, and the
/// working directory, which names a place on their machine. A derived `Debug`
/// renders both — and it renders them at every `{:?}`, every `tracing` field and
/// every `expect` message, which is a leak that no amount of care at the ledger
/// makes safe. The redacted route exists precisely so there is something safe to
/// print instead.
#[derive(Clone, PartialEq, Eq)]
pub struct CodexCommand {
    program: String,
    argv: Vec<String>,
    /// Every argv element that came from outside this adapter.
    ///
    /// Recorded separately because "is this argument flag-shaped?" is only a
    /// question about foreign values. The program is trusted local configuration
    /// and is checked for emptiness instead.
    values: Vec<String>,
    cwd: String,
    env_names: Vec<String>,
}

impl CodexCommand {
    /// `codex exec --json <prompt>`, run in `cwd`.
    #[must_use]
    pub fn exec(program: &str, cwd: &str, prompt: &str, env_names: Vec<String>) -> Self {
        let mut argv: Vec<String> = EXEC_ARGV.iter().map(|part| (*part).to_owned()).collect();
        argv.push(prompt.to_owned());
        Self {
            program: program.to_owned(),
            argv,
            values: vec![prompt.to_owned()],
            cwd: cwd.to_owned(),
            env_names,
        }
    }

    /// The executable this invocation dispatches.
    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The argv the transport dispatches.
    ///
    /// It carries the prompt, because that is where `codex exec` takes it. It
    /// never carries the config home, which travels only in the environment.
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    /// The verified working directory this invocation runs in.
    #[must_use]
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// The *names* of the environment variables the child will be given.
    #[must_use]
    pub fn env_names(&self) -> &[String] {
        &self.env_names
    }

    /// The ledger key the contract suite counts dispatches by.
    ///
    /// The subcommand and its flags, and nothing else. The prompt, the working
    /// directory and the config home are absent by construction, so an assertion
    /// about the process table — or a log line that prints one of these — can
    /// never quote the operator's work.
    #[must_use]
    pub fn route(&self) -> &'static str {
        EXEC_ROUTE
    }

    /// Refuse a foreign value that would be read as a flag.
    ///
    /// A prompt beginning with `-` lands in argv as an option rather than as the
    /// text it was meant to be.
    ///
    /// ponytail: this also refuses a legitimate prompt that happens to begin with
    /// `-`. Refusing typed beats guessing at the parser; relax it to an explicit
    /// `--` separator once a live probe confirms `codex exec` accepts one.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Domain`] for an empty value and for one that starts
    /// with `-`.
    pub fn ensure_dispatchable(&self) -> RuntimeResult<()> {
        for value in &self.values {
            if value.is_empty() {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "CodexCommand",
                    "carries an empty argument",
                )));
            }
            if value.starts_with('-') {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "CodexCommand",
                    "carries a value that would be read as another option",
                )));
            }
        }
        if self.cwd.is_empty() {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexCommand",
                "names no working directory",
            )));
        }
        if self.program.is_empty() {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexCommand",
                "names no executable",
            )));
        }
        Ok(())
    }
}

impl fmt::Debug for CodexCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The redacted route, the shape of the invocation, and the environment
        // variable *names* — the same contract the dispatch ledger follows.
        // Nothing that names the operator's work or their machine.
        f.debug_struct("CodexCommand")
            .field("route", &self.route())
            .field("arguments", &self.argv.len())
            .field("env_names", &self.env_names)
            .field("program", &REDACTED)
            .field("prompt", &REDACTED)
            .field("cwd", &REDACTED)
            .finish()
    }
}

/// A `std::process::Command` that has had one account's environment applied to
/// it, ready to spawn and unsafe to print.
///
/// It exists to keep two things true at once. The adapter has to *build* the
/// command, because verifying the resolved config home means reading it back off
/// the environment block the child will actually receive rather than off a copy.
/// And nothing may print it: `std::process::Command`'s own `Debug` renders the
/// program, every argument and every environment value, which is the prompt and
/// the config home in one line.
///
/// So the command travels wrapped. `Debug` is redacted, there is no accessor that
/// returns an environment value or the argv, and the one way to look inside is
/// [`PreparedCommand::contains`], which answers a yes/no question without handing
/// anything back.
pub struct PreparedCommand(std::process::Command);

impl PreparedCommand {
    /// Wrap a command whose environment has been applied.
    #[must_use]
    pub const fn new(command: std::process::Command) -> Self {
        Self(command)
    }

    /// The names of the environment variables this command *sets*.
    #[must_use]
    pub fn env_names(&self) -> Vec<String> {
        self.0
            .get_envs()
            .filter(|(_, value)| value.is_some())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect()
    }

    /// The names of the environment variables this command explicitly *clears*.
    ///
    /// The evidence that an inherited variable was removed rather than merely
    /// left alone. A name that is cleared and then set appears in
    /// [`PreparedCommand::env_names`] instead, which is the same guarantee read
    /// from the other end.
    #[must_use]
    pub fn cleared_names(&self) -> Vec<String> {
        self.0
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect()
    }

    /// Whether `needle` appears anywhere in this command — program, arguments or
    /// environment values.
    ///
    /// A canary probe, deliberately returning a `bool` and never the text it
    /// searched. It is how a test proves an auth file's contents, a keychain
    /// value or another account's home never reached a dispatch, without the test
    /// itself having to hold any of them beyond the canary it planted.
    #[must_use]
    pub fn contains(&self, needle: &str) -> bool {
        let hit = |value: &OsStr| value.to_string_lossy().contains(needle);
        hit(self.0.get_program())
            || self.0.get_args().any(hit)
            || self.0.get_envs().filter_map(|(_, value)| value).any(hit)
            || self
                .0
                .get_current_dir()
                .is_some_and(|dir| dir.as_os_str().to_string_lossy().contains(needle))
    }

    /// One environment value, for the adapter that is about to spawn this.
    ///
    /// Crate-private, and used for exactly one variable: the resolved
    /// `CODEX_HOME`, so the marker inside it can be verified before a process is
    /// started under it. Reading it back off *this* command rather than from a
    /// copy is the point — what gets verified is what the child gets.
    pub(crate) fn env_value(&self, name: &str) -> Option<String> {
        self.0.get_envs().find_map(|(key, value)| {
            (key == OsStr::new(name)).then(|| value.map(|it| it.to_string_lossy().into_owned()))
        })?
    }

    /// Unwrap, for a transport that is about to spawn it.
    #[must_use]
    pub fn into_inner(self) -> std::process::Command {
        self.0
    }
}

impl fmt::Debug for PreparedCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Names, never values, and never the argv: the derived rendering of the
        // wrapped type is precisely the leak this wrapper exists to prevent.
        f.debug_struct("PreparedCommand")
            .field("env_names", &self.env_names())
            .field("cleared_names", &self.cleared_names())
            .field("values", &REDACTED)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

/// A started Codex process.
///
/// `Debug` is written out for the same reason [`CodexCommand`]'s is: the launch
/// acknowledgement is a raw frame the process printed, which is session content
/// rather than a fact about the dispatch.
#[derive(Clone, PartialEq, Eq)]
pub struct CodexStarted {
    /// The transport's own handle for this process. Correlation evidence for the
    /// dispatch, never a Kontor identifier.
    pub exec_id: ExternalId,
    /// The operating-system process identity, which is what a cancellation
    /// addresses.
    pub process_id: u32,
    /// The process's first stdout line, verbatim.
    ///
    /// A Codex `exec` that has not printed its first frame has not started as far
    /// as Kontor is concerned, so the transport waits for it. It travels raw
    /// because the adapter canonicalizes evidence before it maps anything.
    pub launch_ack: String,
}

/// Everything one process produced since the last drain.
///
/// `Debug` reports how much arrived rather than what it was: the lines are the
/// agent's own output, and a stream that is safe to count is not the same as one
/// that is safe to print.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct CodexDrained {
    /// The stdout lines, in the order the process printed them.
    pub lines: Vec<String>,
    /// How many lines the transport had to discard because its buffer was full.
    ///
    /// Never zero silently: a non-zero count becomes a typed timeline break, so a
    /// caller is told to refetch rather than handed renumbered content with a
    /// hole in it.
    pub dropped: u64,
    /// How the process ended, once it has.
    pub ending: Option<CodexEnding>,
}

impl fmt::Debug for CodexStarted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexStarted")
            .field("exec_id", &self.exec_id)
            .field("process_id", &self.process_id)
            .field("launch_ack", &REDACTED)
            .finish()
    }
}

impl fmt::Debug for CodexDrained {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexDrained")
            .field("lines", &self.lines.len())
            .field("dropped", &self.dropped)
            .field("ending", &self.ending)
            .field("content", &REDACTED)
            .finish()
    }
}

/// What a liveness read found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexLiveness {
    /// The process identity this handle addresses.
    pub process_id: u32,
    /// How it ended, when it has. `None` means the process was observed alive.
    pub ending: Option<CodexEnding>,
}

/// The seam between the adapter's policy and a real Codex process.
#[async_trait]
pub trait CodexTransport: Send + Sync + fmt::Debug {
    /// Start one Codex process and wait for its first stdout frame.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the process could not be started
    /// or produced no frame. That is a fact about the channel and never about the
    /// work: an implementation must not turn a failed spawn into an empty
    /// success.
    async fn start(
        &self,
        command: &CodexCommand,
        prepared: PreparedCommand,
    ) -> RuntimeResult<CodexStarted>;

    /// Take everything one process has produced since the last drain.
    ///
    /// # Errors
    /// As [`CodexTransport::start`], plus a refusal for a handle this transport
    /// does not own.
    async fn drain(&self, exec_id: &ExternalId) -> RuntimeResult<CodexDrained>;

    /// Stop one process, addressed by the handle this transport issued.
    ///
    /// Idempotent: stopping a process that has already ended reports how it
    /// ended rather than failing.
    ///
    /// # Errors
    /// As [`CodexTransport::drain`].
    async fn stop(&self, exec_id: &ExternalId) -> RuntimeResult<CodexEnding>;

    /// Read whether one process is still there, without consuming its output.
    ///
    /// # Errors
    /// As [`CodexTransport::drain`].
    async fn liveness(&self, exec_id: &ExternalId) -> RuntimeResult<CodexLiveness>;
}

// ---------------------------------------------------------------------------
// The live transport
// ---------------------------------------------------------------------------

/// One live Codex child and the buffer its stdout is read into.
struct LiveExec {
    process_id: u32,
    lines: Arc<std::sync::Mutex<VecDeque<String>>>,
    dropped: Arc<AtomicU64>,
    ending: Arc<std::sync::Mutex<Option<CodexEnding>>>,
    child: Arc<AsyncMutex<Option<tokio::process::Child>>>,
}

impl fmt::Debug for LiveExec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The buffer holds the agent's own output, so it is counted and not
        // shown. Without this, `CodexLiveTransport`'s derived rendering would
        // print every undrained line of every live run.
        f.debug_struct("LiveExec")
            .field("process_id", &self.process_id)
            .field(
                "buffered",
                &self
                    .lines
                    .lock()
                    .map(|buffered| buffered.len())
                    .unwrap_or_default(),
            )
            .field("dropped", &self.dropped.load(Ordering::Relaxed))
            .field(
                "ending",
                &self.ending.lock().ok().and_then(|ending| *ending),
            )
            .field("content", &REDACTED)
            .finish()
    }
}

/// The live transport: a real `codex` executable, one child process per launch.
///
/// Bounded in four ways, all of them deliberate: a deadline on the first frame,
/// a deadline on the whole run, a cap on one stdout line, and a cap on the number
/// of undrained lines held for a caller. Stderr is piped to nothing and never
/// read — Codex writes the prompt and the config home into its diagnostics, and
/// those are the two things this adapter refuses to hold.
#[derive(Debug)]
pub struct CodexLiveTransport {
    start_timeout_seconds: u64,
    run_deadline_seconds: u64,
    running: std::sync::Mutex<BTreeMap<String, LiveExec>>,
}

impl CodexLiveTransport {
    /// Build a transport with one deadline for the launch acknowledgement and one
    /// for the whole run.
    ///
    /// It holds no executable and no environment: both arrive inside the
    /// [`PreparedCommand`] the adapter built, because the adapter is the party
    /// that must verify the account environment against the command it is about
    /// to spawn.
    ///
    /// `run_deadline_seconds` bounds the whole process, after which it is killed
    /// and the ending is [`CodexEnding::TimedOut`] — which, like every other
    /// ending, says only that the process stopped.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Domain`] for a zero deadline.
    pub fn new(start_timeout_seconds: u64, run_deadline_seconds: u64) -> RuntimeResult<Self> {
        if start_timeout_seconds == 0 || run_deadline_seconds == 0 {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexDeadline",
                "must be a positive number of seconds",
            )));
        }
        Ok(Self {
            start_timeout_seconds,
            run_deadline_seconds,
            running: std::sync::Mutex::new(BTreeMap::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, LiveExec>> {
        self.running
            .lock()
            .expect("the Codex transport lock is intact")
    }

    fn unknown_handle() -> RuntimeError {
        RuntimeError::Transport {
            rule: "handle does not name a process this transport started",
        }
    }

    /// Read one process's stdout until it closes or the run deadline elapses.
    ///
    /// Kept off the adapter's own task: a caller that never drains must not stop
    /// the child from making progress, and a child that outruns the caller must
    /// not grow an unbounded buffer.
    fn read_stdout(
        stdout: tokio::process::ChildStdout,
        lines: Arc<std::sync::Mutex<VecDeque<String>>>,
        dropped: Arc<AtomicU64>,
        ending: Arc<std::sync::Mutex<Option<CodexEnding>>>,
        child: Arc<AsyncMutex<Option<tokio::process::Child>>>,
        deadline_seconds: u64,
    ) {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            let closed =
                tokio::time::timeout(std::time::Duration::from_secs(deadline_seconds), async {
                    while let Ok(Some(line)) = reader.next_line().await {
                        if line.trim().is_empty() {
                            continue;
                        }
                        if line.len() > MAX_FRAME_BYTES {
                            // Refused rather than truncated: half a frame is not
                            // a frame, and the count is what tells the adapter
                            // its numbering has a hole in it.
                            dropped.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        let mut buffered =
                            lines.lock().expect("the Codex line buffer lock is intact");
                        if buffered.len() >= MAX_BUFFERED_LINES {
                            buffered.pop_front();
                            dropped.fetch_add(1, Ordering::Relaxed);
                        }
                        buffered.push_back(line);
                    }
                })
                .await;

            let mut handle = child.lock().await;
            let reaped = match (closed, handle.as_mut()) {
                // The deadline elapsed with the process still writing.
                (Err(_), Some(process)) => {
                    let _ = process.start_kill();
                    let _ = process.wait().await;
                    CodexEnding::TimedOut
                }
                (Err(_), None) => CodexEnding::TimedOut,
                (Ok(()), Some(process)) => match process.wait().await {
                    Ok(status) => exit_ending(&status),
                    Err(_) => CodexEnding::Vanished,
                },
                (Ok(()), None) => CodexEnding::Eof,
            };
            let mut recorded = ending.lock().expect("the Codex ending lock is intact");
            // An explicit stop wins: it already reaped the child and recorded
            // what it did, and overwriting that here would rename an operator's
            // cancellation after the fact.
            if recorded.is_none() {
                *recorded = Some(reaped);
            }
        });
    }
}

/// The ending one exit status denotes.
///
/// A status is recorded, never interpreted: `code 0` and `code 1` are the same
/// kind of fact, and neither is a verdict on the work.
fn exit_ending(status: &std::process::ExitStatus) -> CodexEnding {
    match status.code() {
        Some(code) => CodexEnding::Exited { code },
        // No code means a signal on every platform this runs on.
        None => CodexEnding::Signalled {
            signal: signal_of(status),
        },
    }
}

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.signal().unwrap_or(0)
}

#[cfg(not(unix))]
fn signal_of(_status: &std::process::ExitStatus) -> i32 {
    0
}

#[async_trait]
impl CodexTransport for CodexLiveTransport {
    async fn start(
        &self,
        command: &CodexCommand,
        prepared: PreparedCommand,
    ) -> RuntimeResult<CodexStarted> {
        command.ensure_dispatchable()?;
        let mut process = tokio::process::Command::from(prepared.into_inner());
        process
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            // Never read, and never inherited either: Codex writes the prompt and
            // the config home into its diagnostics.
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);

        let mut child = process.spawn().map_err(|_| RuntimeError::Transport {
            rule: "process could not be started",
        })?;
        let process_id = child.id().ok_or(RuntimeError::Transport {
            rule: "process ended before it could be addressed",
        })?;
        let stdout = child.stdout.take().ok_or(RuntimeError::Transport {
            rule: "process produced no output channel",
        })?;

        let mut reader = BufReader::new(stdout).lines();
        let launch_ack = tokio::time::timeout(
            std::time::Duration::from_secs(self.start_timeout_seconds),
            reader.next_line(),
        )
        .await
        .map_err(|_| RuntimeError::Transport {
            rule: "process did not acknowledge the launch within the deadline",
        })?
        .map_err(|_| RuntimeError::Transport {
            rule: "channel failed before the runtime answered",
        })?
        .ok_or(RuntimeError::Transport {
            rule: "process ended without acknowledging the launch",
        })?;
        if launch_ack.len() > MAX_FRAME_BYTES {
            return Err(RuntimeError::Transport {
                rule: "answer exceeded the bounded frame size",
            });
        }

        let exec_id = ExternalId::parse(&format!("codex-exec-{}", uuid::Uuid::now_v7()))?;
        let lines = Arc::new(std::sync::Mutex::new(VecDeque::new()));
        let dropped = Arc::new(AtomicU64::new(0));
        let ending = Arc::new(std::sync::Mutex::new(None));
        let handle = Arc::new(AsyncMutex::new(Some(child)));
        Self::read_stdout(
            reader.into_inner().into_inner(),
            Arc::clone(&lines),
            Arc::clone(&dropped),
            Arc::clone(&ending),
            Arc::clone(&handle),
            self.run_deadline_seconds,
        );
        self.lock().insert(
            exec_id.as_str().to_owned(),
            LiveExec {
                process_id,
                lines,
                dropped,
                ending,
                child: handle,
            },
        );
        Ok(CodexStarted {
            exec_id,
            process_id,
            launch_ack,
        })
    }

    async fn drain(&self, exec_id: &ExternalId) -> RuntimeResult<CodexDrained> {
        let running = self.lock();
        let exec = running
            .get(exec_id.as_str())
            .ok_or_else(Self::unknown_handle)?;
        let mut buffered = exec
            .lines
            .lock()
            .expect("the Codex line buffer lock is intact");
        let take = buffered.len().min(MAX_FRAMES_PER_DRAIN);
        let lines = buffered.drain(..take).collect::<Vec<_>>();
        Ok(CodexDrained {
            lines,
            dropped: exec.dropped.swap(0, Ordering::Relaxed),
            ending: *exec.ending.lock().expect("the Codex ending lock is intact"),
        })
    }

    async fn stop(&self, exec_id: &ExternalId) -> RuntimeResult<CodexEnding> {
        let (child, ending) = {
            let running = self.lock();
            let exec = running
                .get(exec_id.as_str())
                .ok_or_else(Self::unknown_handle)?;
            (Arc::clone(&exec.child), Arc::clone(&exec.ending))
        };
        if let Some(recorded) = *ending.lock().expect("the Codex ending lock is intact") {
            return Ok(recorded);
        }
        let mut handle = child.lock().await;
        if let Some(process) = handle.as_mut() {
            let _ = process.start_kill();
            let _ = process.wait().await;
        }
        let mut recorded = ending.lock().expect("the Codex ending lock is intact");
        // A stop that raced the reader task loses: whichever reaped the child
        // first is the one that observed how it ended.
        let outcome = *recorded.get_or_insert(CodexEnding::Killed);
        Ok(outcome)
    }

    async fn liveness(&self, exec_id: &ExternalId) -> RuntimeResult<CodexLiveness> {
        let (process_id, child, ending) = {
            let running = self.lock();
            let exec = running
                .get(exec_id.as_str())
                .ok_or_else(Self::unknown_handle)?;
            (
                exec.process_id,
                Arc::clone(&exec.child),
                Arc::clone(&exec.ending),
            )
        };
        if let Some(recorded) = *ending.lock().expect("the Codex ending lock is intact") {
            return Ok(CodexLiveness {
                process_id,
                ending: Some(recorded),
            });
        }
        let mut handle = child.lock().await;
        let observed = match handle.as_mut().map(tokio::process::Child::try_wait) {
            Some(Ok(Some(status))) => Some(exit_ending(&status)),
            // Alive, or a status this process cannot read. Neither is a verdict.
            Some(Ok(None)) => None,
            Some(Err(_)) => Some(CodexEnding::Vanished),
            None => Some(CodexEnding::Vanished),
        };
        if let Some(seen) = observed {
            let mut recorded = ending.lock().expect("the Codex ending lock is intact");
            recorded.get_or_insert(seen);
        }
        Ok(CodexLiveness {
            process_id,
            ending: observed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(home: Option<&str>, prompt: &str) -> PreparedCommand {
        let mut command = std::process::Command::new("codex");
        command.args(["exec", "--json", prompt]);
        command.env_remove(crate::wire::CODEX_HOME);
        if let Some(home) = home {
            command.env(crate::wire::CODEX_HOME, home);
        }
        PreparedCommand::new(command)
    }

    #[test]
    fn the_ledger_route_never_quotes_the_operators_work() {
        let command = CodexCommand::exec(
            "codex",
            "/private/worktrees/secret-project",
            "the actual prompt",
            vec![crate::wire::CODEX_HOME.to_owned()],
        );
        assert_eq!(command.route(), "codex exec --json");
        assert!(!command.route().contains("the actual prompt"));
        assert!(!command.route().contains("secret-project"));
        // …while the argv, which only the transport sees, still carries the
        // prompt, because that is where `codex exec` reads it from.
        assert!(command.argv().iter().any(|arg| arg == "the actual prompt"));
        assert_eq!(
            command.argv()[..2],
            ["exec".to_owned(), "--json".to_owned()]
        );
        // The home is not in the argv under any circumstances: it travels only
        // in the environment, and only as a name out here.
        assert_eq!(command.env_names(), [crate::wire::CODEX_HOME.to_owned()]);
    }

    #[test]
    fn a_flag_shaped_prompt_cannot_become_another_option() {
        // A prompt interpolated raw is the argv analogue of an id interpolated
        // into a URL path: `--dangerously-bypass` is an option, not a task.
        assert!(
            CodexCommand::exec("codex", "/w/task-1", "--dangerously-bypass", Vec::new())
                .ensure_dispatchable()
                .is_err()
        );
        assert!(
            CodexCommand::exec("codex", "/w/task-1", "", Vec::new())
                .ensure_dispatchable()
                .is_err()
        );
        assert!(
            CodexCommand::exec("codex", "", "do the work", Vec::new())
                .ensure_dispatchable()
                .is_err()
        );
        assert!(
            CodexCommand::exec("", "/w/task-1", "do the work", Vec::new())
                .ensure_dispatchable()
                .is_err()
        );
        CodexCommand::exec("codex", "/w/task-1", "do the work", Vec::new())
            .ensure_dispatchable()
            .expect("an ordinary prompt dispatches");
    }

    #[test]
    fn a_prepared_command_reports_names_and_never_values() {
        let command = prepared(Some("/approved/homes/account-a"), "the actual prompt");
        assert_eq!(command.env_names(), [crate::wire::CODEX_HOME.to_owned()]);
        assert!(command.cleared_names().is_empty());
        let printed = format!("{command:?}");
        assert!(!printed.contains("account-a"), "got {printed}");
        assert!(!printed.contains("the actual prompt"), "got {printed}");
        assert!(printed.contains(crate::wire::CODEX_HOME));
        // The canary probe answers about the command without handing anything
        // back.
        assert!(command.contains("account-a"));
        assert!(!command.contains("account-b"));
    }

    #[test]
    fn an_inherited_config_home_is_cleared_rather_than_left_alone() {
        // The mutant this kills: building the child's environment without the
        // removal, so a `CODEX_HOME` in Kontor's own process leaks into a run
        // that was pinned to a different account.
        let command = prepared(None, "do the work");
        assert_eq!(
            command.cleared_names(),
            [crate::wire::CODEX_HOME.to_owned()],
            "the variable must be explicitly removed, not merely unset"
        );
        assert!(command.env_names().is_empty());
        assert_eq!(command.env_value(crate::wire::CODEX_HOME), None);
        // And when it is resolved, it is the resolved value the child receives.
        let resolved = prepared(Some("/approved/homes/account-a"), "do the work");
        assert_eq!(
            resolved.env_value(crate::wire::CODEX_HOME).as_deref(),
            Some("/approved/homes/account-a")
        );
    }

    #[test]
    fn a_live_transport_needs_positive_deadlines() {
        assert!(CodexLiveTransport::new(30, 900).is_ok());
        assert!(CodexLiveTransport::new(0, 900).is_err());
        assert!(CodexLiveTransport::new(30, 0).is_err());
    }
}
