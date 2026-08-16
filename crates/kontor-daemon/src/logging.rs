//! What a Kontor process is allowed to write to its log, enforced at the sink.
//!
//! Log redaction that lives at the call sites is a convention, and conventions
//! are audited once and then drift. This is a formatter: every event in the
//! process goes through it, so a field nobody thought about — an `Authorization`
//! header stashed in a request-log middleware, a connector payload attached to a
//! debug line, a credential path in a start-up message — is dropped because it
//! is not on the list, not because somebody remembered.
//!
//! Two rules, in this order:
//!
//! 1. **The field name must be on [`ALLOWED_FIELDS`].** The list is control-plane
//!    vocabulary: identities, counts, states, categories and the loopback
//!    address. It contains no field that would carry a secret, a body, a header,
//!    a transcript or a filesystem path to credential material.
//! 2. **The rendered value is scanned.** Even an allowed field is written as
//!    `<redacted>` when its text matches the domain's own credential canary. A
//!    value that looks like a token does not become safe by arriving under an
//!    approved name.
//!
//! The message itself is written as-is and scanned the same way; a `message` is
//! a static or formatted string this codebase wrote, and the scan is what covers
//! the case where the formatted half came from somewhere else.
//!
//! # What an error is allowed to say
//!
//! A `detail` field carries a typed error's `Display`. Every error type in this
//! workspace is written to render a category and a rule, never a stored row
//! value, a credential or a payload — `StoreError::Sqlite` carries SQLite's own
//! text and nothing from the row, `BackupError` carries a path or an opaque
//! category, and `ApiError` carries a code. That is the contract `detail`
//! depends on, and the canary scan is the net under it.

use std::fmt;

use kontor_core::id::{Timestamp, format_utc_timestamp, reject_sensitive_text};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields};
use tracing_subscriber::registry::LookupSpan;

/// Every field name a Kontor log line may carry.
///
/// Adding one is a deliberate decision about what leaves the process. The list
/// is sorted so a reviewer can scan it, and
/// [`ALLOWED_FIELDS_ARE_SORTED`] holds it to that.
pub const ALLOWED_FIELDS: &[&str] = &[
    "address",
    "agent_run_id",
    "ambiguous",
    "attempts",
    "barrier",
    "bind",
    "binding_id",
    "category",
    "consumer",
    "cursor",
    "destination_project",
    "detail",
    "disposition",
    "endpoint",
    "epoch_id",
    "families",
    "findings",
    "generation",
    "import_id",
    "kept",
    "kind",
    "lock",
    "materialized",
    "message",
    "needs_review",
    "outcome",
    "project_id",
    "pruned",
    "realm_id",
    "receipt_id",
    "reconciliation_required",
    "record_count",
    "records_hash",
    "removed",
    // The `&'static str` a refusal names itself by. It is a source constant in
    // every case — `ApiError::rule` is typed to make that impossible to violate
    // — so it carries the same class of text as `detail`. Without it a runtime
    // refusal logs only "the runtime will not work in the workspace this realm
    // asked for", and the five checks behind that sentence are indistinguishable
    // to anyone who is not reading the adapter source.
    "rule",
    "runtime",
    "schema_version",
    "settings",
    "settled",
    "snapshot",
    "source_realm_id",
    "state",
    "state_root",
    // The aggregate a refusal is about, as a `&'static str` naming a kind —
    // "task", "native container binding" — never an id and never a row value.
    // It travels with `rule`, which is useless without it.
    "subject",
    "task_id",
    "team_run_id",
    "undispatched",
];

/// The list is sorted, so `binary_search` is the membership test and a duplicate
/// or a misfiled entry shows up as a compile-time failure rather than as a field
/// that is silently never logged.
const ALLOWED_FIELDS_ARE_SORTED: () = {
    let mut index = 1;
    while index < ALLOWED_FIELDS.len() {
        let (previous, current) = (
            ALLOWED_FIELDS[index - 1].as_bytes(),
            ALLOWED_FIELDS[index].as_bytes(),
        );
        let mut position = 0;
        let shorter = if previous.len() < current.len() {
            previous.len()
        } else {
            current.len()
        };
        while position < shorter && previous[position] == current[position] {
            position += 1;
        }
        let ordered = if position == shorter {
            previous.len() < current.len()
        } else {
            previous[position] < current[position]
        };
        assert!(
            ordered,
            "ALLOWED_FIELDS must be sorted and free of duplicates"
        );
        index += 1;
    }
};

/// Whether a field may be written.
#[must_use]
pub fn field_is_allowed(name: &str) -> bool {
    let () = ALLOWED_FIELDS_ARE_SORTED;
    ALLOWED_FIELDS.binary_search(&name).is_ok()
}

/// What is written when a value matches the credential canary.
pub const REDACTED: &str = "<redacted>";

/// Render one value, or the redaction marker when it looks like a secret.
#[must_use]
pub fn scrub(value: &str) -> String {
    if reject_sensitive_text("log", value).is_err() {
        return REDACTED.to_owned();
    }
    value.to_owned()
}

