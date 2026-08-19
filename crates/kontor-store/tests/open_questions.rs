//! The open-question ledger against a real database (OP-REQ-038).
//!
//! The mutants this suite exists to kill:
//!
//! * an in-place UPDATE or DELETE of a round, a disposition or a trigger firing;
//! * a header field other than the revision changing after the question was
//!   raised;
//! * a question of another project resolving through a globally unique id;
//! * an append from a caller working off a stale revision;
//! * a deferral stored without the trigger that reopens it, or a firing that
//!   names a trigger nobody deferred on;
//! * a reopened deferral firing twice, or a firing against a superseded one;
//! * a round, disposition, firing or shareability stamp lost across a restart, a
//!   deterministic export, or a snapshot restore.

use kontor_core::id::{
    BoundedText, ContentHash, ExternalName, MiniProjectId, OpenQuestionId, ProjectId,
    RoleCatalogId, RoleCode, RoleKey, RoleSlotId, SeatBindingId, SpecVersion, Timestamp,
    TopologyKindKey, TopologyNodeId, TriggerKey, parse_utc_timestamp,
};
use kontor_core::open_question::{
    AmbiguityRound, CloserPolicy, DecisionCitation, Disposition, DispositionOutcome, OpenQuestion,
    OpenQuestionAttachment, OpenQuestionStatus, QuestionScope, ReopeningTrigger, TriggerFiring,
};
use kontor_core::receipt::AggregateRef;
use kontor_core::repository::{
    MiniProjectTopologySnapshot, NewMiniProject, NewProject, NewSeatBinding,
    NewSessionTopologyNode, OpenQuestionRepository, ProjectRepository, ProjectTopologyDefault,
    RepositoryError, TopologyRepository,
};
use kontor_core::spec::{
    CatalogRoleRef, Shareability, ShareabilityClass, ShareabilityTier, TopologySnapshot,
};
use kontor_profiles::bundled_operational_domain;
use kontor_store::backup::{
    ImportPlan, create_snapshot, export_realm, import_export, restore_snapshot,
};
use kontor_store::{SCHEMA_VERSION, SqliteStore};
use std::path::Path;
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn text(value: &str) -> BoundedText {
    BoundedText::parse(value).expect("bounded text")
}

fn trigger(key: &str) -> ReopeningTrigger {
    ReopeningTrigger {
        key: TriggerKey::parse(key).expect("a trigger key"),
        condition: text("the canonical mirror ships to production"),
    }
}

fn citation() -> DecisionCitation {
    DecisionCitation {
        record: AggregateRef::MiniProject {
            mini_project_id: MiniProjectId::generate(),
        },
        revision: ContentHash::of(b"the deciding revision"),
    }
}

/// A project with the bundled Operational topology, one epic and one ECP seat.
struct Fixture {
    home: TempDir,
    store: SqliteStore,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    seat_id: SeatBindingId,
    catalog_id: RoleCatalogId,
    catalog_version: SpecVersion,
}

impl Fixture {
    fn build() -> Self {
        let home = TempDir::new().expect("a temporary directory");
        let mut fixture = Self::open(home, "2026-08-19T09:00:00Z");
        fixture.seed();
        fixture
    }

    fn open(home: TempDir, _created: &str) -> Self {
        let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
        Self {
            home,
            store,
            project_id: ProjectId::generate(),
            mini_project_id: MiniProjectId::generate(),
            seat_id: SeatBindingId::generate(),
            catalog_id: RoleCatalogId::generate(),
            catalog_version: SpecVersion::parse(1).expect("a version"),
        }
    }

