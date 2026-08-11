//! `kontor-store` — SQLite repositories, migrations and event append/replay for the Kontor control plane
//!
//! One concrete store, one ordered SQL migration, no ORM and no migration
//! framework. The [`rusqlite::Connection`] is private and there is no raw-SQL
//! escape hatch: everything a later service needs arrives as a named method on
//! one of the [`kontor_core::repository`] ports, so the invariants enforced in
//! Rust and the invariants enforced by SQL cannot drift apart.
//!
//! Every aggregate mutation opens exactly one transaction, validates the current
//! revision, specification and authority, writes every row, event and receipt it
//! needs, and then commits. A failure leaves the aggregate revision, the event
//! log and the outbox exactly as they were.
//!
//! Three modules carry the runtime-consistency protocol, and each owns one
//! question a control plane gets wrong under crashes:
//!
//! * [`commands`] — *did this command already take effect?* Answered from the
//!   durable receipt and the correlation persisted before the native call, never
//!   from a restart or an expired lease.
//! * [`events`] — *what did the runtime actually tell us, and in what order?*
//!   Raw and normalized evidence lands before any consequence, duplicates map
//!   back to their original cursor, and a missing control fact is recorded as a
//!   gap rather than smoothed over.
//! * [`reconciliation`] — *what does the runtime say exists right now?* Absence
//!   from a completed census costs a run its freshness and nothing else.
//!
//! The rule they share is the one uncertainty always breaks: an absence, a
//! timeout, a closed stream or a missing session is never a completion.

mod commands;
mod events;
mod migrations;
mod policy;
mod reconciliation;
mod repository;

use std::path::Path;

use kontor_core::DomainError;
use kontor_core::id::RealmId;
use kontor_core::realm::RealmMetadata;
use kontor_core::repository::RepositoryError;
use rusqlite::Connection;

pub use commands::intent::DispatchClaim;
pub use commands::receipts::{
    CommandRecovery, CommandTransition, ReceiptTransition, RecordedTransition,
};
pub use events::types::{
    ConsumerPage, ContentDiscontinuity, ContentGapOutcome, ControlGap, ControlObservation,
    ControlObservationOutcome,
};
pub use migrations::SCHEMA_VERSION;
pub use policy::{
    EvaluationBinding, GateRejection, NewArtifactEvidence, NewGateWaiver, ParkPlan, ParkedRecovery,
    RejectionOutcome, StoredRecoveryStep,
};
pub use reconciliation::{
    CensusItem, CensusOutcome, EpochKey, EpochStatus, EpochSummary, ReconciliationEpoch,
    ReconciliationEpochId,
};

/// Everything the store can refuse.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The database was written by a newer Kontor and is not downgraded.
    #[error("database schema version {found} is newer than this binary understands ({expected})")]
    DatabaseTooNew {
        /// The version found in the file.
        found: i64,
        /// The version this binary implements.
        expected: i64,
    },
    /// A connection-level pragma could not be applied or did not take effect.
    #[error("connection pragma `{pragma}` did not take effect")]
    Pragma {
        /// Which pragma.
        pragma: &'static str,
    },
    /// The domain refused the value.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// A repository rule refused the operation.
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    /// SQLite failed. The message carries SQLite's own text, never a row value.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The database's Realm identity is missing, duplicated or malformed.
    ///
    /// This is always fatal and never repaired: a Realm is created once, in the
    /// same transaction as the schema, and any other state means the file is not
    /// a Kontor database this binary may open.
    #[error("realm metadata is invalid: {reason}")]
    InvalidRealmMetadata {
        /// Why the metadata was refused. Never contains stored values.
        reason: &'static str,
    },
    /// An integrity or foreign-key check found a problem.
    #[error("database integrity check failed: {detail}")]
    Integrity {
        /// What the check reported.
        detail: String,
    },
}

impl From<StoreError> for RepositoryError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Domain(domain) => Self::Domain(domain),
            StoreError::Repository(repository) => repository,
            other => Self::Backend {
                detail: other.to_string(),
            },
        }
    }
}