/// The formatter, for both halves of a log line.
///
/// It is one type on purpose. A subscriber has two formatting seams — the event
/// formatter and the *field* formatter, and the second one is the one a span's
/// fields go through — so a redaction that only implements the first is not
/// sink-wide: `info_span!("sync", token = …)` would be formatted by whatever
/// field formatter the builder happened to have and then written out verbatim.
/// Implementing both means every field in the process, on a span or on an event,
/// goes through the same allowlist and the same value canary.
#[derive(Debug, Clone, Copy, Default)]
pub struct Allowlisted;

impl<'writer> FormatFields<'writer> for Allowlisted {
    /// Format a span's (or an event's) fields, dropping everything that is not
    /// allowed and redacting every value that looks like a credential.
    fn format_fields<R: tracing_subscriber::field::RecordFields>(
        &self,
        mut writer: Writer<'writer>,
        fields: R,
    ) -> fmt::Result {
        let mut visitor = AllowlistedFields {
            writer: &mut writer,
            result: Ok(()),
        };
        fields.record(&mut visitor);
        visitor.result
    }
}

impl<S, N> FormatEvent<S, N> for Allowlisted
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();
        write!(
            writer,
            "{} {:>5} {}",
            format_utc_timestamp(Timestamp::now()),
            metadata.level(),
            metadata.target()
        )?;

        // A span's stored fields are looked up as `FormattedFields<Allowlisted>`
        // — the *concrete* type — rather than as `FormattedFields<N>`. That is
        // the fail-closed half of the fix: the extension exists only when this
        // formatter is the one that recorded the span, so a subscriber wired up
        // with a different field formatter writes no span fields at all instead
        // of writing somebody else's unredacted rendering of them.
        if let Some(scope) = context.event_scope() {
            for span in scope.from_root() {
                write!(writer, " {}", span.name())?;
                let extensions = span.extensions();
                if let Some(fields) = extensions.get::<FormattedFields<Self>>()
                    && !fields.is_empty()
                {
                    write!(writer, "{{{fields}}}")?;
                }
            }
        }

        let mut visitor = AllowlistedFields {
            writer: &mut writer,
            result: Ok(()),
        };
        event.record(&mut visitor);
        visitor.result?;
        writeln!(writer)
    }
}

/// Writes the fields that survive both rules, and nothing else.
struct AllowlistedFields<'a, 'writer> {
    writer: &'a mut Writer<'writer>,
    result: fmt::Result,
}

impl AllowlistedFields<'_, '_> {
    fn write(&mut self, field: &Field, value: &str) {
        if self.result.is_err() || !field_is_allowed(field.name()) {
            return;
        }
        let value = scrub(value);
        self.result = if field.name() == "message" {
            write!(self.writer, " {value}")
        } else {
            write!(self.writer, " {}={}", field.name(), value)
        };
    }
}

impl Visit for AllowlistedFields<'_, '_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.write(field, value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.write(field, &format!("{value:?}"));
    }
}

/// Build the subscriber this process logs through, writing to `writer`.
///
/// Both seams are wired to [`Allowlisted`]: `fmt_fields` is what a span's fields
/// go through when the span is created, and `event_format` is what an event goes
/// through when it is written. Wiring only the second leaves span fields
/// unredacted, which is exactly the hole this pairing closes.
///
/// It is one function rather than two so a test can assert against the *real*
/// wiring. A test that assembled its own subscriber would still pass on the day
/// somebody dropped `fmt_fields` from the installed one.
pub fn subscriber<W>(writer: W) -> impl tracing::Subscriber + Send + Sync + 'static
where
    W: for<'writer> tracing_subscriber::fmt::MakeWriter<'writer> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .fmt_fields(Allowlisted)
        .event_format(Allowlisted)
        .with_writer(writer)
        .finish()
}

/// Install the process-wide subscriber on standard output.
///
/// `RUST_LOG` still selects *which* events are emitted; it cannot widen what a
/// line may contain.
pub fn install() {
    use tracing_subscriber::util::SubscriberInitExt;
    let _ = subscriber(std::io::stdout).try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allowlist_admits_control_vocabulary_and_nothing_else() {
        for allowed in ["realm_id", "detail", "barrier", "message"] {
            assert!(field_is_allowed(allowed), "{allowed} must be loggable");
        }
        for refused in [
            "authorization",
            "token",
            "credential_path",
            "body",
            "payload",
            "transcript",
            "prompt",
            "api_key",
        ] {
            assert!(!field_is_allowed(refused), "{refused} must never be logged");
        }
    }

    #[test]
    fn a_value_that_looks_like_a_credential_is_replaced_even_under_an_allowed_name() {
        assert_eq!(scrub("realm claimed"), "realm claimed");
        assert_eq!(
            scrub("Bearer 0123456789abcdef0123456789abcdef"),
            REDACTED,
            "a bearer token must not survive because it arrived as `detail`"
        );
        assert_eq!(scrub("password=hunter2"), REDACTED);
    }
}
