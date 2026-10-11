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

mod support;

use kontor_core::id::{
    CanonicalDocument, CommandReceiptId, ContentHash, ExternalId, ExternalName, IdempotencyKey,
    MiniProjectId, ProjectId, RoleCode, RoleSlotId, RuntimeKindKey, SeatBindingId, Timestamp,
    TopologyKindKey, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::repository::{
    CoreTeamRouteSuccessionCommit, HostedSeatLaunchIntentState, MiniProjectTopologySnapshot,
    NewMiniProject, NewProject, NewSeatBinding, NewSessionTopologyNode, ProjectRepository,
    ProjectTopologyDefault, StoredHostedSeatLaunchIntent, StoredHostedTopologySeat,
    TopologyRepository,
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
    mini_project_id: MiniProjectId,
    ecp_id: TopologyNodeId,
    catalog_id: kontor_core::id::RoleCatalogId,
    catalog_version: kontor_core::id::SpecVersion,
}

impl Fixture {
    fn build() -> Self {
        let home = support::state_root();
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
            mini_project_id,
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
            // This suite proves frozen autonomy, not the succession ledger.
            None,
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
    // 102 rather than the 99 this lane wrote: the ASMA-8190 integration lands
    // four lane migrations that each numbered themselves from the same base, so
    // the numbers are assigned once in the combined head.
    assert_eq!(version, 102);
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

// ---------------------------------------------------------------------------
// ASMA-8098 — the durable lineage readers.
//
// `tpm_lineage::resolve` answers which native authoritatively fills a seat, and
// it can only be as good as what it is handed. The singular readers answer
// "this native" and "this generation", both of which require the caller to have
// already chosen — which is the choice lineage exists to make. These two read
// the whole durable set for one logical seat instead.
//
// Retirement order is the load-bearing detail. The store derives an occupancy
// generation as one more than the number of predecessors retired, so history
// read back in any other order renumbers every occupancy.
// ---------------------------------------------------------------------------

/// Every occupancy of one seat, whole, in the order that defines its ordinal.
#[test]
fn seat_history_reads_back_every_predecessor_in_retirement_order() {
    let fixture = Fixture::build();
    let (seat, first) = fixture.lsa(SeatAutonomy::Bounded);

    fixture
        .store
        .archive_hosted_topology_seat_route(&first, at("2026-09-17T02:00:00Z"), "route correction")
        .expect("the first predecessor is archived");
    let second = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 1),
        observed_at: at("2026-09-17T02:00:01Z"),
        ..first.clone()
    };
    fixture
        .store
        .bind_hosted_topology_seat(&second)
        .expect("the successor binds");
    fixture
        .store
        .archive_hosted_topology_seat_route(
            &second,
            at("2026-09-17T03:00:00Z"),
            "second route correction",
        )
        .expect("the second predecessor is archived");

    let history = fixture
        .store
        .list_hosted_topology_seat_history(fixture.project_id, seat)
        .expect("the seat history reads");
    assert_eq!(
        history
            .iter()
            .map(|row| row.native_identity.native_id.as_str())
            .collect::<Vec<_>>(),
        vec!["lsa-first", "lsa-second"],
        "history must read back in the retirement order that numbers occupancies"
    );
    // Whole occupancies, not bare ids: lineage refuses a record whose route
    // disagrees, and it cannot do that without the route.
    assert!(
        history.iter().all(|row| row.model_rung == rung()),
        "a history row must carry the route its native was bound under"
    );
    assert!(
        history
            .iter()
            .all(|row| row.autonomy == SeatAutonomy::Bounded),
        "a history row must carry the authority its generation ran under"
    );
}

/// Every launch intent of one seat, across generations, oldest first.
#[test]
fn seat_launch_intents_read_back_across_every_generation() {
    let fixture = Fixture::build();
    let (seat, _) = fixture.lsa(SeatAutonomy::Bounded);

    fixture
        .store
        .prepare_hosted_seat_launch_intent(&intent(&fixture, seat, SeatAutonomy::Bounded))
        .expect("the first intent is prepared");
    fixture
        .store
        .install_hosted_seat_launch_intent(
            fixture.project_id,
            seat,
            1,
            &ExternalId::parse("lsa-first").expect("a native id"),
            at("2026-09-17T01:02:00Z"),
        )
        .expect("the first intent installs");
    fixture
        .store
        .prepare_hosted_seat_launch_intent(&StoredHostedSeatLaunchIntent {
            occupancy_generation: 2,
            prepared_at: at("2026-09-17T02:00:30Z"),
            ..intent(&fixture, seat, SeatAutonomy::Bounded)
        })
        .expect("the second intent is prepared");

    let intents = fixture
        .store
        .list_hosted_seat_launch_intents(fixture.project_id, seat)
        .expect("the launch intents read");
    assert_eq!(
        intents
            .iter()
            .map(|row| row.occupancy_generation)
            .collect::<Vec<_>>(),
        vec![1, 2],
        "every generation's intent must be read back, oldest first"
    );
    assert_eq!(
        intents[0].state,
        HostedSeatLaunchIntentState::Installed,
        "an installed intent must report the state that lets it name a native"
    );
    assert_eq!(
        intents[0]
            .observed_native_id
            .as_ref()
            .map(ExternalId::as_str),
        Some("lsa-first"),
        "an installed intent must name the native its launch produced"
    );
    // The unused second intent is prepared and names nothing. Lineage must be
    // able to tell that apart from an installed one, because a prepared intent
    // is evidence a launch was *intended* and never that one happened.
    assert_eq!(intents[1].state, HostedSeatLaunchIntentState::Prepared);
    assert!(intents[1].observed_native_id.is_none());
}

/// The succession readback is validated where it is persisted, not only where
/// it is built.
///
/// `CoreTeamRouteSuccessionCommit.readback` is free JSON. The production daemon
/// constructs a typed document and canonicalizes it first, but the store method
/// is the boundary that makes bytes durable, and a boundary that trusts its
/// caller is not a boundary. A nested forbidden key and a digest that does not
/// describe the bytes must each refuse — and refuse *without* committing the
/// route, so nothing half-written is left for a later export to trip over
/// (ASMA-8187 P2).
#[test]
fn a_succession_readback_is_refused_at_the_store_boundary() {
    let fixture = Fixture::build();
    let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);
    let successor = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 2),
        observed_at: at("2026-09-17T02:05:00Z"),
        ..predecessor.clone()
    };
    let key = IdempotencyKey::parse("asma-8187-boundary").expect("a key");
    let committed_at = at("2026-09-17T02:04:00Z");
    // Claim first. Without an owned claim the commit refuses on the missing
    // row and never reaches the validation under test — which is exactly how a
    // weaker version of this test passed while proving nothing.
    claim_step(&fixture, &key, seat, &predecessor, 1);

    let complete = full_readback(seat, &predecessor, 1, &successor, 2, committed_at);
    let attempt = |readback: serde_json::Value, hash: ContentHash| {
        fixture.store.replace_hosted_topology_seat_route(
            &predecessor,
            &successor,
            committed_at,
            "boundary test",
            Some(&CoreTeamRouteSuccessionCommit {
                idempotency_key: key.clone(),
                readback,
                readback_hash: hash,
                route_committed_at: committed_at,
            }),
        )
    };
    let rehash = |value: &serde_json::Value| {
        CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "readback": value,
        }))
        .expect("it canonicalizes")
        .hash()
        .clone()
    };

    // A readback missing a required identity field is refused. `host` is chosen
    // because nothing downstream reads it: a document can lose it and still
    // satisfy every field anybody compares, which is precisely why the shape
    // itself has to be the rule.
    let mut incomplete = serde_json::to_value(&complete).expect("it serializes");
    incomplete["successor"]
        .as_object_mut()
        .expect("an occupant object")
        .remove("host")
        .expect("the complete readback carries a host");
    let refused = attempt(incomplete.clone(), rehash(&incomplete))
        .expect_err("an incomplete readback was persisted");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the succession readback is not a complete Core Team succession readback",
                ..
            }
        ),
        "{refused:?}"
    );

    // An embellished one is refused too, and this is what closed the last place
    // a free-form member could hide: there is no longer anywhere in a readback
    // to put an undeclared key, nested or otherwise.
    let mut embellished = serde_json::to_value(&complete).expect("it serializes");
    embellished["placement"] = serde_json::json!({"detail": "anything at all"});
    let refused = attempt(embellished.clone(), rehash(&embellished))
        .expect_err("an embellished readback was persisted");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the succession readback is not a complete Core Team succession readback",
                ..
            }
        ),
        "{refused:?}"
    );

    // A complete, truthful document under a digest that does not describe it is
    // equally refused: the hash is re-derived here rather than believed.
    let honest = serde_json::to_value(&complete).expect("it serializes");
    let refused = attempt(honest, ContentHash::of(b"a digest of something else"))
        .expect_err("a mismatched declared hash was persisted");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the succession readback does not match its declared hash",
                ..
            }
        ),
        "{refused:?}"
    );

    // No refusal moved the seat: the predecessor is still the occupant.
    let active = fixture
        .store
        .get_hosted_topology_seat(fixture.project_id, seat)
        .expect("the seat reads")
        .expect("the seat exists");
    assert_eq!(
        active.native_identity.native_id, predecessor.native_identity.native_id,
        "a refused readback still replaced the occupant"
    );
}

