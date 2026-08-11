//! A recorded Codex process: scripted stdout in, a redacted dispatch ledger out.
//!
//! This is the same choice `kontor-runtime` makes with its scripted fake, and for
//! the same reason: the hard cases in this adapter are *orderings*, not payloads.
//! "The refusal happened before any process was started", "the replayed launch
//! did not spawn twice", "the cancellation reached the process this binding
//! names" are all claims about the process table, and a recorded ledger is the
//! only thing that can settle them.
//!
//! Two properties matter and are why this is a transport rather than a stub
//! `codex` binary on `PATH`:
//!
//! * every ending can be scripted — EOF, a zero exit, a non-zero exit, a signal,
//!   a deadline, a kill, a vanished process — which is the whole set of things
//!   this adapter must refuse to read as a verdict;
//! * the ledger keys on the redacted route, so an assertion about a dispatch can
//!   never accidentally quote a prompt or a config home.
//!
//! It is public because the contract suite lives in a sibling test target.
//! Nothing here has an opinion about Codex's behavior — every answer comes from a
//! script the test wrote.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::Mutex;

use async_trait::async_trait;
use kontor_core::id::ExternalId;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};

use crate::client::{
    CodexCommand, CodexDrained, CodexLiveness, CodexStarted, CodexTransport, PreparedCommand,
};
use crate::wire::{CodexEnding, REDACTED};

/// One scripted Codex process.
///
/// `Debug` is written out, like every other rendering in this crate: the scripted
/// lines stand in for the agent's own output, and a fixture that printed them
/// would be modelling the leak rather than the process.
#[derive(Clone, Default)]
pub struct CodexScript {
    /// The stdout line the process answers its launch with.
    launch_ack: String,
    /// Later stdout, one chunk per drain. Chunks rather than one list because
    /// "cancelled while it was still writing" is an ordering, and an ordering
    /// needs more than one moment to exist in.
    chunks: VecDeque<Vec<String>>,
    /// Lines the transport could not keep, reported on the next drain.
    dropped: u64,
    /// How the process ends once its scripted output is exhausted. `None` keeps
    /// it running for as long as the test wants to look at it.
    ending: Option<CodexEnding>,
    /// A start that never produces a process at all.
    start_failure: Option<&'static str>,
}

impl CodexScript {
    /// A process that acknowledges its launch with `launch_ack` and then waits.
    #[must_use]
    pub fn acknowledging(launch_ack: &str) -> Self {
        Self {
            launch_ack: launch_ack.to_owned(),
            ..Self::default()
        }
    }

    /// A process that cannot be started at all.
    #[must_use]
    pub fn failing_to_start(rule: &'static str) -> Self {
        Self {
            start_failure: Some(rule),
            ..Self::default()
        }
    }

    /// Add one drain's worth of stdout.
    #[must_use]
    pub fn then_printing(mut self, lines: &[&str]) -> Self {
        self.chunks
            .push_back(lines.iter().map(|line| (*line).to_owned()).collect());
        self
    }

    /// Report `count` lines the transport had to discard.
    ///
    /// The only honest way to model a stdout reader that fell behind: the content
    /// is gone, and the count is what the adapter turns into a typed gap rather
    /// than renumbering over.
    #[must_use]
    pub const fn dropping(mut self, count: u64) -> Self {
        self.dropped = count;
        self
    }

    /// End the process, once its scripted output has been drained.
    #[must_use]
    pub const fn ending_with(mut self, ending: CodexEnding) -> Self {
        self.ending = Some(ending);
        self
    }
}

/// One dispatch, reduced to what is safe to record.
///
/// Deliberately not the command: the prompt and the config home are the two
/// things this adapter must keep out of every artefact, so they are absent by
/// construction rather than by a redaction someone has to remember.
///
/// `cwd` is the one path here, and it is a *field a test reads deliberately* —
/// "the process ran in the verified worktree" is a property worth asserting. That
/// is a different thing from a path a log line prints by accident, so the field
/// stays and the `Debug` redacts it.
#[derive(Clone, PartialEq, Eq)]
pub struct CodexDispatch {
    /// The redacted route.
    pub route: &'static str,
    /// The working directory the process was given.
    pub cwd: String,
    /// The *names* of the environment variables it was given.
    pub env_names: Vec<String>,
    /// The names it explicitly cleared before those were written.
    pub cleared_names: Vec<String>,
    /// The handle the transport issued.
    pub exec_id: ExternalId,
    /// The process identity it stands for.
    pub process_id: u32,
}

impl fmt::Debug for CodexScript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexScript")
            .field("chunks", &self.chunks.len())
            .field("dropped", &self.dropped)
            .field("ending", &self.ending)
            .field("start_failure", &self.start_failure)
            .field("stdout", &REDACTED)
            .finish()
    }
}

