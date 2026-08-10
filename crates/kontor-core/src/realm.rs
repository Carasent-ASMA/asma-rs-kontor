//! The Realm seam: one database file, one identity, one isolation boundary.
//!
//! A Realm is a state root. Every Kontor database carries exactly one immutable
//! [`RealmMetadata`] row, created in the same atomic initialization as schema v1
//! and never updated, replaced or regenerated.
//!
//! The seam exists because a UUID does not encode where it came from. Inside one
//! database, project scoping is relational; the moment a value crosses a
//! process, wire or cache boundary its identity must be read as
//! `(realm_id, entity_id)`. The envelopes below are that pair, and every one of
//! them is checked *before* a transaction opens:
//!
//! * [`SnapshotEnvelope`] — a point-in-time value plus the cursor it was taken at
//! * [`EventEnvelope`] — one event at one cursor
//! * [`ReceiptEnvelope`] — a command or intake receipt
//! * [`ExportEnvelope`] — a whole redacted export
//!
//! An entity id from another Realm carried under *this* Realm's id still cannot
//! resolve: the row is simply absent from this database. There is deliberately
//! no fallback lookup, no `ATTACH`, and no id remapping.
//!
//! This module is a seam and nothing more. Remote bind, account registries,
//! account tabs, pairing, TLS transport, federation, cross-realm summaries and
//! multi-host coordination are architecture §19 follow-ons and are absent here
//! by design.

use serde::{Deserialize, Serialize};

use crate::id::{EventCursor, ExternalName, RealmId, SchemaVersion, Timestamp};
use crate::{DomainError, DomainResult};

/// The immutable identity of one database file.
///
/// Every field is fixed at creation. There is no label-update method in MVP, and
/// there is no operation anywhere in this crate that produces a *changed*
/// `RealmMetadata` for an existing database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealmMetadata {
    /// This Realm's identity.
    pub realm_id: RealmId,
    /// The envelope/creation contract this Realm was created under. Always 1 in
    /// schema v1, and never rewritten by a later numbered migration.
    pub schema_version: SchemaVersion,
    /// When the Realm was created, in canonical UTC.
    pub created_at: Timestamp,
    /// Optional, bounded, non-secret display text. A freshly initialized
    /// database has none.
    pub display_label: Option<ExternalName>,
}

impl RealmMetadata {
    /// Create the metadata for a brand-new database.
    #[must_use]
    pub fn create(realm_id: RealmId, created_at: Timestamp) -> Self {
        Self {
            realm_id,
            schema_version: crate::id::SCHEMA_VERSION,
            created_at,
            display_label: None,
        }
    }

    /// Validate metadata loaded from storage.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] when the recorded schema version is not
    /// the one this binary creates. The id, timestamp and label have already
    /// been parsed into types that carry their own invariants, so a malformed
    /// value cannot reach this point.
    pub fn validate(&self) -> DomainResult<()> {
        if self.schema_version != crate::id::SCHEMA_VERSION {
            return Err(DomainError::invalid(
                "RealmMetadata",
                "was created under a different envelope contract",
            ));
        }
        Ok(())
    }

    /// Check an incoming Realm id against this Realm.
    ///
    /// # Errors
    /// Returns [`DomainError::RealmMismatch`] naming both ids and nothing else.
    pub fn ensure_matches(&self, found: RealmId) -> DomainResult<()> {
        ensure_realm(self.realm_id, found)
    }
}

/// Compare an expected and a found Realm id.
///
/// # Errors
/// Returns [`DomainError::RealmMismatch`] when they differ. The error carries
/// the two ids — which are not secret — and never the envelope payload.
pub fn ensure_realm(expected: RealmId, found: RealmId) -> DomainResult<()> {
    if expected == found {
        Ok(())
    } else {
        Err(DomainError::RealmMismatch { expected, found })
    }
}

/// A cursor together with the Realm it counts in.
///
/// A bare [`EventCursor`] is only meaningful inside an already-bound store.
/// Anything that resumes, subscribes or snapshots uses this pair, so replaying
/// Realm A's cursor into Realm B is a type-level mistake rather than a silent
/// off-by-one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RealmCursor {
    /// The Realm the cursor belongs to.
    pub realm_id: RealmId,
    /// The position inside that Realm.
    pub cursor: EventCursor,
}