    fn seed(&mut self) {
        let created_at = at("2026-08-19T09:00:00Z");
        let stamp =
            Shareability::default_for(ShareabilityTier::ProjectKnowledge).expect("tier B stamp");
        self.store
            .create_project(&NewProject {
                id: self.project_id,
                name: name("Open question project"),
                root_path: name("/tmp/open-questions"),
                created_at,
            })
            .expect("the project is created");
        self.store
            .create_mini_project(&NewMiniProject {
                id: self.mini_project_id,
                project_id: self.project_id,
                name: name("Operational epic"),
                created_at,
            })
            .expect("the epic is created");

        let domain = bundled_operational_domain().expect("the bundled domain validates");
        let topology = domain.topology_specs.first().expect("a topology").clone();
        let catalog = domain.role_catalogs.first().expect("a catalog").clone();
        self.catalog_id = catalog.catalog_id;
        self.catalog_version = catalog.version;
        let canonical_hash = self
            .store
            .publish_topology_spec(self.project_id, &topology, &stamp, created_at)
            .expect("the topology is published");
        self.store
            .publish_role_catalog(&catalog, &stamp, created_at)
            .expect("the catalog is published");
        let snapshot = TopologySnapshot {
            spec_id: topology.spec_id,
            version: topology.version,
            canonical_hash,
        };
        self.store
            .set_project_topology_default(&ProjectTopologyDefault {
                project_id: self.project_id,
                topology: snapshot.clone(),
                selected_at: created_at,
            })
            .expect("the default is selected");
        self.store
            .pin_mini_project_topology(&MiniProjectTopologySnapshot {
                project_id: self.project_id,
                mini_project_id: self.mini_project_id,
                topology: snapshot.clone(),
                pinned_at: created_at,
            })
            .expect("the epic snapshot is pinned");

        let root_id = TopologyNodeId::generate();
        self.store
            .create_topology_node(&NewSessionTopologyNode {
                id: root_id,
                project_id: self.project_id,
                mini_project_id: None,
                topology: snapshot.clone(),
                kind: TopologyKindKey::parse("PSW").expect("the root kind"),
                parent_id: None,
                task_id: None,
                created_at,
            })
            .expect("the root node is created");
        let epic_node = TopologyNodeId::generate();
        self.store
            .create_topology_node(&NewSessionTopologyNode {
                id: epic_node,
                project_id: self.project_id,
                mini_project_id: Some(self.mini_project_id),
                topology: snapshot.clone(),
                kind: TopologyKindKey::parse("ESW").expect("the epic kind"),
                parent_id: Some(root_id),
                task_id: None,
                created_at,
            })
            .expect("the epic node is created");
        let ecp_node = TopologyNodeId::generate();
        self.store
            .create_topology_node(&NewSessionTopologyNode {
                id: ecp_node,
                project_id: self.project_id,
                mini_project_id: Some(self.mini_project_id),
                topology: snapshot,
                kind: TopologyKindKey::parse("ECP").expect("the control kind"),
                parent_id: Some(epic_node),
                task_id: None,
                created_at,
            })
            .expect("the control node is created");

        let entry = catalog
            .role(&RoleCode::parse("LSA").expect("a code"))
            .expect("the catalog has LSA")
            .clone();
        self.store
            .create_seat_binding(&NewSeatBinding {
                id: self.seat_id,
                project_id: self.project_id,
                topology_node_id: ecp_node,
                role_slot_id: RoleSlotId::parse("lead-software-architect").expect("a slot"),
                role: CatalogRoleRef {
                    catalog_id: self.catalog_id,
                    catalog_revision: self.catalog_version,
                    role_code: entry.role_code,
                    standard_title: entry.standard_title,
                    custom_display_name: None,
                },
                task_id: None,
                team_run_id: None,
                attach_deadline: at("2026-08-19T09:10:00Z"),
                parent_seat_binding_id: None,
                created_at,
            })
            .expect("the seat is created");
    }

    fn database(&self) -> std::path::PathBuf {
        self.home.path().join("kontor.db")
    }

    fn policy(&self) -> CloserPolicy {
        CloserPolicy {
            architecture_closer: RoleKey::parse("lead-software-architect").expect("a role key"),
            process_closer: RoleKey::parse("technical-program-manager").expect("a role key"),
        }
    }

