//! ASMA-8193: a hosted leadership seat's authority belongs to its occupancy
//! generation.
//!
//! The candidate this suite exists to refuse resolved autonomy correctly at
//! launch and then recomputed it from the live plane default on every later
//! control operation. Because `permission_posture` is mutable configuration,
//! flipping it made `inspect` and `retire` compare a freshly computed value
//! against a native launched under the previous one — and the resulting
//! mismatch refuses the retire *before* the predecessor is archived, wedging
//! the one governed path that could install a correctly-routed successor.
//!
//! Every test below is written so the persisted implementation and the
//! recomputing one give *different* answers. A test that still passes when
//! autonomy is re-resolved from configuration proves nothing about which is
//! running, so each one moves the "plane default" after the seat exists.

use kontor_core::id::{
    ExternalId, ExternalName, MiniProjectId, ProjectId, RoleCode, RoleSlotId, RuntimeKindKey,
    SeatBindingId, Timestamp, TopologyKindKey, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::repository::{
    HostedSeatLaunchIntentState, MiniProjectTopologySnapshot, NewMiniProject, NewProject,
    NewSeatBinding, NewSessionTopologyNode, ProjectRepository, ProjectTopologyDefault,
    StoredHostedSeatLaunchIntent, StoredHostedTopologySeat, TopologyRepository,
};
use kontor_core::spec::{
    CatalogRoleRef, ModelRef, ModelRung, ProviderRef, SeatAutonomy, Shareability, ShareabilityTier,
    TopologySnapshot,
};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use rusqlite::Connection;
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical timestamp")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a bounded name")
}

fn rung() -> ModelRung {
    ModelRung {
        provider: ProviderRef("codex".to_owned()),
        model: ModelRef("gpt-5.6".to_owned()),
        effort: None,
    }
}

fn identity(native_id: &str, generation: u64) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("a runtime kind"),
        host: name("paseo-local"),
        generation,
        native_id: ExternalId::parse(native_id).expect("a native id"),
    }
}

struct Fixture {
    _home: TempDir,
    db_path: std::path::PathBuf,
    store: SqliteStore,
    project_id: ProjectId,
    ecp_id: TopologyNodeId,
    catalog_id: kontor_core::id::RoleCatalogId,
    catalog_version: kontor_core::id::SpecVersion,
}

impl Fixture {
    fn build() -> Self {
        let home = TempDir::new().expect("a temporary directory");
        let db_path = home.path().join("kontor.db");
        let store = SqliteStore::open(&db_path).expect("the store opens");
        let project_id = ProjectId::generate();
        let mini_project_id = MiniProjectId::generate();
        let created_at = at("2026-09-17T01:00:00Z");
        let stamp = Shareability::default_for(ShareabilityTier::ProjectKnowledge)
            .expect("tier B classifies");

        store
            .create_project(&NewProject {
                id: project_id,
                name: name("Autonomy project"),
                root_path: name("/tmp/hosted-seat-autonomy"),
                created_at,
            })
            .expect("the project is created");
        store
            .create_mini_project(&NewMiniProject {
                id: mini_project_id,
                project_id,
                name: name("Autonomy epic"),
                created_at,
            })
            .expect("the MiniProject is created");

        let domain = bundled_operational_domain().expect("the bundled domain validates");
        let topology = domain.topology_specs.first().expect("a topology").clone();
        let catalog = domain.role_catalogs.first().expect("a catalog").clone();
        let canonical_hash = store
            .publish_topology_spec(project_id, &topology, &stamp, created_at)
            .expect("the topology is published");
        store
            .publish_role_catalog(&catalog, &stamp, created_at)
            .expect("the catalog is published");
        let snapshot = TopologySnapshot {
            spec_id: topology.spec_id,
            version: topology.version,
            canonical_hash,
        };
        store
            .set_project_topology_default(&ProjectTopologyDefault {
                project_id,
                topology: snapshot.clone(),
                selected_at: created_at,
            })
            .expect("the project default is selected");
        store
            .pin_mini_project_topology(&MiniProjectTopologySnapshot {
                project_id,
                mini_project_id,
                topology: snapshot.clone(),
                pinned_at: created_at,
            })
            .expect("the MiniProject snapshot is pinned");

        let root_id = TopologyNodeId::generate();
        store
            .create_topology_node(&NewSessionTopologyNode {
                id: root_id,
                project_id,
                mini_project_id: None,
                topology: snapshot.clone(),
                kind: TopologyKindKey::parse("PSW").expect("the root kind"),
                parent_id: None,
                task_id: None,
                created_at,
            })
            .expect("the project root is created");
        let epic_id = TopologyNodeId::generate();
        store
            .create_topology_node(&NewSessionTopologyNode {
                id: epic_id,
                project_id,
                mini_project_id: Some(mini_project_id),
                topology: snapshot.clone(),
                kind: TopologyKindKey::parse("ESW").expect("the epic kind"),
                parent_id: Some(root_id),
                task_id: None,
                created_at,
            })
            .expect("the epic node is created");
        let ecp_id = TopologyNodeId::generate();
        store
            .create_topology_node(&NewSessionTopologyNode {
                id: ecp_id,
                project_id,
                mini_project_id: Some(mini_project_id),
                topology: snapshot,
                kind: TopologyKindKey::parse("ECP").expect("the control-plane kind"),
                parent_id: Some(epic_id),
                task_id: None,
                created_at,
            })
            .expect("the control-plane node is created");

        Self {
            _home: home,
            db_path,
            store,
            project_id,
            ecp_id,
            catalog_id: catalog.catalog_id,
            catalog_version: catalog.version,
        }
    }

