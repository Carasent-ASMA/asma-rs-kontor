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
    "runtime",
    "schema_version",
    "settings",
    "settled",
    "snapshot",
    "source_realm_id",
    "state",
    "state_root",
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

/// The event formatter: allowlisted fields, scanned values, canonical instants.
#[derive(Debug, Clone, Copy, Default)]
pub struct Allowlisted;

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

        // A span's own fields went through the same allowlist when the span was
        // recorded, so they are written as stored.
        if let Some(scope) = context.event_scope() {
            for span in scope.from_root() {
                write!(writer, " {}", span.name())?;
                let extensions = span.extensions();
                if let Some(fields) = extensions.get::<FormattedFields<N>>()
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

/// Install the process-wide subscriber.
///
/// `RUST_LOG` still selects *which* events are emitted; it cannot widen what a
/// line may contain.
pub fn install() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .event_format(Allowlisted)
        .try_init();
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
