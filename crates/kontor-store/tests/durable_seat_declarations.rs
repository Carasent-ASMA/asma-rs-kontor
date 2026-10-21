//! Durable desk seat declarations in the store (ASMA-8451): the immutable
//! dated row, its ownership rules, the receipt kinds that declare and
//! reconcile it, and the export generation that carries it.

mod support;

use jiff::civil;
use kontor_core::durable_seat::{DurableSeatDeclaration, DurableSeatRecoveryOwner};
use kontor_core::id::{
    ContentHash, DeskKey, DurableSeatKey, ExternalName, ProjectId, RoleCode, SeatBindingId,
    Timestamp, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::repository::{
    NewProject, ProjectRepository, RepositoryError, StoredDesk, TeamDefinitionRepository,
    TopologyRepository,
};
use kontor_core::spec::{
    EffortLevel, ModelRef, ModelRung, ProviderRef, Shareability, ShareabilityTier,
    TeamDefinitionSnapshot,
};
use kontor_profiles::bundled_operational_domain;
use kontor_store::Applied;
use kontor_store::SqliteStore;
use kontor_store::backup::{KontorExportV1, export_realm};
use rusqlite::Connection;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

struct Fixture {
    home: tempfile::TempDir,
    store: SqliteStore,
    project_id: ProjectId,
    desk: StoredDesk,
}

impl Fixture {
    /// A project with one desk row. The row needs only a published Team
    /// Definition to pin; its nodes are irrelevant to a declaration.
    fn build() -> Self {
        let home = support::state_root();
        let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
        let project_id = ProjectId::generate();
        let created_at = at("2026-10-11T09:00:00Z");
        store
            .create_project(&NewProject {
                id: project_id,
                name: name("Durable seat project"),
                root_path: name("/tmp/durable-seat-project"),
                created_at,
            })
            .expect("the project is created");
        let domain = bundled_operational_domain().expect("the bundled domain");
        let definition = domain
            .team_definitions
            .first()
            .expect("a definition")
            .clone();
        let topology = domain
            .topology_specs
            .iter()
            .find(|topology| {
                topology.spec_id == definition.topology.spec_id
                    && topology.version == definition.topology.version
            })
            .expect("the definition's topology");
        store
            .publish_topology_spec(
                project_id,
                topology,
                &Shareability::default_for(ShareabilityTier::ProjectKnowledge)
                    .expect("tier B classifies"),
                created_at,
            )
            .expect("the topology publishes");
        store
            .publish_team_definition(project_id, &definition, created_at)
            .expect("the definition publishes");
        let desk = StoredDesk {
            project_id,
            desk_key: DeskKey::parse("pr-review").expect("desk key"),
            topology_node_id: TopologyNodeId::generate(),
            workspace_node_id: TopologyNodeId::generate(),
            team_definition: TeamDefinitionSnapshot::from_revision(&definition)
                .expect("a snapshot"),
            created_at,
        };
        store.create_desk(&desk).expect("the desk row is written");
        Self {
            home,
            store,
            project_id,
            desk,
        }
    }

    /// One of the two ASMA-8419 PR REVIEW seats, as a Paseo-direct decision.
    fn paseo_direct(&self, key: &str, label: &str, procedure: &str) -> DurableSeatDeclaration {
        DurableSeatDeclaration {
            project_id: self.project_id,
            desk_key: self.desk.desk_key.clone(),
            seat_key: DurableSeatKey::parse(key).expect("seat key"),
            title: name(&format!("LSA • {label}")),
            role_code: RoleCode::parse("LSA").expect("role"),
            seat_label: name(label),
            model_rung: None,
            procedure: Some(name(procedure)),
            recovery_owner: DurableSeatRecoveryOwner::PaseoDirect,
            decided_by: name("Test Owner"),
            decided_on: civil::date(2026, 10, 8),
            seat_binding_id: None,
            declared_at: at("2026-10-11T09:30:00Z"),
        }
    }

    fn daemon_owned(&self, key: &str, label: &str) -> DurableSeatDeclaration {
        DurableSeatDeclaration {
            model_rung: Some(ModelRung {
                provider: ProviderRef("claude".to_owned()),
                model: ModelRef("opus".to_owned()),
                effort: Some(EffortLevel::High),
            }),
            procedure: None,
            recovery_owner: DurableSeatRecoveryOwner::Daemon,
            seat_binding_id: Some(SeatBindingId::generate()),
            ..self.paseo_direct(key, label, "unused")
        }
    }

    fn raw(&self) -> Connection {
        Connection::open(self.home.path().join("kontor.db")).expect("a raw connection")
    }
}

#[test]
fn declarations_are_listed_by_key_and_survive_a_reopen() {
    let f = Fixture::build();
    let general = f.paseo_direct(
        "general",
        "GENERAL",
        ".github/skills/asma-pr-review/SKILL.md",
    );
    let checker = f.paseo_direct(
        "pr-drafts-status-checker",
        "PR DRAFTS STATUS CHECKER",
        ".github/skills/pr-draft-status-checker/SKILL.md",
    );
    let owned = f.daemon_owned("triage", "TRIAGE");
    for declaration in [&general, &checker, &owned] {
        let (stored, applied) = f.store.declare_durable_seat(declaration).expect("declares");
        assert_eq!(applied, Applied::Created);
        assert_eq!(&stored, declaration);
    }

    let path = f.home.path().join("kontor.db");
    drop(f.store);
    let reopened = SqliteStore::open(&path).expect("the store reopens");
    assert_eq!(
        reopened
            .list_durable_seat_declarations(f.project_id, &f.desk.desk_key)
            .expect("lists"),
        vec![general, checker, owned.clone()],
        "listed by seat key"
    );
    assert_eq!(
        reopened
            .get_durable_seat_declaration(f.project_id, &f.desk.desk_key, &owned.seat_key)
            .expect("reads"),
        Some(owned)
    );
}

#[test]
fn the_same_decision_is_unchanged_and_a_different_one_conflicts() {
    let f = Fixture::build();
    let owned = f.daemon_owned("triage", "TRIAGE");
    f.store.declare_durable_seat(&owned).expect("declares");

    // A retry whose answer was lost minted a fresh binding and instant. It is
    // the same decision, and the seat keeps the binding first planned.
    let mut retry = owned.clone();
    retry.seat_binding_id = Some(SeatBindingId::generate());
    retry.declared_at = at("2026-10-11T10:00:00Z");
    let (stored, applied) = f.store.declare_durable_seat(&retry).expect("replays");
    assert_eq!(applied, Applied::Unchanged);
    assert_eq!(stored.seat_binding_id, owned.seat_binding_id);
    assert_eq!(stored.declared_at, owned.declared_at);

    let mut amended = owned.clone();
    amended.decided_by = name("Adam");
    assert!(matches!(
        f.store.declare_durable_seat(&amended),
        Err(RepositoryError::Conflict {
            subject: "durable seat declaration",
            ..
        })
    ));

    // One title names one seat in a desk, whoever owns it: a daemon-owned
    // seat cannot be declared over a Paseo-direct seat's title.
    let direct = f.paseo_direct(
        "general",
        "GENERAL",
        ".github/skills/asma-pr-review/SKILL.md",
    );
    f.store.declare_durable_seat(&direct).expect("declares");
    let over = f.daemon_owned("general-again", "GENERAL");
    assert!(matches!(
        f.store.declare_durable_seat(&over),
        Err(RepositoryError::Conflict { .. })
    ));

    let mut deskless = f.daemon_owned("elsewhere", "ELSEWHERE");
    deskless.desk_key = DeskKey::parse("adam").expect("desk key");
    assert!(
        matches!(
            f.store.declare_durable_seat(&deskless),
            Err(RepositoryError::Conflict { .. })
        ),
        "a seat belongs to a desk that exists"
    );
    assert_eq!(
        f.store
            .list_durable_seat_declarations(f.project_id, &f.desk.desk_key)
            .expect("lists")
            .len(),
        2,
        "nothing refused was written"
    );
}

#[test]
fn a_declaration_is_immutable_permanent_and_owned_exactly_once() {
    let f = Fixture::build();
    let direct = f.paseo_direct(
        "general",
        "GENERAL",
        ".github/skills/asma-pr-review/SKILL.md",
    );
    f.store.declare_durable_seat(&direct).expect("declares");
    let connection = f.raw();
    let updated = connection.execute(
        "UPDATE durable_seat_declarations SET recovery_owner = 'daemon'",
        [],
    );
    assert!(
        updated
            .expect_err("a declaration is immutable")
            .to_string()
            .contains("a durable seat declaration is a dated decision and is immutable")
    );
    let deleted = connection.execute("DELETE FROM durable_seat_declarations", []);
    assert!(
        deleted
            .expect_err("a declaration is permanent")
            .to_string()
            .contains("a durable seat declaration is never withdrawn")
    );

    let insert = |owner: &str, binding: &str, rung: &str, key: &str| {
        connection.execute(
            &format!(
                "INSERT INTO durable_seat_declarations
                     (project_id, desk_key, seat_key, title, role_code, seat_label,
                      model_rung, procedure, recovery_owner, decided_by, decided_on,
                      seat_binding_id, declared_at)
                 VALUES ('{}', 'pr-review', '{key}', 'LSA • {key}', 'LSA', '{key}',
                         {rung}, NULL, '{owner}', 'Test Owner', '2026-10-08', {binding},
                         '2026-10-11T09:30:00Z')",
                f.project_id
            ),
            [],
        )
    };
    let binding = format!("'{}'", SeatBindingId::generate());
    let rung = r#"'{"provider":"claude","model":"opus"}'"#;
    assert!(
        insert("daemon", "NULL", rung, "a").is_err(),
        "a daemon-owned seat plans its binding"
    );
    assert!(
        insert("daemon", &binding, "NULL", "b").is_err(),
        "a daemon-owned seat declares its route"
    );
    assert!(
        insert("paseo_direct", &binding, "NULL", "c").is_err(),
        "a Paseo-direct seat is never bound by Kontor"
    );
    assert!(
        insert("nobody", "NULL", "NULL", "d").is_err(),
        "the owner is a closed value"
    );
    insert("daemon", &binding, rung, "e").expect("positive control: a complete daemon row");
    assert_eq!(
        f.store
            .list_durable_seat_declarations(f.project_id, &f.desk.desk_key)
            .expect("lists")
            .len(),
        2
    );
}

/// v132 applied over the v128 receipt shape keeps every receipt, admits the
/// two durable seat kinds, and keeps the kind list closed.
#[test]
fn v132_keeps_every_receipt_and_closes_in_exactly_two_kinds() {
    const PROJECT: &str = "0193f000-0000-7000-8000-000000000301";
    const CONTEXT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let v128 = include_str!("../migrations/0128_planning_pair_member_natives.sql");
    let start = v128
        .find("CREATE TABLE command_receipts_v128 (")
        .expect("the v128 receipt shape");
    let end = start + v128[start..].find(") STRICT;").expect("its end") + ") STRICT;".len();
    let receipts = v128[start..end].replace("command_receipts_v128", "command_receipts");
    let connection = Connection::open_in_memory().expect("the migration fixture opens");
    connection
        .execute_batch(&format!(
            "CREATE TABLE projects (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE desks (
                 project_id TEXT NOT NULL, desk_key TEXT NOT NULL,
                 PRIMARY KEY (project_id, desk_key)
             ) STRICT;
             {receipts}
             INSERT INTO projects VALUES ('{PROJECT}');
             INSERT INTO command_receipts
                 (id, project_id, idempotency_key, kind, target, target_revision, intent,
                  intent_hash, state, attempts, created_at, updated_at, execution_mode)
             VALUES ('0193f000-0000-7000-8000-000000000302', '{PROJECT}', 'before',
                     'recover_planning_pair_seat', '{{}}', 3, '{{}}', '{CONTEXT}',
                     'confirmed', 0, '2026-10-11T10:00:00Z', '2026-10-11T10:00:00Z',
                     'local');"
        ))
        .expect("the v128 fixture");
    let read = || -> String {
        connection
            .query_row(
                "SELECT json_group_array(json_array(id, kind, idempotency_key, target_revision,
                                                    state, intent_hash, execution_mode))
                   FROM command_receipts",
                [],
                |row| row.get(0),
            )
            .expect("the receipts read")
    };
    let before = read();
    connection
        .execute_batch(include_str!("../migrations/0132_durable_desk_seats.sql"))
        .expect("v132 applies over the historical rows");
    assert_eq!(read(), before, "every receipt is copied unchanged");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version reads");
    assert_eq!(version, 132);

    let receipt = |id: &str, key: &str, kind: &str| {
        connection.execute(
            &format!(
                "INSERT INTO command_receipts
                     (id, project_id, idempotency_key, kind, target, target_revision, intent,
                      intent_hash, state, attempts, created_at, updated_at, execution_mode)
                 VALUES ('{id}', '{PROJECT}', '{key}', '{kind}', '{{}}', 4, '{{}}', '{CONTEXT}',
                         'intent_persisted', 0, '2026-10-11T10:01:00Z',
                         '2026-10-11T10:01:00Z', 'local')"
            ),
            [],
        )
    };
    receipt(
        "0193f000-0000-7000-8000-000000000303",
        "declare",
        "declare_durable_seat",
    )
    .expect("the declaration kind is closed in");
    receipt(
        "0193f000-0000-7000-8000-000000000304",
        "reconcile",
        "reconcile_durable_seats",
    )
    .expect("the reconciliation kind is closed in");
    assert!(
        receipt(
            "0193f000-0000-7000-8000-000000000305",
            "restart",
            "restart_durable_seat",
        )
        .is_err(),
        "the kind list stays closed"
    );
    assert!(
        connection
            .execute("DELETE FROM command_receipts", [])
            .is_err(),
        "the rebuilt ledger keeps its no-delete trigger"
    );
    assert!(
        connection
            .execute(
                "UPDATE command_receipts SET kind = 'declare_durable_seat' WHERE idempotency_key = 'before'",
                [],
            )
            .is_err(),
        "the rebuilt ledger keeps its identity trigger"
    );
}