    fn role(&self, code: &str) -> CatalogRoleRef {
        let domain = bundled_operational_domain().expect("the bundled domain validates");
        let catalog = domain.role_catalogs.first().expect("a catalog").clone();
        let entry = catalog
            .role(&RoleCode::parse(code).expect("a standard role code"))
            .expect("the catalog has the role")
            .clone();
        CatalogRoleRef {
            catalog_id: self.catalog_id,
            catalog_revision: self.catalog_version,
            role_code: entry.role_code,
            standard_title: entry.standard_title,
            custom_display_name: None,
        }
    }

    fn seat(&self, code: &str, slot: &str) -> SeatBindingId {
        let id = SeatBindingId::generate();
        self.store
            .create_seat_binding(&NewSeatBinding {
                id,
                project_id: self.project_id,
                topology_node_id: self.ecp_id,
                role_slot_id: RoleSlotId::parse(slot).expect("a role slot"),
                role: self.role(code),
                task_id: None,
                team_run_id: None,
                attach_deadline: at("2026-09-17T02:00:00Z"),
                parent_seat_binding_id: None,
                created_at: at("2026-09-17T01:00:00Z"),
            })
            .expect("the seat binding is created");
        id
    }

    fn lsa(&self, autonomy: SeatAutonomy) -> (SeatBindingId, StoredHostedTopologySeat) {
        let seat = self.seat("LSA", "epic.lsa");
        let hosted = StoredHostedTopologySeat {
            project_id: self.project_id,
            seat_binding_id: seat,
            model_rung: rung(),
            native_identity: identity("lsa-first", 1),
            autonomy,
            provider_session_id: None,
            observed_at: at("2026-09-17T01:01:00Z"),
        };
        self.store
            .bind_hosted_topology_seat(&hosted)
            .expect("the hosted seat is bound");
        (seat, hosted)
    }
}

// ---------------------------------------------------------------------------
// Store round trip
// ---------------------------------------------------------------------------

/// The value a generation was launched under survives the store.
///
/// `Bounded` is chosen deliberately: it is not [`SeatAutonomy::standard`], so a
/// reader that returns the fallback — or one that recomputes a default — gives
/// a different answer than a reader that reads the column.
#[test]
fn a_hosted_seat_reads_back_the_authority_it_was_launched_under() {
    let fixture = Fixture::build();
    let (seat, _) = fixture.lsa(SeatAutonomy::Bounded);

    let read = fixture
        .store
        .get_hosted_topology_seat(fixture.project_id, seat)
        .expect("the hosted seat reads")
        .expect("the hosted seat exists");

    assert_eq!(
        read.autonomy,
        SeatAutonomy::Bounded,
        "the persisted authority is the one the seat was launched under"
    );
}