/// Trailing effects are proved at the boundary, not asserted by the caller.
///
/// The latch used to take two booleans and believe them, so the persistence
/// boundary recorded an opinion rather than what durable state says. Both are
/// now derived from the exact rows the effects should have written, and a
/// receipt additionally re-proves the readback against its own ledger columns
/// before it binds (ASMA-8187 P2, remedy 2).
#[test]
fn succession_effects_and_readback_are_proved_before_a_receipt_binds() {
    let fixture = Fixture::build();
    let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);
    let successor = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 2),
        observed_at: at("2026-09-17T02:05:00Z"),
        ..predecessor.clone()
    };
    let key = IdempotencyKey::parse("asma-8187-effects").expect("a key");
    let intent_hash = succession_intent(&key).hash().clone();
    let committed_at = at("2026-09-17T02:04:00Z");

    let (readback, readback_hash) = readback_document(&full_readback(
        seat,
        &predecessor,
        1,
        &successor,
        2,
        committed_at,
    ));

    fixture
        .store
        .claim_core_team_route_succession(
            &kontor_core::repository::NewCoreTeamRouteSuccessionClaim {
                idempotency_key: key.clone(),
                intent_hash: intent_hash.clone(),
                project_id: fixture.project_id,
                mini_project_id: fixture.mini_project_id,
                seat_binding_id: seat,
                predecessor_native_id: predecessor.native_identity.native_id.clone(),
                predecessor_generation: predecessor.native_identity.generation,
                predecessor_occupancy_generation: 1,
                successor_occupancy_generation: 2,
                successor_credential_generation: 2,
                claimed_at: at("2026-09-17T02:03:00Z"),
            },
        )
        .expect("the claim is taken");
    fixture
        .store
        .replace_hosted_topology_seat_route(
            &predecessor,
            &successor,
            committed_at,
            "effects proof",
            Some(&CoreTeamRouteSuccessionCommit {
                idempotency_key: key.clone(),
                readback,
                readback_hash,
                route_committed_at: committed_at,
            }),
        )
        .expect("the route and ledger commit");

    // Neither effect has landed yet, so the commit refuses and latches nothing.
    assert!(
        fixture
            .store
            .commit_core_team_route_succession_effects(&key)
            .is_err(),
        "unlanded effects were latched anyway"
    );
    assert!(
        !fixture
            .store
            .get_core_team_route_succession(&key)
            .expect("the ledger reads")
            .expect("the row exists")
            .is_complete()
    );

    // The launch intent alone is not enough: the observation must also be
    // bound to this exact successor.
    fixture
        .store
        .prepare_hosted_seat_launch_intent(&StoredHostedSeatLaunchIntent {
            project_id: fixture.project_id,
            seat_binding_id: seat,
            occupancy_generation: 2,
            autonomy: SeatAutonomy::Supervised,
            model_rung: rung(),
            state: HostedSeatLaunchIntentState::Prepared,
            observed_native_id: None,
            prepared_at: at("2026-09-17T02:04:30Z"),
            installed_at: None,
        })
        .expect("the intent prepares");
    fixture
        .store
        .install_hosted_seat_launch_intent(
            fixture.project_id,
            seat,
            2,
            &successor.native_identity.native_id,
            successor.observed_at,
        )
        .expect("the intent installs");
    assert!(
        fixture
            .store
            .commit_core_team_route_succession_effects(&key)
            .is_err(),
        "an unobserved SeatBinding was latched as observed"
    );

    // With both effects genuinely landed, the commit succeeds.
    fixture
        .store
        .observe_seat_binding(
            fixture.project_id,
            seat,
            &kontor_core::repository::SeatLivenessObservation {
                attached_at: Some(successor.observed_at),
                ..kontor_core::repository::SeatLivenessObservation::default()
            },
            successor.observed_at,
        )
        .expect("the binding is observed");
    fixture
        .store
        .commit_core_team_route_succession_effects(&key)
        .expect("proved effects commit");
    assert!(
        fixture
            .store
            .get_core_team_route_succession(&key)
            .expect("the ledger reads")
            .expect("the row exists")
            .is_complete()
    );

    // A coherent succession, presenting the receipt its own command recorded,
    // binds. Nothing about the receipt is taken on trust: the binder reads it
    // and requires the project, key, kind, target epic and intent to be this
    // command's own.
    fixture
        .store
        .bind_core_team_route_succession_receipt(
            &key,
            &intent_hash,
            recorded_receipt(&fixture, &key),
            at("2026-09-17T02:06:00Z"),
        )
        .expect("a coherent succession binds the receipt its command recorded");
}

/// A readback naming another successor never becomes durable.
///
/// This used to be a binder test: the write boundary could not know which
/// successor the transition installed, so a coherent, correctly hashed document
/// naming a different native was accepted and refused later. The boundary now
/// compares the document against the exact seat rows it is committing, so the
/// refusal has moved to where the truth is (ASMA-8187 P2). The binder's own
/// comparison still stands behind it, and is exercised separately against a row
/// written by another path.
#[test]
fn a_readback_naming_another_successor_never_becomes_durable() {
    let fixture = Fixture::build();
    let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);
    let successor = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 2),
        observed_at: at("2026-09-17T02:05:00Z"),
        ..predecessor.clone()
    };
    let key = IdempotencyKey::parse("asma-8187-disagree").expect("a key");
    let committed_at = at("2026-09-17T02:04:00Z");
    claim_step(&fixture, &key, seat, &predecessor, 1);

    // Complete, correctly hashed, internally coherent — and naming a native
    // this transition is not installing.
    let mut lying = full_readback(seat, &predecessor, 1, &successor, 2, committed_at);
    lying.successor.native_id = ExternalId::parse("someone-elses-native").expect("a native id");
    let (lying, lying_hash) = readback_document(&lying);

    let refused = fixture
        .store
        .replace_hosted_topology_seat_route(
            &predecessor,
            &successor,
            committed_at,
            "disagreement proof",
            Some(&CoreTeamRouteSuccessionCommit {
                idempotency_key: key.clone(),
                readback: lying,
                readback_hash: lying_hash,
                route_committed_at: committed_at,
            }),
        )
        .expect_err("a readback naming another successor was persisted");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the succession readback does not describe the successor being installed",
                ..
            }
        ),
        "{refused:?}"
    );

    // Zero route movement: the predecessor is still the occupant and the claim
    // is still uncommitted.
    assert_eq!(
        fixture
            .store
            .get_hosted_topology_seat(fixture.project_id, seat)
            .expect("the seat reads")
            .expect("the seat exists")
            .native_identity
            .native_id,
        predecessor.native_identity.native_id
    );
    assert!(
        fixture
            .store
            .get_core_team_route_succession(&key)
            .expect("the ledger reads")
            .expect("the claim survives")
            .route_committed_at
            .is_none()
    );
}

// ---------------------------------------------------------------------------
// ASMA-8187 remedy 5 — a succession crossing a Realm boundary.
//
// The disposition these tests hold to: the complete succession row, receipt and
// readback included, must survive an import as inspectable evidence, and must
// restore no live succession, idempotency, credential, grant, placement or
// materialization authority in the destination. Preservation and authority are
// separate questions, and the tests below ask them separately.
// ---------------------------------------------------------------------------

/// Build three successions on one seat, in the three states one can be left in.
///
/// Successive rather than parallel, because exclusivity permits exactly one
/// claim per (project, seat, predecessor occupancy): the seat walks 1→2→3→4 and
/// each step is left at a different point. Built through the store's own
/// boundary rather than planted, so the rows that cross the Realm boundary are
/// the rows the domain actually produces.
fn three_succession_states(
    fixture: &Fixture,
    seat: SeatBindingId,
    first: &StoredHostedTopologySeat,
) -> [IdempotencyKey; 3] {
    let completed = IdempotencyKey::parse("asma-8187-crossing-complete").expect("a key");
    let committed = IdempotencyKey::parse("asma-8187-crossing-committed").expect("a key");
    let claimed = IdempotencyKey::parse("asma-8187-crossing-claimed").expect("a key");

    let step = |key: &IdempotencyKey,
                predecessor: &StoredHostedTopologySeat,
                successor_native: &str,
                predecessor_occupancy: u64| {
        let successor = StoredHostedTopologySeat {
            native_identity: identity(successor_native, predecessor_occupancy + 1),
            observed_at: at("2026-09-17T03:0{}:00Z"
                .replace("{}", &predecessor_occupancy.to_string())
                .as_str()),
            ..predecessor.clone()
        };
        fixture
            .store
            .claim_core_team_route_succession(
                &kontor_core::repository::NewCoreTeamRouteSuccessionClaim {
                    idempotency_key: key.clone(),
                    intent_hash: succession_intent(key).hash().clone(),
                    project_id: fixture.project_id,
                    mini_project_id: fixture.mini_project_id,
                    seat_binding_id: seat,
                    predecessor_native_id: predecessor.native_identity.native_id.clone(),
                    predecessor_generation: predecessor.native_identity.generation,
                    predecessor_occupancy_generation: predecessor_occupancy,
                    successor_occupancy_generation: predecessor_occupancy + 1,
                    successor_credential_generation: predecessor_occupancy + 1,
                    claimed_at: at("2026-09-17T03:00:00Z"),
                },
            )
            .expect("the claim is taken");
        successor
    };

    // 1 → 2, carried all the way to a bound receipt.
    let second = step(&completed, first, "lsa-second", 1);
    let committed_at = at("2026-09-17T03:01:30Z");
    let (readback, readback_hash) =
        readback_document(&full_readback(seat, first, 1, &second, 2, committed_at));
    fixture
        .store
        .replace_hosted_topology_seat_route(
            first,
            &second,
            committed_at,
            "crossing fixture",
            Some(&CoreTeamRouteSuccessionCommit {
                idempotency_key: completed.clone(),
                readback,
                readback_hash,
                route_committed_at: committed_at,
            }),
        )
        .expect("the first transition commits");
    land_effects(fixture, seat, &second, 2);
    fixture
        .store
        .commit_core_team_route_succession_effects(&completed)
        .expect("the proved effects commit");
    let receipt = recorded_receipt(fixture, &completed);
    fixture
        .store
        .bind_core_team_route_succession_receipt(
            &completed,
            succession_intent(&completed).hash(),
            receipt,
            at("2026-09-17T03:02:00Z"),
        )
        .expect("the completed succession binds its receipt");

    // 2 → 3, committed with both effects still owed.
    let third = step(&committed, &second, "lsa-third", 2);
    let second_at = at("2026-09-17T03:03:00Z");
    let (second_readback, second_hash) =
        readback_document(&full_readback(seat, &second, 2, &third, 3, second_at));
    fixture
        .store
        .replace_hosted_topology_seat_route(
            &second,
            &third,
            second_at,
            "crossing fixture",
            Some(&CoreTeamRouteSuccessionCommit {
                idempotency_key: committed.clone(),
                readback: second_readback,
                readback_hash: second_hash,
                route_committed_at: second_at,
            }),
        )
        .expect("the second transition commits");

    // 3 → 4, claimed and nothing more.
    let _ = step(&claimed, &third, "lsa-fourth", 3);

    [completed, committed, claimed]
}

