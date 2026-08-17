//! OP-02 native container persistence and the OP-REQ-039 attachment evidence.
//!
//! The three mutants below are the three ways the pre-OP-02 reader concluded a
//! seat was healthy when it was not. Each one is written so a correct
//! implementation and the defect it replaces give *different* answers: a test
//! that passes under both proves nothing about which one is running.

use kontor_core::id::{
    ExternalId, ExternalName, MiniProjectId, ProjectId, RoleCode, RoleSlotId, RuntimeKindKey,
    SeatBindingId, Timestamp, TopologyKindKey, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::repository::{
    MiniProjectTopologySnapshot, NewMiniProject, NewNativeContainerBinding, NewProject,
    NewSeatBinding, NewSessionTopologyNode, ProjectRepository, ProjectTopologyDefault,
    SeatLivenessObservation, TopologyRepository,
};
use kontor_core::spec::{CatalogRoleRef, Shareability, ShareabilityTier, TopologySnapshot};
use kontor_core::state::{
    NativeRuntimeIdentity, ObservedContainerKind, ObservedRunState, SeatAttachment,
};
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use kontor_store::backup::export_realm;
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

/// A project with the bundled Operational topology published, one epic node and
/// one control-plane node below it.
struct Fixture {
    home: TempDir,
    store: SqliteStore,
    project_id: ProjectId,
    epic_id: TopologyNodeId,
    ecp_id: TopologyNodeId,
    tsw_id: TopologyNodeId,
    snapshot: TopologySnapshot,
    mini_project_id: MiniProjectId,
    catalog_id: kontor_core::id::RoleCatalogId,
    catalog_version: kontor_core::id::SpecVersion,
}

impl Fixture {
    fn build() -> Self {
        let home = TempDir::new().expect("a temporary directory");
        let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
        let project_id = ProjectId::generate();
        let mini_project_id = MiniProjectId::generate();
        let created_at = at("2026-08-16T01:00:00Z");
        let stamp = Shareability::default_for(ShareabilityTier::ProjectKnowledge)
            .expect("tier B classifies");

        store
            .create_project(&NewProject {
                id: project_id,
                name: name("Operational project"),
                root_path: name("/tmp/operational-liveness"),
                created_at,
            })
            .expect("the project is created");
        store
            .create_mini_project(&NewMiniProject {
                id: mini_project_id,
                project_id,
                name: name("Operational epic"),
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
                topology: snapshot.clone(),
                kind: TopologyKindKey::parse("ECP").expect("the control-plane kind"),
                parent_id: Some(epic_id),
                task_id: None,
                created_at,
            })
            .expect("the control-plane node is created");
        // A task workspace under the same epic. Its seats are owned by the
        // epic's control seats, which is what makes orphanhood observable.
        let tsw_id = TopologyNodeId::generate();
        store
            .create_topology_node(&NewSessionTopologyNode {
                id: tsw_id,
                project_id,
                mini_project_id: Some(mini_project_id),
                topology: snapshot.clone(),
                kind: TopologyKindKey::parse("TSW").expect("the task workspace kind"),
                parent_id: Some(epic_id),
                task_id: None,
                created_at,
            })
            .expect("the task workspace node is created");
        Self {
            home,
            store,
            project_id,
            epic_id,
            ecp_id,
            tsw_id,
            snapshot,
            mini_project_id,
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

    /// One seat on a node, with its attachment deadline fixed here and now.
    fn seat(
        &self,
        node: TopologyNodeId,
        code: &str,
        slot: &str,
        created_at: Timestamp,
        attach_deadline: Timestamp,
        parent: Option<SeatBindingId>,
    ) -> SeatBindingId {
        let id = SeatBindingId::generate();
        self.store
            .create_seat_binding(&NewSeatBinding {
                id,
                project_id: self.project_id,
                topology_node_id: node,
                role_slot_id: RoleSlotId::parse(slot).expect("a role slot"),
                role: self.role(code),
                task_id: None,
                team_run_id: None,
                attach_deadline,
                parent_seat_binding_id: parent,
                created_at,
            })
            .expect("the seat binding is created");
        id
    }
}

// ---------------------------------------------------------------------------
// Native container persistence
// ---------------------------------------------------------------------------

fn identity(native_id: &str, generation: u64) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("a runtime kind"),
        host: name("paseo-local"),
        generation,
        native_id: ExternalId::parse(native_id).expect("a native id"),
    }
}

#[test]
fn a_native_container_binding_survives_restart_and_export() {
    let fixture = Fixture::build();
    let bound_at = at("2026-08-16T02:00:00Z");
    let bound = fixture
        .store
        .bind_topology_node_container(&NewNativeContainerBinding {
            topology_node_id: fixture.epic_id,
            project_id: fixture.project_id,
            container_binding_id: ExternalId::parse("prj_da432f9269aa936f")
                .expect("a container binding id"),
            identity: identity("prj_da432f9269aa936f", 7),
            observed_kind: ObservedContainerKind::Project,
            canonical_cwd: Some(name("/Users/igor/carasent/asma-modules")),
            observed_at: bound_at,
        })
        .expect("the epic node is bound to its native project");
    assert_eq!(bound.identity.generation, 7);

    // Restart: the process that held the binding is gone, the row is not.
    let database = fixture.home.path().join("kontor.db");
    drop(fixture.store);
    let store = SqliteStore::open(&database).expect("the store reopens");

    let after = store
        .get_topology_node_container(fixture.project_id, fixture.epic_id)
        .expect("the read succeeds")
        .expect("the binding survived the restart");
    // Every part of the identity, because any subset of it matches a container
    // a restart has already replaced.
    assert_eq!(after.identity, identity("prj_da432f9269aa936f", 7));
    assert_eq!(after.observed_kind, ObservedContainerKind::Project);
    assert_eq!(
        after.canonical_cwd.as_ref().map(ExternalName::as_str),
        Some("/Users/igor/carasent/asma-modules")
    );
    assert_eq!(after.bound_at, bound_at);
    assert_eq!(after.last_readback_at, bound_at);

    // And the export carries it, so a restore is not a quietly unplaced realm.
    let exported = export_realm(&store, at("2026-08-16T05:00:00Z")).expect("the realm exports");
    let row = exported
        .records
        .topology_node_containers
        .iter()
        .find(|row| row.topology_node_id == fixture.epic_id.to_string())
        .expect("the container binding is exported");
    assert_eq!(row.native_id, "prj_da432f9269aa936f");
    assert_eq!(row.generation, 7);
    assert_eq!(row.observed_kind, "project");
    assert_eq!(
        row.canonical_cwd.as_deref(),
        Some("/Users/igor/carasent/asma-modules")
    );
}

#[test]
fn re_confirming_a_binding_advances_the_readback_without_rebinding() {
    let fixture = Fixture::build();
    let request = |observed_at: Timestamp| NewNativeContainerBinding {
        topology_node_id: fixture.epic_id,
        project_id: fixture.project_id,
        container_binding_id: ExternalId::parse("prj_da432f9269aa936f").expect("a binding id"),
        identity: identity("prj_da432f9269aa936f", 7),
        observed_kind: ObservedContainerKind::Project,
        canonical_cwd: None,
        observed_at,
    };
    let first = fixture
        .store
        .bind_topology_node_container(&request(at("2026-08-16T02:00:00Z")))
        .expect("the node is bound");
    let again = fixture
        .store
        .bind_topology_node_container(&request(at("2026-08-16T03:00:00Z")))
        .expect("re-confirming the same container is idempotent");

    assert_eq!(again.bound_at, first.bound_at, "the binding is not remade");
    assert_eq!(
        again.last_readback_at,
        at("2026-08-16T03:00:00Z"),
        "a stale binding has to be visible as stale"
    );
}

#[test]
fn a_disagreeing_identity_is_reported_rather_than_silently_repaired() {
    let fixture = Fixture::build();
    fixture
        .store
        .bind_topology_node_container(&NewNativeContainerBinding {
            topology_node_id: fixture.epic_id,
            project_id: fixture.project_id,
            container_binding_id: ExternalId::parse("prj_da432f9269aa936f").expect("a binding id"),
            identity: identity("prj_da432f9269aa936f", 7),
            observed_kind: ObservedContainerKind::Project,
            canonical_cwd: None,
            observed_at: at("2026-08-16T02:00:00Z"),
        })
        .expect("the node is bound");

    // Kontor says one container, the runtime shows another. Rewriting the row
    // to match would make the node point at whatever was seen last.
    let refusal = fixture
        .store
        .bind_topology_node_container(&NewNativeContainerBinding {
            topology_node_id: fixture.epic_id,
            project_id: fixture.project_id,
            container_binding_id: ExternalId::parse("prj_0000000000000000").expect("a binding id"),
            identity: identity("prj_0000000000000000", 7),
            observed_kind: ObservedContainerKind::Project,
            canonical_cwd: None,
            observed_at: at("2026-08-16T04:00:00Z"),
        })
        .expect_err("a disagreement is not a rebinding");
    assert!(
        refusal.to_string().contains("another native container"),
        "{refusal:?}"
    );

    // The stored binding is untouched.
    let held = fixture
        .store
        .get_topology_node_container(fixture.project_id, fixture.epic_id)
        .expect("the read succeeds")
        .expect("the binding is still there");
    assert_eq!(held.identity, identity("prj_da432f9269aa936f", 7));
}

#[test]
fn one_native_container_cannot_be_claimed_by_two_nodes() {
    let fixture = Fixture::build();
    let request = |node: TopologyNodeId| NewNativeContainerBinding {
        topology_node_id: node,
        project_id: fixture.project_id,
        container_binding_id: ExternalId::parse("prj_da432f9269aa936f").expect("a binding id"),
        identity: identity("prj_da432f9269aa936f", 7),
        observed_kind: ObservedContainerKind::Project,
        canonical_cwd: None,
        observed_at: at("2026-08-16T02:00:00Z"),
    };
    fixture
        .store
        .bind_topology_node_container(&request(fixture.epic_id))
        .expect("the first node is bound");
    let refusal = fixture
        .store
        .bind_topology_node_container(&request(fixture.ecp_id))
        .expect_err("two nodes cannot own one native container");
    assert!(
        refusal.to_string().contains("another topology node"),
        "{refusal:?}"
    );
}

// ---------------------------------------------------------------------------
// The three OP-REQ-039 mutants
// ---------------------------------------------------------------------------

/// Mutant 1 — the deadline is recomputed at read time from `created_at`.
///
/// The seat below is created with a deadline of one minute and read five
/// minutes later. The stored deadline has passed, so the seat failed to attach.
/// A reader that re-derived `created_at + 10 minutes` instead would still be
/// inside the grace window and would answer `Pending` — which is how a seat that
/// never attached stays indistinguishable from one that is merely slow.
#[test]
fn the_attach_deadline_is_the_one_fixed_at_creation() {
    let fixture = Fixture::build();
    let created_at = at("2026-08-16T02:00:00Z");
    fixture.seat(
        fixture.ecp_id,
        "LSA",
        "epic.lsa",
        created_at,
        at("2026-08-16T02:01:00Z"),
        None,
    );

    let concluded = fixture
        .store
        .list_seat_attachments(
            fixture.project_id,
            fixture.ecp_id,
            at("2026-08-16T02:05:00Z"),
        )
        .expect("the read succeeds");
    assert_eq!(
        concluded,
        vec![SeatAttachment::AttachmentFailed],
        "the stored deadline had passed; only a re-derived one would still be open"
    );
}

/// Mutant 2 — a generic confirmation is read as activity.
///
/// The seat below was confirmed attached five minutes ago and says `running`,
/// and has produced no observed activity at all. Attachment is fresh; activity
/// is absent. A reader that took either the confirmation instant or the
/// runtime's self-report as activity would answer `Attached`, which is exactly
/// how a wedged seat looks busy for as long as anything keeps polling it.
#[test]
fn a_confirmation_and_a_self_report_are_not_activity() {
    let fixture = Fixture::build();
    let seat = fixture.seat(
        fixture.ecp_id,
        "LSA",
        "epic.lsa",
        at("2026-08-16T02:00:00Z"),
        at("2026-08-16T02:10:00Z"),
        None,
    );
    fixture
        .store
        .observe_seat_binding(
            fixture.project_id,
            seat,
            &SeatLivenessObservation {
                attached_at: Some(at("2026-08-16T03:00:00Z")),
                activity_at: None,
                runtime_reported: Some(ObservedRunState::Running),
                ..SeatLivenessObservation::default()
            },
            at("2026-08-16T03:00:00Z"),
        )
        .expect("the attachment is recorded");

    let concluded = fixture
        .store
        .list_seat_attachments(
            fixture.project_id,
            fixture.ecp_id,
            at("2026-08-16T03:05:00Z"),
        )
        .expect("the read succeeds");
    assert_eq!(
        concluded,
        vec![SeatAttachment::Stalled],
        "attached with no observed activity is a stall, whatever the runtime says"
    );

    // And an *observed* event is what moves it, which is the other half of the
    // same rule: the distinction has to make a difference in both directions.
    fixture
        .store
        .observe_seat_binding(
            fixture.project_id,
            seat,
            &SeatLivenessObservation {
                activity_at: Some(at("2026-08-16T03:04:00Z")),
                ..SeatLivenessObservation::default()
            },
            at("2026-08-16T03:04:00Z"),
        )
        .expect("the activity is recorded");
    assert_eq!(
        fixture
            .store
            .list_seat_attachments(
                fixture.project_id,
                fixture.ecp_id,
                at("2026-08-16T03:05:00Z")
            )
            .expect("the read succeeds"),
        vec![SeatAttachment::Attached]
    );
}

/// Mutant 3 — orphanhood is hard-coded to `false`.
///
/// The child seat below is attached and freshly active; its owning epic seat has
/// been released. Its owner is gone, so nothing is steering it. A reader that
/// could not see the parent — which is what `parent_closed: false` means — would
/// answer `Attached` and keep counting the seat as capacity.
#[test]
fn an_orphan_is_concluded_from_the_owning_epic_seat() {
    let fixture = Fixture::build();
    let created_at = at("2026-08-16T02:00:00Z");
    let deadline = at("2026-08-16T02:10:00Z");
    // The epic's control seat lives in the ECP; the task seat it owns lives in
    // a TSW under the same epic. The ESW itself hosts no seats at all.
    let parent = fixture.seat(
        fixture.ecp_id,
        "LSA",
        "epic.lsa",
        created_at,
        deadline,
        None,
    );
    let child = fixture.seat(
        fixture.tsw_id,
        "SWE",
        "task.swe",
        created_at,
        deadline,
        Some(parent),
    );
    // The child is as healthy as a seat can be.
    fixture
        .store
        .observe_seat_binding(
            fixture.project_id,
            child,
            &SeatLivenessObservation {
                attached_at: Some(at("2026-08-16T03:00:00Z")),
                activity_at: Some(at("2026-08-16T03:04:00Z")),
                runtime_reported: Some(ObservedRunState::Running),
                ..SeatLivenessObservation::default()
            },
            at("2026-08-16T03:04:00Z"),
        )
        .expect("the child's liveness is recorded");

    let now = at("2026-08-16T03:05:00Z");
    assert_eq!(
        fixture
            .store
            .list_seat_attachments(fixture.project_id, fixture.tsw_id, now)
            .expect("the read succeeds"),
        vec![SeatAttachment::Attached],
        "with its owner open, this seat is genuinely working"
    );

    // The owner is released. Nothing about the child's own evidence changes.
    fixture
        .store
        .observe_seat_binding(
            fixture.project_id,
            parent,
            &SeatLivenessObservation {
                released_at: Some(at("2026-08-16T03:04:30Z")),
                ..SeatLivenessObservation::default()
            },
            at("2026-08-16T03:04:30Z"),
        )
        .expect("the owner is released");

    assert_eq!(
        fixture
            .store
            .list_seat_attachments(fixture.project_id, fixture.tsw_id, now)
            .expect("the read succeeds"),
        vec![SeatAttachment::Orphaned],
        "a seat whose owner is gone is steered by nobody, however healthy it looks"
    );
    assert!(
        SeatAttachment::Orphaned.is_excluded(),
        "an orphan must not be counted as capacity"
    );
}

#[test]
fn an_observation_records_what_was_seen_and_erases_nothing_else() {
    let fixture = Fixture::build();
    let seat = fixture.seat(
        fixture.ecp_id,
        "LSA",
        "epic.lsa",
        at("2026-08-16T02:00:00Z"),
        at("2026-08-16T02:10:00Z"),
        None,
    );
    fixture
        .store
        .observe_seat_binding(
            fixture.project_id,
            seat,
            &SeatLivenessObservation {
                attached_at: Some(at("2026-08-16T02:05:00Z")),
                activity_at: Some(at("2026-08-16T02:06:00Z")),
                ..SeatLivenessObservation::default()
            },
            at("2026-08-16T02:06:00Z"),
        )
        .expect("the first observation is recorded");
    // A later observation that saw only the self-report must not blank the two
    // instants recorded above.
    let after = fixture
        .store
        .observe_seat_binding(
            fixture.project_id,
            seat,
            &SeatLivenessObservation {
                runtime_reported: Some(ObservedRunState::WaitingInput),
                ..SeatLivenessObservation::default()
            },
            at("2026-08-16T02:07:00Z"),
        )
        .expect("the second observation is recorded");

    assert_eq!(after.last_attached_at, Some(at("2026-08-16T02:05:00Z")));
    assert_eq!(after.last_activity_at, Some(at("2026-08-16T02:06:00Z")));
    assert_eq!(after.runtime_reported, Some(ObservedRunState::WaitingInput));
    assert_eq!(
        after.attach_deadline,
        at("2026-08-16T02:10:00Z"),
        "the deadline is never rewritten by an observation"
    );
}

#[test]
fn seat_liveness_evidence_survives_restart_and_export() {
    let fixture = Fixture::build();
    let seat = fixture.seat(
        fixture.ecp_id,
        "LSA",
        "epic.lsa",
        at("2026-08-16T02:00:00Z"),
        at("2026-08-16T02:01:00Z"),
        None,
    );
    fixture
        .store
        .observe_seat_binding(
            fixture.project_id,
            seat,
            &SeatLivenessObservation {
                attached_at: Some(at("2026-08-16T02:00:30Z")),
                activity_at: Some(at("2026-08-16T02:00:45Z")),
                runtime_reported: Some(ObservedRunState::Running),
                ..SeatLivenessObservation::default()
            },
            at("2026-08-16T02:00:45Z"),
        )
        .expect("the observation is recorded");

    let database = fixture.home.path().join("kontor.db");
    drop(fixture.store);
    let store = SqliteStore::open(&database).expect("the store reopens");

    // The deadline is still the one fixed at creation, so the conclusion after a
    // restart is the same conclusion as before it.
    assert_eq!(
        store
            .list_seat_attachments(
                fixture.project_id,
                fixture.ecp_id,
                at("2026-08-16T02:40:00Z")
            )
            .expect("the read succeeds"),
        vec![SeatAttachment::Stalled]
    );

    let exported = export_realm(&store, at("2026-08-16T05:00:00Z")).expect("the realm exports");
    let row = exported
        .records
        .seat_bindings
        .iter()
        .find(|row| row.id == seat.to_string())
        .expect("the seat binding is exported");
    assert_eq!(row.attach_deadline, "2026-08-16T02:01:00Z");
    assert_eq!(
        row.last_attached_at.as_deref(),
        Some("2026-08-16T02:00:30Z")
    );
    assert_eq!(
        row.last_activity_at.as_deref(),
        Some("2026-08-16T02:00:45Z")
    );
    assert_eq!(row.runtime_reported.as_deref(), Some("running"));
}

// ---------------------------------------------------------------------------
// The task a node serves
// ---------------------------------------------------------------------------

/// Admission locates a task's node before any seat binding exists, and one task
/// resolves to exactly one node or to none.
#[test]
fn a_task_resolves_to_at_most_one_active_topology_node() {
    let fixture = Fixture::build();
    let task_id = kontor_core::id::TaskId::generate();
    fixture
        .store
        .create_task(&kontor_core::repository::NewTask {
            id: task_id,
            project_id: fixture.project_id,
            mini_project_id: None,
            title: name("A delivery"),
            module: None,
            state: kontor_core::state::TaskState::Todo,
            created_at: at("2026-08-16T01:00:00Z"),
        })
        .expect("the task is created");

    // Before a node exists, the answer is "none" rather than an error: a
    // project running no Operational topology is normal, not broken.
    assert!(
        fixture
            .store
            .get_task_topology_node(fixture.project_id, task_id)
            .expect("the read succeeds")
            .is_none()
    );

    let served = TopologyNodeId::generate();
    fixture
        .store
        .create_topology_node(&NewSessionTopologyNode {
            id: served,
            project_id: fixture.project_id,
            mini_project_id: Some(fixture.mini_project_id),
            topology: fixture.snapshot.clone(),
            kind: TopologyKindKey::parse("TSW").expect("the task workspace kind"),
            parent_id: Some(fixture.epic_id),
            task_id: Some(task_id),
            created_at: at("2026-08-16T01:00:00Z"),
        })
        .expect("the task's node is created");

    assert_eq!(
        fixture
            .store
            .get_task_topology_node(fixture.project_id, task_id)
            .expect("the read succeeds")
            .expect("the task has a node")
            .id,
        served
    );

    // A second active node for the same task would make "the task's workspace"
    // ambiguous exactly where admission needs one answer.
    let refusal = fixture
        .store
        .create_topology_node(&NewSessionTopologyNode {
            id: TopologyNodeId::generate(),
            project_id: fixture.project_id,
            mini_project_id: Some(fixture.mini_project_id),
            topology: fixture.snapshot.clone(),
            kind: TopologyKindKey::parse("TSW").expect("the task workspace kind"),
            parent_id: Some(fixture.epic_id),
            task_id: Some(task_id),
            created_at: at("2026-08-16T01:00:00Z"),
        })
        .expect_err("one task cannot have two live workspaces");
    assert!(refusal.to_string().contains("constraint"), "{refusal:?}");
}