/// Re-binding the identical generation is the lost-acknowledgement path: the
/// launch happened, the acknowledgement did not arrive, and the command is
/// replayed. It must converge on the same row, not a second authority.
#[test]
fn replaying_an_unacknowledged_launch_is_idempotent() {
    let fixture = Fixture::build();
    let (seat, hosted) = fixture.lsa(SeatAutonomy::Bounded);

    let refreshed = StoredHostedTopologySeat {
        provider_session_id: Some(ExternalId::parse("thread-1").expect("a session id")),
        observed_at: at("2026-09-17T01:05:00Z"),
        ..hosted
    };
    fixture
        .store
        .bind_hosted_topology_seat(&refreshed)
        .expect("the same generation refreshes in place");

    let read = fixture
        .store
        .get_hosted_topology_seat(fixture.project_id, seat)
        .expect("the hosted seat reads")
        .expect("the hosted seat exists");
    assert_eq!(
        read.autonomy,
        SeatAutonomy::Bounded,
        "a replayed launch keeps the authority the generation already had"
    );
    assert_eq!(read.observed_at, at("2026-09-17T01:05:00Z"));
}

/// The plane default moved between the launch and the replay. Re-binding the
/// same native under a *different* authority is not a refresh — it is an
/// attempt to change what a live generation may do without retiring it.
#[test]
fn a_live_generation_cannot_change_its_authority_in_place() {
    let fixture = Fixture::build();
    let (_, hosted) = fixture.lsa(SeatAutonomy::Supervised);

    let widened = StoredHostedTopologySeat {
        autonomy: SeatAutonomy::Bounded,
        ..hosted
    };
    let refusal = fixture
        .store
        .bind_hosted_topology_seat(&widened)
        .expect_err("a live generation cannot be re-bound under a new authority");

    assert!(
        refusal.to_string().contains("autonomy"),
        "the refusal names what actually differed: {refusal:?}"
    );
}

// ---------------------------------------------------------------------------
// Retirement and replacement
// ---------------------------------------------------------------------------

/// A retired generation keeps its own authority in history. An audit that had
/// to infer a predecessor's authority from whatever configuration is live has
/// no evidence at all.
#[test]
fn history_retains_the_authority_the_retired_generation_ran_under() {
    let fixture = Fixture::build();
    let (seat, hosted) = fixture.lsa(SeatAutonomy::Bounded);

    fixture
        .store
        .archive_hosted_topology_seat_route(&hosted, at("2026-09-17T02:00:00Z"), "route correction")
        .expect("the predecessor is archived");

    let archived = fixture
        .store
        .get_hosted_topology_seat_history(
            fixture.project_id,
            seat,
            &ExternalId::parse("lsa-first").expect("a native id"),
        )
        .expect("history reads")
        .expect("the predecessor is in history");
    assert_eq!(
        archived.autonomy,
        SeatAutonomy::Bounded,
        "the retired generation is remembered as what it actually was"
    );
}

/// The whole point of option A: a changed plane default reaches a seat only as
/// a *successor* generation, and the predecessor keeps what it ran under.
///
/// This is the test the recomputing implementation cannot pass. Under it both
/// rows would report whatever the live default currently is, so the assertion
/// that the two generations differ is the one that dies.
#[test]
fn a_changed_default_applies_to_the_successor_and_not_the_predecessor() {
    let fixture = Fixture::build();
    let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);

    // The operator flips `permission_posture` and drives the audited
    // retire/replace path. Only the new generation may see the new value.
    let successor = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 2),
        autonomy: SeatAutonomy::Bounded,
        observed_at: at("2026-09-17T02:05:00Z"),
        ..predecessor.clone()
    };
    fixture
        .store
        .replace_hosted_topology_seat_route(
            &predecessor,
            &successor,
            at("2026-09-17T02:04:00Z"),
            "authorized Core Team provider/model route correction",
        )
        .expect("the route is replaced");

    let active = fixture
        .store
        .get_hosted_topology_seat(fixture.project_id, seat)
        .expect("the active seat reads")
        .expect("a successor is active");
    let archived = fixture
        .store
        .get_hosted_topology_seat_history(
            fixture.project_id,
            seat,
            &ExternalId::parse("lsa-first").expect("a native id"),
        )
        .expect("history reads")
        .expect("the predecessor is in history");

    assert_eq!(
        active.autonomy,
        SeatAutonomy::Bounded,
        "the successor generation carries the new default"
    );
    assert_eq!(
        archived.autonomy,
        SeatAutonomy::Supervised,
        "the predecessor keeps the authority it actually ran under"
    );
    assert_ne!(
        active.autonomy, archived.autonomy,
        "a configuration change must be visible as a generation boundary, not \
         rewritten across every row"
    );
}