    fn raise(&self) -> OpenQuestion {
        let question = OpenQuestion::raise(
            OpenQuestionId::generate(),
            self.project_id,
            self.mini_project_id,
            text("whether the mirror is authoritative"),
            QuestionScope::Architecture,
            OpenQuestionAttachment::Document(ContentHash::of(b"the plan revision")),
            self.seat_id,
            text("two documents disagree and neither cites the other"),
            vec![text("treat the mirror as authoritative"), text("refuse")],
            at("2026-08-19T09:05:00Z"),
        )
        .expect("a valid question");
        self.store
            .raise_question(self.project_id, &question)
            .expect("the question is stored");
        question
    }

    /// Close a question through the store, mirroring the aggregate's own append.
    fn dispose(&self, question: &mut OpenQuestion, outcome: DispositionOutcome) {
        let expected = question.revision;
        let ordinal = question
            .dispose(
                self.seat_id,
                &self.policy().architecture_closer,
                &self.policy(),
                outcome,
                None,
                at("2026-08-19T09:20:00Z"),
            )
            .expect("the closer may close");
        let disposition = question
            .dispositions
            .iter()
            .find(|entry| entry.ordinal == ordinal)
            .expect("the appended disposition")
            .clone();
        self.store
            .append_question_disposition(
                self.project_id,
                question.question_id,
                expected,
                &disposition,
            )
            .expect("the disposition is stored");
    }
}

fn raw(database: &Path) -> rusqlite::Connection {
    let connection = rusqlite::Connection::open(database).expect("the database opens");
    // Foreign keys are per-connection in SQLite. Without this a raw probe would
    // pass rows the daemon's own connection would refuse, and a test asserting a
    // refusal could be satisfied by the wrong rule entirely.
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("foreign keys are enforced on this probe");
    connection
}

// ---------------------------------------------------------------------------
// Migration inventory
// ---------------------------------------------------------------------------

#[test]
fn the_ledger_lands_in_the_declared_schema_generation() {
    let fixture = Fixture::build();
    let connection = raw(&fixture.database());
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the schema version reads");
    assert_eq!(
        version, SCHEMA_VERSION,
        "the ledger migration is part of this build's inventory"
    );
    for table in [
        "open_questions",
        "open_question_rounds",
        "open_question_dispositions",
        "open_question_trigger_firings",
    ] {
        let found: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                rusqlite::params![table],
                |row| row.get(0),
            )
            .expect("the table lookup runs");
        assert_eq!(found, 1, "{table} exists at this schema generation");
    }
}

// ---------------------------------------------------------------------------
// Schema immutability
// ---------------------------------------------------------------------------

#[test]
fn a_child_row_can_be_neither_updated_nor_deleted() {
    let fixture = Fixture::build();
    let mut question = fixture.raise();
    fixture.dispose(
        &mut question,
        DispositionOutcome::Deferred(trigger("canonical-mirror-shipped")),
    );
    let firing = TriggerFiring {
        ordinal: 1,
        disposition_ordinal: 1,
        trigger: TriggerKey::parse("canonical-mirror-shipped").expect("a key"),
        observed_by: fixture.seat_id,
        recorded_at: at("2026-08-19T09:30:00Z"),
    };
    fixture
        .store
        .fire_deferred_trigger(
            fixture.project_id,
            question.question_id,
            question.revision,
            &firing,
        )
        .expect("the firing is stored");

    let connection = raw(&fixture.database());
    for table in [
        "open_question_rounds",
        "open_question_dispositions",
        "open_question_trigger_firings",
    ] {
        assert!(
            connection
                .execute(&format!("UPDATE {table} SET ordinal = 99"), [])
                .is_err(),
            "{table} must refuse an in-place UPDATE"
        );
        assert!(
            connection
                .execute(&format!("DELETE FROM {table}"), [])
                .is_err(),
            "{table} must refuse a DELETE"
        );
    }
    assert!(
        connection
            .execute("DELETE FROM open_questions", [])
            .is_err(),
        "a question cannot be deleted"
    );
}

