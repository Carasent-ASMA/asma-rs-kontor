//! Stored-domain, schema/SQL and backup boundary regressions, not live proof.
use super::*;
use kontor_core::consultation::ConsultationFamily;
use kontor_core::id::{RoleSlotId, Timestamp};
use rusqlite::{Connection, params};

#[test]
fn inactive_binding_and_node_each_refuse_unchanged() {
    for subject in ["seat_bindings", "topology_nodes"] {
        for state in ["retired", "archived"] {
            let f = Fixture::build();
            let ident = if subject == "seat_bindings" {
                f.seat.to_string()
            } else {
                f.node.to_string()
            };
            f.sql()
                .execute(
                    &format!("UPDATE {subject} SET lifecycle=?1 WHERE id=?2"),
                    params![state, ident],
                )
                .expect("inactive");
            f.refuse_unchanged(&f.request());
        }
    }
}

#[test]
fn consultation_uses_stored_run_slot_revisions_and_its_own_generation() {
    let f = Fixture::build();
    let (r, run, node) = f.consultation(Some(f.task));
    let p = f.store.prepare_attestation_token(&r).expect("prepare");
    let t = p.selected_token.expect("token");
    assert_eq!(t.occupancy_generation, 7);
    assert_eq!(t.task_id, Some(f.task));
    assert_eq!(t.topology_node_id, node);
    assert_eq!(
        t.provenance,
        AttestationSeatProvenance::Consultation {
            run_id: run,
            role_slot_id: RoleSlotId::parse("advisor.sa").expect("slot"),
            run_revision: AggregateRevision::INITIAL
        }
    );
    let mut wrong = r.clone();
    wrong.token_id = id("other");
    wrong.expected_token_head_revision = Some(1);
    wrong.expected_occupancy_generation = 1;
    f.refuse_unchanged(&wrong);
    wrong.expected_occupancy_generation = 7;
    wrong.task_id = None;
    f.refuse_unchanged(&wrong);
}

#[test]
fn consultation_eligible_phases_are_explicit_and_terminal_or_needs_human_refuse() {
    for state in [
        "materializing",
        "running",
        "awaiting_judge",
        "needs_human",
        "settled",
    ] {
        let f = Fixture::build();
        let (r, run, _) = f.consultation(None);
        f.sql().execute("UPDATE consultation_runs SET state=?1,revision=revision+1,settled_at=CASE WHEN ?1='settled' THEN updated_at ELSE NULL END WHERE run_id=?2",params![state,run.as_text()]).expect("phase fixture");
        if matches!(state, "materializing" | "running" | "awaiting_judge") {
            let t = f
                .store
                .prepare_attestation_token(&r)
                .expect("eligible")
                .selected_token
                .expect("token");
            assert!(
                matches!(t.provenance,AttestationSeatProvenance::Consultation {run_revision,..} if run_revision.get()==2)
            );
        } else {
            f.refuse_unchanged(&r);
        }
    }
}

#[test]
fn unknown_native_and_missing_consultation_never_imply_hosted_provenance() {
    let f = Fixture::build();
    let (r, run, _) = f.consultation(None);
    f.sql().execute("UPDATE consultation_seats SET runtime_kind=NULL,host=NULL,generation=NULL,native_id=NULL,observed_at=NULL WHERE run_id=?1",[run.as_text()]).expect("unbound fixture");
    f.refuse_unchanged(&r);
    f.sql()
        .execute(
            "DELETE FROM consultation_seats WHERE run_id=?1",
            [run.as_text()],
        )
        .expect("missing seat fixture");
    f.refuse_unchanged(&r);
}