/// Archival compares the whole predecessor, autonomy included. A caller that
/// describes the seat it is retiring incorrectly is refused rather than
/// silently archiving a row it did not actually read.
#[test]
fn archival_refuses_a_predecessor_whose_authority_does_not_match() {
    let fixture = Fixture::build();
    let (_, hosted) = fixture.lsa(SeatAutonomy::Supervised);

    let misdescribed = StoredHostedTopologySeat {
        autonomy: SeatAutonomy::Bounded,
        ..hosted
    };
    let refusal = fixture
        .store
        .archive_hosted_topology_seat_route(
            &misdescribed,
            at("2026-09-17T02:00:00Z"),
            "route correction",
        )
        .expect_err("a mismatched predecessor is refused");
    assert!(
        refusal.to_string().contains("differs"),
        "the refusal is the predecessor-mismatch conflict: {refusal:?}"
    );
}

// ---------------------------------------------------------------------------
// Migration
// ---------------------------------------------------------------------------

/// Pre-feature rows carry no autonomy, because no schema before v99 recorded
/// one. Every such row was written by a build whose hosted create passed a
/// hardcoded `Supervised`, so that is what they migrate to — and it is also the
/// least authority the domain has, which is what makes the backfill safe rather
/// than merely convenient.
#[test]
fn pre_feature_hosted_rows_migrate_to_the_least_authority() {
    let connection = Connection::open_in_memory().expect("an in-memory database");
    // What `migrate` itself does: reference enforcement is lifted for the
    // duration, because a rebuild necessarily leaves child rows pointing at a
    // table that does not exist for the space of two statements.
    connection
        .pragma_update(None, "foreign_keys", false)
        .expect("reference enforcement is lifted for the rebuild");
    connection
        .execute_batch(
            "CREATE TABLE projects (id TEXT NOT NULL PRIMARY KEY) STRICT;
             CREATE TABLE seat_bindings (id TEXT NOT NULL PRIMARY KEY) STRICT;
             CREATE TABLE hosted_topology_seats (
                 seat_binding_id     TEXT NOT NULL PRIMARY KEY,
                 project_id          TEXT NOT NULL,
                 model_rung          TEXT NOT NULL CHECK (json_valid(model_rung)),
                 runtime_kind        TEXT NOT NULL,
                 host                TEXT NOT NULL,
                 generation          INTEGER NOT NULL CHECK (generation >= 0),
                 native_id           TEXT NOT NULL,
                 provider_session_id TEXT NULL,
                 observed_at         TEXT NOT NULL,
                 UNIQUE (runtime_kind, host, generation, native_id)
             ) STRICT;
             CREATE INDEX ix_hosted_topology_seats_project
                 ON hosted_topology_seats(project_id, seat_binding_id);
             CREATE TABLE hosted_topology_seat_history (
                 seat_binding_id     TEXT NOT NULL,
                 project_id          TEXT NOT NULL,
                 generation          INTEGER NOT NULL CHECK (generation >= 0),
                 model_rung          TEXT NOT NULL CHECK (json_valid(model_rung)),
                 runtime_kind        TEXT NOT NULL,
                 host                TEXT NOT NULL,
                 native_id           TEXT NOT NULL,
                 provider_session_id TEXT NULL,
                 observed_at         TEXT NOT NULL,
                 retired_at          TEXT NOT NULL,
                 retirement_reason   TEXT NOT NULL,
                 PRIMARY KEY (project_id, seat_binding_id, native_id),
                 UNIQUE (runtime_kind, host, generation, native_id)
             ) STRICT;
             INSERT INTO hosted_topology_seats VALUES
                 ('seat-1', 'project-1', '{\"provider\":\"codex\"}', 'paseo.agent',
                  'paseo-local', 1, 'lsa-live', NULL, '2026-09-16T00:00:00Z');
             INSERT INTO hosted_topology_seat_history VALUES
                 ('seat-1', 'project-1', 0, '{\"provider\":\"codex\"}', 'paseo.agent',
                  'paseo-local', 'lsa-retired', NULL, '2026-09-15T00:00:00Z',
                  '2026-09-15T01:00:00Z', 'earlier correction');
             PRAGMA user_version = 98;",
        )
        .expect("the v98 hosted-seat shape is seeded");

    connection
        .execute_batch(include_str!(
            "../migrations/0102_hosted_seat_autonomy_generation.sql"
        ))
        .expect("v102 migrates the hosted-seat shape");

    let active: String = connection
        .query_row(
            "SELECT autonomy FROM hosted_topology_seats WHERE native_id = 'lsa-live'",
            [],
            |row| row.get(0),
        )
        .expect("the active row survives the rebuild");
    let retired: String = connection
        .query_row(
            "SELECT autonomy FROM hosted_topology_seat_history WHERE native_id = 'lsa-retired'",
            [],
            |row| row.get(0),
        )
        .expect("the history row survives the rebuild");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the schema version");

    assert_eq!(active, "supervised", "a pre-feature active row is narrowed");
    assert_eq!(
        retired, "supervised",
        "a pre-feature history row is narrowed"
    );
    assert_eq!(version, 99);
}

