//! One JSON value on standard output, one exit class, and diagnostics kept out of
//! the way.
//!
//! # The three rules this module exists to keep
//!
//! 1. **Exactly one JSON value reaches standard output.** A caller pipes `kontor`
//!    into `jq` and gets a document, not a log with a document in it. Every
//!    diagnostic goes to standard error.
//! 2. **The exit code is a class, not a number somebody picked.** A script branches
//!    on it without parsing the body: retryable is 5, "read it again" is 4, "you
//!    may not" is 3. [`ExitClass::of`] is derived from the daemon's own code
//!    vocabulary rather than from a string this crate invented.
//! 3. **A broken pipe is not a failure.** `kontor events-list | head -1` closes the
//!    pipe under us on purpose. Reporting that as an error would make every
//!    ordinary shell idiom look broken, so it exits 0 and says nothing.
//!
//! # Why the daemon's codes are not translated
//!
//! The body printed on standard output is the one the daemon sent, unchanged. This
//! crate reads one field out of it — `code` — to choose an exit class, and rewrites
//! nothing. A CLI that renamed `revision_conflict` into its own vocabulary would be
//! a second contract with its own drift, and the revision the caller is owed lives
//! in that body.
//!
//! An unrecognised code exits 1. That is deliberate and not a catch-all: the twelve
//! codes plus `invalid_request` are a *closed* vocabulary, so a code outside it
//! means the thing answering is not a Realm of this contract generation — a
//! protocol problem, not a refusal to interpret.

use std::io::Write as _;

use kontor_mcp::Envelope;

/// What a caller's shell learns from an exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitClass {
    /// The command did what it said, including an idempotent replay.
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
    pub(crate) const fn code(self) -> u8 {
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
    /// sits with `not_found`: neither will succeed on a retry. And why
    /// `capacity_exhausted` sits with `reconciliation_pending`: both are "the
    /// plane is not able to take this yet", and both clear without the caller
    /// changing anything about the request.
    #[must_use]
    pub(crate) fn of(code: &str) -> Self {
        match code {
            "unauthenticated" | "forbidden" => Self::Refused,
            "realm_mismatch"
            | "revision_conflict"
            | "idempotency_conflict"
            | "stale_binding"
            | "resnapshot_required"
            | "timeline_refetch_required" => Self::Conflict,
            "reconciliation_pending" | "unavailable" | "capacity_exhausted" => Self::Unavailable,
            "not_found" | "unsupported_capability" | "role_slot_unbound" => Self::Absent,
            "invalid_request" => Self::Local,
            // Outside the closed vocabulary: whatever answered is not a realm of
            // this contract generation.
            _ => Self::Unexpected,
        }
    }
}

/// Write one envelope and report the exit class its status earned.
///
/// The daemon's body is printed whether it succeeded or not: a refusal carries the
/// revision, the receipt or the rule the caller needs, and swallowing it to print a
/// message would throw away the only useful part.
#[must_use]
pub(crate) fn emit(envelope: &Envelope) -> ExitClass {
    let class = if envelope.is_success() {
        ExitClass::Success
    } else {
        envelope.code().map_or(ExitClass::Unexpected, ExitClass::of)
    };
    match serde_json::to_string_pretty(envelope) {
        Err(_) => {
            note("the answer could not be rendered as JSON");
            ExitClass::Unexpected
        }
        Ok(document) => write_document(&mut std::io::stdout(), &document, class),
    }
}

/// Write one local refusal — nothing was dispatched — and report its class.
#[must_use]
pub(crate) fn emit_local(tool: &str, code: &str, rule: &str) -> ExitClass {
    let document = serde_json::json!({
        "tool": tool,
        "code": code,
        "rule": rule,
        "dispatched": false,
    });
    let class = ExitClass::of(code);
    match serde_json::to_string_pretty(&document) {
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
fn write_document(out: &mut impl std::io::Write, document: &str, class: ExitClass) -> ExitClass {
    match writeln!(out, "{document}").and_then(|()| out.flush()) {
        Ok(()) => class,
        // `kontor events-list | head -1` is a caller reading what it wanted and
        // leaving. There is nothing wrong, and nowhere left to say so.
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => ExitClass::Success,
        Err(_) => ExitClass::Unexpected,
    }
}

/// Put one diagnostic on standard error.
///
/// Never on standard output: the document there is the contract, and a hint in the
/// middle of it would break every caller that parses it.
pub(crate) fn note(detail: impl std::fmt::Display) {
    // A failed write to standard error is ignored on purpose. There is no third
    // stream to complain to, and losing a hint must not change an exit code.
    let _ = writeln!(std::io::stderr(), "kontor: {detail}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_in_the_closed_vocabulary_has_a_class_and_none_is_zero() {
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
            "capacity_exhausted",
            "role_slot_unbound",
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
                "{code} is in the closed vocabulary and must have its own class"
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
        assert_eq!(
            ExitClass::of("reconciliation_pending"),
            ExitClass::Unavailable
        );
        assert_eq!(
            ExitClass::of("capacity_exhausted"),
            ExitClass::Unavailable,
            "a spent ceiling clears on its own, so the answer is to come back — \
             not to re-read a revision that was never stale"
        );
        assert_eq!(
            ExitClass::of("unsupported_capability"),
            ExitClass::Absent,
            "a runtime that never declared an operation will not declare it on a retry"
        );
        assert_eq!(ExitClass::of("invalid_request"), ExitClass::Local);
    }

    #[test]
    fn an_unrecognised_code_is_a_protocol_problem_and_not_a_refusal() {
        for code in ["teapot", "", "FORBIDDEN", "not-found"] {
            assert_eq!(ExitClass::of(code), ExitClass::Unexpected, "{code}");
        }
    }

    /// A writer that fails every write with one chosen kind.
    struct Refusing(std::io::ErrorKind);

    impl std::io::Write for Refusing {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(self.0, "refused"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(self.0, "refused"))
        }
    }

    #[test]
    fn a_closed_pipe_is_a_clean_exit_and_any_other_write_failure_is_not() {
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
    fn a_refusal_from_the_daemon_keeps_its_body_and_earns_its_own_class() {
        let envelope = Envelope {
            tool: "kontor_epic_apply".to_owned(),
            status: 409,
            body: serde_json::json!({ "code": "revision_conflict", "current_revision": 9 }),
        };
        let mut sink = Vec::new();
        let class = if envelope.is_success() {
            ExitClass::Success
        } else {
            envelope.code().map_or(ExitClass::Unexpected, ExitClass::of)
        };
        assert_eq!(class, ExitClass::Conflict);
        write_document(
            &mut sink,
            &serde_json::to_string_pretty(&envelope).expect("a document"),
            class,
        );
        let written = String::from_utf8(sink).expect("UTF-8");
        assert!(
            written.contains("\"current_revision\": 9"),
            "the revision the caller is owed survives: {written}"
        );
    }
}