/// Land both trailing effects for one successor, honestly.
fn land_effects(
    fixture: &Fixture,
    seat: SeatBindingId,
    successor: &StoredHostedTopologySeat,
    occupancy: u64,
) {
    fixture
        .store
        .prepare_hosted_seat_launch_intent(&StoredHostedSeatLaunchIntent {
            project_id: fixture.project_id,
            seat_binding_id: seat,
            occupancy_generation: occupancy,
            autonomy: SeatAutonomy::Supervised,
            model_rung: rung(),
            state: HostedSeatLaunchIntentState::Prepared,
            observed_native_id: None,
            prepared_at: at("2026-09-17T03:01:40Z"),
            installed_at: None,
        })
        .expect("the intent prepares");
    fixture
        .store
        .install_hosted_seat_launch_intent(
            fixture.project_id,
            seat,
            occupancy,
            &successor.native_identity.native_id,
            successor.observed_at,
        )
        .expect("the intent installs");
    fixture
        .store
        .observe_seat_binding(
            fixture.project_id,
            seat,
            &kontor_core::repository::SeatLivenessObservation {
                attached_at: Some(successor.observed_at),
                ..kontor_core::repository::SeatLivenessObservation::default()
            },
            successor.observed_at,
        )
        .expect("the binding is observed");
}

/// Count one table through a read-only connection.
fn rows(path: &std::path::Path, table: &str) -> i64 {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("the database opens");
    connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("the table is readable")
}

/// A succession crosses a Realm boundary as evidence and as nothing else.
///
/// Both halves matter and they pull in opposite directions. The row has to
/// survive *whole* — a digest over bytes an investigator cannot read proves
/// only that somebody had them — and it must confer nothing: no live
/// succession, no exclusivity token, no replayable key, no receipt, no
/// placement, no launch intent, no seat (ASMA-8187 acceptance 5).
#[test]
fn a_succession_crosses_a_realm_boundary_as_evidence_and_not_as_authority() {
    let source = Fixture::build();
    let (seat, first) = source.lsa(SeatAutonomy::Supervised);
    let keys = three_succession_states(&source, seat, &first);

    let export = kontor_store::backup::export_realm(&source.store, at("2026-09-17T04:00:00Z"))
        .expect("the source Realm exports");
    assert_eq!(
        export.schema_version,
        kontor_store::backup::EXPORT_SCHEMA_VERSION
    );
    assert_eq!(
        export.records.core_team_route_successions.len(),
        3,
        "an export that drops a succession state cannot preserve one"
    );
    // The three states are genuinely distinct in the document, not three copies
    // of the easy one.
    let receipted = export
        .records
        .core_team_route_successions
        .iter()
        .filter(|row| row.receipt_id.is_some())
        .count();
    let committed = export
        .records
        .core_team_route_successions
        .iter()
        .filter(|row| row.route_committed_at.is_some() && row.receipt_id.is_none())
        .count();
    let claimed = export
        .records
        .core_team_route_successions
        .iter()
        .filter(|row| row.route_committed_at.is_none())
        .count();
    assert_eq!((receipted, committed, claimed), (1, 1, 1));

    // The document survives its own serialization at this generation.
    let bytes = serde_json::to_vec(&export).expect("the export serializes");
    let parsed = kontor_store::backup::KontorExportV1::parse(&bytes)
        .expect("the current generation parses its own document");
    assert_eq!(
        parsed.records.core_team_route_successions,
        export.records.core_team_route_successions
    );

    // A separately initialized destination Realm, with its own project.
    // Genuinely created rather than cloned from the suite's template: every
    // clone shares the template's Realm identity, and an import into the Realm
    // that produced the document is refused as a restore.
    let destination_home = support::created_state_root();
    let destination_path = destination_home.path().join("kontor.db");
    let destination = support::open_created_realm(&destination_path);
    let destination_project = ProjectId::generate();
    destination
        .create_project(&NewProject {
            id: destination_project,
            name: name("Receiving project"),
            root_path: name("/tmp/hosted-seat-crossing"),
            created_at: at("2026-09-17T04:10:00Z"),
        })
        .expect("the destination project is created");

    let report = kontor_store::backup::import_export(
        &destination,
        &parsed,
        &kontor_store::backup::ImportPlan::redacted_import_into(destination_project),
        at("2026-09-17T04:20:00Z"),
    )
    .expect("the export imports");
    assert!(report.reconciliation_required);

    // --- preserved -------------------------------------------------------
    let lineage: Vec<_> = destination
        .imported_records(&report.import_id.as_hyphenated().to_string())
        .expect("the lineage reads")
        .into_iter()
        .filter(|row| row.record_kind == "core_team_route_successions")
        .collect();
    assert_eq!(lineage.len(), 3);
    assert!(
        lineage.iter().all(|row| row.disposition == "recorded"),
        "a succession was imported as something other than non-live lineage"
    );

    let evidence = destination
        .imported_record_evidence(&report.import_id.as_hyphenated().to_string())
        .expect("the evidence reads");
    assert_eq!(
        evidence.len(),
        3,
        "the succession content did not survive the crossing"
    );
    for row in &evidence {
        assert_eq!(row.record_kind, "core_team_route_successions");
        let content: serde_json::Value =
            serde_json::from_str(&row.content).expect("the evidence is readable JSON");
        // The digest describes these exact bytes, recomputed here rather than
        // trusted: evidence whose hash is merely copied proves nothing.
        let mut canonical = serde_json::to_vec(&content).expect("the content re-serializes");
        canonical.push(b'\n');
        assert_eq!(
            ContentHash::of(&canonical).to_string(),
            row.content_hash,
            "the preserved content does not match its preserved digest"
        );
        let matching = lineage
            .iter()
            .find(|entry| entry.source_identity == row.source_identity)
            .expect("the evidence names a lineage row");
        assert_eq!(matching.source_hash, row.content_hash);
    }

    // The completed succession is readable *whole* — receipt and readback
    // included. That is the part a digest alone cannot give an investigator.
    let complete = evidence
        .iter()
        .find(|row| row.source_identity == keys[0].as_str())
        .expect("the completed succession survived");
    let content: serde_json::Value =
        serde_json::from_str(&complete.content).expect("readable JSON");
    assert!(content["receipt_id"].is_string(), "{content}");
    assert!(content["receipted_at"].is_string(), "{content}");
    assert_eq!(content["launch_intent_installed"], serde_json::json!(1));
    assert_eq!(content["seat_binding_observed"], serde_json::json!(1));
    assert_eq!(
        content["successor_occupancy_generation"],
        serde_json::json!(2)
    );
    // The readback is a JSON document stored as text, so it crosses as text and
    // is read back the same way. What matters is that the whole of it is here.
    let readback: serde_json::Value = serde_json::from_str(
        content["readback"]
            .as_str()
            .expect("the readback crossed as its stored text"),
    )
    .expect("the preserved readback is readable JSON");
    assert_eq!(
        readback["successor"]["native_id"],
        serde_json::json!("lsa-second")
    );
    assert_eq!(
        readback["predecessor"]["occupancy_generation"],
        serde_json::json!(1)
    );
    assert_eq!(
        readback["grant_subject"]["generation"],
        serde_json::json!(2)
    );
    assert!(content["readback_hash"].is_string(), "{content}");
    // And the readback still answers for its own digest, across the boundary.
    assert_eq!(
        CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "readback": readback,
        }))
        .expect("the preserved readback canonicalizes")
        .hash()
        .to_string(),
        content["readback_hash"]
            .as_str()
            .expect("the digest crossed")
            .to_owned(),
        "the preserved readback no longer matches the digest its source recorded"
    );

    // --- and not live ----------------------------------------------------
    for table in [
        "core_team_route_successions",
        "hosted_topology_seats",
        "hosted_topology_seat_launch_intents",
        "hosted_topology_seat_history",
        "seat_bindings",
        "command_receipts",
    ] {
        assert_eq!(
            rows(&destination_path, table),
            0,
            "the import created live `{table}` state in the destination"
        );
    }
    for key in &keys {
        assert!(
            destination
                .get_core_team_route_succession(key)
                .expect("the destination ledger reads")
                .is_none(),
            "an imported key resolves to a live succession in the destination"
        );
        assert!(
            kontor_core::repository::CommandRepository::get_receipt_by_key(&destination, key)
                .expect("the destination receipts read")
                .is_none(),
            "an imported key authorizes a destination command"
        );
    }
    // No exclusivity token either: the destination seat is unclaimed, so an
    // imported succession cannot fence a local one.
    assert!(
        destination
            .core_team_route_succession_owner(destination_project, seat, 1)
            .expect("the destination ledger reads")
            .is_none(),
        "an imported succession owns a destination seat's occupancy"
    );

    // --- and not forwarded onward ----------------------------------------
    // The destination can export itself, and what it exports is its own. An
    // imported succession must not reappear in that document: forwarding it
    // would put a second Realm's account of an event into a third one under
    // this Realm's name, with this Realm's digest over it.
    let onward = kontor_store::backup::export_realm(&destination, at("2026-09-17T05:00:00Z"))
        .expect("the destination Realm exports");
    assert_eq!(onward.source_realm_id, destination.realm_id());
    assert!(
        onward.records.core_team_route_successions.is_empty(),
        "an imported succession was forwarded as a destination record"
    );
    assert!(
        onward
            .redaction_summary
            .excluded_tables
            .contains_key("imported_record_evidence"),
        "the evidence exclusion must be disclosed rather than inferred from an absence"
    );

    // --- and unforgeable afterwards --------------------------------------
    let writable = Connection::open(&destination_path).expect("the destination opens");
    assert!(
        writable
            .execute("UPDATE imported_record_evidence SET content = '{}'", [])
            .is_err(),
        "imported evidence was rewritten"
    );
    assert!(
        writable
            .execute("DELETE FROM imported_record_evidence", [])
            .is_err(),
        "imported evidence was deleted"
    );
}