#[test]
fn hosted_generation_is_history_count_plus_one_not_runtime_generation() {
    let f = Fixture::build();
    let mut r = f.request();
    f.sql().execute("INSERT INTO hosted_topology_seat_history (seat_binding_id,project_id,generation,model_rung,runtime_kind,host,native_id,autonomy,provider_session_id,observed_at,retired_at,retirement_reason) SELECT seat_binding_id,project_id,generation,model_rung,runtime_kind,host,'old-native',autonomy,NULL,observed_at,observed_at,'fixture prior occupancy' FROM hosted_topology_seats WHERE seat_binding_id=?1",[f.seat.to_string()]).expect("history");
    f.refuse_unchanged(&r);
    r.expected_occupancy_generation = 2;
    assert_eq!(
        f.store
            .prepare_attestation_token(&r)
            .expect("current")
            .selected_token
            .expect("token")
            .occupancy_generation,
        2
    );
}

#[test]
fn raw_sql_cannot_replace_rewrite_unrevoke_or_delete_metadata_or_heads() {
    let f = Fixture::build();
    let r = f.request();
    f.store.prepare_attestation_token(&r).expect("prepare");
    let sql = f.sql();
    let before = f.counts();
    for statement in [
        "UPDATE prepared_attestation_tokens SET payload_digest=printf('%064d',0)",
        "UPDATE prepared_attestation_tokens SET task_id=NULL,not_before=11",
        "DELETE FROM prepared_attestation_tokens",
        "DELETE FROM attestation_token_heads",
        "UPDATE attestation_token_heads SET revision=2",
        "INSERT OR REPLACE INTO prepared_attestation_tokens SELECT * FROM prepared_attestation_tokens",
        "INSERT OR REPLACE INTO attestation_token_heads SELECT * FROM attestation_token_heads",
    ] {
        assert!(
            sql.execute_batch(statement).is_err(),
            "unguarded SQL: {statement}"
        );
        assert_eq!(f.counts(), before);
    }
    f.store
        .revoke_prepared_attestation_token(&f.scope, &r.issuer, &r.token_id, 1)
        .expect("revoke");
    assert!(
        sql.execute_batch("UPDATE prepared_attestation_tokens SET revoked_revision=NULL")
            .is_err()
    );
    assert!(
        sql.execute_batch("UPDATE prepared_attestation_tokens SET revoked_revision=3")
            .is_err()
    );
    assert!(
        sql.execute_batch("UPDATE attestation_token_heads SET revision=1")
            .is_err()
    );
    assert_eq!(f.counts(), (1, 1, 2));
}

#[test]
fn checked_token_revision_overflow_leaves_no_partial_preparation_or_revocation() {
    let f = Fixture::build();
    let mut r = f.request();
    f.store.prepare_attestation_token(&r).expect("prepare");
    f.sql().execute_batch("DROP TRIGGER attestation_token_heads_monotonic; UPDATE attestation_token_heads SET revision=9223372036854775807;").expect("isolated overflow fixture");
    r.token_id = id("next");
    r.expected_token_head_revision = Some(i64::MAX as u64);
    f.refuse_unchanged(&r);
    assert!(
        f.store
            .revoke_prepared_attestation_token(&f.scope, &r.issuer, &id("token-1"), i64::MAX as u64)
            .is_err()
    );
    assert_eq!(f.counts(), (1, 1, i64::MAX));
}

#[test]
fn two_store_preparation_race_has_one_cas_winner_and_one_unchanged_conflict() {
    let f = Fixture::build();
    let path = f.home.path().join("kontor.db");
    let request = f.request();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|n| {
            let path = path.clone();
            let mut r = request.clone();
            r.token_id = id(&format!("race-{n}"));
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = SqliteStore::open(&path).expect("store");
                barrier.wait();
                store.prepare_attestation_token(&r)
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("thread"))
        .collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(RepositoryError::Conflict { .. })))
            .count(),
        1
    );
    assert_eq!(f.counts(), (1, 1, 1));
}

