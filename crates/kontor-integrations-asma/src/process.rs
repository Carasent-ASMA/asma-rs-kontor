//! The one process boundary.
//!
//! Everything this crate does to the world happens here, and it happens exactly
//! one way: spawn the resolved `asma` executable with an **argv array**, hand it
//! at most one JSON document on stdin, read a bounded stdout and a bounded
//! stderr under a wall-clock budget, and decode one JSON document back.
//!
//! There is deliberately no transport trait: one implementation behind an
//! interface is an indirection, not an abstraction. Tests substitute a temporary
//! executable, which is a truer double than a mocked trait — it exercises real
//! argv, real pipes, real exit codes and real timeouts.
//!
//! There is also deliberately no shell. `Command` passes argv straight to
//! `execvp`, so a model name containing `;` or `$(…)` is one literal argument
//! and cannot be interpreted.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

use crate::{AsmaError, UnavailableReason};

/// Default wall-clock budget for one `asma` invocation.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Default upper bound on one invocation's stdout, in bytes.
pub const DEFAULT_MAX_STDOUT_BYTES: usize = 1024 * 1024;

/// Upper bound on the diagnostic tail kept from stderr, in bytes.
const MAX_STDERR_BYTES: usize = 4096;

/// The resolved `asma` executable, with the budgets one invocation runs under.
///
/// The path is resolved by the caller (a deployment concern) and validated here:
/// an empty path, or one that is not a file, is refused at construction rather
/// than surfacing as a spawn failure on the first real operation.
#[derive(Debug, Clone)]
pub struct AsmaExecutable {
    executable: PathBuf,
    timeout: Duration,
    max_stdout_bytes: usize,
}

impl AsmaExecutable {
    /// Resolve the executable with the default budgets.
    ///
    /// # Errors
    /// Returns [`AsmaError::Refused`] when the path is empty or does not name an
    /// existing file.
    pub fn new(executable: impl Into<PathBuf>) -> Result<Self, AsmaError> {
        Self::with_budgets(executable, DEFAULT_TIMEOUT, DEFAULT_MAX_STDOUT_BYTES)
    }

    /// Resolve the executable with explicit budgets.
    ///
    /// # Errors
    /// Returns [`AsmaError::Refused`] when the path is empty or does not name an
    /// existing file, when the timeout is zero, or when the output bound is
    /// zero.
    pub fn with_budgets(
        executable: impl Into<PathBuf>,
        timeout: Duration,
        max_stdout_bytes: usize,
    ) -> Result<Self, AsmaError> {
        let executable = executable.into();
        if executable.as_os_str().is_empty() {
            return Err(AsmaError::refused(
                "resolve",
                "the asma executable path must not be empty",
            ));
        }
        if !executable.is_file() {
            return Err(AsmaError::refused(
                "resolve",
                "the asma executable path does not name a file",
            ));
        }
        if timeout.is_zero() {
            return Err(AsmaError::refused(
                "resolve",
                "an invocation budget of zero can never succeed",
            ));
        }
        if max_stdout_bytes == 0 {
            return Err(AsmaError::refused(
                "resolve",
                "an output bound of zero can never hold a response",
            ));
        }
        Ok(Self {
            executable,
            timeout,
            max_stdout_bytes,
        })
    }

