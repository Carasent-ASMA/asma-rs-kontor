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

use kontor_core::id::{
    CanonicalDocument, ContentHash, ExternalName, RealmId, SchemaVersion, Timestamp,
    parse_utc_timestamp,
};
use kontor_core::realm::RealmMetadata;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::StoreError;

/// The schema generation this binary implements.
pub const SCHEMA_VERSION: i64 = 64;

/// The bounded busy timeout applied to every connection.
///
/// It has to cover the longest thing another connection can legitimately be
/// holding the database for, and that is one complete first-open migration
/// chain — every generation from zero, in a single transaction. The chain grows
/// with every schema generation and each table rebuild in it costs real
/// milliseconds, so a budget sized against a shorter chain turns an ordinary
/// concurrent first open into a spurious "database is locked" on a loaded
/// machine. Thirty seconds is still a bound: a genuinely stuck peer still
/// fails, it just is not confused with a busy one.
const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

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
    // Schema v4. Durable scheduler leases — expiry, fencing token, holder and
    // kind on the v1 lease — realm-wide module contention, append-only lease
    // history and the canonical admission decision.
    include_str!("../migrations/0004_scheduler_admission.sql"),
    // Schema v5. The destination half of a redacted import: this Realm's own
    // import receipt and the append-only lineage of the source records it was
    // built from.
    include_str!("../migrations/0005_redacted_import.sql"),
    // Schema v6. The terminal half of intake: append-only approval, rejection
    // and bounded auto-arm decisions about a proposal, and one lineage row per
    // task those decisions created.
    include_str!("../migrations/0006_intake_decisions.sql"),
    // Schema v7. Holiday import provenance: which importer produced a source
    // revision, what the request asked for, what it refused, and the superseding
    // chain that makes exactly one import current per calendar.
    include_str!("../migrations/0007_calendar_imports.sql"),
    // Schema v8. Immutable mini-project/task window revisions that feed the
    // production calendar resolver rather than stopping at its pure API.
    include_str!("../migrations/0008_child_calendar_windows.sql"),
    // Schema v9. The append-only revocation that disarms an immutable
    // execution authorization.
    include_str!("../migrations/0009_authorization_revocation.sql"),
    // Schema v10. The bootstrap and disarm command kinds, which widen the
    // closed `command_receipts.kind` list.
    include_str!("../migrations/0010_bootstrap_command_kinds.sql"),
    // Schema v11. The pre-run provider-account selection a task carries before
    // any run exists to record one.
    include_str!("../migrations/0011_task_account_selection.sql"),
    // Schema v12. The command kinds used by the corrective public surface.
    include_str!("../migrations/0012_surface_command_kinds.sql"),
    // Schema v13. The immutable requested/effective context-window pair one run
    // was launched under, and every recorded attempt to compact its seat.
    include_str!("../migrations/0013_context_policy_and_compaction.sql"),
    include_str!("../migrations/0014_registered_profile_packs.sql"),
    include_str!("../migrations/0015_realm_idempotency_bindings.sql"),
    include_str!("../migrations/0016_task_worktrees.sql"),
    include_str!("../migrations/0017_runtime_binding_snapshots.sql"),
    include_str!("../migrations/0018_role_turns.sql"),
    include_str!("../migrations/0019_team_closure_on_settled_turns.sql"),
    include_str!("../migrations/0020_role_slot_waivers.sql"),
    include_str!("../migrations/0021_native_memory.sql"),
    include_str!("../migrations/0022_teams_editor.sql"),
    include_str!("../migrations/0023_operational_topology.sql"),
    include_str!("../migrations/0024_replace_seat_command.sql"),
    include_str!("../migrations/0025_document_shareability.sql"),
    // Schema v26. The durable native container binding per topology node, and
    // the OP-REQ-039 attachment evidence a logical seat is concluded from —
    // the deadline fixed at creation, the last observed attachment, the last
    // observed *activity*, the owning epic seat, and the runtime's self-report
    // as quotable evidence.
    include_str!("../migrations/0026_operational_liveness.sql"),
    // Schema v27. The delivery task a task-scoped node serves, so admission can
    // locate the node before it creates the seat binding that would otherwise
    // have been the only way to find it.
    include_str!("../migrations/0027_task_topology_node.sql"),
    // Schema v28. Account-owned capacity evidence: the immutable raw reading a
    // native collector took, and the operator override that stands beside it
    // rather than over it. Cooldown stops being another program's file.
    include_str!("../migrations/0028_native_capacity.sql"),
    // Schema v29. Topology specification publication through `/v1`, and the
    // explicit epic upgrade that moves a pin — which is why the pin row becomes
    // writable by that one operation, and why the closed kind list grows by two.
    include_str!("../migrations/0029_topology_publication.sql"),
    // Schema v30. The container retitle command. One kind, and no title column:
    // what a container is called is the runtime's fact, read back rather than
    // mirrored.
    include_str!("../migrations/0030_retitle_container_command.sql"),
    // Schema v31. The bounded task reopen: `done -> ready` and nothing else, so
    // the lifecycle action the surface advertises can reach the domain rule
    // written for it.
    include_str!("../migrations/0031_bounded_task_reopen.sql"),
    // Schema v32. Immutable Project Core Team revisions, and the one command
    // that publishes them. Kept as whole revisions because promotion freezes
    // the exact one an epic was staffed from.
    include_str!("../migrations/0032_core_team_revisions.sql"),
    // Schema v33. Quick sessions, their one promotion, and the roster an epic
    // freezes at that moment -- plus the four OP-04 commands. The promotion row
    // carries its ids so a resumed apply reconciles rather than rebuilds.
    include_str!("../migrations/0033_quick_sessions_and_promotion.sql"),
    // Schema v34. Immutable Advisor profile and Committee template revisions,
    // and the two Admin publications that write them. One table for both
    // families because they are one storage shape, discriminated by `family`.
    include_str!("../migrations/0034_consultation_profiles.sql"),
    // Schema v35. Published epic Completion Profiles, one durable completion run
    // per epic, and the TPM wake outbox -- plus the three OP-06 commands. The
    // wake's primary key is the one-wake-per-observation rule, so a replayed
    // callback collides with the intent already standing instead of opening a
    // second turn.
    include_str!("../migrations/0035_epic_completion.sql"),
    // Schema v36. Repository-backed Advisor/Committee runs, their exact native
    // seats, immutable Committee findings and the five run command kinds.
    include_str!("../migrations/0036_consultation_runs.sql"),
    // Schema v37. An
    // escalation reaches a human with a recommendation, its author and the
    // deliberation path already walked (OP-REQ-036): a `needs_human` row states
    // its brief.
    include_str!("../migrations/0037_escalation_brief.sql"),
    // Schema v38. One command kind for
    // installing a trigger revision, which is how a bounded auto-arm capability
    // is declared at all. The final rebuild carries the union of both merged
    // lineages and restores the receipt immutability triggers.
    include_str!("../migrations/0038_publish_trigger_command.sql"),
    include_str!("../migrations/0039_committee_remediation.sql"),
    // Schema v40. One immutable Advisor advice artifact, authored by the exact
    // attested seat before a Realm operator records the requester's disposition.
    include_str!("../migrations/0040_advisor_advice.sql"),
    include_str!("../migrations/0041_open_questions.sql"),
    // Schema v42. The historical lifecycle fact carried by an epic import. It
    // is cleared by the first native lifecycle transition, so imported
    // completion is never confused with certified native closure.
    include_str!("../migrations/0042_imported_task_lifecycle.sql"),
    // Schema v43. Runtime-facing epic identity belongs to each epic rather than
    // one startup-loaded runtime plane, allowing several epics in one project.
    include_str!("../migrations/0043_epic_execution_scopes.sql"),
    // Schema v44. Persistent leadership seats are not Delivery AgentRuns, but
    // still need an exact native identity for idempotent messages and restart
    // recovery. SeatBinding remains their logical identity.
    include_str!("../migrations/0044_hosted_topology_seats.sql"),
    // Schema v45. Project-pinned external-workflow installation and a distinct
    // terminal task-withdrawal state/receipt.
    include_str!("../migrations/0045_admin_workflow_install_and_withdrawal.sql"),
    // Schema v46. Explicit task display identity plus immutable predecessor
    // evidence for persistent Core Team route correction.
    include_str!("../migrations/0046_task_short_codes_and_hosted_route_history.sql"),
    // Schema v47. Explicit immutable epic AI short names consumed by pinned
    // native-container naming templates.
    include_str!("../migrations/0047_configurable_native_names.sql"),
    // Schema v48. Per-account, per-provider quota state: durable, self-expiring
    // and finer-grained than the composition-time `unavailable_providers` set.
    include_str!("../migrations/0048_provider_quota_states.sql"),
    // Schema v49. `command_execution_mode`: a synchronous control-plane
    // operation is not an undispatched command, and before this column both
    // were written with an outbox row.
    include_str!("../migrations/0049_command_execution_mode.sql"),
    // v50 admits the provider's own usage endpoint as a quota authority, so a
    // window can be recorded as reopened without a human noticing it did.
    include_str!("../migrations/0050_provider_report_quota_source.sql"),
    // v51 adds concurrent quota windows, credit headroom and `cannot_report`
    // on top of that poller. It does not replace v50's source or invent a
    // second collector.
    include_str!("../migrations/0051_provider_quota_headroom.sql"),
    // Schema v52. Additional modules a task changes, and identity matching so a
    // slash admission cannot steal a live dotted module holdout.
    include_str!("../migrations/0052_task_modules_and_module_identity.sql"),
    // v53 lets an admission record default-allow: `authorization_id` may be
    // NULL when nothing narrowed the run. Disarm is a stop, not a return to
    // unarmed, so a revoked covering grant is carried as a blocker instead.
    include_str!("../migrations/0053_default_allow_admission.sql"),
    // Schema v54. Per-project, per-subject authority and the evidence-backed
    // one-way replacement for the realm-global memory switch.
    include_str!("../migrations/0054_project_subject_authority.sql"),
    // Schema v55. Gate recovery session
    // evidence: the citation of the evaluator's own session record that a
    // verdict recorded on behalf of a closed evaluator seat is transcribed
    // from.
    include_str!("../migrations/0055_gate_recovery_evidence.sql"),
    // Schema v56. Project topology selection is separately authorized from
    // moving one immutable epic pin.
    include_str!("../migrations/0056_project_topology_selection.sql"),
    // Schema v57. Durable native Jira materialization, exact readback bindings,
    // and ASMA activation after the whole epic is confirmed.
    include_str!("../migrations/0057_jira_materialization.sql"),
    // Schema v58. Idempotent project-scoped legacy backlog import receipt.
    include_str!("../migrations/0058_backlog_import_command.sql"),
    include_str!("../migrations/0059_core_team_seat_claim_command.sql"),
    include_str!("../migrations/0060_consultation_seat_recovery.sql"),
    include_str!("../migrations/0061_consultation_seat_occupancy_fencing.sql"),
    // Schema v62. The exact immutable workflow/policy result of a profile
    // selection is committed with its local command receipt and effect.
    include_str!("../migrations/0062_profile_selection_outcomes.sql"),
    // Schema v63. Imported profile-selection results retain their exact source
    // policy and row hash as destination-owned, non-executable lineage.
    include_str!("../migrations/0063_imported_profile_selection_outcomes.sql"),
    // Schema v64. Remediation freezes a failed result instead of opening a
    // mutable second round, and its source round is safe for either round.
    include_str!("../migrations/0064_committee_remediation_rounds.sql"),
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

    // Reference enforcement is lifted for the migration and restored before
    // anything else can use the connection.
    //
    // This is SQLite's own documented procedure for a schema change that rebuilds
    // a table, and it has to happen *here* rather than inside a script, because
    // `PRAGMA foreign_keys` is silently ignored inside a transaction. A rebuild —
    // create the new shape, copy, drop the old, rename — necessarily leaves every
    // child row pointing at a table that does not exist for the space of two
    // statements, and with enforcement on, the `DROP` fails.
    //
    // Nothing is weakened by it: `foreign_key_check` runs against the whole
    // database before the commit, so a migration that *did* strand a row rolls
    // back with the same finality as one that failed outright, and
    // `verify_applied` then proves enforcement is back on before the store is
    // handed to anyone.
    connection.pragma_update(None, "foreign_keys", false)?;
    let outcome = apply_pending(connection, &realm, version);
    // Restored on both paths, including the early returns inside `apply_pending`:
    // a failed migration must not leave the connection with enforcement off.
    connection.pragma_update(None, "foreign_keys", true)?;
    outcome?;

    verify_applied(connection)?;
    load_realm(connection)
}