/// Evidence may only stand beside lineage the import declared non-live.
///
/// The disposition is the whole guarantee, so it is enforced where it cannot be
/// forgotten. A record that became destination state, or one that was refused,
/// must not also carry a second unreconciled copy of itself.
#[test]
fn imported_evidence_cannot_accompany_a_live_disposition() {
    let home = support::state_root();
    let path = home.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("the destination opens");
    let project = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project,
            name: name("Disposition project"),
            root_path: name("/tmp/hosted-seat-disposition"),
            created_at: at("2026-09-17T04:00:00Z"),
        })
        .expect("the project is created");

    let connection = Connection::open(&path).expect("the database opens");
    connection
        .execute_batch(&format!(
            "INSERT INTO import_receipts
                 (id, project_id, source_realm_id, export_schema_version, source_schema_version,
                  records_hash, exported_at, imported_at, record_count, materialized_count)
             VALUES ('01a0e000-0000-7000-8000-000000000001', '{project}',
                     '01a0e000-0000-7000-8000-0000000000ff', 13, 121, '{hash}',
                     '2026-09-17T04:00:00Z', '2026-09-17T04:01:00Z', 2, 0);
             INSERT INTO imported_records
                 (import_id, record_kind, source_identity, source_hash, disposition,
                  reason_code, recorded_at)
             VALUES ('01a0e000-0000-7000-8000-000000000001', 'core_team_route_successions',
                     'live-one', '{hash}', 'materialized', NULL, '2026-09-17T04:01:00Z'),
                    ('01a0e000-0000-7000-8000-000000000001', 'core_team_route_successions',
                     'refused-one', '{hash}', 'refused', 'unsupported', '2026-09-17T04:01:00Z');",
            project = project,
            hash = "0".repeat(64),
        ))
        .expect("the lineage is planted");

    for identity in ["live-one", "refused-one"] {
        let refused = connection.execute(
            "INSERT INTO imported_record_evidence
                 (import_id, record_kind, source_identity, content, content_hash, recorded_at)
             VALUES ('01a0e000-0000-7000-8000-000000000001', 'core_team_route_successions',
                     ?1, '{\"a\":1}', ?2, '2026-09-17T04:01:00Z')",
            rusqlite::params![identity, "0".repeat(64)],
        );
        assert!(
            refused.is_err(),
            "evidence was admitted beside a `{identity}` disposition"
        );
    }

    // And a lineage row that does not exist at all cannot acquire evidence.
    assert!(
        connection
            .execute(
                "INSERT INTO imported_record_evidence
                     (import_id, record_kind, source_identity, content, content_hash, recorded_at)
                 VALUES ('01a0e000-0000-7000-8000-000000000001', 'core_team_route_successions',
                         'no-such-record', '{\"a\":1}', ?1, '2026-09-17T04:01:00Z')",
                rusqlite::params![&"0".repeat(64)],
            )
            .is_err(),
        "evidence was admitted for a record no import ever saw"
    );
}

/// An export generation that never defined successions cannot carry or hide one.
///
/// Both directions are the same rule read from two sides. A v12 document must
/// not carry succession rows it has no fields for, and a v12 document must not
/// be offered by a database new enough to hold them — because then its silence
/// is indistinguishable from there being none, and the receipt and readback a
/// succession is reconstructed from would vanish in a round trip.
#[test]
fn a_legacy_export_generation_can_neither_carry_nor_conceal_a_succession() {
    let fixture = Fixture::build();
    let (seat, first) = fixture.lsa(SeatAutonomy::Supervised);
    three_succession_states(&fixture, seat, &first);

    let export = kontor_store::backup::export_realm(&fixture.store, at("2026-09-17T04:00:00Z"))
        .expect("the export");
    assert!(!export.records.core_team_route_successions.is_empty());
    let base = serde_json::to_value(&export).expect("the export serializes");

    let rehash = |document: &mut serde_json::Value| {
        let mut records = serde_json::to_vec(
            document
                .get("records")
                .expect("the document carries records"),
        )
        .expect("the records serialize");
        records.push(b'\n');
        document["records_hash"] = serde_json::json!(ContentHash::of(&records).to_string());
    };

    // Carrying them under a generation that never defined them.
    let mut carrying = base.clone();
    carrying["schema_version"] = serde_json::json!(12);
    carrying["database_schema_version"] = serde_json::json!(119);
    rehash(&mut carrying);
    match kontor_store::backup::KontorExportV1::parse(
        &serde_json::to_vec(&carrying).expect("the bytes"),
    ) {
        Err(kontor_store::backup::BackupError::Verification { detail }) => assert_eq!(
            detail,
            "the legacy export generation carries Core Team route successions it did not define",
        ),
        other => panic!("a v12 document cannot carry successions, got {other:?}"),
    }

    // Concealing them: a database that holds the ledger offering a generation
    // that cannot represent it.
    let mut concealing = base.clone();
    concealing["schema_version"] = serde_json::json!(12);
    concealing["records"]["core_team_route_successions"] = serde_json::json!([]);
    concealing["continuity_summary"]["record_counts"]["core_team_route_successions"] =
        serde_json::json!(0);
    rehash(&mut concealing);
    match kontor_store::backup::KontorExportV1::parse(
        &serde_json::to_vec(&concealing).expect("the bytes"),
    ) {
        Err(kontor_store::backup::BackupError::Verification { detail }) => assert_eq!(
            detail,
            "the legacy export generation cannot prove Core Team route succession completeness",
        ),
        other => panic!("a succession-capable database cannot export as v12, got {other:?}"),
    }

    // And the continuity summary must disclose every one of them.
    let mut understated = base;
    understated["continuity_summary"]["record_counts"]["core_team_route_successions"] =
        serde_json::json!(1);
    rehash(&mut understated);
    assert!(
        kontor_store::backup::KontorExportV1::parse(
            &serde_json::to_vec(&understated).expect("the bytes")
        )
        .is_err(),
        "an export understated how many successions it carries"
    );
}

// ---------------------------------------------------------------------------
// ASMA-8187 P1 — an external native id is the provider's to reissue.
// ---------------------------------------------------------------------------

/// A later occupancy wearing an earlier native's name cannot complete its row.
///
/// `native_id` is an external identifier. Nothing stops a provider from issuing
/// one it has used before, and when it does, two different natives doing two
/// different jobs answer to the same name. A pending succession that compared
/// only that name would accept the *later* occupant as proof that its own
/// effects landed, and mint a receipt for work belonging to its successor's
/// successor. The occupancy generation is what the seat cannot reuse, and the
/// runtime generation is what the identity cannot (ASMA-8187 P1).
#[test]
fn a_reused_native_id_in_a_later_occupancy_cannot_complete_an_older_succession() {
    let fixture = Fixture::build();
    let (seat, first) = fixture.lsa(SeatAutonomy::Supervised);
    let stranded = IdempotencyKey::parse("asma-8187-recycled-stranded").expect("a key");
    let later = IdempotencyKey::parse("asma-8187-recycled-later").expect("a key");

    // Occupancy 1 → 2 commits and is then abandoned with both effects owed.
    let second = StoredHostedTopologySeat {
        native_identity: identity("recycled", 5),
        observed_at: at("2026-09-17T06:01:00Z"),
        ..first.clone()
    };
    claim_step(&fixture, &stranded, seat, &first, 1);
    commit_step(
        &fixture,
        &stranded,
        seat,
        &first,
        &second,
        1,
        at("2026-09-17T06:01:30Z"),
    );
    assert!(
        !fixture
            .store
            .get_core_team_route_succession(&stranded)
            .expect("the ledger reads")
            .expect("the row exists")
            .is_complete()
    );

    // Occupancy 2 → 3 installs a *different* native under the *same* external
    // id. This is the reuse: same name, later runtime generation, later
    // occupancy.
    let third = StoredHostedTopologySeat {
        native_identity: identity("recycled", 9),
        observed_at: at("2026-09-17T06:02:00Z"),
        ..first.clone()
    };
    claim_step(&fixture, &later, seat, &second, 2);
    commit_step(
        &fixture,
        &later,
        seat,
        &second,
        &third,
        2,
        at("2026-09-17T06:02:30Z"),
    );
    land_effects(&fixture, seat, &third, 3);
    fixture
        .store
        .commit_core_team_route_succession_effects(&later)
        .expect("the later succession's own effects are proved");

    // The stranded row must not be able to read the later occupant as its own.
    let refused = fixture
        .store
        .commit_core_team_route_succession_effects(&stranded)
        .expect_err("a reused native id completed an abandoned succession");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the seat has left the occupancy generation this succession installed",
                ..
            }
        ),
        "the refusal was not the occupancy fence: {refused:?}"
    );
    let stale = fixture
        .store
        .get_core_team_route_succession(&stranded)
        .expect("the ledger reads")
        .expect("the row exists");
    assert!(
        !stale.is_complete(),
        "the stranded succession reads complete"
    );
    assert!(stale.receipt_id.is_none());
    assert!(
        fixture
            .store
            .bind_core_team_route_succession_receipt(
                &stranded,
                &stale.intent_hash,
                recorded_receipt(&fixture, &stranded),
                at("2026-09-17T06:03:00Z"),
            )
            .is_err(),
        "an abandoned succession was receipted from a reused native id"
    );
}

/// The identity comparison stands on its own, not only on the occupancy count.
///
/// Within one occupancy the store will not let the active identity change, so
/// this state is not one the domain produces — it is what another writer, or a
/// restored file, could leave behind. The latch must still refuse it, because a
/// guard that is only reachable through a second guard is not a guard.
#[test]
fn an_active_native_of_another_runtime_generation_is_not_this_successor() {
    let fixture = Fixture::build();
    let (seat, first) = fixture.lsa(SeatAutonomy::Supervised);
    let key = IdempotencyKey::parse("asma-8187-generation-swap").expect("a key");
    let second = StoredHostedTopologySeat {
        native_identity: identity("swapped", 5),
        observed_at: at("2026-09-17T06:05:00Z"),
        ..first.clone()
    };
    claim_step(&fixture, &key, seat, &first, 1);
    commit_step(
        &fixture,
        &key,
        seat,
        &first,
        &second,
        1,
        at("2026-09-17T06:05:30Z"),
    );
    land_effects(&fixture, seat, &second, 2);
    // Everything is honest so far, so the latch would commit.
    fixture
        .store
        .get_core_team_route_succession(&key)
        .expect("the ledger reads")
        .expect("the row exists");

    // Now only the runtime generation moves. The occupancy count is untouched,
    // the external id is untouched, and the observation instant still matches.
    let connection = Connection::open(&fixture.db_path).expect("the database opens");
    connection
        .execute(
            "UPDATE hosted_topology_seats SET generation = 9
              WHERE project_id = ?1 AND seat_binding_id = ?2",
            rusqlite::params![fixture.project_id.to_string(), seat.to_string()],
        )
        .expect("the generation is swapped underneath");
    assert_eq!(
        fixture
            .store
            .hosted_topology_seat_occupancy_generation(fixture.project_id, seat)
            .expect("the occupancy reads"),
        Some(2),
        "the occupancy fence must not be what refuses here"
    );

    let refused = fixture
        .store
        .commit_core_team_route_succession_effects(&key)
        .expect_err("a native of another runtime generation proved this succession");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the succession's trailing effects are not both proved against its successor",
                ..
            }
        ),
        "{refused:?}"
    );
}