    /// The resolved path, for receipts and diagnostics.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.executable
    }

    /// Run one operation and decode its single JSON response.
    ///
    /// `request` is written to stdin when present; stdin is closed immediately
    /// afterwards so a child that waits for end-of-input makes progress.
    ///
    /// # Errors
    /// Returns [`AsmaError::Unavailable`] for a spawn failure, a pipe failure, a
    /// timeout, output past the bound, a non-zero exit and a response that is
    /// not the expected JSON document. Every diagnostic is bounded and scrubbed
    /// of credential material before it is carried.
    pub(crate) async fn run_json<Q, R>(
        &self,
        operation: &'static str,
        argv: &[&str],
        request: Option<&Q>,
    ) -> Result<R, AsmaError>
    where
        Q: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let payload = match request {
            Some(request) => Some(serde_json::to_vec(request).map_err(|error| {
                AsmaError::unavailable(
                    operation,
                    UnavailableReason::MalformedResponse,
                    format!("the request could not be serialized: {error}"),
                )
            })?),
            None => None,
        };
        let output = self.exchange(operation, argv, payload.as_deref()).await?;
        serde_json::from_slice(&output).map_err(|error| {
            AsmaError::unavailable(
                operation,
                UnavailableReason::MalformedResponse,
                format!("the response is not the expected JSON document: {error}"),
            )
        })
    }

    /// Spawn, feed, drain and reap one child, returning its stdout.
    async fn exchange(
        &self,
        operation: &'static str,
        argv: &[&str],
        payload: Option<&[u8]>,
    ) -> Result<Vec<u8>, AsmaError> {
        let mut child = self.spawn(operation, argv, payload.is_some())?;
        let collected = tokio::time::timeout(
            self.timeout,
            collect(&mut child, payload, self.max_stdout_bytes),
        )
        .await;

        let Ok(collected) = collected else {
            // The budget is the only thing standing between a wedged child and a
            // wedged control plane, so the kill is not best-effort cleanup.
            let _ = child.start_kill();
            return Err(AsmaError::unavailable(
                operation,
                UnavailableReason::Timeout,
                format!("no response within {:?}", self.timeout),
            ));
        };
        let Collected {
            stdout,
            stderr,
            status,
            truncated,
        } = collected.map_err(|error| {
            AsmaError::unavailable(
                operation,
                UnavailableReason::Transport,
                format!("the child's pipes failed: {error}"),
            )
        })?;

        if truncated {
            return Err(AsmaError::unavailable(
                operation,
                UnavailableReason::OversizedOutput,
                format!("wrote more than {} bytes", self.max_stdout_bytes),
            ));
        }
        if !status.success() {
            return Err(AsmaError::unavailable(
                operation,
                UnavailableReason::ExitStatus,
                format!(
                    "exited with {}: {}",
                    describe(status),
                    String::from_utf8_lossy(&stderr)
                ),
            ));
        }
        Ok(stdout)
    }

    fn spawn(
        &self,
        operation: &'static str,
        argv: &[&str],
        piped_stdin: bool,
    ) -> Result<Child, AsmaError> {
        let mut command = Command::new(&self.executable);
        command
            .args(argv.iter().map(OsStr::new))
            .stdin(if piped_stdin {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // A control plane that drops a delegation must not leave the
            // delegate running against a live external system.
            .kill_on_drop(true);
        command.spawn().map_err(|error| {
            AsmaError::unavailable(
                operation,
                UnavailableReason::Spawn,
                format!("could not start the asma executable: {error}"),
            )
        })
    }
}

/// One child's bounded output and exit status.
struct Collected {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status: ExitStatus,
    truncated: bool,
}

/// Feed stdin, drain both output pipes under their bounds, and reap the child.
///
/// Reading is concurrent with writing because a child that answers before it has
/// consumed all of stdin — or one that fills its stdout pipe before reading
/// stdin — would otherwise deadlock against us.
async fn collect(
    child: &mut Child,
    payload: Option<&[u8]>,
    max_stdout_bytes: usize,
) -> std::io::Result<Collected> {
    let mut stdin = child.stdin.take();
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("the child exposed no stdout pipe"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("the child exposed no stderr pipe"))?;

    let mut out = Vec::new();
    let mut err = Vec::new();
    let write = async {
        if let (Some(stdin), Some(payload)) = (stdin.as_mut(), payload) {
            stdin.write_all(payload).await?;
            stdin.shutdown().await?;
        }
        // Closing stdin is part of the protocol, not cleanup: a child reading to
        // end-of-input never returns until it happens.
        drop(stdin.take());
        Ok::<(), std::io::Error>(())
    };
    // One byte past the bound is read on purpose: it is the only way to tell
    // "exactly at the bound" from "over it" without a second syscall.
    let read_out = async {
        (&mut stdout)
            .take(max_stdout_bytes as u64 + 1)
            .read_to_end(&mut out)
            .await
    };
    let read_err = async {
        (&mut stderr)
            .take(MAX_STDERR_BYTES as u64)
            .read_to_end(&mut err)
            .await
    };
    let (_, _, _) = tokio::try_join!(write, read_out, read_err)?;

    let status = child.wait().await?;
    let truncated = out.len() > max_stdout_bytes;
    Ok(Collected {
        stdout: out,
        stderr: err,
        status,
        truncated,
    })
}

/// Describe an exit status without depending on a platform's `Display`.
fn describe(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("code {code}"),
        None => "a signal".to_owned(),
    }
}