/// Run every pending migration in one transaction, with reference enforcement
/// already lifted by [`migrate`].
fn apply_pending(
    connection: &mut Connection,
    realm: &RealmMetadata,
    version: i64,
) -> Result<(), StoreError> {
    let _ = version;
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
        return Ok(());
    }

    // Master and the operational-recovery branch both shipped schema numbers 34
    // and 35 before they met. The former lineage has the escalation brief (and,
    // at v35, `publish_trigger`) but no consultation profiles. Recognize that
    // durable shape rather than interpreting its number as the canonical one.
    // Its escalation objects are already present, so the convergence installs
    // the consultation generations, the union receipt rebuild, and every later
    // append-only migration.
    let operational_hardening_lineage = matches!(version, 34 | 35)
        && !table_exists(&transaction, "consultation_profile_revisions")?;
    if operational_hardening_lineage {
        // Everything from 33 onward except the escalation brief this lineage
        // already installed by hand. Enumerated rather than listed: the list
        // used to be spelled out index by index and ended at the last migration
        // that existed when it was written, so appending a migration left this
        // one lineage stranded a version short and `verify_applied` refused the
        // open. A skip-set cannot fall behind.
        const ALREADY_INSTALLED: usize = 36;
        for (index, migration) in MIGRATIONS.iter().enumerate().skip(33) {
            if index == ALREADY_INSTALLED {
                continue;
            }
            transaction.execute_batch(migration)?;
        }
    } else {
        // Every pending script runs here, in order, in this one transaction.
        // Each ends with its own transactional `PRAGMA user_version`: any
        // failure rolls the version and every object back together.
        let pending = usize::try_from(version).map_err(|_| StoreError::Pragma {
            pragma: "user_version",
        })?;
        for migration in &MIGRATIONS[pending..] {
            transaction.execute_batch(migration)?;
        }
    }

    // Schema 47's SQL installs the durable receipt before this one guarded
    // data migration runs. The built-in revision keeps the same identity and
    // version, so every reference must move to the new canonical hash in this
    // transaction or none of them may move at all.
    if version < 47 {
        canonicalize_operational_topology_v47(&transaction)?;
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

    // The whole database is checked before the commit, not just the tables the
    // scripts touched. Enforcement was off while they ran, so this is the only
    // thing standing between a rebuild that stranded a child row and a committed
    // database that has one — and it rolls back exactly like any other failure.
    {
        let mut statement = transaction.prepare("PRAGMA foreign_key_check")?;
        let mut rows = statement.query([])?;
        if let Some(row) = rows.next()? {
            let table: String = row.get(0)?;
            return Err(StoreError::Integrity {
                detail: format!("migration left a dangling foreign key in `{table}`"),
            });
        }
    }
    transaction.commit()?;
    Ok(())
}