/// Fail closed. The column admits exactly the three spellings `SeatAutonomy`
/// serializes; a row carrying anything else aborts rather than being loaded
/// under an authority no evidence supports.
#[test]
fn a_contradictory_authority_is_refused_by_the_schema() {
    let fixture = Fixture::build();
    let (seat, _) = fixture.lsa(SeatAutonomy::Supervised);

    let connection = Connection::open(&fixture.db_path).expect("a second connection opens");
    let refusal = connection
        .execute(
            "UPDATE hosted_topology_seats SET autonomy = 'unrestricted'
             WHERE seat_binding_id = ?1",
            [seat.to_string()],
        )
        .expect_err("an unknown authority is refused by the column's CHECK");

    assert!(
        refusal.to_string().contains("CHECK"),
        "the schema itself refuses it: {refusal:?}"
    );
}

// ---------------------------------------------------------------------------
// Pre-effect launch intent
// ---------------------------------------------------------------------------

/// Build one prepared intent for a seat's first occupancy.
fn intent(
    fixture: &Fixture,
    seat: SeatBindingId,
    autonomy: SeatAutonomy,
) -> StoredHostedSeatLaunchIntent {
    StoredHostedSeatLaunchIntent {
        project_id: fixture.project_id,
        seat_binding_id: seat,
        occupancy_generation: 1,
        autonomy,
        model_rung: rung(),
        state: HostedSeatLaunchIntentState::Prepared,
        observed_native_id: None,
        prepared_at: at("2026-09-17T01:00:30Z"),
        installed_at: None,
    }
}

/// The window the verification gate rejected: a native exists, its occupancy
/// does not, and the only durable record of what it was launched under is this.
#[test]
fn a_prepared_intent_holds_the_authority_before_any_occupancy_exists() {
    let fixture = Fixture::build();
    let seat = fixture.seat("LSA", "epic.lsa");

    fixture
        .store
        .prepare_hosted_seat_launch_intent(&intent(&fixture, seat, SeatAutonomy::Bounded))
        .expect("the intent is recorded before the native call");

    assert!(
        fixture
            .store
            .get_hosted_topology_seat(fixture.project_id, seat)
            .expect("the occupancy reads")
            .is_none(),
        "the occupancy must not exist yet -- that is the window under test"
    );
    let recorded = fixture
        .store
        .get_hosted_seat_launch_intent(fixture.project_id, seat, 1)
        .expect("the intent reads")
        .expect("the intent exists");
    assert_eq!(recorded.autonomy, SeatAutonomy::Bounded);
    assert_eq!(recorded.state, HostedSeatLaunchIntentState::Prepared);
    assert!(recorded.observed_native_id.is_none());
}

/// Replaying the same launch converges on the row it already wrote.
#[test]
fn preparing_the_same_launch_twice_is_one_intent() {
    let fixture = Fixture::build();
    let seat = fixture.seat("LSA", "epic.lsa");
    let first = intent(&fixture, seat, SeatAutonomy::Bounded);

    fixture
        .store
        .prepare_hosted_seat_launch_intent(&first)
        .expect("the first attempt records the intent");
    fixture
        .store
        .prepare_hosted_seat_launch_intent(&StoredHostedSeatLaunchIntent {
            prepared_at: at("2026-09-17T01:09:00Z"),
            ..first
        })
        .expect("replaying the same decision is unchanged");

    let recorded = fixture
        .store
        .get_hosted_seat_launch_intent(fixture.project_id, seat, 1)
        .expect("the intent reads")
        .expect("the intent exists");
    assert_eq!(
        recorded.prepared_at,
        at("2026-09-17T01:00:30Z"),
        "a replay keeps the instant the decision was actually made"
    );
}