#[test]
fn only_the_revision_of_a_header_may_move() {
    let fixture = Fixture::build();
    let question = fixture.raise();
    let connection = raw(&fixture.database());

    assert!(
        connection
            .execute(
                "UPDATE open_questions SET subject = 'something else' WHERE question_id = ?1",
                rusqlite::params![question.question_id.to_string()],
            )
            .is_err(),
        "rewriting the subject is a rewrite wearing a smaller word"
    );
    assert!(
        connection
            .execute(
                "UPDATE open_questions SET shareability_class = 'kontor_local'
                 WHERE question_id = ?1",
                rusqlite::params![question.question_id.to_string()],
            )
            .is_err(),
        "the write-time classification is never revised"
    );
    connection
        .execute(
            "UPDATE open_questions SET revision = revision + 1 WHERE question_id = ?1",
            rusqlite::params![question.question_id.to_string()],
        )
        .expect("the revision is the one column that moves");
}

#[test]
fn a_deferral_without_a_trigger_and_a_firing_without_a_deferral_are_both_refused() {
    let fixture = Fixture::build();
    let question = fixture.raise();
    let connection = raw(&fixture.database());
    let params = rusqlite::params![
        fixture.project_id.to_string(),
        question.question_id.to_string(),
    ];

    assert!(
        connection
            .execute(
                "INSERT INTO open_question_dispositions
                     (project_id, question_id, ordinal, author_seat_id, kind, trigger_key,
                      payload, supersedes, recorded_at)
                 VALUES (?1, ?2, 1, (SELECT id FROM seat_bindings LIMIT 1), 'deferred', NULL,
                         '{}', NULL, '2026-08-19T09:20:00Z')",
                params,
            )
            .is_err(),
        "a deferral must name the trigger that reopens it"
    );
    assert!(
        connection
            .execute(
                "INSERT INTO open_question_dispositions
                     (project_id, question_id, ordinal, author_seat_id, kind, trigger_key,
                      payload, supersedes, recorded_at)
                 VALUES (?1, ?2, 1, (SELECT id FROM seat_bindings LIMIT 1), 'resolved',
                         'a-trigger', '{}', NULL, '2026-08-19T09:20:00Z')",
                params,
            )
            .is_err(),
        "only a deferral names a trigger"
    );
}

#[test]
fn a_firing_that_names_the_wrong_trigger_is_refused_by_the_schema() {
    // Isolating the trigger rule takes a deferral that *exists*: a firing against
    // a missing disposition is refused by the foreign key, which would leave this
    // rule untested while looking tested. Here disposition 1 is a real deferral on
    // one trigger, and the firing names a different one — so only
    // `open_question_firing_matches_its_deferral` can refuse it.
    let fixture = Fixture::build();
    let mut question = fixture.raise();
    fixture.dispose(
        &mut question,
        DispositionOutcome::Deferred(trigger("canonical-mirror-shipped")),
    );
    let connection = raw(&fixture.database());

    assert!(
        connection
            .execute(
                "INSERT INTO open_question_trigger_firings
                     (project_id, question_id, ordinal, disposition_ordinal, trigger_key,
                      observed_by_seat_id, recorded_at)
                 VALUES (?1, ?2, 1, 1, 'a-completely-different-trigger', ?3,
                         '2026-08-19T09:30:00Z')",
                rusqlite::params![
                    fixture.project_id.to_string(),
                    question.question_id.to_string(),
                    fixture.seat_id.to_string(),
                ],
            )
            .is_err(),
        "a firing must name the exact trigger its deferral deferred on"
    );

    // The same insert naming the deferral's own trigger is accepted, which is what
    // proves the refusal above was the trigger rule and not a blanket rejection.
    connection
        .execute(
            "INSERT INTO open_question_trigger_firings
                 (project_id, question_id, ordinal, disposition_ordinal, trigger_key,
                  observed_by_seat_id, recorded_at)
             VALUES (?1, ?2, 1, 1, 'canonical-mirror-shipped', ?3, '2026-08-19T09:30:00Z')",
            rusqlite::params![
                fixture.project_id.to_string(),
                question.question_id.to_string(),
                fixture.seat_id.to_string(),
            ],
        )
        .expect("the exact trigger its deferral named is accepted");
}

