//! Authenticated adapters and the one canonical envelope they all produce.
//!
//! An adapter is where a source system's own shape is read, and it is the only
//! place: what leaves here is [`CanonicalSourceEvent`], and nothing downstream
//! can tell a pull request from a monitoring alert except by reading the data
//! it declared. That is the point. A matcher that could recognize a source kind
//! would eventually branch on one, and a deployment's own kinds would then work
//! only as well as somebody remembered to add them.
//!
//! ## The envelope
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "attributes": { "…": "…" },
//!   "event_schema": "…",
//!   "event_schema_version": 1,
//!   "external_event_id": "…",
//!   "observed_at": "2026-01-01T00:00:00Z",
//!   "source_connection": "…",
//!   "source_kind": "…",
//!   "subject": "…"
//! }
//! ```
//!
//! Keys are sorted and the bytes are compact, because
//! [`kontor_core::id::CanonicalDocument`] canonicalizes them: two adapters that
//! build the same facts in a different order produce the same digest, and that
//! digest is what deduplication is keyed on.
//!
//! ## Redaction is not advisory here
//!
//! `CanonicalDocument::from_value` runs the shared sensitive-material scanner
//! over every key and every string. An adapter that forwards a bearer token, a
//! cookie or a national identity number in an attribute is refused at
//! canonicalization, before a digest exists and long before SQL — so an
//! unredacted envelope is not something this crate can produce, rather than
//! something it tries not to.

use std::collections::BTreeMap;

use kontor_core::id::{
    AccountProfileId, CanonicalDocument, EventSchemaKey, ExternalId, ExternalName,
    SourceConnectionKey, SourceEventId, SourceKindKey, SpecVersion, Timestamp,
    format_utc_timestamp,
};
use kontor_core::spec::{CanonicalSourceEvent, SourceIdentity, SourceProcessingState};
use kontor_core::{DomainError, DomainResult};

use crate::matching::ENVELOPE_SCHEMA_VERSION;

/// The maximum number of attributes one envelope may declare.
///
/// A bound rather than a taste: the envelope is stored, hashed and replayed, and
/// an unbounded attribute map would make a source system the author of this
/// database's row sizes.
pub const MAX_ATTRIBUTES: usize = 64;

/// One authenticated inbound event, as an adapter hands it over.
///
/// The adapter has already proved the connection is one this Realm configured
/// and that the caller may write to it — [`InboundEvent::authenticated_as`] is
/// that proof carried forward, not a request for one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundEvent {
    /// Which kind of source this connection is. Data, never a branch condition.
    pub source_kind: SourceKindKey,
    /// Which configured connection of that kind.
    pub source_connection: SourceConnectionKey,
    /// The account the adapter authenticated the delivery as.
    pub authenticated_as: AccountProfileId,
    /// The event id as the source system spells it.
    pub external_event_id: ExternalId,
    /// The event schema the adapter normalized to.
    pub event_schema: EventSchemaKey,
    /// That schema's revision.
    pub event_schema_version: SpecVersion,
    /// When the source system observed the event.
    pub observed_at: Timestamp,
    /// A short human subject, for operators reading a queue of proposals.
    pub subject: ExternalName,
    /// The normalized, non-secret facts triggers may filter and deduplicate on.
    pub attributes: BTreeMap<String, serde_json::Value>,
}

/// Normalize one authenticated inbound event into a canonical source event.
///
/// # Errors
/// Returns [`DomainError`] when the attribute map exceeds [`MAX_ATTRIBUTES`],
/// when an attribute name is not a legal envelope key, and — through
/// [`CanonicalDocument::from_value`] — when the envelope is too large, too deep,
/// carries a non-finite number or carries sensitive material.
pub fn canonicalize(
    event: &InboundEvent,
    id: SourceEventId,
    ingested_at: Timestamp,
) -> DomainResult<CanonicalSourceEvent> {
    if event.attributes.len() > MAX_ATTRIBUTES {
        return Err(DomainError::invalid(
            "InboundEvent",
            "declares more attributes than one envelope may carry",
        ));
    }
    // Attribute names are part of a pointer a trigger pins, so they obey one
    // lexical rule rather than whatever the source system happened to send.
    // Without this, a filter could be written against a key that only a
    // particular JSON encoder produces.
    for name in event.attributes.keys() {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(DomainError::invalid(
                "InboundEvent",
                "an attribute name is not lowercase ascii, digits and underscores",
            ));
        }
    }

    let attributes: serde_json::Map<String, serde_json::Value> =
        event.attributes.clone().into_iter().collect();
    let envelope = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": ENVELOPE_SCHEMA_VERSION.get(),
        "source_kind": event.source_kind.as_str(),
        "source_connection": event.source_connection.as_str(),
        "event_schema": event.event_schema.as_str(),
        "event_schema_version": event.event_schema_version.get(),
        "external_event_id": event.external_event_id.as_str(),
        "observed_at": format_utc_timestamp(event.observed_at),
        "subject": event.subject.as_str(),
        "attributes": serde_json::Value::Object(attributes),
    }))?;

    Ok(CanonicalSourceEvent {
        id,
        identity: SourceIdentity {
            source_kind: event.source_kind.clone(),
            source_connection: event.source_connection.clone(),
            external_event_id: event.external_event_id.clone(),
        },
        envelope,
        external_observed_at: event.observed_at,
        ingested_at,
        // Nothing has evaluated it yet, and that is the whole point of writing
        // it down first: the state a stored event starts in is `received`, and
        // the existence of a receipt — not this column — is what later says a
        // decision was reached.
        processing_state: SourceProcessingState::Received,
    })
}