/// Claim one succession step through the store's own boundary.
fn claim_step(
    fixture: &Fixture,
    key: &IdempotencyKey,
    seat: SeatBindingId,
    predecessor: &StoredHostedTopologySeat,
    predecessor_occupancy: u64,
) {
    fixture
        .store
        .claim_core_team_route_succession(
            &kontor_core::repository::NewCoreTeamRouteSuccessionClaim {
                idempotency_key: key.clone(),
                intent_hash: succession_intent(key).hash().clone(),
                project_id: fixture.project_id,
                mini_project_id: fixture.mini_project_id,
                seat_binding_id: seat,
                predecessor_native_id: predecessor.native_identity.native_id.clone(),
                predecessor_generation: predecessor.native_identity.generation,
                predecessor_occupancy_generation: predecessor_occupancy,
                successor_occupancy_generation: predecessor_occupancy + 1,
                successor_credential_generation: predecessor_occupancy + 1,
                claimed_at: at("2026-09-17T06:00:00Z"),
            },
        )
        .expect("the claim is taken");
}

/// Commit one succession step's route and ledger row together.
fn commit_step(
    fixture: &Fixture,
    key: &IdempotencyKey,
    seat: SeatBindingId,
    predecessor: &StoredHostedTopologySeat,
    successor: &StoredHostedTopologySeat,
    predecessor_occupancy: u64,
    committed_at: Timestamp,
) {
    let (readback, readback_hash) = readback_document(&full_readback(
        seat,
        predecessor,
        predecessor_occupancy,
        successor,
        predecessor_occupancy + 1,
        committed_at,
    ));
    fixture
        .store
        .replace_hosted_topology_seat_route(
            predecessor,
            successor,
            committed_at,
            "reuse fixture",
            Some(&CoreTeamRouteSuccessionCommit {
                idempotency_key: key.clone(),
                readback,
                readback_hash,
                route_committed_at: committed_at,
            }),
        )
        .expect("the transition and its ledger row commit");
}

// ---------------------------------------------------------------------------
// ASMA-8187 P1 — this Realm's own bearers are credential material too.
// ---------------------------------------------------------------------------

/// A format-valid seat bearer is refused by value, wherever it appears.
///
/// The forbidden-key list stops `credential` and `token`; it cannot stop a
/// caller choosing the key `note`, and once the readback became a typed
/// structure it could not stop a caller putting one in a *declared* field
/// either. A Kontor seat bearer carries no `Bearer ` prefix and matched none of
/// the provider markers, so this Realm's own credential was the one kind of
/// secret its scanner did not know. What is refused now is the value
/// (ASMA-8187 P1).
#[test]
fn a_real_format_seat_bearer_is_refused_by_value_wherever_it_appears() {
    let fixture = Fixture::build();
    let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);
    let successor = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 2),
        observed_at: at("2026-09-17T07:01:00Z"),
        ..predecessor.clone()
    };
    let key = IdempotencyKey::parse("asma-8187-bearer-value").expect("a key");
    let committed_at = at("2026-09-17T07:01:30Z");
    claim_step(&fixture, &key, seat, &predecessor, 1);

    // Synthetic: the shape a real bearer has, over a signature that is not one.
    // No live credential is minted, read or logged anywhere in this test.
    for bearer in [
        format!("kontor-seat-v2.{seat}.2.{}", "a".repeat(64)),
        format!("kontor-seat-v1.{seat}.{}", "b".repeat(64)),
    ] {
        // In a free document, under a key nobody would think to forbid.
        assert!(
            CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "handover": {"note": format!("resume with {bearer}")},
            }))
            .is_err(),
            "a seat bearer canonicalized under a neutral key"
        );

        // And inside a *declared* readback field, where the shape check cannot
        // be what refuses it.
        let mut carrying = full_readback(seat, &predecessor, 1, &successor, 2, committed_at);
        carrying.retired_at = format!("{} {bearer}", carrying.retired_at);
        let value = serde_json::to_value(&carrying).expect("it serializes");
        assert!(
            CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "readback": value,
            }))
            .is_err(),
            "a seat bearer canonicalized inside a declared field"
        );
        let refused = fixture.store.replace_hosted_topology_seat_route(
            &predecessor,
            &successor,
            committed_at,
            "bearer boundary",
            Some(&CoreTeamRouteSuccessionCommit {
                idempotency_key: key.clone(),
                readback: value,
                readback_hash: ContentHash::of(b"whatever digest the caller claims"),
                route_committed_at: committed_at,
            }),
        );
        assert!(refused.is_err(), "the store persisted a seat bearer");
    }

    // And nothing moved on the way out.
    assert_eq!(
        fixture
            .store
            .get_hosted_topology_seat(fixture.project_id, seat)
            .expect("the seat reads")
            .expect("the seat exists")
            .native_identity
            .native_id,
        predecessor.native_identity.native_id,
        "a refused bearer still replaced the occupant"
    );
    assert!(
        fixture
            .store
            .get_core_team_route_succession(&key)
            .expect("the ledger reads")
            .expect("the claim survives")
            .route_committed_at
            .is_none()
    );
}

/// A foreign document carrying a seat bearer is refused before any write.
///
/// Construction scans what this Realm publishes. A foreign document is the one
/// copy nobody here vouched for, and it was believed on its digest alone: a
/// self-consistent export could carry another Realm's bearer — or this one's —
/// straight into durable evidence. The document below is internally consistent
/// on purpose, so the digest is not what refuses it.
#[test]
fn a_foreign_document_carrying_a_seat_bearer_is_refused_before_any_write() {
    let source = Fixture::build();
    let (seat, first) = source.lsa(SeatAutonomy::Supervised);
    three_succession_states(&source, seat, &first);
    let mut export = kontor_store::backup::export_realm(&source.store, at("2026-09-17T07:10:00Z"))
        .expect("the clean source Realm exports");

    let destination_home = support::created_state_root();
    let destination_path = destination_home.path().join("kontor.db");
    let destination = support::open_created_realm(&destination_path);
    let destination_project = ProjectId::generate();
    destination
        .create_project(&NewProject {
            id: destination_project,
            name: name("Receiving project"),
            root_path: name("/tmp/hosted-seat-bearer"),
            created_at: at("2026-09-17T07:11:00Z"),
        })
        .expect("the destination project is created");

    // The readback crosses as a JSON *string*, so this is the embedded pass:
    // a scanner that only walked the outer structure would see one opaque
    // value and wave it through.
    let bearer = format!("kontor-seat-v2.{seat}.2.{}", "c".repeat(64));
    let row = export
        .records
        .core_team_route_successions
        .iter_mut()
        .find(|row| row.readback.is_some())
        .expect("a committed succession carries a readback");
    let mut readback: serde_json::Value =
        serde_json::from_str(row.readback.as_ref().expect("a readback")).expect("readable JSON");
    readback["handover"] = serde_json::json!({"note": bearer});
    row.readback = Some(serde_json::to_string(&readback).expect("it re-serializes"));
    // Made self-consistent again, so the refusal cannot be a digest mismatch.
    export.records_hash = ContentHash::of(
        &export
            .canonical_records_bytes()
            .expect("the records canonicalize"),
    );

    let refused = kontor_store::backup::import_export(
        &destination,
        &export,
        &kontor_store::backup::ImportPlan::redacted_import_into(destination_project),
        at("2026-09-17T07:12:00Z"),
    )
    .expect_err("a document carrying a seat bearer was imported");
    assert!(
        matches!(refused, kontor_store::backup::BackupError::Redaction { .. }),
        "the refusal was not the canary scan: {refused:?}"
    );

    // Refused before the transaction: no receipt, no lineage, no evidence.
    for table in [
        "import_receipts",
        "imported_records",
        "imported_record_evidence",
    ] {
        assert_eq!(
            rows(&destination_path, table),
            0,
            "a refused import wrote `{table}`"
        );
    }

    // The same document is refused on the file path too, so nothing reaches
    // `import_export` in the first place.
    let bytes = serde_json::to_vec(&export).expect("the document serializes");
    assert!(
        matches!(
            kontor_store::backup::KontorExportV1::parse(&bytes),
            Err(kontor_store::backup::BackupError::Redaction { .. })
        ),
        "parsing accepted a document carrying a seat bearer"
    );
}

/// An authentic schema-12 document verifies, imports, and back-fills empty.
///
/// The generation-13 field is injected on parse so an older document presents
/// the current record type. That injection must be invisible to every path that
/// reproduces the source's own bytes: a genuine v12 export was hashed over
/// records that never had the key, and a v12 continuity summary never counted
/// it. Reading the document must not change what it says.
///
/// The fixture below is emitted the way a schema-12 implementation emitted one —
/// the key is absent, not empty, and the digest is taken over those exact bytes
/// (ASMA-8187 P1).
#[test]
fn an_authentic_schema_twelve_document_verifies_and_imports_with_an_empty_backfill() {
    let source = Fixture::build();
    let (seat, first) = source.lsa(SeatAutonomy::Supervised);
    three_succession_states(&source, seat, &first);
    let current = kontor_store::backup::export_realm(&source.store, at("2026-09-17T08:00:00Z"))
        .expect("the source Realm exports");
    assert!(
        !current.records.core_team_route_successions.is_empty(),
        "the fixture must start from a Realm that does hold the ledger"
    );

    // Emit as generation twelve: the key is absent from the records object and
    // from the continuity vocabulary, and the database is one that predates the
    // ledger, so the document is not claiming to have dropped anything.
    let mut document = serde_json::to_value(&current).expect("the export serializes");
    document["schema_version"] = serde_json::json!(12);
    document["database_schema_version"] = serde_json::json!(119);
    document["records"]
        .as_object_mut()
        .expect("a records object")
        .remove("core_team_route_successions")
        .expect("the current generation carries the field");
    document["continuity_summary"]["record_counts"]
        .as_object_mut()
        .expect("a counts object")
        .remove("core_team_route_successions")
        .expect("the current generation counts the field");
    // Generation fourteen's desks are later still (ASMA-8450).
    document["records"]
        .as_object_mut()
        .expect("a records object")
        .remove("desks")
        .expect("the current generation carries desks");
    document["continuity_summary"]["record_counts"]
        .as_object_mut()
        .expect("a counts object")
        .remove("desks")
        .expect("the current generation counts desks");
    let authentic_records_bytes = {
        let mut bytes = serde_json::to_vec(&document["records"]).expect("the records serialize");
        bytes.push(b'\n');
        bytes
    };
    document["records_hash"] =
        serde_json::json!(ContentHash::of(&authentic_records_bytes).to_string());
    let bytes = serde_json::to_vec(&document).expect("the document serializes");

    // It parses, and the field arrives empty rather than absent.
    let parsed = kontor_store::backup::KontorExportV1::parse(&bytes)
        .expect("an authentic schema-12 document verifies");
    assert_eq!(parsed.schema_version, 12);
    assert!(
        parsed.records.core_team_route_successions.is_empty(),
        "the back-fill is not empty"
    );

    // And reading it did not change what it says: the digest still describes
    // the source's own bytes, and the continuity vocabulary is still v12's.
    assert_eq!(
        parsed
            .canonical_records_bytes()
            .expect("the records canonicalize"),
        authentic_records_bytes,
        "parsing rehashed an authentic v12 document as something else"
    );
    assert!(
        !parsed
            .continuity_summary
            .record_counts
            .contains_key("core_team_route_successions"),
        "a v12 continuity summary gained a generation-13 count"
    );

    // It imports, and brings no Core Team evidence with it, because it carried
    // none.
    let destination_home = support::created_state_root();
    let destination_path = destination_home.path().join("kontor.db");
    let destination = support::open_created_realm(&destination_path);
    let destination_project = ProjectId::generate();
    destination
        .create_project(&NewProject {
            id: destination_project,
            name: name("Legacy receiving project"),
            root_path: name("/tmp/hosted-seat-legacy"),
            created_at: at("2026-09-17T08:01:00Z"),
        })
        .expect("the destination project is created");
    let report = kontor_store::backup::import_export(
        &destination,
        &parsed,
        &kontor_store::backup::ImportPlan::redacted_import_into(destination_project),
        at("2026-09-17T08:02:00Z"),
    )
    .expect("an authentic schema-12 document imports");
    assert!(
        destination
            .imported_records(&report.import_id.as_hyphenated().to_string())
            .expect("the lineage reads")
            .iter()
            .all(|row| row.record_kind != "core_team_route_successions"),
        "a v12 document produced Core Team lineage it never carried"
    );
    assert_eq!(rows(&destination_path, "imported_record_evidence"), 0);
}