// ---------------------------------------------------------------------------
// Isolation and concurrency
// ---------------------------------------------------------------------------

#[test]
fn a_question_of_another_project_does_not_resolve() {
    let fixture = Fixture::build();
    let question = fixture.raise();
    let stranger = ProjectId::generate();

    assert!(
        fixture
            .store
            .get_question(stranger, question.question_id)
            .expect("the read succeeds")
            .is_none(),
        "a valid id from another project is not tenant isolation"
    );
    assert!(
        fixture
            .store
            .list_questions_for_epic(stranger, fixture.mini_project_id)
            .expect("the read succeeds")
            .is_empty()
    );
    assert!(
        matches!(
            fixture.store.append_question_round(
                stranger,
                question.question_id,
                question.revision,
                &AmbiguityRound {
                    ordinal: 2,
                    author: fixture.seat_id,
                    why_ambiguous: text("a later reading"),
                    options: vec![text("an option")],
                    supersedes: None,
                    recorded_at: at("2026-08-19T09:40:00Z"),
                },
            ),
            Err(RepositoryError::Conflict { .. } | RepositoryError::Domain(_))
        ),
        "an append scoped to the wrong project writes nothing"
    );
}

#[test]
fn an_append_from_a_stale_revision_is_refused() {
    let fixture = Fixture::build();
    let mut question = fixture.raise();
    fixture.dispose(
        &mut question,
        DispositionOutcome::NotRelevant(text("the surface was withdrawn")),
    );

    // The first append moved the head, so the revision the caller started with
    // is now stale.
    let stale = kontor_core::id::AggregateRevision::INITIAL;
    let refusal = fixture
        .store
        .append_question_round(
            fixture.project_id,
            question.question_id,
            stale,
            &AmbiguityRound {
                ordinal: 2,
                author: fixture.seat_id,
                why_ambiguous: text("a later reading"),
                options: vec![text("an option")],
                supersedes: None,
                recorded_at: at("2026-08-19T09:40:00Z"),
            },
        )
        .expect_err("a stale caller writes nothing");
    assert!(matches!(refusal, RepositoryError::Conflict { .. }));

    let stored = fixture
        .store
        .get_question(fixture.project_id, question.question_id)
        .expect("the read succeeds")
        .expect("the question is there");
    assert_eq!(
        stored.rounds.len(),
        1,
        "the refused append left no round behind"
    );
}

#[test]
fn only_the_current_deferral_reopens_and_it_reopens_once() {
    let fixture = Fixture::build();
    let mut question = fixture.raise();
    fixture.dispose(
        &mut question,
        DispositionOutcome::Deferred(trigger("canonical-mirror-shipped")),
    );
    let key = TriggerKey::parse("canonical-mirror-shipped").expect("a key");
    let firing = TriggerFiring {
        ordinal: 1,
        disposition_ordinal: 1,
        trigger: key.clone(),
        observed_by: fixture.seat_id,
        recorded_at: at("2026-08-19T09:30:00Z"),
    };
    let next = fixture
        .store
        .fire_deferred_trigger(
            fixture.project_id,
            question.question_id,
            question.revision,
            &firing,
        )
        .expect("the current deferral reopens");

    let second = TriggerFiring {
        ordinal: 2,
        ..firing
    };
    assert!(
        fixture
            .store
            .fire_deferred_trigger(fixture.project_id, question.question_id, next, &second)
            .is_err(),
        "one deferral reopens once"
    );

    let stored = fixture
        .store
        .get_question(fixture.project_id, question.question_id)
        .expect("the read succeeds")
        .expect("the question is there");
    assert_eq!(stored.status(), OpenQuestionStatus::Reopened);
    assert_eq!(stored.firings.len(), 1);
}

