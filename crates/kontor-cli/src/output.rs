//! One JSON value on standard output, one exit class, and diagnostics kept out of
//! the way.
//!
//! # The three rules this module exists to keep
//!
//! 1. **Exactly one JSON value reaches standard output.** A caller pipes `kontor`
//!    into `jq` and gets a document, not a log with a document in it. Every
//!    diagnostic — a hint about a missing credential file, a note that a stream
//!    read hit its bound — goes to standard error.
//! 2. **The exit code is a class, not a number somebody picked.** A script
//!    branches on it without parsing the body: retryable is 5, "read it again" is
//!    4, "you may not" is 3. [`ExitClass`] is the whole mapping and
//!    [`ExitClass::of`] is derived from the daemon's own code vocabulary rather
//!    than from a string this crate invented.
//! 3. **A broken pipe is not a failure.** `kontor events | head -1` closes the
//!    pipe under us on purpose. Reporting that as an error would make every
//!    ordinary shell idiom look broken, so it exits 0 and says nothing.
//!
//! # Why the daemon's codes are not translated
//!
//! The failure body printed on standard output is the `ApiErrorBody` the daemon
//! sent, unchanged. This crate reads one field out of it — `code` — to choose an
//! exit class, and rewrites nothing. A CLI that renamed `revision_conflict` into
//! its own vocabulary would be a second contract with its own drift, and the
//! revision the caller is owed lives in that body.
//!
//! An unrecognised code exits 1. That is deliberate and it is not a catch-all: the
//! twelve codes plus `invalid_request` are a *closed* vocabulary, so a code outside
//! it means the thing answering is not a Realm of this contract generation — which
//! is a protocol problem, not a refusal to interpret.

use std::io::{IsTerminal, Write};

use kontor_mcp::Envelope;

/// What a caller's shell learns from an exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitClass {
    /// The command did what it said, including an idempotent replay and a valid
    /// dry run.
    Success,
    /// Something answered, but not this contract. A transport or protocol failure.
    Unexpected,
    /// The command line itself, or this machine's configuration.
    Local,
    /// The Realm would not authenticate or would not authorize.
    Refused,
    /// The caller's state is stale: read it again and retry.
    Conflict,
    /// A dependency is not ready. Retryable without changing anything.
    Unavailable,
    /// The thing addressed does not exist, or the runtime cannot do it.
    Absent,
}

impl ExitClass {
    /// The process exit code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Unexpected => 1,
            Self::Local => 2,
            Self::Refused => 3,
            Self::Conflict => 4,
            Self::Unavailable => 5,
            Self::Absent => 6,
        }
    }

    /// The class one stable machine code belongs to.
    ///
    /// The grouping is by *what a caller should do next*, which is why
    /// `resnapshot_required` and `timeline_refetch_required` sit with
    /// `revision_conflict`: all three mean "the position you hold is not usable,
    /// read again from what the body tells you". And why `unsupported_capability`
    /// sits with `not_found`: neither will succeed on a retry, so neither belongs
    /// with the retryable class.
    #[must_use]
    pub fn of(code: &str) -> Self {
        match code {
            "unauthenticated" | "forbidden" => Self::Refused,
            "realm_mismatch"
            | "revision_conflict"
            | "idempotency_conflict"
            | "stale_binding"
            | "resnapshot_required"
            | "timeline_refetch_required" => Self::Conflict,
            "reconciliation_pending" | "unavailable" => Self::Unavailable,
            "not_found" | "unsupported_capability" => Self::Absent,
            "invalid_request" => Self::Local,
            // Outside the closed vocabulary: whatever answered is not a realm of
            // this contract generation.
            _ => Self::Unexpected,
        }
    }
}

/// Write one successful envelope and report the exit class.
///
/// # Errors
/// Never returns an error: a broken pipe is reported as success, and any other
/// write failure as [`ExitClass::Unexpected`], because a caller that cannot be
/// written to cannot be told about it either.
#[must_use]
pub fn emit(envelope: &Envelope) -> ExitClass {
    match serde_json::to_string_pretty(envelope) {
        Err(_) => {
            note("the answer could not be rendered as JSON");
            ExitClass::Unexpected
        }
        Ok(document) => write_document(&mut std::io::stdout(), &document, ExitClass::Success),
    }
}

/// Write one refusal body and report the exit class it maps to.
#[must_use]
pub fn emit_refusal(body: &serde_json::Value, code: &str) -> ExitClass {
    let class = ExitClass::of(code);
    match serde_json::to_string_pretty(body) {
        Err(_) => {
            note("the refusal could not be rendered as JSON");
            ExitClass::Unexpected
        }
        Ok(document) => write_document(&mut std::io::stdout(), &document, class),
    }
}

/// Write one document, turning a closed pipe into a clean exit.
///
/// The writer is a parameter so the broken-pipe rule is *tested* rather than
/// asserted in a comment: forcing a real `EPIPE` on a real standard output would
/// need a child process and a closed pipe, and a unit test with a writer that
/// refuses is the same claim without either.
fn write_document(out: &mut impl Write, document: &str, class: ExitClass) -> ExitClass {
    match writeln!(out, "{document}").and_then(|()| out.flush()) {
        Ok(()) => class,
        // `kontor events | head -1` is a caller reading what it wanted and leaving.
        // There is nothing wrong, and there is also nowhere left to say so.
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => ExitClass::Success,
        Err(_) => ExitClass::Unexpected,
    }
}