#[test]
fn consultation_slot_node_cross_run_and_ambiguous_domain_refuse_unchanged() {
    for case in 0..4 {
        let f = Fixture::build();
        let (r, run, node) = f.consultation(None);
        let sql = f.sql();
        match case {
            0 => {
                sql.execute(
                    "UPDATE consultation_seats SET role_slot_id='different' WHERE run_id=?1",
                    [run.as_text()],
                )
                .expect("slot fixture");
            }
            1 => {
                sql.execute(
                    "UPDATE seat_bindings SET topology_node_id=?1 WHERE id=?2",
                    params![f.node.to_string(), r.seat_binding_id.to_string()],
                )
                .expect("hosting node fixture");
            }
            2 => {
                let (_, other, _) = f.consultation(None);
                sql.execute("UPDATE consultation_seats SET run_id=?1,role_slot_id='different' WHERE run_id=?2",params![other.as_text(),run.as_text()]).expect("cross-run fixture");
            }
            _ => {
                sql.execute("INSERT INTO hosted_topology_seats (seat_binding_id,project_id,model_rung,runtime_kind,host,generation,native_id,autonomy,provider_session_id,observed_at) SELECT ?1,project_id,model_rung,runtime_kind,host,generation,?2,autonomy,NULL,observed_at FROM hosted_topology_seats WHERE seat_binding_id=?3",params![r.seat_binding_id.to_string(),r.expected_native_identity.native_id.as_str(),f.seat.to_string()]).expect("ambiguous domain fixture");
            }
        }
        f.refuse_unchanged(&r);
        assert_ne!(node, f.node);
    }
}

#[test]
fn a_container_native_identity_cannot_masquerade_as_seat_provenance() {
    let f = Fixture::build();
    let r = f.request();
    let n = &r.expected_native_identity;
    f.sql().execute("INSERT INTO topology_node_containers (topology_node_id,project_id,container_binding_id,runtime_kind,host,generation,native_id,observed_kind,bound_at,last_readback_at,revision) VALUES (?1,?2,'container-fixture',?3,?4,?5,?6,'workspace','2026-10-03T20:00:00Z','2026-10-03T20:00:00Z',1)",params![f.node.to_string(),f.scope.project_id.to_string(),n.runtime_kind.as_str(),n.host.as_str(),i64::try_from(n.generation).expect("fixture generation"),n.native_id.as_str()]).expect("container");
    assert!(matches!(
        f.refuse_unchanged(&r),
        RepositoryError::Conflict {
            rule: "native metadata is absent or ambiguous with another subject",
            ..
        }
    ));
}

#[test]
fn migration125_transaction_rolls_back_all_tables_and_version_on_ddl_failure() {
    let mut c = Connection::open_in_memory().expect("fixture");
    c.execute_batch("PRAGMA user_version=124; CREATE TABLE prepared_attestation_tokens (sentinel INTEGER) STRICT;").expect("collision fixture");
    {
        let tx = c.transaction().expect("transaction");
        assert!(
            tx.execute_batch(include_str!(
                "../../migrations/0125_prepared_attestation_tokens.sql"
            ))
            .is_err()
        );
    }
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .expect("version"),
        124
    );
    assert_eq!(c.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='attestation_token_heads'",[],|r|r.get::<_,i64>(0)).expect("table"),0);
}