// ---------------------------------------------------------------------------
// Restart, export and snapshot round trips
// ---------------------------------------------------------------------------

/// One question carrying every kind of child row, so nothing can be lost
/// silently by a round trip that only preserves the common case.
fn full_history(fixture: &Fixture) -> OpenQuestion {
    let mut question = fixture.raise();
    let expected = question.revision;
    let ordinal = question
        .append_round(
            fixture.seat_id,
            text("the earlier reading missed the tenant column"),
            vec![text("scope the read by project")],
            Some(1),
            at("2026-08-19T09:10:00Z"),
        )
        .expect("the correction appends");
    let round = question
        .rounds
        .iter()
        .find(|entry| entry.ordinal == ordinal)
        .expect("the appended round")
        .clone();
    fixture
        .store
        .append_question_round(fixture.project_id, question.question_id, expected, &round)
        .expect("the round is stored");

    fixture.dispose(
        &mut question,
        DispositionOutcome::Deferred(trigger("canonical-mirror-shipped")),
    );
    let firing = TriggerFiring {
        ordinal: 1,
        disposition_ordinal: 1,
        trigger: TriggerKey::parse("canonical-mirror-shipped").expect("a key"),
        observed_by: fixture.seat_id,
        recorded_at: at("2026-08-19T09:30:00Z"),
    };
    let revision = fixture
        .store
        .fire_deferred_trigger(
            fixture.project_id,
            question.question_id,
            question.revision,
            &firing,
        )
        .expect("the firing is stored");
    question
        .fire_trigger(&firing.trigger, fixture.seat_id, firing.recorded_at)
        .expect("the aggregate agrees");

    // Then a superseding resolution on top of the reopened deferral.
    let disposition = Disposition {
        ordinal: 2,
        author: fixture.seat_id,
        outcome: DispositionOutcome::Resolved(citation()),
        supersedes: Some(1),
        recorded_at: at("2026-08-19T09:40:00Z"),
    };
    fixture
        .store
        .append_question_disposition(
            fixture.project_id,
            question.question_id,
            revision,
            &disposition,
        )
        .expect("the correction is stored");

    fixture
        .store
        .get_question(fixture.project_id, question.question_id)
        .expect("the read succeeds")
        .expect("the question is there")
}

fn assert_history_intact(stored: &OpenQuestion) {
    assert_eq!(stored.rounds.len(), 2, "both rounds survived");
    assert_eq!(stored.rounds[1].supersedes, Some(1));
    assert_eq!(stored.dispositions.len(), 2, "both dispositions survived");
    assert_eq!(stored.dispositions[1].supersedes, Some(1));
    assert!(
        matches!(
            stored.dispositions[0].outcome,
            DispositionOutcome::Deferred(_)
        ),
        "the superseded deferral is still readable as a deferral"
    );
    assert_eq!(stored.firings.len(), 1, "the trigger firing survived");
    assert_eq!(
        stored.shareability.class,
        ShareabilityClass::ProjectShared,
        "the write-time stamp survived"
    );
    assert_eq!(stored.status(), OpenQuestionStatus::Resolved);
}

#[test]
fn every_round_disposition_and_firing_survives_a_restart() {
    let fixture = Fixture::build();
    let expected = full_history(&fixture);
    assert_history_intact(&expected);

    let reopened = SqliteStore::open(&fixture.database()).expect("the store reopens");
    let stored = reopened
        .get_question(fixture.project_id, expected.question_id)
        .expect("the read succeeds")
        .expect("the question survived the restart");
    assert_eq!(stored, expected, "the aggregate is byte-for-byte the same");
    assert_history_intact(&stored);
}