/// The Kontor control-plane database: one file, one Realm.
///
/// The connection is file-backed and private. WAL, foreign keys and a bounded
/// busy timeout are applied *and verified* on every connection, because only WAL
/// persists in the file — the other two are per-connection and would otherwise
/// silently revert on reopen.
#[derive(Debug)]
pub struct SqliteStore {
    connection: Connection,
    /// Loaded once at open and never mutated, so every ingress check compares
    /// against the same value for the lifetime of the store.
    realm: RealmMetadata,
}

impl SqliteStore {
    /// Open (creating if needed) and migrate a database file.
    ///
    /// A `user_version` of 0 applies migration 0001; 1 is an idempotent open;
    /// anything greater fails with [`StoreError::DatabaseTooNew`] and no
    /// destructive downgrade.
    ///
    /// # Errors
    /// Returns [`StoreError`] when the file cannot be opened, a pragma does not
    /// take effect, the schema is too new, or the migration fails — in which
    /// case nothing is left behind and `user_version` stays 0.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path)?;
        migrations::configure_connection(&connection)?;
        let realm = migrations::migrate(&mut connection)?;
        Ok(Self { connection, realm })
    }

    /// This database's immutable Realm identity.
    ///
    /// The same value for the lifetime of the store. There is no setter, no
    /// label update and no regeneration path.
    #[must_use]
    pub const fn realm_metadata(&self) -> &RealmMetadata {
        &self.realm
    }

    /// This database's Realm id.
    #[must_use]
    pub const fn realm_id(&self) -> RealmId {
        self.realm.realm_id
    }

    /// The schema version recorded in the file.
    ///
    /// # Errors
    /// Returns [`StoreError::Sqlite`] on backend failure.
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        migrations::read_user_version(&self.connection)
    }

    /// Run SQLite's full `integrity_check`.
    ///
    /// # Errors
    /// Returns [`StoreError::Integrity`] when the database is damaged.
    pub fn integrity_check(&self) -> Result<(), StoreError> {
        let report: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if report == "ok" {
            Ok(())
        } else {
            Err(StoreError::Integrity { detail: report })
        }
    }

    /// Run SQLite's `quick_check`.
    ///
    /// # Errors
    /// Returns [`StoreError::Integrity`] when the database is damaged.
    pub fn quick_check(&self) -> Result<(), StoreError> {
        let report: String = self
            .connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if report == "ok" {
            Ok(())
        } else {
            Err(StoreError::Integrity { detail: report })
        }
    }

    /// Run SQLite's `foreign_key_check`.
    ///
    /// # Errors
    /// Returns [`StoreError::Integrity`] when a dangling reference exists.
    pub fn foreign_key_check(&self) -> Result<(), StoreError> {
        let mut statement = self.connection.prepare("PRAGMA foreign_key_check")?;
        let mut rows = statement.query([])?;
        if let Some(row) = rows.next()? {
            let table: String = row.get(0)?;
            return Err(StoreError::Integrity {
                detail: format!("dangling foreign key in `{table}`"),
            });
        }
        Ok(())
    }

    /// Whether foreign keys are enforced on this connection.
    ///
    /// # Errors
    /// Returns [`StoreError::Sqlite`] on backend failure.
    pub fn foreign_keys_enabled(&self) -> Result<bool, StoreError> {
        Ok(self
            .connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))?
            == 1)
    }

    /// The journal mode in force.
    ///
    /// # Errors
    /// Returns [`StoreError::Sqlite`] on backend failure.
    pub fn journal_mode(&self) -> Result<String, StoreError> {
        Ok(self
            .connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?)
    }

    /// The busy timeout in force, in milliseconds.
    ///
    /// # Errors
    /// Returns [`StoreError::Sqlite`] on backend failure.
    pub fn busy_timeout_ms(&self) -> Result<i64, StoreError> {
        Ok(self
            .connection
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))?)
    }
}