/// Put one diagnostic on standard error.
///
/// Never on standard output: the document there is the contract, and a hint in the
/// middle of it would break every caller that parses it.
pub fn note(detail: impl std::fmt::Display) {
    let mut err = std::io::stderr();
    // A failed write to standard error is ignored on purpose. There is no third
    // stream to complain to, and losing a hint must not change an exit code.
    let _ = writeln!(err, "kontor: {detail}");
}

/// Whether standard output is a terminal.
///
/// Used only to decide whether a hint is worth printing at all: a caller piping
/// into `jq` does not want prose, and a caller at a prompt is the one who benefits
/// from being told what to do next.
#[must_use]
pub fn interactive() -> bool {
    std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_in_the_closed_vocabulary_has_a_class_and_none_is_zero() {
        // The vocabulary is `kontor_api::error::ApiErrorCode`. Every one of them
        // must map to a non-zero class: a refusal that exited 0 would make `set -e`
        // useless against this control plane.
        for code in [
            "unauthenticated",
            "forbidden",
            "realm_mismatch",
            "revision_conflict",
            "idempotency_conflict",
            "unsupported_capability",
            "stale_binding",
            "resnapshot_required",
            "timeline_refetch_required",
            "reconciliation_pending",
            "unavailable",
            "not_found",
            "invalid_request",
        ] {
            let class = ExitClass::of(code);
            assert_ne!(
                class,
                ExitClass::Success,
                "{code} is a refusal and must not exit 0"
            );
            assert_ne!(
                class,
                ExitClass::Unexpected,
                "{code} is in the closed vocabulary and must have its own class, not the \
                 unrecognised-code fallback"
            );
        }
    }

    #[test]
    fn the_exit_classes_are_the_numbers_a_script_branches_on() {
        assert_eq!(ExitClass::Success.code(), 0);
        assert_eq!(ExitClass::Unexpected.code(), 1);
        assert_eq!(ExitClass::Local.code(), 2);
        assert_eq!(ExitClass::Refused.code(), 3);
        assert_eq!(ExitClass::Conflict.code(), 4);
        assert_eq!(ExitClass::Unavailable.code(), 5);
        assert_eq!(ExitClass::Absent.code(), 6);
    }

    #[test]
    fn each_code_lands_in_the_class_that_says_what_to_do_next() {
        assert_eq!(ExitClass::of("unauthenticated"), ExitClass::Refused);
        assert_eq!(ExitClass::of("forbidden"), ExitClass::Refused);
        // Read it again, then retry.
        for code in [
            "realm_mismatch",
            "revision_conflict",
            "idempotency_conflict",
            "stale_binding",
            "resnapshot_required",
            "timeline_refetch_required",
        ] {
            assert_eq!(ExitClass::of(code), ExitClass::Conflict, "{code}");
        }
        // Retryable unchanged.
        assert_eq!(
            ExitClass::of("reconciliation_pending"),
            ExitClass::Unavailable
        );
        assert_eq!(ExitClass::of("unavailable"), ExitClass::Unavailable);
        // Never going to succeed as asked.
        assert_eq!(ExitClass::of("not_found"), ExitClass::Absent);
        assert_eq!(
            ExitClass::of("unsupported_capability"),
            ExitClass::Absent,
            "a runtime that never declared an operation will not declare it on a retry"
        );
        assert_eq!(ExitClass::of("invalid_request"), ExitClass::Local);
    }

    /// A writer that fails every write with one chosen kind.
    struct Refusing(std::io::ErrorKind);

    impl Write for Refusing {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(self.0, "refused"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(self.0, "refused"))
        }
    }

    #[test]
    fn a_closed_pipe_is_a_clean_exit_and_any_other_write_failure_is_not() {
        // `kontor events | head -1` closes the pipe deliberately. Reporting that as
        // a failure would make every ordinary shell idiom look broken.
        assert_eq!(
            write_document(
                &mut Refusing(std::io::ErrorKind::BrokenPipe),
                "{}",
                ExitClass::Success
            ),
            ExitClass::Success
        );
        assert_eq!(
            write_document(
                &mut Refusing(std::io::ErrorKind::BrokenPipe),
                "{}",
                ExitClass::Conflict
            ),
            ExitClass::Success,
            "a caller that stopped reading is not owed the refusal it did not wait for"
        );
        // Anything else is a real failure: the caller asked for a document and
        // cannot be told it did not arrive.
        assert_eq!(
            write_document(
                &mut Refusing(std::io::ErrorKind::PermissionDenied),
                "{}",
                ExitClass::Success
            ),
            ExitClass::Unexpected
        );
    }

    #[test]
    fn a_written_document_keeps_the_class_it_was_given() {
        let mut sink = Vec::new();
        assert_eq!(
            write_document(&mut sink, "{\"a\":1}", ExitClass::Conflict),
            ExitClass::Conflict
        );
        assert_eq!(
            String::from_utf8(sink).expect("the document is UTF-8"),
            "{\"a\":1}\n",
            "exactly one document and one newline reach standard output"
        );
    }

    #[test]
    fn an_unrecognised_code_is_a_protocol_problem_and_not_a_refusal() {
        for code in ["teapot", "", "FORBIDDEN", "not-found"] {
            assert_eq!(
                ExitClass::of(code),
                ExitClass::Unexpected,
                "{code} is outside the closed vocabulary"
            );
        }
    }
}
