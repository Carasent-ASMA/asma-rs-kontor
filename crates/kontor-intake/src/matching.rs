//! Which trigger revisions a canonical envelope matches, and in what order.
//!
//! Matching is four comparisons and a pointer walk, in this order:
//!
//! 1. the event's **source kind** and **connection** equal the trigger's;
//! 2. the envelope's declared **event schema** and **pinned schema revision**
//!    equal the trigger's — a trigger written against revision 2 of a schema does
//!    not silently fire on revision 3;
//! 3. every **filter clause** resolves and equals its literal;
//! 4. the trigger's **dedup expression** resolves, which is what makes the
//!    decision replayable.
//!
//! Nothing here interprets a source kind. Steps 1 and 2 are equality between two
//! opaque keys, and step 3 is equality between a resolved JSON pointer and a
//! declared literal. There is no branch anywhere on *which* key it was.

use kontor_core::id::{CanonicalDocument, SchemaVersion, SpecVersion};
use kontor_core::spec::{CanonicalSourceEvent, TriggerSpec};
use kontor_core::{DomainError, DomainResult};

/// The generation of the envelope this crate writes and reads.
pub const ENVELOPE_SCHEMA_VERSION: SchemaVersion = kontor_core::id::SCHEMA_VERSION;

/// The event schema an envelope declares, as a pair.
fn declared_schema(envelope: &CanonicalDocument) -> DomainResult<(String, u32)> {
    let value: serde_json::Value = serde_json::from_str(envelope.json())
        .map_err(|_| DomainError::invalid("CanonicalSourceEvent", "envelope is not valid JSON"))?;
    let schema = value
        .pointer("/event_schema")
        .and_then(serde_json::Value::as_str)
        .ok_or(DomainError::Invalid {
            subject: "CanonicalSourceEvent",
            rule: "the envelope declares no event schema",
        })?;
    let version = value
        .pointer("/event_schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|raw| u32::try_from(raw).ok())
        .ok_or(DomainError::Invalid {
            subject: "CanonicalSourceEvent",
            rule: "the envelope declares no event schema revision",
        })?;
    Ok((schema.to_owned(), version))
}

/// Every trigger revision that matches this event, in a deterministic order.
///
/// The order is trigger key ascending, then revision *descending*, so the
/// newest revision of a trigger is always considered before an older one and two
/// callers on two machines agree about which match is first.
///
/// The candidate list is the caller's: this function does not read a database,
/// and a trigger that is not in the list is not a trigger this decision knew
/// about — which is precisely what the receipt's pinned revision records.
///
/// # Errors
/// Returns [`DomainError`] when the envelope is not readable as the document it
/// claims to be. A trigger whose dedup expression does not resolve is *not* an
/// error: it is a trigger that does not match this event, because a decision
/// that cannot be deduplicated cannot be replayed.
pub fn match_triggers<'a>(
    event: &CanonicalSourceEvent,
    triggers: &'a [TriggerSpec],
) -> DomainResult<Vec<&'a TriggerSpec>> {
    let (schema, schema_version) = declared_schema(&event.envelope)?;
    let mut matched: Vec<&TriggerSpec> = Vec::new();
    for trigger in triggers {
        if trigger.source_kind != event.identity.source_kind
            || trigger.source_connection != event.identity.source_connection
        {
            continue;
        }
        if trigger.event_schema.as_str() != schema
            || trigger.event_schema_version != SpecVersion::parse(schema_version)?
        {
            continue;
        }
        if !trigger.matches(&event.envelope)? {
            continue;
        }
        if trigger.dedup.evaluate(&event.envelope).is_err() {
            continue;
        }
        matched.push(trigger);
    }
    matched.sort_by(|left, right| {
        left.id
            .as_str()
            .cmp(right.id.as_str())
            .then(right.version.get().cmp(&left.version.get()))
    });
    Ok(matched)
}