impl RealmCursor {
    /// Qualify a bare cursor.
    #[must_use]
    pub const fn new(realm_id: RealmId, cursor: EventCursor) -> Self {
        Self { realm_id, cursor }
    }

    /// Unwrap the position, proving the Realm first.
    ///
    /// # Errors
    /// Returns [`DomainError::RealmMismatch`] when the cursor belongs elsewhere.
    pub fn resolve(&self, expected: RealmId) -> DomainResult<EventCursor> {
        ensure_realm(expected, self.realm_id)?;
        Ok(self.cursor)
    }
}

macro_rules! realm_envelope {
    (
        $(#[$meta:meta])*
        $name:ident { $( $(#[$field_meta:meta])* $field:ident : $ty:ty ),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name<T> {
            /// The Realm this value came from.
            pub realm_id: RealmId,
            $( $(#[$field_meta])* pub $field: $ty, )*
            /// The carried value.
            pub value: T,
        }

        impl<T> $name<T> {
            /// Borrow the value, proving the Realm first.
            ///
            /// # Errors
            /// Returns [`DomainError::RealmMismatch`] when the envelope belongs
            /// to another Realm. The payload is never included in the error.
            pub fn peek(&self, expected: RealmId) -> DomainResult<&T> {
                ensure_realm(expected, self.realm_id)?;
                Ok(&self.value)
            }

            /// Take the value, proving the Realm first.
            ///
            /// # Errors
            /// As [`Self::peek`].
            pub fn open(self, expected: RealmId) -> DomainResult<T> {
                ensure_realm(expected, self.realm_id)?;
                Ok(self.value)
            }
        }
    };
}

realm_envelope! {
    /// A point-in-time value and the cursor it is consistent with.
    ///
    /// A subscriber resumes strictly *after* `snapshot_cursor`, in the same
    /// Realm.
    SnapshotEnvelope {
        /// The position the snapshot is consistent with.
        snapshot_cursor: EventCursor,
    }
}

realm_envelope! {
    /// One event at one position in one Realm.
    EventEnvelope {
        /// The position of this event.
        cursor: EventCursor,
    }
}

realm_envelope! {
    /// A receipt produced in one Realm.
    ///
    /// Receipts have no cursor of their own: they are addressed by idempotency
    /// key inside their Realm.
    ReceiptEnvelope {}
}

realm_envelope! {
    /// A whole export taken from one Realm.
    ///
    /// Importing one elsewhere is explicitly *not* an identity update: the only
    /// supported future path initializes a different Realm and produces new
    /// destination receipts.
    ExportEnvelope {
        /// The envelope contract the export was written under.
        schema_version: SchemaVersion,
        /// When the export was taken.
        exported_at: Timestamp,
    }
}

impl<T> SnapshotEnvelope<T> {
    /// Wrap a value with the cursor it is consistent with.
    pub const fn new(realm_id: RealmId, snapshot_cursor: EventCursor, value: T) -> Self {
        Self {
            realm_id,
            snapshot_cursor,
            value,
        }
    }

    /// The snapshot position as a Realm-qualified cursor.
    #[must_use]
    pub const fn cursor(&self) -> RealmCursor {
        RealmCursor::new(self.realm_id, self.snapshot_cursor)
    }
}

impl<T> EventEnvelope<T> {
    /// Wrap an event at a position.
    pub const fn new(realm_id: RealmId, cursor: EventCursor, value: T) -> Self {
        Self {
            realm_id,
            cursor,
            value,
        }
    }

    /// The event position as a Realm-qualified cursor.
    #[must_use]
    pub const fn realm_cursor(&self) -> RealmCursor {
        RealmCursor::new(self.realm_id, self.cursor)
    }
}

impl<T> ReceiptEnvelope<T> {
    /// Wrap a receipt.
    pub const fn new(realm_id: RealmId, value: T) -> Self {
        Self { realm_id, value }
    }
}

impl<T> ExportEnvelope<T> {
    /// Wrap an export.
    pub const fn new(realm_id: RealmId, exported_at: Timestamp, value: T) -> Self {
        Self {
            realm_id,
            schema_version: crate::id::SCHEMA_VERSION,
            exported_at,
            value,
        }
    }
}