/// The canonical intent one succession's command is admitted under.
///
/// Shared by the claim and by the command receipt because the binder now
/// requires them to agree: a receipt whose intent differs from the claim's is a
/// receipt for another command, whatever key it wears (ASMA-8187 P2).
fn succession_intent(key: &IdempotencyKey) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "operation": "core_team_route_correction",
        "idempotency_key": key.as_str(),
    }))
    .expect("the intent canonicalizes")
}

/// Record the exact command receipt this succession's binder requires.
fn recorded_receipt(fixture: &Fixture, key: &IdempotencyKey) -> CommandReceiptId {
    kontor_core::repository::CommandRepository::record_local_command(
        &fixture.store,
        &kontor_core::repository::NewLocalCommand {
            project_id: fixture.project_id,
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: key.clone(),
            kind: kontor_core::receipt::CommandKind::CorrectCoreTeamRoute,
            target: kontor_core::receipt::AggregateRef::MiniProject {
                mini_project_id: fixture.mini_project_id,
            },
            target_revision: kontor_core::id::AggregateRevision::INITIAL,
            intent: succession_intent(key),
            created_at: at("2026-09-17T02:05:30Z"),
        },
    )
    .expect("the command receipt is recorded")
    .id
}

// ---------------------------------------------------------------------------
// ASMA-8187 P2 — a receipt is evidence for one command, and for one only.
// ---------------------------------------------------------------------------

/// A succession cannot bind a receipt recorded for another command.
///
/// The foreign key proves the receipt exists. It says nothing about whose it
/// is, and the binder used to take the caller's word: any recorded receipt id
/// would do. A succession pointing at somebody else's completion is a ledger
/// asserting something it never saw (ASMA-8187 P2).
#[test]
fn a_succession_cannot_bind_a_receipt_recorded_for_another_command() {
    let fixture = Fixture::build();
    let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);
    let key = IdempotencyKey::parse("asma-8187-own-receipt").expect("a key");
    let stranger = IdempotencyKey::parse("asma-8187-other-command").expect("a key");
    let successor = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 2),
        observed_at: at("2026-09-17T09:01:00Z"),
        ..predecessor.clone()
    };
    claim_step(&fixture, &key, seat, &predecessor, 1);
    commit_step(
        &fixture,
        &key,
        seat,
        &predecessor,
        &successor,
        1,
        at("2026-09-17T09:01:30Z"),
    );
    land_effects(&fixture, seat, &successor, 2);
    fixture
        .store
        .commit_core_team_route_succession_effects(&key)
        .expect("the effects are proved");

    // A perfectly real receipt — for a different command.
    let foreign = recorded_receipt(&fixture, &stranger);
    let refused = fixture
        .store
        .bind_core_team_route_succession_receipt(
            &key,
            succession_intent(&key).hash(),
            foreign,
            at("2026-09-17T09:02:00Z"),
        )
        .expect_err("a succession bound another command's receipt");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the receipt was recorded for another command",
                ..
            }
        ),
        "{refused:?}"
    );
    assert!(
        fixture
            .store
            .get_core_team_route_succession(&key)
            .expect("the ledger reads")
            .expect("the row exists")
            .receipt_id
            .is_none(),
        "a refused binding still wrote a receipt"
    );

    // Its own receipt binds, and the instant it bound at is then immutable.
    let own = recorded_receipt(&fixture, &key);
    fixture
        .store
        .bind_core_team_route_succession_receipt(
            &key,
            succession_intent(&key).hash(),
            own,
            at("2026-09-17T09:03:00Z"),
        )
        .expect("a succession binds the receipt its own command recorded");

    let _ = own;
}

/// The instant a succession completed cannot be rewritten afterwards.
///
/// Split from the uniqueness rule deliberately. Both arrived in one migration
/// and were once proved by one assertion, so a mutant could remove either guard
/// and still be caught by the other's failure — the test could not say which
/// rule was doing the work (ASMA-8187 P2).
#[test]
fn a_bound_succession_cannot_have_its_completion_instant_rewritten() {
    let fixture = Fixture::build();
    let (_seat, key, _successor) = bound_succession(&fixture, "asma-8187-frozen-instant");
    let connection = Connection::open(&fixture.db_path).expect("the database opens");
    assert!(
        connection
            .execute(
                "UPDATE core_team_route_successions SET receipted_at = '2026-01-01T00:00:00Z'
                  WHERE idempotency_key = ?1",
                rusqlite::params![key.as_str()],
            )
            .is_err(),
        "the instant a succession completed was rewritten"
    );
}

/// One receipt completes one succession, by any route at all.
///
/// Reached without asking the binder, because the binder already refuses a
/// foreign receipt on identity: the rule under test here is the schema's, and a
/// test that went through the binder would be proving the binder again.
#[test]
fn one_receipt_cannot_be_bound_to_two_successions() {
    let fixture = Fixture::build();
    let (seat, key, successor) = bound_succession(&fixture, "asma-8187-one-receipt");
    let bound = fixture
        .store
        .get_core_team_route_succession(&key)
        .expect("the ledger reads")
        .expect("the row exists")
        .receipt_id
        .expect("the succession is bound");

    // A second succession on the same seat, carried all the way to complete and
    // deliberately left unbound.
    //
    // Complete matters: `0120` already forbids a receipt on a row whose effects
    // have not landed, so an incomplete second row would be refused by *that*
    // rule and the uniqueness index would never be reached. The first version of
    // this test made exactly that mistake, and the uniqueness mutant survived it.
    let later = IdempotencyKey::parse("asma-8187-second-claimant").expect("a key");
    let third = StoredHostedTopologySeat {
        native_identity: identity("lsa-third", 3),
        observed_at: at("2026-09-17T09:04:00Z"),
        ..successor.clone()
    };
    claim_step(&fixture, &later, seat, &successor, 2);
    commit_step(
        &fixture,
        &later,
        seat,
        &successor,
        &third,
        2,
        at("2026-09-17T09:04:30Z"),
    );
    land_effects(&fixture, seat, &third, 3);
    fixture
        .store
        .commit_core_team_route_succession_effects(&later)
        .expect("the second succession's effects are proved");

    let connection = Connection::open(&fixture.db_path).expect("the database opens");
    assert!(
        connection
            .execute(
                "UPDATE core_team_route_successions
                    SET receipt_id = ?2, receipted_at = '2026-09-17T09:05:00Z'
                  WHERE idempotency_key = ?1",
                rusqlite::params![later.as_str(), bound.to_string()],
            )
            .is_err(),
        "one receipt was bound to two successions"
    );
}

/// One complete, receipted succession on a fresh seat.
fn bound_succession(
    fixture: &Fixture,
    key: &str,
) -> (SeatBindingId, IdempotencyKey, StoredHostedTopologySeat) {
    let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);
    let key = IdempotencyKey::parse(key).expect("a key");
    let successor = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 2),
        observed_at: at("2026-09-17T09:01:00Z"),
        ..predecessor.clone()
    };
    let committed_at = at("2026-09-17T09:01:30Z");
    claim_step(fixture, &key, seat, &predecessor, 1);
    commit_step(
        fixture,
        &key,
        seat,
        &predecessor,
        &successor,
        1,
        committed_at,
    );
    land_effects(fixture, seat, &successor, 2);
    fixture
        .store
        .commit_core_team_route_succession_effects(&key)
        .expect("the effects are proved");
    fixture
        .store
        .bind_core_team_route_succession_receipt(
            &key,
            succession_intent(&key).hash(),
            recorded_receipt(fixture, &key),
            at("2026-09-17T09:03:00Z"),
        )
        .expect("the succession binds its own receipt");
    (seat, key, successor)
}