impl fmt::Debug for CodexDispatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexDispatch")
            .field("route", &self.route)
            .field("env_names", &self.env_names)
            .field("cleared_names", &self.cleared_names)
            .field("exec_id", &self.exec_id)
            .field("process_id", &self.process_id)
            .field("cwd", &REDACTED)
            .finish()
    }
}

#[derive(Debug)]
struct RecordedExec {
    script: CodexScript,
    process_id: u32,
    ending: Option<CodexEnding>,
    drops_reported: bool,
}

/// A recorded Codex executable.
///
/// `Debug` is written out for one reason the production types do not have: the
/// canary lists hold exactly the strings a test planted to prove they never
/// escape, so a fixture that printed its own watch list would hand them back out
/// through the very channel it exists to police.
pub struct RecordedCodex {
    scripts: Mutex<VecDeque<CodexScript>>,
    execs: Mutex<BTreeMap<String, RecordedExec>>,
    dispatches: Mutex<Vec<CodexDispatch>>,
    calls: Mutex<Vec<String>>,
    /// Canaries the fixture proves never reached a dispatch.
    canaries: Mutex<Vec<String>>,
    /// Canaries that a dispatch was actually found to carry.
    leaked: Mutex<Vec<String>>,
    next: Mutex<u32>,
}

impl fmt::Debug for RecordedCodex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count =
            |held: &Mutex<Vec<String>>| held.lock().map(|values| values.len()).unwrap_or_default();
        f.debug_struct("RecordedCodex")
            .field(
                "dispatches",
                &self.dispatches.lock().ok().map(|it| it.len()),
            )
            .field("calls", &count(&self.calls))
            .field("canaries", &count(&self.canaries))
            .field("leaked", &count(&self.leaked))
            .field("watched_values", &REDACTED)
            .finish_non_exhaustive()
    }
}

impl Default for RecordedCodex {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordedCodex {
    /// An executable that has been asked for nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scripts: Mutex::new(VecDeque::new()),
            execs: Mutex::new(BTreeMap::new()),
            dispatches: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
            canaries: Mutex::new(Vec::new()),
            leaked: Mutex::new(Vec::new()),
            next: Mutex::new(0),
        }
    }

    /// Queue the next process this executable will start.
    #[must_use]
    pub fn running(self, script: CodexScript) -> Self {
        self.scripts
            .lock()
            .expect("the fixture lock is intact")
            .push_back(script);
        self
    }

    /// Watch for `canary` in every dispatch's program, argv, working directory
    /// and environment *values*.
    ///
    /// The probe answers a yes/no question and never returns what it searched, so
    /// a test can plant an auth file's contents or another account's home and
    /// prove they never reached a process without holding either afterwards.
    #[must_use]
    pub fn watching_for(self, canary: &str) -> Self {
        self.canaries
            .lock()
            .expect("the fixture lock is intact")
            .push(canary.to_owned());
        self
    }

    /// Every canary that was actually found in a dispatch. Empty is the answer
    /// the account-isolation claim needs.
    #[must_use]
    pub fn leaked_canaries(&self) -> Vec<String> {
        self.leaked
            .lock()
            .expect("the fixture lock is intact")
            .clone()
    }

    /// Every call made so far, in order, as its redacted route.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("the fixture lock is intact")
            .clone()
    }

    /// How many calls of `route` were made.
    #[must_use]
    pub fn count(&self, route: &str) -> usize {
        self.calls()
            .iter()
            .filter(|made| made.as_str() == route)
            .count()
    }

    /// Every call that could have changed the machine.
    ///
    /// A refusal that must happen before any process exists is proved by this
    /// being empty.
    #[must_use]
    pub fn mutations(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|made| made != "codex liveness" && made != "codex drain")
            .collect()
    }

    /// Every process that was started, with the redacted facts about it.
    #[must_use]
    pub fn dispatches(&self) -> Vec<CodexDispatch> {
        self.dispatches
            .lock()
            .expect("the fixture lock is intact")
            .clone()
    }

    /// How many processes were started.
    #[must_use]
    pub fn started(&self) -> usize {
        self.dispatches().len()
    }

    /// Forget every recorded call, keeping the queued scripts and the live
    /// processes.
    pub fn take_calls(&self) -> Vec<String> {
        std::mem::take(&mut self.calls.lock().expect("the fixture lock is intact"))
    }

    fn record(&self, route: &'static str) {
        self.calls
            .lock()
            .expect("the fixture lock is intact")
            .push(route.to_owned());
    }

    fn unknown_handle() -> RuntimeError {
        RuntimeError::Transport {
            rule: "handle does not name a process this transport started",
        }
    }
}

/// Share one recorded executable between the adapter and the test that inspects
/// it.
///
/// `CodexAdapter` takes ownership of its transport, so a test that also wants to
/// read the dispatch ledger needs a second handle to the same fixture rather than
/// a copy of it — a copy would have its own ledger and would prove nothing.
#[async_trait]
impl CodexTransport for std::sync::Arc<RecordedCodex> {
    async fn start(
        &self,
        command: &CodexCommand,
        prepared: PreparedCommand,
    ) -> RuntimeResult<CodexStarted> {
        Self::as_ref(self).start(command, prepared).await
    }

