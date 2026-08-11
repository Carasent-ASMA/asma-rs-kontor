//! Connection pragmas, the ordered schema migration and Realm initialization.
//!
//! There is deliberately no migration framework here. An ordered array of
//! `include_str!`, one `user_version` dispatch and one `BEGIN IMMEDIATE`
//! transaction is the whole mechanism, and it is enough: either every object of
//! every migration up to [`SCHEMA_VERSION`] exists, `user_version` says so
//! **and** exactly one Realm row exists, or the database is still exactly as it
//! was before the open.
//!
//! Every pending migration runs inside the *same* transaction, so a two-step
//! upgrade cannot stop half way: a failure in `0002` rolls `0001` back with it
//! and leaves `user_version` at 0. Each script ends with its own
//! `PRAGMA user_version = N`, which is transactional, so the recorded version
//! and the objects it describes commit or roll back together.
//!
//! Realm creation is part of that same transaction on purpose. A database with a
//! schema but no identity, or an identity but no schema, is not a state this
//! code can produce — and it is never repaired after the fact.
//!
//! The Realm row's own `schema_version` column is *not* this version. It records
//! the envelope contract a Realm was created under
//! ([`kontor_core::id::SCHEMA_VERSION`]), which a later numbered migration never
//! rewrites; this constant is how far the persisted tables have been brought.

use std::time::{Duration, Instant};

use kontor_core::id::{ExternalName, RealmId, SchemaVersion, Timestamp, parse_utc_timestamp};
use kontor_core::realm::RealmMetadata;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::StoreError;

/// The schema generation this binary implements.
pub const SCHEMA_VERSION: i64 = 3;

/// The bounded busy timeout applied to every connection.
const BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);

/// How long to wait between attempts at the one statement the busy handler does
/// not cover. Short enough to be invisible, long enough not to spin a core.
const BUSY_RETRY_INTERVAL: Duration = Duration::from_millis(10);

/// Whether a `rusqlite` failure is SQLite refusing because someone else holds
/// the database.
fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::DatabaseBusy
                || failure.code == rusqlite::ErrorCode::DatabaseLocked
    )
}

/// The migrations, in order. `MIGRATIONS[n]` brings a database from
/// `user_version` `n` to `n + 1`, and each one ends with the
/// `PRAGMA user_version` that records it.
///
/// The list is the whole dispatch table: the index *is* the version it upgrades
/// from, so a migration can only ever be appended, and `SCHEMA_VERSION` is
/// asserted against its length below rather than maintained by hand.
const MIGRATIONS: &[&str] = &[
    // Schema v1. Byte-frozen from the first accepted KON-MVP-03 commit onward.
    include_str!("../migrations/0001_init.sql"),
    // Schema v2. The non-secret account profile: harness, opaque credential
    // reference, environment/routing/capability documents, enabled, revision.
    include_str!("../migrations/0002_account_profiles_expanded.sql"),
    // Schema v3. Guardrail evaluations, artifact/waiver/approval evidence,
    // bounded recovery episodes and steps, and the reviewer principal a
    // rejection counter is derived from.
    include_str!("../migrations/0003_guardrails_and_recovery.sql"),
];

const _: () = assert!(
    MIGRATIONS.len() == SCHEMA_VERSION as usize,
    "every schema version needs exactly one migration script"
);

/// Apply and verify the connection-level pragmas.
///
/// WAL persists in the file, but `foreign_keys` and `busy_timeout` do not: they
/// are per-connection and must be set — and checked — every time a connection is
/// opened, or a reopened database would silently stop enforcing references.
pub(crate) fn configure_connection(connection: &Connection) -> Result<(), StoreError> {
    // The busy handler is armed *first*, before any statement that can contend.
    // Switching a fresh database into WAL takes an exclusive lock, so two
    // processes opening the same new file at once will collide right there — and
    // with no handler installed yet that surfaces as an immediate `SQLITE_BUSY`
    // rather than the bounded wait every other write gets.
    connection.busy_timeout(BUSY_TIMEOUT)?;
    let busy_timeout: i64 = connection.query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
    if busy_timeout != i64::try_from(BUSY_TIMEOUT.as_millis()).unwrap_or(i64::MAX) {
        return Err(StoreError::Pragma {
            pragma: "busy_timeout",
        });
    }

    // Switching journal mode is one of the few statements the busy handler does
    // *not* cover: SQLite returns `SQLITE_BUSY` immediately while any other
    // connection holds the database open, without consulting the timeout. So it
    // gets the same bounded budget explicitly. Once the mode is WAL it persists
    // in the file, so this only ever spins on a genuinely concurrent first open.
    let deadline = Instant::now() + BUSY_TIMEOUT;
    let journal_mode: String = loop {
        match connection.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        }) {
            Ok(mode) => break mode,
            Err(error) if is_busy(&error) && Instant::now() < deadline => {
                std::thread::sleep(BUSY_RETRY_INTERVAL);
            }
            Err(error) => return Err(error.into()),
        }
    };
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Pragma {
            pragma: "journal_mode",
        });
    }

    connection.pragma_update(None, "foreign_keys", true)?;
    let foreign_keys: i64 = connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    if foreign_keys != 1 {
        return Err(StoreError::Pragma {
            pragma: "foreign_keys",
        });
    }
    Ok(())
}