/// A complete readback for one succession step, built as the domain type.
///
/// The suite used to hand-write partial JSON objects, which agreed with the
/// ledger on the handful of fields anybody compared and said nothing about the
/// rest. Building the real structure is what makes the completeness rules
/// testable at all (ASMA-8187 P2).
fn full_readback(
    seat: SeatBindingId,
    predecessor: &StoredHostedTopologySeat,
    predecessor_occupancy: u64,
    successor: &StoredHostedTopologySeat,
    successor_occupancy: u64,
    retired_at: Timestamp,
) -> kontor_core::repository::CoreTeamRouteSuccessionReadback {
    let occupant = |seat: &StoredHostedTopologySeat, occupancy: u64| {
        kontor_core::repository::CoreTeamRouteOccupant {
            native_id: seat.native_identity.native_id.clone(),
            runtime_kind: seat.native_identity.runtime_kind.as_str().to_owned(),
            host: seat.native_identity.host.as_str().to_owned(),
            generation: seat.native_identity.generation,
            provider_session_id: seat.provider_session_id.clone(),
            occupancy_generation: occupancy,
            model_route: seat.model_rung.clone(),
        }
    };
    kontor_core::repository::CoreTeamRouteSuccessionReadback {
        seat_binding_id: seat,
        predecessor: occupant(predecessor, predecessor_occupancy),
        successor: occupant(successor, successor_occupancy),
        grant_subject: kontor_core::repository::CoreTeamRouteGrantSubject {
            generation: successor_occupancy,
            subject_seat_binding_id: seat,
            subject_digest: kontor_core::repository::core_team_grant_subject_digest(
                seat,
                successor_occupancy,
            ),
        },
        retired_at: retired_at.to_string(),
    }
}

/// The same readback as JSON, with its canonical digest.
fn readback_document(
    readback: &kontor_core::repository::CoreTeamRouteSuccessionReadback,
) -> (serde_json::Value, ContentHash) {
    let value = serde_json::to_value(readback).expect("the readback serializes");
    let hash = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "readback": value,
    }))
    .expect("the readback canonicalizes")
    .hash()
    .clone();
    (value, hash)
}

/// Every derived and redundant value in a readback is re-proved, not read.
///
/// A readback repeats what the ledger already knows and carries one value —
/// the grant-subject digest — that is derived from values it carries itself.
/// Repetition is only evidence if somebody checks it; otherwise it is a second
/// copy of whatever the caller decided to say (ASMA-8187 P2).
#[test]
fn a_readback_whose_derived_or_redundant_values_are_wrong_is_refused() {
    let fixture = Fixture::build();
    let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);
    let successor = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 2),
        observed_at: at("2026-09-17T10:01:00Z"),
        ..predecessor.clone()
    };
    let committed_at = at("2026-09-17T10:01:30Z");
    let rehash = |value: &serde_json::Value| {
        CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "readback": value,
        }))
        .expect("it canonicalizes")
        .hash()
        .clone()
    };

    // A digest that is not the one its own subject produces, refused where the
    // document is written.
    let key = IdempotencyKey::parse("asma-8187-bad-digest").expect("a key");
    claim_step(&fixture, &key, seat, &predecessor, 1);
    let mut wrong_digest = full_readback(seat, &predecessor, 1, &successor, 2, committed_at);
    wrong_digest.grant_subject.subject_digest = ContentHash::of(b"a digest of another subject");
    let value = serde_json::to_value(&wrong_digest).expect("it serializes");
    let refused = fixture
        .store
        .replace_hosted_topology_seat_route(
            &predecessor,
            &successor,
            committed_at,
            "derived values",
            Some(&CoreTeamRouteSuccessionCommit {
                idempotency_key: key.clone(),
                readback: value.clone(),
                readback_hash: rehash(&value),
                route_committed_at: committed_at,
            }),
        )
        .expect_err("a fabricated grant-subject digest was persisted");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the succession readback's grant subject digest is not the one it describes",
                ..
            }
        ),
        "{refused:?}"
    );

    // A readback naming one native as both occupants describes no succession.
    let mut circular = full_readback(seat, &predecessor, 1, &successor, 2, committed_at);
    circular.successor.native_id = predecessor.native_identity.native_id.clone();
    circular.successor.generation = predecessor.native_identity.generation;
    let value = serde_json::to_value(&circular).expect("it serializes");
    let refused = fixture
        .store
        .replace_hosted_topology_seat_route(
            &predecessor,
            &successor,
            committed_at,
            "derived values",
            Some(&CoreTeamRouteSuccessionCommit {
                idempotency_key: key.clone(),
                readback: value.clone(),
                readback_hash: rehash(&value),
                route_committed_at: committed_at,
            }),
        )
        .expect_err("a readback naming one native twice was persisted");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the succession readback names one native as both occupants",
                ..
            }
        ),
        "{refused:?}"
    );

    // And a complete, internally coherent, correctly hashed readback whose
    // successor generation is not the one the ledger recorded is refused at
    // binding — where the ledger is.
    let mut drifting = full_readback(seat, &predecessor, 1, &successor, 2, committed_at);
    drifting.successor.generation = 99;
    drifting
        .check_internal_consistency()
        .expect("the drifting readback is coherent on its own terms");
    let value = serde_json::to_value(&drifting).expect("it serializes");
    let refused = fixture
        .store
        .replace_hosted_topology_seat_route(
            &predecessor,
            &successor,
            committed_at,
            "derived values",
            Some(&CoreTeamRouteSuccessionCommit {
                idempotency_key: key.clone(),
                readback: value.clone(),
                readback_hash: rehash(&value),
                route_committed_at: committed_at,
            }),
        )
        .expect_err("a readback claiming another runtime generation was persisted");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the succession readback does not describe the successor being installed",
                ..
            }
        ),
        "{refused:?}"
    );
    assert!(
        fixture
            .store
            .get_core_team_route_succession(&key)
            .expect("the ledger reads")
            .expect("the claim survives")
            .route_committed_at
            .is_none(),
        "a refused readback still committed its route"
    );
}

// ---------------------------------------------------------------------------
// ASMA-8187 P2 — the upgrade asks the rule of the rows that predate it.
// ---------------------------------------------------------------------------

/// Rewind one realm to schema 121: remove the later 0123/0124 memory tables
/// and what `0122` added, restore `0120`'s trigger, and stamp `user_version`.
///
/// The rewind is what makes the upgrade reachable from a test at all. A realm
/// that has already run `0122` cannot run it again, so a fixture that wants to
/// prove what the upgrade does to pre-existing rows has to put the realm back
/// where those rows lived.
fn rewind_to_schema_121(path: &std::path::Path) {
    let connection = Connection::open(path).expect("the database opens");
    // This stopped fixture started at today's schema. A historical stamp must
    // also remove the later additive tables, or a forward open would correctly
    // refuse to create them again. Refuse to erase any fixture evidence.
    for table in [
        "desks",
        "prepared_attestation_tokens",
        "attestation_token_heads",
        "attestation_authority_keys",
        "attestation_authority_heads",
        "planning_pair_member_natives",
        "planning_pair_contributions",
        "planning_pair_record_revisions",
        "planning_pair_placements",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("the later fixture table reads");
        assert_eq!(
            count, 0,
            "a historical rewind must not erase {table} evidence"
        );
        connection
            .execute_batch(&format!("DROP TABLE {table};"))
            .expect("the empty later fixture table is removed");
    }
    connection
        .execute_batch(
            "DROP TABLE memory_projection_rebuild_results;
             DROP TABLE memory_projection_rebuild_keys;
             DROP TABLE memory_recall_keys;
             DROP TABLE memory_recall_metadata;
             DROP TABLE memory_experience_proposals;
             DROP TABLE memory_projection_active;
             DROP TABLE memory_projection_snapshots;
             DROP TABLE memory_experience_eligibility;
             DROP INDEX ux_core_team_route_succession_receipt;
             DROP TRIGGER core_team_route_succession_claim_is_frozen;",
        )
        .expect("the generation-122 artefacts are removed");
}

/// Restore `0120`'s frozen-claim trigger and stamp the realm at 121.
fn stamp_schema_121(path: &std::path::Path) {
    let connection = Connection::open(path).expect("the database opens");
    connection
        .execute_batch(concat!(
            r#"CREATE TRIGGER core_team_route_succession_claim_is_frozen
BEFORE UPDATE ON core_team_route_successions
WHEN OLD.idempotency_key IS NOT NEW.idempotency_key
  OR OLD.intent_hash IS NOT NEW.intent_hash
  OR OLD.project_id IS NOT NEW.project_id
  OR OLD.mini_project_id IS NOT NEW.mini_project_id
  OR OLD.seat_binding_id IS NOT NEW.seat_binding_id
  OR OLD.predecessor_native_id IS NOT NEW.predecessor_native_id
  OR OLD.predecessor_generation IS NOT NEW.predecessor_generation
  OR OLD.predecessor_occupancy_generation IS NOT NEW.predecessor_occupancy_generation
  OR OLD.successor_occupancy_generation IS NOT NEW.successor_occupancy_generation
  OR OLD.successor_credential_generation IS NOT NEW.successor_credential_generation
  OR OLD.claimed_at IS NOT NEW.claimed_at
  -- A committed transition is immutable evidence.
  OR (OLD.route_committed_at IS NOT NULL AND (
         OLD.successor_native_id IS NOT NEW.successor_native_id
      OR OLD.successor_generation IS NOT NEW.successor_generation
      OR OLD.readback IS NOT NEW.readback
      OR OLD.readback_hash IS NOT NEW.readback_hash
      OR OLD.route_committed_at IS NOT NEW.route_committed_at))
  -- The receipt binds once and never moves.
  OR (OLD.receipt_id IS NOT NULL AND OLD.receipt_id IS NOT NEW.receipt_id)
  -- Pending effects latch forward only.
  OR (OLD.launch_intent_installed = 1 AND NEW.launch_intent_installed = 0)
  OR (OLD.seat_binding_observed = 1 AND NEW.seat_binding_observed = 0)
BEGIN
    SELECT RAISE(ABORT,
        'a Core Team route succession cannot rewrite its claim or its committed evidence');
END;"#,
            "\nPRAGMA user_version = 121;",
        ))
        .expect("the realm is stamped at 121");
}