#[test]
fn the_ledger_exports_deterministically_and_completely() {
    let fixture = Fixture::build();
    let question = full_history(&fixture);

    let first = export_realm(&fixture.store, at("2026-08-19T10:00:00Z")).expect("the export runs");
    let second = export_realm(&fixture.store, at("2026-08-19T11:00:00Z")).expect("the export runs");
    assert_eq!(
        first.records_hash, second.records_hash,
        "two exports of one unchanged ledger hash identically"
    );

    let records = &first.records;
    assert_eq!(records.open_questions.len(), 1);
    assert_eq!(records.open_question_rounds.len(), 2);
    assert_eq!(records.open_question_dispositions.len(), 2);
    assert_eq!(records.open_question_trigger_firings.len(), 1);

    let exported = &records.open_questions[0];
    assert_eq!(exported.question_id, question.question_id.to_string());
    assert_eq!(exported.shareability_class, "project_shared");
    assert_eq!(exported.shareability_provenance, "type_default");
    assert!(
        records
            .open_question_dispositions
            .iter()
            .any(|row| row.kind == "deferred" && row.trigger_key.is_some()),
        "the deferral exports the trigger it parked on"
    );

    // The document round-trips through its own parser, digest check included.
    let bytes = first.canonical_bytes().expect("the document renders");
    let parsed = kontor_store::backup::KontorExportV1::parse(&bytes).expect("it parses back");
    assert_eq!(parsed.records, first.records);
}

#[test]
fn the_ledger_survives_a_snapshot_restore() {
    let fixture = Fixture::build();
    let expected = full_history(&fixture);

    let outcome = create_snapshot(
        &fixture.database(),
        &fixture.home.path().join("backups"),
        at("2026-08-19T12:00:00Z"),
    )
    .expect("the snapshot is taken");

    let destination = fixture.home.path().join("restored").join("kontor.db");
    restore_snapshot(&outcome.snapshot, &destination, at("2026-08-19T12:05:00Z"))
        .expect("the snapshot restores");

    let restored = SqliteStore::open(&destination).expect("the restored database opens as a store");
    let stored = restored
        .get_question(fixture.project_id, expected.question_id)
        .expect("the read succeeds")
        .expect("the question survived the restore");
    assert_eq!(stored, expected);
    assert_history_intact(&stored);
}

#[test]
fn an_import_records_every_ledger_row_as_lineage() {
    // The import path materializes versioned specifications only; open questions
    // are project *state*, so the honest outcome is a recorded lineage entry per
    // row rather than a second realm's live ledger. This asserts the four kinds
    // reach that lineage, which is what makes `import.rs` correct by omission
    // rather than merely unmodified.
    let fixture = Fixture::build();
    full_history(&fixture);
    let export = export_realm(&fixture.store, at("2026-08-19T13:00:00Z")).expect("the export runs");

    let home = TempDir::new().expect("a temporary directory");
    let destination =
        SqliteStore::open(&home.path().join("kontor.db")).expect("the destination migrates");
    let into = ProjectId::generate();
    destination
        .create_project(&NewProject {
            id: into,
            name: name("Destination project"),
            root_path: name("/tmp/open-questions-destination"),
            created_at: at("2026-08-19T13:05:00Z"),
        })
        .expect("the destination project exists");

    let report = import_export(
        &destination,
        &export,
        &ImportPlan::redacted_import_into(into),
        at("2026-08-19T13:10:00Z"),
    )
    .expect("the export is imported");

    let recorded = destination
        .imported_records(&report.import_id.to_string())
        .expect("the lineage is readable");
    for (kind, expected) in [
        ("open_questions", 1),
        ("open_question_rounds", 2),
        ("open_question_dispositions", 2),
        ("open_question_trigger_firings", 1),
    ] {
        let found = recorded
            .iter()
            .filter(|row| row.record_kind == kind)
            .count();
        assert_eq!(found, expected, "{kind} is accounted for in the lineage");
    }
    assert!(
        recorded
            .iter()
            .filter(|row| row.record_kind.starts_with("open_question"))
            .all(|row| row.disposition == "recorded"),
        "a question is recorded, never materialized into another realm's live ledger"
    );
}