#[test]
fn the_typed_export_carries_declarations_and_no_older_generation_can_carry_or_conceal_them() {
    let f = Fixture::build();
    let owned = f.daemon_owned("triage", "TRIAGE");
    f.store.declare_durable_seat(&owned).expect("declares");
    let export = export_realm(&f.store, at("2026-10-11T11:00:00Z")).expect("the Realm exports");
    assert_eq!(export.records.durable_seat_declarations.len(), 1);
    let row = &export.records.durable_seat_declarations[0];
    assert_eq!(row.seat_key, "triage");
    assert_eq!(row.recovery_owner, "daemon");
    assert_eq!(row.decided_on, "2026-10-08");
    assert_eq!(
        row.seat_binding_id.as_deref(),
        owned.seat_binding_id.map(|id| id.to_string()).as_deref()
    );
    let base = serde_json::to_value(&export).expect("serializes");
    KontorExportV1::parse(&serde_json::to_vec(&base).expect("bytes"))
        .expect("positive control: the current generation verifies");

    let rehash = |document: &mut serde_json::Value| {
        let mut records =
            serde_json::to_vec(document.get("records").expect("records")).expect("serializes");
        records.push(b'\n');
        document["records_hash"] = serde_json::json!(ContentHash::of(&records).to_string());
    };
    let refusal = |document: serde_json::Value| -> String {
        match KontorExportV1::parse(&serde_json::to_vec(&document).expect("bytes")) {
            Err(kontor_store::backup::BackupError::Verification { detail }) => detail.to_owned(),
            other => panic!("expected a verification refusal, got {other:?}"),
        }
    };

    let mut carrying = base.clone();
    carrying["schema_version"] = serde_json::json!(14);
    carrying["database_schema_version"] = serde_json::json!(131);
    rehash(&mut carrying);
    assert_eq!(
        refusal(carrying),
        "the legacy export generation carries durable seat declarations it did not define"
    );

    let mut concealing = base.clone();
    concealing["schema_version"] = serde_json::json!(14);
    concealing["records"]["durable_seat_declarations"] = serde_json::json!([]);
    concealing["continuity_summary"]["record_counts"]["durable_seat_declarations"] =
        serde_json::json!(0);
    rehash(&mut concealing);
    assert_eq!(
        refusal(concealing),
        "the legacy export generation cannot prove durable seat declaration completeness"
    );

    let mut understated = base;
    understated["continuity_summary"]["record_counts"]["durable_seat_declarations"] =
        serde_json::json!(0);
    rehash(&mut understated);
    assert!(
        KontorExportV1::parse(&serde_json::to_vec(&understated).expect("bytes")).is_err(),
        "the continuity summary must disclose every declaration"
    );
}