/// A binding this generation cannot vouch for stops the upgrade.
///
/// `0122` starts proving that a bound receipt is its succession's own. Rows
/// bound before it were bound by a binder that proved none of that, so the rule
/// has to be asked of them once rather than assumed from the day enforcement
/// begins. It fails closed: a realm carrying an unverifiable binding does not
/// open, because the alternative is a database that quietly calls it verified
/// (ASMA-8187 P2).
#[test]
fn a_mismatched_historical_binding_stops_the_upgrade_at_0122() {
    let fixture = Fixture::build();
    let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);
    let key = IdempotencyKey::parse("asma-8187-upgrade-bad").expect("a key");
    let stranger = IdempotencyKey::parse("asma-8187-upgrade-other").expect("a key");
    let successor = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 2),
        observed_at: at("2026-09-17T11:01:00Z"),
        ..predecessor.clone()
    };
    let committed_at = at("2026-09-17T11:01:30Z");
    claim_step(&fixture, &key, seat, &predecessor, 1);
    commit_step(
        &fixture,
        &key,
        seat,
        &predecessor,
        &successor,
        1,
        committed_at,
    );
    land_effects(&fixture, seat, &successor, 2);
    fixture
        .store
        .commit_core_team_route_succession_effects(&key)
        .expect("the effects are proved");
    fixture
        .store
        .bind_core_team_route_succession_receipt(
            &key,
            succession_intent(&key).hash(),
            recorded_receipt(&fixture, &key),
            at("2026-09-17T11:02:00Z"),
        )
        .expect("the succession binds its own receipt");
    // A second, entirely real receipt — for a different command.
    let foreign = recorded_receipt(&fixture, &stranger);
    // Close the store but keep its directory: `TempDir` deletes on drop, and the
    // reopen below needs the file to still be there.
    let Fixture {
        _home,
        db_path,
        store,
        ..
    } = fixture;
    drop(store);
    let path = db_path;

    // Put the realm back where a pre-122 binder lived, repoint the binding at
    // the foreign receipt the old binder would have accepted, and stamp it.
    rewind_to_schema_121(&path);
    Connection::open(&path)
        .expect("the database opens")
        .execute(
            "UPDATE core_team_route_successions SET receipt_id = ?2 WHERE idempotency_key = ?1",
            rusqlite::params![key.as_str(), foreign.to_string()],
        )
        .expect("the historical mis-binding is planted");
    stamp_schema_121(&path);

    // The upgrade must refuse to open it.
    let refused = SqliteStore::open(&path);
    assert!(
        refused.is_err(),
        "a realm carrying a binding this generation cannot vouch for was opened anyway"
    );

    // And refusing left the realm as it found it: the migration's work rolls
    // back, so nothing is half-upgraded and the row is still there to inspect.
    let connection = Connection::open(&path).expect("the database opens");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version reads");
    assert_eq!(
        version, 121,
        "a refused upgrade left the realm part-migrated"
    );
    let bound: String = connection
        .query_row(
            "SELECT receipt_id FROM core_team_route_successions WHERE idempotency_key = ?1",
            rusqlite::params![key.as_str()],
            |row| row.get(0),
        )
        .expect("the row survives");
    assert_eq!(
        bound,
        foreign.to_string(),
        "a refused upgrade rewrote the row"
    );
}

/// A valid historical binding upgrades untouched.
///
/// The companion to the refusal, and the reason the check reads rather than
/// repairs: a realm whose bindings were already coherent must cross `0122`
/// without anything being rewritten, quarantined or dropped.
#[test]
fn a_valid_historical_binding_survives_the_upgrade_to_0122() {
    let fixture = Fixture::build();
    let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);
    let key = IdempotencyKey::parse("asma-8187-upgrade-good").expect("a key");
    let successor = StoredHostedTopologySeat {
        native_identity: identity("lsa-second", 2),
        observed_at: at("2026-09-17T11:05:00Z"),
        ..predecessor.clone()
    };
    let committed_at = at("2026-09-17T11:05:30Z");
    claim_step(&fixture, &key, seat, &predecessor, 1);
    commit_step(
        &fixture,
        &key,
        seat,
        &predecessor,
        &successor,
        1,
        committed_at,
    );
    land_effects(&fixture, seat, &successor, 2);
    fixture
        .store
        .commit_core_team_route_succession_effects(&key)
        .expect("the effects are proved");
    let receipt = recorded_receipt(&fixture, &key);
    fixture
        .store
        .bind_core_team_route_succession_receipt(
            &key,
            succession_intent(&key).hash(),
            receipt,
            at("2026-09-17T11:06:00Z"),
        )
        .expect("the succession binds its own receipt");
    let Fixture {
        _home,
        db_path,
        store,
        ..
    } = fixture;
    drop(store);
    let path = db_path;

    rewind_to_schema_121(&path);
    stamp_schema_121(&path);

    let reopened = SqliteStore::open(&path).expect("a coherent realm upgrades");
    assert_eq!(
        reopened.schema_version().expect("the version reads"),
        kontor_store::SCHEMA_VERSION
    );
    let settled = reopened
        .get_core_team_route_succession(&key)
        .expect("the ledger reads")
        .expect("the row survives the upgrade");
    assert_eq!(
        settled.receipt_id,
        Some(receipt),
        "the upgrade moved a valid binding"
    );
    assert!(settled.is_complete());
    assert_eq!(
        settled.receipted_at.map(|instant| instant.to_string()),
        Some(at("2026-09-17T11:06:00Z").to_string()),
        "the upgrade rewrote a valid completion instant"
    );
}

/// Every readback field is compared to the transition, not just the ones the
/// ledger happens to duplicate.
///
/// Runtime kind, host, provider session, model route and the retirement instant
/// are carried by the readback and by nothing else durable. If they are never
/// compared they are never evidence: a complete, internally coherent, correctly
/// hashed document could describe a different placement, on a different host,
/// running a different route, retired at a different time, and the ledger's two
/// identity columns would agree with all of it.
///
/// Each case is rehashed after mutation, so the digest is never what refuses,
/// and each must leave the route exactly where it was (ASMA-8187 P2).
#[test]
fn every_readback_field_is_bound_to_the_transition_it_commits() {
    type Mutation = fn(&mut kontor_core::repository::CoreTeamRouteSuccessionReadback);
    let committed_at = at("2026-09-17T12:01:30Z");
    let successor_rule = "the succession readback does not describe the successor being installed";
    let cases: [(&str, &str, Mutation); 6] = [
        ("runtime kind", successor_rule, |readback| {
            readback.successor.runtime_kind = "another.runtime".to_owned();
        }),
        ("host", successor_rule, |readback| {
            readback.successor.host = "another-host".to_owned();
        }),
        ("provider session", successor_rule, |readback| {
            readback.successor.provider_session_id =
                Some(ExternalId::parse("another-session").expect("a session id"));
        }),
        ("model route", successor_rule, |readback| {
            readback.successor.model_route.model =
                kontor_core::spec::ModelRef("another-model".to_owned());
        }),
        (
            "predecessor host",
            "the succession readback does not describe the predecessor being retired",
            |readback| readback.predecessor.host = "another-host".to_owned(),
        ),
        (
            "retirement instant",
            "the succession readback records another retirement instant",
            |readback| readback.retired_at = at("2020-01-01T00:00:00Z").to_string(),
        ),
    ];

    for (field, expected, mutate) in cases {
        let fixture = Fixture::build();
        let (seat, predecessor) = fixture.lsa(SeatAutonomy::Supervised);
        let successor = StoredHostedTopologySeat {
            native_identity: identity("lsa-second", 2),
            provider_session_id: Some(ExternalId::parse("session-two").expect("a session id")),
            observed_at: at("2026-09-17T12:01:00Z"),
            ..predecessor.clone()
        };
        let key = IdempotencyKey::parse("asma-8187-field-bound").expect("a key");
        claim_step(&fixture, &key, seat, &predecessor, 1);

        let mut readback = full_readback(seat, &predecessor, 1, &successor, 2, committed_at);
        mutate(&mut readback);
        let (value, hash) = readback_document(&readback);

        let refused = fixture
            .store
            .replace_hosted_topology_seat_route(
                &predecessor,
                &successor,
                committed_at,
                "field binding",
                Some(&CoreTeamRouteSuccessionCommit {
                    idempotency_key: key.clone(),
                    readback: value,
                    readback_hash: hash,
                    route_committed_at: committed_at,
                }),
            )
            .unwrap_err();
        assert!(
            matches!(
                &refused,
                kontor_core::repository::RepositoryError::Conflict { rule, .. } if *rule == expected
            ),
            "a wrong {field} was refused for the wrong reason: {refused:?}"
        );

        // Zero route movement, every time.
        assert_eq!(
            fixture
                .store
                .get_hosted_topology_seat(fixture.project_id, seat)
                .expect("the seat reads")
                .expect("the seat exists")
                .native_identity
                .native_id,
            predecessor.native_identity.native_id,
            "a refused {field} still replaced the occupant"
        );
        assert!(
            fixture
                .store
                .get_core_team_route_succession(&key)
                .expect("the ledger reads")
                .expect("the claim survives")
                .route_committed_at
                .is_none(),
            "a refused {field} still committed its route"
        );
        assert!(
            fixture
                .store
                .list_hosted_topology_seat_history_native_ids(fixture.project_id, seat)
                .expect("the history reads")
                .is_empty(),
            "a refused {field} still retired the predecessor"
        );
    }
}

/// A replayed binding is proved before it may answer `Unchanged`.
///
/// The idempotent path used to return first and verify never. That made the
/// most common way this row is read the one way it was never checked: a row
/// bound by some other route — an upgrade, a restore, a direct write — would be
/// re-affirmed on replay without its receipt or its readback being looked at
/// once.
///
/// Reaching the ordering needs a row that is *already* bound and whose binding
/// does not hold, which the binder itself will not produce; the planted state
/// below is what another writer could leave behind (ASMA-8187 P2).
#[test]
fn a_replayed_binding_is_proved_before_it_answers_unchanged() {
    let fixture = Fixture::build();
    let (_seat, key, _successor) = bound_succession(&fixture, "asma-8187-replay-proof");
    let stranger = IdempotencyKey::parse("asma-8187-replay-other").expect("a key");
    let foreign = recorded_receipt(&fixture, &stranger);

    // Repoint the existing binding at a receipt recorded for another command.
    // The frozen trigger exists to stop exactly this, so it is taken out of the
    // way first — that is the point: the binder must not depend on it.
    let connection = Connection::open(&fixture.db_path).expect("the database opens");
    connection
        .execute_batch("DROP TRIGGER core_team_route_succession_claim_is_frozen;")
        .expect("the freeze is lifted for the plant");
    connection
        .execute(
            "UPDATE core_team_route_successions SET receipt_id = ?2 WHERE idempotency_key = ?1",
            rusqlite::params![key.as_str(), foreign.to_string()],
        )
        .expect("the mis-binding is planted");

    // Replaying with the receipt the row now names must be refused, not
    // confirmed. An `Unchanged` here would be the ledger agreeing with itself.
    let refused = fixture
        .store
        .bind_core_team_route_succession_receipt(
            &key,
            succession_intent(&key).hash(),
            foreign,
            at("2026-09-17T13:00:00Z"),
        )
        .expect_err("a replay confirmed a binding nobody had proved");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                rule: "the receipt was recorded for another command",
                ..
            }
        ),
        "{refused:?}"
    );
}