    async fn drain(&self, exec_id: &ExternalId) -> RuntimeResult<CodexDrained> {
        Self::as_ref(self).drain(exec_id).await
    }

    async fn stop(&self, exec_id: &ExternalId) -> RuntimeResult<CodexEnding> {
        Self::as_ref(self).stop(exec_id).await
    }

    async fn liveness(&self, exec_id: &ExternalId) -> RuntimeResult<CodexLiveness> {
        Self::as_ref(self).liveness(exec_id).await
    }
}

#[async_trait]
impl CodexTransport for RecordedCodex {
    async fn start(
        &self,
        command: &CodexCommand,
        prepared: PreparedCommand,
    ) -> RuntimeResult<CodexStarted> {
        // Recorded *before* the answer is decided, so a start that then fails
        // still counts as a process this adapter tried to create. Recording only
        // the successful ones would make "the refusal started nothing" untestable
        // in the one direction that matters.
        self.record(command.route());
        command.ensure_dispatchable()?;

        // The canary probe runs on every dispatch, not only the ones a test
        // remembers to check.
        for canary in self
            .canaries
            .lock()
            .expect("the fixture lock is intact")
            .iter()
        {
            if prepared.contains(canary) {
                self.leaked
                    .lock()
                    .expect("the fixture lock is intact")
                    .push(canary.clone());
            }
        }

        let script = self
            .scripts
            .lock()
            .expect("the fixture lock is intact")
            .pop_front()
            .ok_or(RuntimeError::Transport {
                rule: "process could not be started",
            })?;
        if let Some(rule) = script.start_failure {
            return Err(RuntimeError::Transport { rule });
        }

        let ordinal = {
            let mut next = self.next.lock().expect("the fixture lock is intact");
            *next += 1;
            *next
        };
        let exec_id = ExternalId::parse(&format!("codex-exec-{ordinal}"))?;
        let process_id = 40_000 + ordinal;
        self.dispatches
            .lock()
            .expect("the fixture lock is intact")
            .push(CodexDispatch {
                route: command.route(),
                cwd: command.cwd().to_owned(),
                env_names: prepared.env_names(),
                cleared_names: prepared.cleared_names(),
                exec_id: exec_id.clone(),
                process_id,
            });
        let launch_ack = script.launch_ack.clone();
        self.execs
            .lock()
            .expect("the fixture lock is intact")
            .insert(
                exec_id.as_str().to_owned(),
                RecordedExec {
                    script,
                    process_id,
                    ending: None,
                    drops_reported: false,
                },
            );
        Ok(CodexStarted {
            exec_id,
            process_id,
            launch_ack,
        })
    }

    async fn drain(&self, exec_id: &ExternalId) -> RuntimeResult<CodexDrained> {
        self.record("codex drain");
        let mut execs = self.execs.lock().expect("the fixture lock is intact");
        let exec = execs
            .get_mut(exec_id.as_str())
            .ok_or_else(Self::unknown_handle)?;
        let lines = exec.script.chunks.pop_front().unwrap_or_default();
        let dropped = if exec.drops_reported {
            0
        } else {
            exec.drops_reported = true;
            exec.script.dropped
        };
        // The scripted ending arrives once there is nothing left to print, which
        // is the ordering a real process has: output first, then EOF.
        if exec.script.chunks.is_empty() && exec.ending.is_none() {
            exec.ending = exec.script.ending;
        }
        Ok(CodexDrained {
            lines,
            dropped,
            ending: exec.ending,
        })
    }

    async fn stop(&self, exec_id: &ExternalId) -> RuntimeResult<CodexEnding> {
        self.record("codex stop");
        let mut execs = self.execs.lock().expect("the fixture lock is intact");
        let exec = execs
            .get_mut(exec_id.as_str())
            .ok_or_else(Self::unknown_handle)?;
        // Idempotent, and it never renames an ending that already happened: a
        // process that had already gone was not killed by this call.
        Ok(*exec.ending.get_or_insert(CodexEnding::Killed))
    }

    async fn liveness(&self, exec_id: &ExternalId) -> RuntimeResult<CodexLiveness> {
        self.record("codex liveness");
        let mut execs = self.execs.lock().expect("the fixture lock is intact");
        let exec = execs
            .get_mut(exec_id.as_str())
            .ok_or_else(Self::unknown_handle)?;
        // A process whose scripted output is exhausted has ended, whether or not
        // anyone drained it — which is how "the process disappeared while nobody
        // was looking" is expressed.
        if exec.ending.is_none() && exec.script.chunks.is_empty() {
            exec.ending = exec.script.ending;
        }
        Ok(CodexLiveness {
            process_id: exec.process_id,
            ending: exec.ending,
        })
    }
}