/// No silent escalation. The native this intent describes may already exist,
/// and it cannot be re-created under a wider authority than it was started
/// with, so a second resolution for the same generation is refused rather than
/// allowed to overwrite the first.
#[test]
fn one_generation_cannot_be_prepared_under_a_second_authority() {
    let fixture = Fixture::build();
    let seat = fixture.seat("LSA", "epic.lsa");
    fixture
        .store
        .prepare_hosted_seat_launch_intent(&intent(&fixture, seat, SeatAutonomy::Supervised))
        .expect("the first decision is recorded");

    let refusal = fixture
        .store
        .prepare_hosted_seat_launch_intent(&intent(&fixture, seat, SeatAutonomy::Bounded))
        .expect_err("a second authority for one generation is refused");

    assert!(
        refusal.to_string().contains("second autonomy"),
        "the refusal names the escalation it stopped: {refusal:?}"
    );
    assert_eq!(
        fixture
            .store
            .get_hosted_seat_launch_intent(fixture.project_id, seat, 1)
            .expect("the intent reads")
            .expect("the intent exists")
            .autonomy,
        SeatAutonomy::Supervised,
        "the original decision survives the attempt to widen it"
    );
}

/// The occupancy consumes the intent and names the native it produced.
#[test]
fn binding_an_occupancy_reconciles_its_intent() {
    let fixture = Fixture::build();
    let seat = fixture.seat("LSA", "epic.lsa");
    fixture
        .store
        .prepare_hosted_seat_launch_intent(&intent(&fixture, seat, SeatAutonomy::Bounded))
        .expect("the intent is recorded");
    let native = ExternalId::parse("lsa-first").expect("a native id");

    fixture
        .store
        .install_hosted_seat_launch_intent(
            fixture.project_id,
            seat,
            1,
            &native,
            at("2026-09-17T01:02:00Z"),
        )
        .expect("the intent is reconciled");

    let recorded = fixture
        .store
        .get_hosted_seat_launch_intent(fixture.project_id, seat, 1)
        .expect("the intent reads")
        .expect("the intent exists");
    assert_eq!(recorded.state, HostedSeatLaunchIntentState::Installed);
    assert_eq!(recorded.observed_native_id, Some(native.clone()));
    assert_eq!(
        recorded.autonomy,
        SeatAutonomy::Bounded,
        "reconciliation records the native, never restates the authority"
    );

    fixture
        .store
        .install_hosted_seat_launch_intent(
            fixture.project_id,
            seat,
            1,
            &native,
            at("2026-09-17T01:03:00Z"),
        )
        .expect("repeating the same reconciliation is unchanged");
}

/// One intent describes one occupancy. An installed row pointing at a second
/// native would make the evidence ambiguous exactly where it must be exact.
#[test]
fn an_installed_intent_refuses_a_second_native() {
    let fixture = Fixture::build();
    let seat = fixture.seat("LSA", "epic.lsa");
    fixture
        .store
        .prepare_hosted_seat_launch_intent(&intent(&fixture, seat, SeatAutonomy::Bounded))
        .expect("the intent is recorded");
    fixture
        .store
        .install_hosted_seat_launch_intent(
            fixture.project_id,
            seat,
            1,
            &ExternalId::parse("lsa-first").expect("a native id"),
            at("2026-09-17T01:02:00Z"),
        )
        .expect("the intent is reconciled");

    let refusal = fixture
        .store
        .install_hosted_seat_launch_intent(
            fixture.project_id,
            seat,
            1,
            &ExternalId::parse("lsa-duplicate").expect("a native id"),
            at("2026-09-17T01:04:00Z"),
        )
        .expect_err("a duplicate native is refused");
    assert!(
        refusal.to_string().contains("another native"),
        "the refusal names the duplication it stopped: {refusal:?}"
    );
}

/// The schema itself refuses a rewritten decision, so no future caller can
/// restate an authority by going around the repository.
#[test]
fn the_schema_refuses_to_rewrite_a_recorded_decision() {
    let fixture = Fixture::build();
    let seat = fixture.seat("LSA", "epic.lsa");
    fixture
        .store
        .prepare_hosted_seat_launch_intent(&intent(&fixture, seat, SeatAutonomy::Supervised))
        .expect("the intent is recorded");

    let connection = Connection::open(&fixture.db_path).expect("a second connection opens");
    let refusal = connection
        .execute(
            "UPDATE hosted_topology_seat_launch_intents SET autonomy = 'bounded'
             WHERE seat_binding_id = ?1",
            [seat.to_string()],
        )
        .expect_err("the trigger refuses a restated authority");
    assert!(
        refusal.to_string().contains("never changes it"),
        "the schema itself refuses it: {refusal:?}"
    );
}