const OPERATIONAL_TOPOLOGY_SPEC_ID: &str = "01936f5a-1000-7000-8000-000000000001";
const OPERATIONAL_TOPOLOGY_V1_PRIOR_HASH: &str =
    "36551ae60f0d354cfe5093b48f482f42227ce99d7cc704a02a3afa92e302dbf1";
const OPERATIONAL_TOPOLOGY_V1_CANONICAL_HASH: &str =
    "c112faff3f0ad0d8893bd41a1a53215816e0bd93cd9d65ed359ba74d0822254b";

/// Replace only the known bundled v1 document with its typed naming shape.
fn canonicalize_operational_topology_v47(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<(), StoreError> {
    let mut statement = transaction.prepare(
        "SELECT project_id, definition, definition_hash
         FROM topology_specs WHERE spec_id = ?1 AND version = 1
         ORDER BY project_id",
    )?;
    let rows = statement
        .query_map([OPERATIONAL_TOPOLOGY_SPEC_ID], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    if rows.is_empty() {
        return Ok(());
    }

    let prior = ContentHash::parse(OPERATIONAL_TOPOLOGY_V1_PRIOR_HASH)?;
    let expected = ContentHash::parse(OPERATIONAL_TOPOLOGY_V1_CANONICAL_HASH)?;
    let migrated_at = Timestamp::now().to_string();
    let mut replacements = Vec::with_capacity(rows.len());
    for (project_id, definition, definition_hash) in rows {
        if definition_hash != prior.as_str() {
            return Err(StoreError::Integrity {
                detail:
                    "schema 47 found the built-in topology id/version with an unknown prior hash"
                        .to_owned(),
            });
        }
        let stored = CanonicalDocument::from_stored(&definition, &prior)?;
        let mut value = stored
            .deserialize::<serde_json::Value>()
            .map_err(StoreError::Domain)?;
        canonical_operational_topology_value(&mut value)?;
        let canonical = CanonicalDocument::from_value(&value)?;
        if canonical.hash() != &expected {
            return Err(StoreError::Integrity {
                detail: "schema 47 built-in topology canonicalization produced an unexpected hash"
                    .to_owned(),
            });
        }
        for (table, hash_column, version_column) in [
            ("project_topology_defaults", "canonical_hash", "version"),
            (
                "mini_project_topology_snapshots",
                "canonical_hash",
                "version",
            ),
            ("topology_nodes", "spec_hash", "spec_version"),
        ] {
            let sql = format!(
                "SELECT COUNT(*) FROM {table}
                 WHERE project_id = ?1 AND spec_id = ?2
                   AND {version_column} = 1 AND {hash_column} <> ?3"
            );
            let mismatches: i64 = transaction.query_row(
                &sql,
                params![project_id, OPERATIONAL_TOPOLOGY_SPEC_ID, prior.as_str(),],
                |row| row.get(0),
            )?;
            if mismatches != 0 {
                return Err(StoreError::Integrity {
                    detail: "schema 47 found an unknown built-in topology reference hash"
                        .to_owned(),
                });
            }
        }
        replacements.push((project_id, canonical.json().to_owned()));
    }

    transaction.execute_batch("DROP TRIGGER topology_specs_are_immutable;")?;
    for (project_id, definition) in replacements {
        transaction.execute(
            "UPDATE topology_specs SET definition = ?1, definition_hash = ?2
             WHERE project_id = ?3 AND spec_id = ?4 AND version = 1",
            params![
                definition,
                expected.as_str(),
                project_id,
                OPERATIONAL_TOPOLOGY_SPEC_ID,
            ],
        )?;
        for statement in [
            "UPDATE project_topology_defaults SET canonical_hash = ?1
             WHERE project_id = ?2 AND spec_id = ?3 AND version = 1 AND canonical_hash = ?4",
            "UPDATE mini_project_topology_snapshots SET canonical_hash = ?1
             WHERE project_id = ?2 AND spec_id = ?3 AND version = 1 AND canonical_hash = ?4",
            "UPDATE topology_nodes SET spec_hash = ?1
             WHERE project_id = ?2 AND spec_id = ?3 AND spec_version = 1 AND spec_hash = ?4",
        ] {
            transaction.execute(
                statement,
                params![
                    expected.as_str(),
                    project_id,
                    OPERATIONAL_TOPOLOGY_SPEC_ID,
                    prior.as_str(),
                ],
            )?;
        }
        transaction.execute(
            "INSERT INTO topology_spec_canonicalization_receipts
                 (project_id, spec_id, version, prior_hash, canonical_hash,
                  migrated_at, reason)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6)",
            params![
                project_id,
                OPERATIONAL_TOPOLOGY_SPEC_ID,
                prior.as_str(),
                expected.as_str(),
                migrated_at,
                "ASMA-7967 typed native-name templates",
            ],
        )?;
    }
    transaction.execute_batch(
        "CREATE TRIGGER topology_specs_are_immutable
         BEFORE UPDATE ON topology_specs
         BEGIN
             SELECT RAISE(ABORT, 'topology specification revisions are immutable');
         END;",
    )?;
    Ok(())
}

fn canonical_operational_topology_value(value: &mut serde_json::Value) -> Result<(), StoreError> {
    let object = value.as_object_mut().ok_or_else(|| StoreError::Integrity {
        detail: "schema 47 built-in topology definition is not an object".to_owned(),
    })?;
    object.insert("name_separator".to_owned(), serde_json::json!(" • "));
    let kinds = object
        .get_mut("node_kinds")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| StoreError::Integrity {
            detail: "schema 47 built-in topology has no node-kind array".to_owned(),
        })?;
    for declared in kinds {
        let kind = declared
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| StoreError::Integrity {
                detail: "schema 47 built-in topology has a node kind without a key".to_owned(),
            })?;
        let (container, seat) = match kind {
            "PSW" => (literal_template("Project Session Workspace"), None),
            "QSW" => (
                literal_template("Quick Session Workspace"),
                Some(token_template(&["AREA_CODE"])),
            ),
            "ESW" => (scoped_container_template(), None),
            "ECP" => (
                scoped_container_template(),
                Some(scoped_container_template()),
            ),
            "TSW" => (
                scoped_container_template(),
                Some(token_template(&["AREA_CODE", "KONTOR_BACKLOG_CODE"])),
            ),
            "ASW" => (
                literal_template("Advisor Session Workspace"),
                Some(token_template(&["AREA_CODE"])),
            ),
            "CSW" => (
                literal_template("Committee Session Workspace"),
                Some(token_template(&["AREA_CODE"])),
            ),
            _ => {
                return Err(StoreError::Integrity {
                    detail: "schema 47 built-in topology contains an unknown node kind".to_owned(),
                });
            }
        };
        let declared = declared
            .as_object_mut()
            .expect("a node carrying a string kind is an object");
        declared.insert("name_template".to_owned(), container);
        match seat {
            Some(seat) => {
                declared.insert("seat_name_template".to_owned(), seat);
            }
            None => {
                declared.remove("seat_name_template");
            }
        }
    }
    Ok(())
}

fn scoped_container_template() -> serde_json::Value {
    token_template(&["AREA_CODE", "JIRA_CODE", "KONTOR_BACKLOG_CODE"])
}

fn token_template(tokens: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "segments": tokens
            .iter()
            .map(|token| serde_json::json!({"kind": "token", "value": token}))
            .collect::<Vec<_>>()
    })
}

fn literal_template(literal: &str) -> serde_json::Value {
    serde_json::json!({
        "segments": [{"kind": "literal", "value": literal}]
    })
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, StoreError> {
    let found: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
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