#[test]
fn prepared_and_revoked_history_cannot_be_exported_or_restored_to_an_older_head() {
    use kontor_store::backup::{BackupError, create_snapshot, restore_snapshot};
    let f = Fixture::build();
    let r = f.request();
    let database = f.home.path().join("kontor.db");
    let backups = f.home.path().join("backups");
    let before = create_snapshot(&database, &backups, Timestamp::now())
        .expect("preparation-before snapshot");
    f.store.prepare_attestation_token(&r).expect("prepare");
    f.store
        .revoke_prepared_attestation_token(&f.scope, &r.issuer, &r.token_id, 1)
        .expect("revoke");
    assert!(matches!(
        kontor_store::backup::export_realm(&f.store, Timestamp::now()),
        Err(BackupError::Verification { .. })
    ));
    let current = create_snapshot(&database, &backups, Timestamp::now())
        .expect("current structural snapshot");
    drop(f.store);
    let bytes = std::fs::read(&database).expect("target bytes");
    let listing = || {
        let mut names: Vec<_> = std::fs::read_dir(f.home.path())
            .expect("directory")
            .map(|e| e.expect("entry").file_name())
            .collect();
        names.sort();
        names
    };
    let names = listing();
    assert!(matches!(
        restore_snapshot(&before.snapshot, &database, Timestamp::now()),
        Err(BackupError::Verification { .. })
    ));
    assert_eq!(std::fs::read(&database).expect("unchanged target"), bytes);
    assert_eq!(listing(), names);
    let absent = f.home.path().join("absent-target/kontor.db");
    let source = std::fs::read(&current.snapshot).expect("source");
    assert!(matches!(
        restore_snapshot(&current.snapshot, &absent, Timestamp::now()),
        Err(BackupError::Verification { .. })
    ));
    assert!(!absent.parent().expect("parent").exists());
    assert_eq!(
        std::fs::read(&current.snapshot).expect("unchanged source"),
        source
    );
    let reopened = SqliteStore::open(&database).expect("reopen");
    let p = reopened
        .read_prepared_attestation_token(&r.scope, &r.issuer, &r.token_id)
        .expect("read")
        .expect("head");
    assert_eq!(p.head_revision, 2);
    assert_eq!(p.selected_token.expect("token").revoked_revision, Some(2));
}

#[test]
fn all_family_qualified_registry_metadata_is_retained_and_disposed_pairs_refuse() {
    for family in [
        ConsultationFamily::Advisor,
        ConsultationFamily::Committee,
        ConsultationFamily::PlanningPair,
    ] {
        let f = Fixture::build();
        let (mut r, run, _) = f.consultation_family(None, family);
        let first = f
            .store
            .prepare_attestation_token(&r)
            .expect("prepare registry metadata");
        assert!(
            matches!(first.selected_token.expect("token").provenance,AttestationSeatProvenance::Consultation {run_id,..} if run_id==run && run_id.family()==family)
        );
        if family == ConsultationFamily::PlanningPair {
            f.sql().execute("UPDATE consultation_runs SET state='disposed',revision=revision+1 WHERE run_id=?1",[run.as_text()]).expect("disposed registry fixture");
            r.token_id = id("after-disposed");
            r.expected_token_head_revision = Some(1);
            f.refuse_unchanged(&r);
        }
    }
}

#[test]
fn raw_sql_insert_refuses_false_key_commitments_and_invalid_bounds_atomically() {
    let f = Fixture::build();
    f.store
        .prepare_attestation_token(&f.request())
        .expect("prepare");
    let sql = f.sql();
    let columns: Vec<String> = sql
        .prepare("PRAGMA table_info(prepared_attestation_tokens)")
        .expect("table")
        .query_map([], |row| row.get(1))
        .expect("columns")
        .collect::<Result<_, _>>()
        .expect("columns");
    for (field, value) in [
        ("key_material_digest", "printf('%064d',0)"),
        ("key_registered_revision", "2"),
        ("preparation_key_head_revision", "2"),
        ("not_before", "0"),
        ("expires_at", "1001"),
        ("native_generation", "0"),
        ("occupancy_generation", "0"),
        ("token_id", "printf('%257d',1)"),
        ("expires_at", "not_before"),
    ] {
        let expressions: Vec<_> = columns
            .iter()
            .map(|column| match column.as_str() {
                name if name == field => value.to_owned(),
                "token_id" => "token_id||'-new'".into(),
                "registered_revision" => "2".into(),
                name => name.to_owned(),
            })
            .collect();
        assert!(sql.execute_batch(&format!("INSERT INTO prepared_attestation_tokens SELECT {} FROM prepared_attestation_tokens",expressions.join(","))).is_err(),"false SQL commitment accepted: {field}");
        assert_eq!(f.counts(), (1, 1, 1));
    }
}