/// Read `PRAGMA user_version`.
pub(crate) fn read_user_version(connection: &Connection) -> Result<i64, StoreError> {
    Ok(connection.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

/// Bring the database to [`SCHEMA_VERSION`] and return its Realm identity.
///
/// * `0` — create every schema generation and exactly one Realm row inside one
///   `BEGIN IMMEDIATE` transaction.
/// * `1..SCHEMA_VERSION` — apply the remaining migrations in the same single
///   transaction. The Realm already exists and is never touched.
/// * `SCHEMA_VERSION` — load and validate the existing Realm; opening is
///   idempotent.
/// * `> SCHEMA_VERSION` — refuse. A newer schema is never downgraded, truncated
///   or guessed at.
pub(crate) fn migrate(connection: &mut Connection) -> Result<RealmMetadata, StoreError> {
    let version = read_user_version(connection)?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::DatabaseTooNew {
            found: version,
            expected: SCHEMA_VERSION,
        });
    }
    if version == SCHEMA_VERSION {
        return load_realm(connection);
    }

    // The Realm identity is generated here, in Rust, so it is a real UUIDv7 with
    // a real creation instant rather than something SQLite invented. It is only
    // used when this open is the one that creates the database.
    let realm = RealmMetadata::create(RealmId::generate(), Timestamp::now());

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    // Re-read the version now that the write lock is actually held. The first
    // read above was unlocked: with two processes opening the same new file at
    // once, the loser can wait out the whole busy timeout here and only then
    // acquire the lock — by which point the winner has already committed the
    // schema and a Realm. Running an applied migration again would fail on the
    // first duplicate object, so without this check a concurrent first open is a
    // hard error instead of the idempotent open it should be. The same read also
    // covers a concurrent *upgrade*: the loser sees the newer version and skips
    // the scripts the winner already ran.
    let version = read_user_version(&transaction)?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::DatabaseTooNew {
            found: version,
            expected: SCHEMA_VERSION,
        });
    }
    if version == SCHEMA_VERSION {
        // Someone else brought it up to date while we waited. Their Realm is the
        // Realm; the one generated above is discarded unused.
        drop(transaction);
        return load_realm(connection);
    }

    // Every pending script runs here, in order, in this one transaction. Each
    // ends with its own `PRAGMA user_version`, which is transactional: any
    // failure rolls the version back to where it started along with every object
    // any of the scripts created.
    let pending = usize::try_from(version).map_err(|_| StoreError::Pragma {
        pragma: "user_version",
    })?;
    for migration in &MIGRATIONS[pending..] {
        transaction.execute_batch(migration)?;
    }

    // The Realm is created exactly once, by the open that created the schema. An
    // upgrade from an existing generation finds it already there and must not
    // mint a second identity for a database that already has one.
    if version == 0 {
        transaction.execute(
            "INSERT INTO realm_metadata (singleton, realm_id, schema_version, created_at, display_label)
             VALUES (1, ?1, ?2, ?3, NULL)",
            params![
                realm.realm_id.to_string(),
                i64::from(realm.schema_version.get()),
                realm.created_at.to_string()
            ],
        )?;
    }
    let rows: i64 =
        transaction.query_row("SELECT count(*) FROM realm_metadata", [], |row| row.get(0))?;
    if rows != 1 {
        // Rolls back the Realm row, every schema object and `user_version`
        // together.
        return Err(StoreError::InvalidRealmMetadata {
            reason: "initialization did not produce exactly one realm row",
        });
    }
    transaction.commit()?;

    verify_applied(connection)?;
    load_realm(connection)
}

/// Load and validate the single Realm row. Never repairs, inserts or replaces.
fn load_realm(connection: &Connection) -> Result<RealmMetadata, StoreError> {
    verify_applied(connection)?;

    let rows: i64 =
        connection.query_row("SELECT count(*) FROM realm_metadata", [], |row| row.get(0))?;
    if rows != 1 {
        return Err(StoreError::InvalidRealmMetadata {
            reason: "expected exactly one realm row",
        });
    }

    let found: Option<(String, i64, String, Option<String>)> = connection
        .query_row(
            "SELECT realm_id, schema_version, created_at, display_label
             FROM realm_metadata WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((realm_id, schema_version, created_at, display_label)) = found else {
        return Err(StoreError::InvalidRealmMetadata {
            reason: "the realm row is missing",
        });
    };

    let realm_id = RealmId::parse(&realm_id).map_err(|_| StoreError::InvalidRealmMetadata {
        reason: "the stored realm id is not a canonical version 7 UUID",
    })?;
    let schema_version = u32::try_from(schema_version)
        .ok()
        .and_then(|value| SchemaVersion::parse(value).ok())
        .ok_or(StoreError::InvalidRealmMetadata {
            reason: "the stored realm schema version is not one this binary creates",
        })?;
    let created_at =
        parse_utc_timestamp(&created_at).map_err(|_| StoreError::InvalidRealmMetadata {
            reason: "the stored realm creation time is not canonical UTC",
        })?;
    let display_label = display_label
        .as_deref()
        .map(ExternalName::parse)
        .transpose()
        .map_err(|_| StoreError::InvalidRealmMetadata {
            reason: "the stored realm label is not valid non-secret display text",
        })?;

    let realm = RealmMetadata {
        realm_id,
        schema_version,
        created_at,
        display_label,
    };
    realm
        .validate()
        .map_err(|_| StoreError::InvalidRealmMetadata {
            reason: "the stored realm metadata failed validation",
        })?;
    Ok(realm)
}

/// Confirm after migration that the version and the reference enforcement are
/// what we think they are.
fn verify_applied(connection: &Connection) -> Result<(), StoreError> {
    let version = read_user_version(connection)?;
    if version != SCHEMA_VERSION {
        return Err(StoreError::Pragma {
            pragma: "user_version",
        });
    }
    let foreign_keys: i64 = connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    if foreign_keys != 1 {
        return Err(StoreError::Pragma {
            pragma: "foreign_keys",
        });
    }
    Ok(())
}
