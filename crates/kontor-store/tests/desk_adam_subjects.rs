//! Desk Adam subjects in the store (ASMA-8452): the subject row, the subject
//! node that hangs off an older desk workspace, the one live session a subject
//! may host, and the export generation that carries the rows.
//!
//! Every topology and Team Definition revision is derived here from the
//! bundled defaults and published in the test, so these rules are pinned
//! independently of any lineage version a successor might ship at.

mod support;

use kontor_core::id::{
    ContentHash, DeskKey, ExternalId, ExternalName, MiniProjectId, ProjectId, RoleCode, RoleSlotId,
    RuntimeKindKey, SeatBindingId, Timestamp, TopologyKindKey, TopologyNodeId, TopologySpecId,
    parse_utc_timestamp,
};
use kontor_core::repository::{
    MiniProjectTopologySnapshot, NewMiniProject, NewNativeContainerBinding, NewProject,
    NewSeatBinding, NewSessionTopologyNode, ProjectRepository, RepositoryError,
    SeatLivenessObservation, StoredDesk, StoredDeskAdamSubject, StoredHostedTopologySeat,
    TeamDefinitionRepository, TopologyRepository,
};
use kontor_core::spec::{
    CatalogRoleRef, ModelRef, ModelRung, ProjectSessionTopologySpec, ProviderRef, SeatAutonomy,
    Shareability, ShareabilityTier, TeamDefinitionSnapshot, TeamDefinitionSpec, TopologySnapshot,
};
use kontor_core::state::{NativeRuntimeIdentity, ObservedContainerKind};
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use kontor_store::backup::{KontorExportV1, create_snapshot, export_realm, restore_snapshot};
use serde_json::{Value, json};

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn kind(text: &str) -> TopologyKindKey {
    TopologyKindKey::parse(text).expect("a kind")
}

fn desk_key(text: &str) -> DeskKey {
    DeskKey::parse(text).expect("a desk key")
}

fn default_stamp() -> Shareability {
    Shareability::default_for(ShareabilityTier::ProjectKnowledge).expect("tier B classifies")
}

fn snapshot_of(topology: &ProjectSessionTopologySpec) -> TopologySnapshot {
    TopologySnapshot {
        spec_id: topology.spec_id,
        version: topology.version,
        canonical_hash: topology.canonicalize().expect("canonical").hash().clone(),
    }
}

fn help(full_name: &str) -> Value {
    json!({"full_name": full_name, "meaning": "A kind.", "category": "session_topology",
           "lifecycle": "current"})
}

fn area() -> Value {
    json!({"segments": [{"kind": "token", "value": "AREA_CODE"}]})
}

fn desk_kinds() -> Vec<Value> {
    vec![
        json!({"kind": "DESK", "allowed_parents": ["PSW"], "cardinality": {"minimum": 0},
               "projection_capabilities": ["native_root"], "read_only": false,
               "name_template": area(), "code_help": help("Durable Desk")}),
        json!({"kind": "DWS", "allowed_parents": ["DESK"],
               "cardinality": {"minimum": 1, "maximum": 1},
               "projection_capabilities": ["native_child", "session_host"], "read_only": false,
               "name_template": area(), "seat_name_template": area(),
               "code_help": help("Desk Workspace")}),
    ]
}

fn subject_kind(code: &str, parents: &[&str], capabilities: &[&str]) -> Value {
    json!({"kind": code, "allowed_parents": parents, "cardinality": {"minimum": 0},
           "projection_capabilities": capabilities, "read_only": false,
           "name_template": area(), "seat_name_template": area(),
           "code_help": help("Desk Adam Subject")})
}

fn subject_kinds() -> Vec<Value> {
    vec![
        subject_kind("ADE", &["DWS"], &["parent_session_host"]),
        subject_kind("ADH", &["DWS"], &["parent_session_host"]),
    ]
}

/// The bundled default topology at `version` with `extra` kinds appended.
fn revision(version: u32, extra: Vec<Value>) -> ProjectSessionTopologySpec {
    let mut value = serde_json::to_value(base_topology()).expect("serializes");
    value["node_kinds"]
        .as_array_mut()
        .expect("node kinds")
        .extend(extra);
    value["version"] = json!(version);
    serde_json::from_value(value).expect("a topology revision")
}

fn base_topology() -> ProjectSessionTopologySpec {
    let domain = bundled_operational_domain().expect("the bundled domain");
    let definition = domain.team_definitions.first().expect("a definition");
    domain
        .topology_specs
        .iter()
        .find(|topology| {
            topology.spec_id == definition.topology.spec_id
                && topology.version == definition.topology.version
        })
        .expect("the default's topology")
        .clone()
}

fn with_kinds(mut kinds: Vec<Value>, more: Vec<Value>) -> Vec<Value> {
    kinds.extend(more);
    kinds
}

fn token(value: &str) -> Value {
    json!({"kind": "token", "value": value})
}

fn slot(id: &str, role: &str) -> Value {
    json!({"slot_id": id, "role_code": role, "capability_profile": "epic-leadership"})
}

/// The bundled default definition extended to `topology` with the two desks
/// and, when asked, the desk leadership on the `adam` desk.
fn definition(
    version: u32,
    topology: &ProjectSessionTopologySpec,
    leadership: bool,
) -> TeamDefinitionSpec {
    let domain = bundled_operational_domain().expect("the bundled domain");
    let default = domain
        .team_definitions
        .first()
        .expect("a definition")
        .clone();
    let mut value = serde_json::to_value(&default).expect("serializes");
    let containers = value["containers"].as_array_mut().expect("containers");
    containers.push(json!({
        "kind": "DESK", "parent": null, "prefix": "DESK",
        "projection_capabilities": ["native_root"], "read_only": false,
        "name_template": {"segments": [token("PREFIX"), token("DESK_NAME")]}}));
    containers.push(json!({
        "kind": "DWS", "parent": "DESK", "prefix": "ADAM",
        "projection_capabilities": ["native_child", "session_host"], "read_only": false,
        "name_template": {"segments": [token("PREFIX")]}}));
    if leadership {
        containers.push(json!({
            "kind": "ADE", "parent": "DWS", "prefix": "ADE",
            "projection_capabilities": ["parent_session_host"], "read_only": false,
            "name_template": {"segments": [token("PREFIX")]},
            "seat_name_template": {"segments": [
                token("ROLE_CODE"), token("EPIC_JIRA_KEY"), token("TOPIC")]},
            "slots": [slot("lsa", "LSA"), slot("tpm", "TPM")]}));
        containers.push(json!({
            "kind": "ADH", "parent": "DWS", "prefix": "ADH",
            "projection_capabilities": ["parent_session_host"], "read_only": false,
            "name_template": {"segments": [token("PREFIX")]},
            "seat_name_template": {"segments": [
                token("ROLE_CODE"), {"kind": "literal", "value": "ADHOC"}, token("TOPIC")]},
            "slots": [slot("lsa", "LSA")]}));
        value["desk_leadership"] =
            json!({"desk_key": "adam", "epic_kind": "ADE", "ad_hoc_kind": "ADH"});
    }
    value["desks"] = json!([
        {"desk_key": "adam", "display_name": "ADAM", "kind": "DESK", "workspace_kind": "DWS"},
        {"desk_key": "pr-review", "display_name": "PR REVIEW", "kind": "DESK",
         "workspace_kind": "DWS"}
    ]);
    value["version"] = json!(version);
    value["topology"] = serde_json::to_value(snapshot_of(topology)).expect("a snapshot");
    serde_json::from_value(value).expect("a definition revision")
}

fn rung() -> ModelRung {
    ModelRung {
        provider: ProviderRef("codex".to_owned()),
        model: ModelRef("gpt-5.6".to_owned()),
        effort: None,
    }
}

fn native(native_id: &str) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("a runtime kind"),
        host: name("paseo-local"),
        generation: 1,
        native_id: ExternalId::parse(native_id).expect("a native id"),
    }
}

struct Fixture {
    home: tempfile::TempDir,
    store: SqliteStore,
    project_id: ProjectId,
    base_version: u32,
    /// The successor of the desks' revision that introduces the subject kinds.
    leadership: TopologySnapshot,
    leadership_definition: TeamDefinitionSpec,
    root: TopologyNodeId,
    adam: StoredDesk,
    review: StoredDesk,
    catalog: kontor_core::spec::RoleCatalogRevision,
    created_at: Timestamp,
}

impl Fixture {
    fn build() -> Self {
        let home = support::state_root();
        let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
        let project_id = ProjectId::generate();
        let created_at = at("2026-10-11T09:00:00Z");
        store
            .create_project(&NewProject {
                id: project_id,
                name: name("Desk Adam project"),
                root_path: name("/tmp/desk-adam-project"),
                created_at,
            })
            .expect("the project is created");
        let domain = bundled_operational_domain().expect("the bundled domain");
        let catalog = domain.role_catalogs.first().expect("a catalog").clone();
        store
            .publish_role_catalog(&catalog, &default_stamp(), created_at)
            .expect("the catalog publishes");

        let base = base_topology();
        let base_version = base.version.get();
        let desks = revision(base_version + 1, desk_kinds());
        let leadership = revision(base_version + 2, with_kinds(desk_kinds(), subject_kinds()));
        for topology in [&base, &desks, &leadership] {
            store
                .publish_topology_spec(project_id, topology, &default_stamp(), created_at)
                .expect("the topology publishes");
        }
        let default_version = domain
            .team_definitions
            .first()
            .expect("a definition")
            .version
            .get();
        let desk_definition = definition(default_version + 1, &desks, false);
        let leadership_definition = definition(default_version + 2, &leadership, true);
        for definition in [&desk_definition, &leadership_definition] {
            store
                .publish_team_definition(project_id, definition, created_at)
                .expect("the definition publishes");
        }

        let root = TopologyNodeId::generate();
        store
            .create_topology_node(&NewSessionTopologyNode {
                id: root,
                project_id,
                mini_project_id: None,
                topology: snapshot_of(&base),
                kind: kind("PSW"),
                parent_id: None,
                task_id: None,
                created_at,
            })
            .expect("the project root is placed on the default revision");
        let desks = snapshot_of(&desks);
        let place = |key: &str| {
            let desk = StoredDesk {
                project_id,
                desk_key: desk_key(key),
                topology_node_id: TopologyNodeId::generate(),
                workspace_node_id: TopologyNodeId::generate(),
                team_definition: TeamDefinitionSnapshot::from_revision(&desk_definition)
                    .expect("a snapshot"),
                created_at,
            };
            store.create_desk(&desk).expect("the desk row is written");
            for (id, code, parent) in [
                (desk.topology_node_id, "DESK", root),
                (desk.workspace_node_id, "DWS", desk.topology_node_id),
            ] {
                store
                    .create_topology_node(&NewSessionTopologyNode {
                        id,
                        project_id,
                        mini_project_id: None,
                        topology: desks.clone(),
                        kind: kind(code),
                        parent_id: Some(parent),
                        task_id: None,
                        created_at,
                    })
                    .expect("the desk node is placed");
            }
            desk
        };
        let adam = place("adam");
        let review = place("pr-review");
        Self {
            home,
            store,
            project_id,
            base_version,
            leadership: snapshot_of(&leadership),
            leadership_definition,
            root,
            adam,
            review,
            catalog,
            created_at,
        }
    }

    fn publish(&self, topology: &ProjectSessionTopologySpec) -> TopologySnapshot {
        self.store
            .publish_topology_spec(self.project_id, topology, &default_stamp(), self.created_at)
            .expect("the topology publishes");
        snapshot_of(topology)
    }

    /// One epic pinned to `topology`.
    fn epic(&self, topology: &TopologySnapshot) -> MiniProjectId {
        let epic = MiniProjectId::generate();
        self.store
            .create_mini_project(&NewMiniProject {
                id: epic,
                project_id: self.project_id,
                name: name("An epic"),
                created_at: self.created_at,
            })
            .expect("the epic is created");
        self.store
            .pin_mini_project_topology(&MiniProjectTopologySnapshot {
                project_id: self.project_id,
                mini_project_id: epic,
                topology: topology.clone(),
                pinned_at: self.created_at,
            })
            .expect("the epic is pinned");
        epic
    }

    fn planned(
        &self,
        desk: &StoredDesk,
        epic: Option<MiniProjectId>,
        topic: &str,
        origin: Option<TopologyNodeId>,
    ) -> StoredDeskAdamSubject {
        StoredDeskAdamSubject {
            project_id: self.project_id,
            topology_node_id: TopologyNodeId::generate(),
            desk_key: desk.desk_key.clone(),
            mini_project_id: epic,
            topic: name(topic),
            origin_topology_node_id: origin,
            team_definition: TeamDefinitionSnapshot::from_revision(&self.leadership_definition)
                .expect("a snapshot"),
            created_at: self.created_at,
        }
    }

    fn subject_node(
        &self,
        subject: &StoredDeskAdamSubject,
        parent: TopologyNodeId,
        topology: &TopologySnapshot,
    ) -> Result<(), RepositoryError> {
        self.store
            .create_topology_node(&NewSessionTopologyNode {
                id: subject.topology_node_id,
                project_id: self.project_id,
                mini_project_id: subject.mini_project_id,
                topology: topology.clone(),
                kind: kind(if subject.mini_project_id.is_some() {
                    "ADE"
                } else {
                    "ADH"
                }),
                parent_id: Some(parent),
                task_id: None,
                created_at: self.created_at,
            })
            .map(|_| ())
    }

    /// One whole subject on the leadership revision: the row, then its node.
    fn open(
        &self,
        desk: &StoredDesk,
        epic: Option<MiniProjectId>,
        topic: &str,
    ) -> StoredDeskAdamSubject {
        let subject = self.planned(desk, epic, topic, None);
        self.store
            .create_desk_adam_subject(&subject)
            .expect("the subject row is written");
        self.subject_node(&subject, desk.workspace_node_id, &self.leadership)
            .expect("the subject hangs off its desk's workspace");
        subject
    }

    fn role(&self, code: &str) -> CatalogRoleRef {
        let entry = self
            .catalog
            .role(&RoleCode::parse(code).expect("a role code"))
            .expect("the catalog has the role")
            .clone();
        CatalogRoleRef {
            catalog_id: self.catalog.catalog_id,
            catalog_revision: self.catalog.version,
            role_code: entry.role_code,
            standard_title: entry.standard_title,
            custom_display_name: None,
        }
    }

    fn seat(&self, node: TopologyNodeId, code: &str) -> SeatBindingId {
        self.try_seat(node, code)
            .expect("the seat binding is created")
    }

    fn try_seat(&self, node: TopologyNodeId, code: &str) -> Result<SeatBindingId, RepositoryError> {
        let id = SeatBindingId::generate();
        self.store
            .create_seat_binding(&NewSeatBinding {
                id,
                project_id: self.project_id,
                topology_node_id: node,
                role_slot_id: RoleSlotId::parse(&code.to_lowercase()).expect("a slot"),
                role: self.role(code),
                task_id: None,
                team_run_id: None,
                attach_deadline: at("2026-10-11T10:00:00Z"),
                parent_seat_binding_id: None,
                created_at: self.created_at,
            })
            .map(|_| id)
    }

    fn retire(&self, seat: SeatBindingId) {
        let now = at("2026-10-11T09:30:00Z");
        self.store
            .observe_seat_binding(
                self.project_id,
                seat,
                &SeatLivenessObservation {
                    released_at: Some(now),
                    ..SeatLivenessObservation::default()
                },
                now,
            )
            .expect("the seat retires");
    }

    fn hosted(&self, seat: SeatBindingId, native_id: &str) -> StoredHostedTopologySeat {
        StoredHostedTopologySeat {
            project_id: self.project_id,
            seat_binding_id: seat,
            model_rung: rung(),
            native_identity: native(native_id),
            autonomy: SeatAutonomy::Supervised,
            provider_session_id: None,
            observed_at: at("2026-10-11T09:01:00Z"),
        }
    }

    fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.home.path().join("kontor.db")).expect("a raw connection")
    }
}

fn conflict<T: std::fmt::Debug>(result: Result<T, RepositoryError>) -> bool {
    matches!(result, Err(RepositoryError::Conflict { .. }))
}

#[test]
fn epic_and_ad_hoc_subjects_hang_off_an_older_desk_workspace_and_survive_a_reopen() {
    let f = Fixture::build();
    let epic = f.epic(&f.leadership);
    let led = f.open(&f.adam, Some(epic), "kontord durable desks");
    let ad_hoc = f.open(&f.adam, None, "release readiness");

    for (subject, scope) in [(&led, Some(epic)), (&ad_hoc, None)] {
        let node = f
            .store
            .get_topology_node(f.project_id, subject.topology_node_id)
            .expect("reads")
            .expect("the node exists");
        assert_eq!(node.mini_project_id, scope, "the subject carries its scope");
        assert_eq!(node.parent_id, Some(f.adam.workspace_node_id));
        assert_eq!(
            node.topology, f.leadership,
            "placed under the epic's revision"
        );
    }
    let workspace = f
        .store
        .get_topology_node(f.project_id, f.adam.workspace_node_id)
        .expect("reads")
        .expect("the workspace exists");
    assert_eq!(workspace.mini_project_id, None, "the host stays epic-free");
    assert_ne!(
        workspace.topology, f.leadership,
        "positive control: the host is on an older revision"
    );

    let path = f.home.path().join("kontor.db");
    drop(f.store);
    let reopened = SqliteStore::open(&path).expect("the store reopens");
    assert_eq!(
        reopened
            .get_epic_desk_adam_subject(f.project_id, epic)
            .expect("reads"),
        Some(led.clone())
    );
    assert_eq!(
        reopened
            .get_ad_hoc_desk_adam_subject(
                f.project_id,
                &f.adam.desk_key,
                &name("release readiness")
            )
            .expect("reads"),
        Some(ad_hoc.clone())
    );
    assert_eq!(
        reopened
            .get_desk_adam_subject(f.project_id, ad_hoc.topology_node_id)
            .expect("reads"),
        Some(ad_hoc.clone())
    );
    assert_eq!(
        reopened
            .list_desk_adam_subjects(f.project_id, &f.adam.desk_key)
            .expect("lists")
            .len(),
        2
    );
    assert!(
        reopened
            .list_desk_adam_subjects(f.project_id, &f.review.desk_key)
            .expect("lists")
            .is_empty(),
        "another desk holds none of them"
    );
}

#[test]
fn one_subject_per_epic_topic_and_binding_and_every_row_is_immutable() {
    let f = Fixture::build();
    let epic = f.epic(&f.leadership);
    f.open(&f.adam, Some(epic), "first");

    // One epic Adam per epic, on any desk.
    for desk in [&f.adam, &f.review] {
        assert!(conflict(f.store.create_desk_adam_subject(&f.planned(
            desk,
            Some(epic),
            "second",
            None
        ))));
    }

    // One ad-hoc subject per topic on a desk; another desk is another tab.
    let ad_hoc = f.open(&f.adam, None, "triage");
    assert!(conflict(f.store.create_desk_adam_subject(
        &f.planned(&f.adam, None, "triage", None)
    )));
    f.store
        .create_desk_adam_subject(&f.planned(&f.review, None, "triage", None))
        .expect("positive control: the same topic on another desk");

    // A binding names an ad-hoc subject of the same desk, once, ever.
    let bound_epic = f.epic(&f.leadership);
    let bound = f.planned(
        &f.adam,
        Some(bound_epic),
        "bound",
        Some(ad_hoc.topology_node_id),
    );
    f.store
        .create_desk_adam_subject(&bound)
        .expect("the ad-hoc subject binds to an epic");
    let again = f.epic(&f.leadership);
    assert!(
        conflict(f.store.create_desk_adam_subject(&f.planned(
            &f.adam,
            Some(again),
            "rebound",
            Some(ad_hoc.topology_node_id)
        ))),
        "an ad-hoc subject never binds to a second epic"
    );
    let led = f
        .store
        .get_epic_desk_adam_subject(f.project_id, epic)
        .expect("reads")
        .expect("the first subject");
    assert!(
        conflict(f.store.create_desk_adam_subject(&f.planned(
            &f.adam,
            Some(again),
            "from an epic",
            Some(led.topology_node_id)
        ))),
        "an epic subject is never another's origin"
    );
    let elsewhere = f.open(&f.review, None, "elsewhere");
    assert!(
        conflict(f.store.create_desk_adam_subject(&f.planned(
            &f.adam,
            Some(again),
            "across desks",
            Some(elsewhere.topology_node_id)
        ))),
        "a binding stays on its desk"
    );
    assert!(
        conflict(f.store.create_desk_adam_subject(&f.planned(
            &f.adam,
            None,
            "ad-hoc origin",
            Some(elsewhere.topology_node_id)
        ))),
        "only an epic subject is bound from anything"
    );
    let mut separator = f.planned(&f.adam, None, "plain", None);
    separator.topic = name("a • b");
    assert!(conflict(f.store.create_desk_adam_subject(&separator)));
    assert_eq!(
        f.store
            .get_desk_adam_subject_bound_from(f.project_id, ad_hoc.topology_node_id)
            .expect("reads"),
        Some(bound)
    );

    let connection = f.raw();
    let updated = connection.execute("UPDATE desk_adam_subjects SET topic = 'renamed'", []);
    assert!(
        updated
            .expect_err("a subject is immutable")
            .to_string()
            .contains("a desk Adam subject keeps its node")
    );
    let deleted = connection.execute("DELETE FROM desk_adam_subjects", []);
    assert!(
        deleted
            .expect_err("a subject is permanent")
            .to_string()
            .contains("a desk Adam subject is never deleted")
    );
}

#[test]
fn the_bridge_admits_only_the_registered_workspace_from_a_newer_revision() {
    let f = Fixture::build();
    let epic = f.epic(&f.leadership);

    // Unregistered: no subject row names the node.
    let unregistered = f.planned(&f.adam, Some(epic), "unregistered", None);
    assert!(conflict(f.subject_node(
        &unregistered,
        f.adam.workspace_node_id,
        &f.leadership
    )));

    // Wrong host: the row names the adam desk, the node asks for another's.
    let subject = f.planned(&f.adam, Some(epic), "wrong host", None);
    f.store
        .create_desk_adam_subject(&subject)
        .expect("the row is written");
    assert!(conflict(f.subject_node(
        &subject,
        f.review.workspace_node_id,
        &f.leadership
    )));
    // Wrong scope: the row is the epic's, the node claims to be ad-hoc.
    let mut unscoped = subject.clone();
    unscoped.mini_project_id = None;
    assert!(conflict(f.subject_node(
        &unscoped,
        f.adam.workspace_node_id,
        &f.leadership
    )));
    // A desk node is no host, and the epic's own snapshot still binds.
    assert!(conflict(f.subject_node(
        &subject,
        f.adam.topology_node_id,
        &f.leadership
    )));
    let other = f.publish(&revision(
        f.base_version + 3,
        with_kinds(desk_kinds(), subject_kinds()),
    ));
    let snapshot_refusal = f
        .subject_node(&subject, f.adam.workspace_node_id, &other)
        .expect_err("the epic is pinned to another revision");
    assert!(
        snapshot_refusal
            .to_string()
            .contains("differs from the MiniProject snapshot"),
        "{snapshot_refusal}"
    );
    // Positive control: the exact registered host.
    f.subject_node(&subject, f.adam.workspace_node_id, &f.leadership)
        .expect("the registered workspace hosts its subject");
    assert!(
        f.store
            .get_topology_node(f.project_id, unregistered.topology_node_id)
            .expect("reads")
            .is_none(),
        "a refused subject leaves no node"
    );
}

#[test]
fn the_bridge_refuses_older_changed_cross_lineage_and_disagreeing_revisions() {
    let f = Fixture::build();
    let v = f.base_version;

    // A host whose declaration changed between the two revisions.
    let mut changed = revision(v + 3, with_kinds(desk_kinds(), subject_kinds()));
    for declared in &mut changed.node_kinds {
        if declared.kind == kind("DWS") {
            declared.read_only = true;
        }
    }
    let changed = f.publish(&changed);
    let epic = f.epic(&changed);
    let subject = f.planned(&f.adam, Some(epic), "changed host", None);
    f.store.create_desk_adam_subject(&subject).expect("row");
    assert!(conflict(f.subject_node(
        &subject,
        f.adam.workspace_node_id,
        &changed
    )));

    // A subject kind from a revision older than the host's, which the host's
    // revision does not declare: a desk placed on a later desk-only revision.
    let later_desks = f.publish(&revision(v + 4, desk_kinds()));
    let late = StoredDesk {
        desk_key: desk_key("late"),
        topology_node_id: TopologyNodeId::generate(),
        workspace_node_id: TopologyNodeId::generate(),
        ..f.adam.clone()
    };
    f.store.create_desk(&late).expect("desk row");
    for (id, code, parent) in [
        (late.topology_node_id, "DESK", f.root),
        (late.workspace_node_id, "DWS", late.topology_node_id),
    ] {
        f.store
            .create_topology_node(&NewSessionTopologyNode {
                id,
                project_id: f.project_id,
                mini_project_id: None,
                topology: later_desks.clone(),
                kind: kind(code),
                parent_id: Some(parent),
                task_id: None,
                created_at: f.created_at,
            })
            .expect("the late desk is placed");
    }
    let older_epic = f.epic(&f.leadership);
    let older = f.planned(&late, Some(older_epic), "older", None);
    f.store.create_desk_adam_subject(&older).expect("row");
    assert!(
        conflict(f.subject_node(&older, late.workspace_node_id, &f.leadership)),
        "the bridge only looks forward along the lineage"
    );

    // A host revision that knows the subject kind but places it elsewhere.
    let mut disagreeing_kinds = desk_kinds();
    disagreeing_kinds.push(subject_kind("ADE", &["TSW"], &["parent_session_host"]));
    let disagreeing = f.publish(&revision(v + 5, disagreeing_kinds));
    let placed = StoredDesk {
        desk_key: desk_key("placed"),
        topology_node_id: TopologyNodeId::generate(),
        workspace_node_id: TopologyNodeId::generate(),
        ..f.adam.clone()
    };
    f.store.create_desk(&placed).expect("desk row");
    for (id, code, parent) in [
        (placed.topology_node_id, "DESK", f.root),
        (placed.workspace_node_id, "DWS", placed.topology_node_id),
    ] {
        f.store
            .create_topology_node(&NewSessionTopologyNode {
                id,
                project_id: f.project_id,
                mini_project_id: None,
                topology: disagreeing.clone(),
                kind: kind(code),
                parent_id: Some(parent),
                task_id: None,
                created_at: f.created_at,
            })
            .expect("the disagreeing desk is placed");
    }
    let newer = f.publish(&revision(v + 6, with_kinds(desk_kinds(), subject_kinds())));
    let newer_epic = f.epic(&newer);
    let disagreed = f.planned(&placed, Some(newer_epic), "disagreed", None);
    f.store.create_desk_adam_subject(&disagreed).expect("row");
    assert!(
        conflict(f.subject_node(&disagreed, placed.workspace_node_id, &newer)),
        "both revisions must permit the edge"
    );

    // Another lineage entirely, with every kind and a higher version.
    let mut foreign = revision(v + 7, with_kinds(desk_kinds(), subject_kinds()));
    foreign.spec_id = TopologySpecId::generate();
    let foreign = f.publish(&foreign);
    let foreign_epic = f.epic(&foreign);
    let crossing = f.planned(&f.adam, Some(foreign_epic), "crossing", None);
    f.store.create_desk_adam_subject(&crossing).expect("row");
    assert!(conflict(f.subject_node(
        &crossing,
        f.adam.workspace_node_id,
        &foreign
    )));

    // Positive control: the same newer revision on the original desk.
    let fine = f.planned(&f.adam, Some(f.epic(&newer)), "fine", None);
    f.store.create_desk_adam_subject(&fine).expect("row");
    f.subject_node(&fine, f.adam.workspace_node_id, &newer)
        .expect("a newer additive revision is admitted");
}

#[test]
fn a_subject_hosts_one_live_session_and_owns_no_container() {
    let f = Fixture::build();
    let first = f.open(&f.adam, Some(f.epic(&f.leadership)), "first");
    let second = f.open(&f.adam, Some(f.epic(&f.leadership)), "second");

    let lsa = f.seat(first.topology_node_id, "LSA");
    let tpm = f.seat(first.topology_node_id, "TPM");
    let lsa_live = f.hosted(lsa, "first-lsa");
    f.store
        .bind_hosted_topology_seat(&lsa_live)
        .expect("the planning session is hosted");
    assert!(
        conflict(
            f.store
                .bind_hosted_topology_seat(&f.hosted(tpm, "first-tpm"))
        ),
        "a second live session on the subject is a conflict"
    );
    // The schema says why, whatever path writes the row.
    let raw = f.raw().execute(
        "INSERT INTO hosted_topology_seats
             (seat_binding_id, project_id, model_rung, runtime_kind, host, generation,
              native_id, autonomy, provider_session_id, observed_at)
         SELECT ?1, project_id, model_rung, runtime_kind, host, generation,
                'first-tpm-raw', autonomy, provider_session_id, observed_at
           FROM hosted_topology_seats WHERE seat_binding_id = ?2",
        rusqlite::params![tpm.to_string(), lsa.to_string()],
    );
    assert!(
        raw.expect_err("the schema refuses it too")
            .to_string()
            .contains("hosts one live session at a time")
    );

    // Two epic Adams and a durable same-role seat share the workspace.
    let other = f.seat(second.topology_node_id, "LSA");
    f.store
        .bind_hosted_topology_seat(&f.hosted(other, "second-lsa"))
        .expect("another subject's session is its own");
    let durable = f.seat(f.adam.workspace_node_id, "LSA");
    f.store
        .bind_hosted_topology_seat(&f.hosted(durable, "durable-lsa"))
        .expect("a durable seat in the workspace is no subject's");

    // The handoff archives the predecessor first; then the successor binds.
    f.store
        .archive_hosted_topology_seat_route(&lsa_live, at("2026-10-11T09:30:00Z"), "handoff")
        .expect("the planning session is archived");
    f.store
        .bind_hosted_topology_seat(&f.hosted(tpm, "first-tpm"))
        .expect("the coordinating session is hosted after it");

    let container = f
        .store
        .bind_topology_node_container(&NewNativeContainerBinding {
            topology_node_id: first.topology_node_id,
            project_id: f.project_id,
            container_binding_id: ExternalId::parse("wks_subject").expect("an id"),
            identity: native("wks_subject"),
            observed_kind: ObservedContainerKind::Workspace,
            canonical_cwd: None,
            readback: None,
            bound_at: f.created_at,
            observed_at: f.created_at,
        });
    assert!(
        container
            .expect_err("a subject owns no container")
            .to_string()
            .contains("owns no native container")
    );
}

#[test]
fn a_retired_seat_no_longer_holds_its_subject() {
    let f = Fixture::build();
    let subject = f.open(&f.adam, Some(f.epic(&f.leadership)), "retired");
    let lsa = f.seat(subject.topology_node_id, "LSA");
    let tpm = f.seat(subject.topology_node_id, "TPM");
    f.store
        .bind_hosted_topology_seat(&f.hosted(lsa, "planning"))
        .expect("the planning session is hosted");
    assert!(
        conflict(
            f.store
                .bind_hosted_topology_seat(&f.hosted(tpm, "coordinating"))
        ),
        "positive control: the live LSA holds the subject"
    );

    f.retire(lsa);
    assert!(
        f.store
            .get_hosted_topology_seat(f.project_id, lsa)
            .expect("reads")
            .is_some(),
        "the retired seat's last occupancy is still on record"
    );
    f.store
        .bind_hosted_topology_seat(&f.hosted(tpm, "coordinating"))
        .expect("a retired seat no longer counts as the live session");
}

#[test]
fn an_ad_hoc_subject_binds_only_after_its_fence_and_then_seats_no_one() {
    let f = Fixture::build();
    let ad_hoc = f.open(&f.adam, None, "spike");
    let lsa = f.seat(ad_hoc.topology_node_id, "LSA");
    let live = f.hosted(lsa, "spike-lsa");
    f.store
        .bind_hosted_topology_seat(&live)
        .expect("the ad-hoc LSA is hosted");
    let bound = f.planned(
        &f.adam,
        Some(f.epic(&f.leadership)),
        "spike",
        Some(ad_hoc.topology_node_id),
    );

    // The retired seat's occupancy alone still holds the binding back...
    f.retire(lsa);
    assert!(
        conflict(f.store.create_desk_adam_subject(&bound)),
        "an unarchived native occupancy blocks the binding"
    );
    // ...and so does an active seat alone, on a subject never hosted.
    let idle = f.open(&f.adam, None, "idle");
    let idle_lsa = f.seat(idle.topology_node_id, "LSA");
    let idle_bound = f.planned(
        &f.adam,
        Some(f.epic(&f.leadership)),
        "idle",
        Some(idle.topology_node_id),
    );
    assert!(
        conflict(f.store.create_desk_adam_subject(&idle_bound)),
        "an active ad-hoc seat blocks the binding"
    );
    assert_eq!(
        f.store
            .get_desk_adam_subject_bound_from(f.project_id, ad_hoc.topology_node_id)
            .expect("reads"),
        None,
        "nothing was bound"
    );

    // Fenced: the seat retired and its native archived. The same rows bind.
    f.store
        .archive_hosted_topology_seat_route(&live, at("2026-10-11T09:31:00Z"), "fence")
        .expect("the ad-hoc native is archived");
    f.store
        .create_desk_adam_subject(&bound)
        .expect("the fenced ad-hoc subject binds");
    f.retire(idle_lsa);
    f.store
        .create_desk_adam_subject(&idle_bound)
        .expect("the retired idle subject binds");

    // Bound, the ad-hoc subject never seats or hosts anyone again.
    assert!(
        conflict(f.try_seat(ad_hoc.topology_node_id, "LSA")),
        "a bound ad-hoc subject takes no new seat"
    );
    assert!(
        conflict(
            f.store
                .bind_hosted_topology_seat(&f.hosted(lsa, "spike-lsa-again"))
        ),
        "a bound ad-hoc subject's retired seat is never hosted again"
    );
    let durable = f.seat(f.adam.workspace_node_id, "LSA");
    f.store
        .bind_hosted_topology_seat(&f.hosted(durable, "durable-lsa"))
        .expect("positive control: the workspace still hosts its own seat");
    let raw = f.raw().execute(
        "INSERT INTO hosted_topology_seats
             (seat_binding_id, project_id, model_rung, runtime_kind, host, generation,
              native_id, autonomy, provider_session_id, observed_at)
         SELECT ?1, project_id, model_rung, runtime_kind, host, generation,
                'spike-lsa-raw', autonomy, provider_session_id, observed_at
           FROM hosted_topology_seats WHERE seat_binding_id = ?2",
        rusqlite::params![lsa.to_string(), durable.to_string()],
    );
    assert!(
        raw.expect_err("the schema refuses it too")
            .to_string()
            .contains("a bound ad-hoc subject seats no one again")
    );
    let raw = f.raw().execute_batch(&format!(
        "CREATE TEMP TABLE copied AS SELECT * FROM seat_bindings WHERE id = '{lsa}';
         UPDATE copied SET id = '{fresh}', lifecycle = 'active', released_at = NULL;
         INSERT INTO seat_bindings SELECT * FROM copied;",
        fresh = SeatBindingId::generate(),
    ));
    assert!(
        raw.expect_err("the schema refuses a new seat too")
            .to_string()
            .contains("a bound ad-hoc subject seats no one again")
    );
}

#[test]
fn the_receipt_kinds_admit_desk_adam_commands_and_stay_closed() {
    let f = Fixture::build();
    let connection = f.raw();
    let receipt = |id: &str, key: &str, kind: &str| {
        connection.execute(
            &format!(
                "INSERT INTO command_receipts
                     (id, project_id, idempotency_key, kind, target, target_revision, intent,
                      intent_hash, state, attempts, created_at, updated_at, execution_mode)
                 VALUES ('{id}', '{project}', '{key}', '{kind}', '{{}}', 1, '{{}}', '{hash}',
                         'intent_persisted', 0, '2026-10-11T09:00:00Z',
                         '2026-10-11T09:00:00Z', 'local')",
                project = f.project_id,
                hash = "0".repeat(64),
            ),
            [],
        )
    };
    for (index, kind) in [
        "open_ad_hoc_desk_adam",
        "hand_off_desk_adam",
        // Reserved for the sibling durable-seat generation, so either one can
        // land first without narrowing the other's receipts.
        "declare_durable_seat",
        "reconcile_durable_seats",
    ]
    .into_iter()
    .enumerate()
    {
        receipt(
            &format!("0193f000-0000-7000-8000-00000000013{index}"),
            kind,
            kind,
        )
        .expect("the kind is admitted");
    }
    assert!(
        receipt(
            "0193f000-0000-7000-8000-000000000139",
            "unknown",
            "hand_off_desk_adam_twice",
        )
        .is_err(),
        "the kind list stays closed"
    );
    assert!(
        connection
            .execute("DELETE FROM command_receipts", [])
            .expect_err("the rebuilt table keeps its guards")
            .to_string()
            .contains("command receipts are not deletable")
    );
}

#[test]
fn an_epic_upgrade_cannot_change_where_its_subject_hosts_sessions() {
    let f = Fixture::build();
    let epic = f.epic(&f.leadership);
    let subject = f.open(&f.adam, Some(epic), "upgrade");
    f.seat(subject.topology_node_id, "LSA");

    let mut native_kinds = desk_kinds();
    native_kinds.push(subject_kind(
        "ADE",
        &["DWS"],
        &["native_child", "session_host"],
    ));
    native_kinds.push(subject_kind("ADH", &["DWS"], &["parent_session_host"]));
    let native_revision = f.publish(&revision(f.base_version + 3, native_kinds));
    let refusal = f
        .store
        .repin_mini_project_topology(&MiniProjectTopologySnapshot {
            project_id: f.project_id,
            mini_project_id: epic,
            topology: native_revision,
            pinned_at: f.created_at,
        })
        .expect_err("the subject would need a container of its own");
    assert!(
        refusal
            .to_string()
            .contains("changes where an existing node hosts its sessions"),
        "{refusal}"
    );

    let same = f.publish(&revision(
        f.base_version + 4,
        with_kinds(desk_kinds(), subject_kinds()),
    ));
    f.store
        .repin_mini_project_topology(&MiniProjectTopologySnapshot {
            project_id: f.project_id,
            mini_project_id: epic,
            topology: same.clone(),
            pinned_at: f.created_at,
        })
        .expect("positive control: the same hosting moves");
    assert_eq!(
        f.store
            .get_topology_node(f.project_id, subject.topology_node_id)
            .expect("reads")
            .expect("the subject")
            .topology,
        same
    );
}

#[test]
fn a_verified_snapshot_restore_keeps_every_subject() {
    let f = Fixture::build();
    let subject = f.open(&f.adam, Some(f.epic(&f.leadership)), "restored");
    let outcome = create_snapshot(
        &f.home.path().join("kontor.db"),
        &f.home.path().join("snapshots"),
        f.created_at,
    )
    .expect("the snapshot is taken");
    let destination = f.home.path().join("restored").join("kontor.db");
    restore_snapshot(&outcome.snapshot, &destination, f.created_at).expect("restores");
    let restored = SqliteStore::open(&destination).expect("the restored store opens");
    assert_eq!(
        restored
            .list_desk_adam_subjects(f.project_id, &f.adam.desk_key)
            .expect("lists"),
        vec![subject]
    );
}

#[test]
fn the_typed_export_carries_subjects_and_no_older_generation_can_carry_or_conceal_them() {
    let f = Fixture::build();
    let subject = f.open(&f.adam, None, "exported");
    let export = export_realm(&f.store, at("2026-10-11T10:00:00Z")).expect("the Realm exports");
    assert_eq!(export.records.desk_adam_subjects.len(), 1);
    assert_eq!(
        export.records.desk_adam_subjects[0].topology_node_id,
        subject.topology_node_id.to_string()
    );
    assert_eq!(export.records.desk_adam_subjects[0].subject_kind, "ad_hoc");
    let base = serde_json::to_value(&export).expect("serializes");
    KontorExportV1::parse(&serde_json::to_vec(&base).expect("bytes"))
        .expect("positive control: the current generation verifies");

    let rehash = |document: &mut Value| {
        let mut records =
            serde_json::to_vec(document.get("records").expect("records")).expect("serializes");
        records.push(b'\n');
        document["records_hash"] = json!(ContentHash::of(&records).to_string());
    };
    let refusal = |document: Value| -> String {
        match KontorExportV1::parse(&serde_json::to_vec(&document).expect("bytes")) {
            Err(kontor_store::backup::BackupError::Verification { detail }) => detail.to_owned(),
            other => panic!("expected a verification refusal, got {other:?}"),
        }
    };

    let mut carrying = base.clone();
    carrying["schema_version"] = json!(14);
    carrying["database_schema_version"] = json!(131);
    rehash(&mut carrying);
    assert_eq!(
        refusal(carrying),
        "the legacy export generation carries desk Adam subjects it did not define"
    );

    let mut concealing = base.clone();
    concealing["schema_version"] = json!(14);
    concealing["records"]["desk_adam_subjects"] = json!([]);
    concealing["continuity_summary"]["record_counts"]["desk_adam_subjects"] = json!(0);
    rehash(&mut concealing);
    assert_eq!(
        refusal(concealing),
        "the legacy export generation cannot prove desk Adam subject completeness"
    );

    let mut understated = base;
    understated["continuity_summary"]["record_counts"]["desk_adam_subjects"] = json!(0);
    rehash(&mut understated);
    assert!(
        KontorExportV1::parse(&serde_json::to_vec(&understated).expect("bytes")).is_err(),
        "the continuity summary must disclose every subject"
    );
}
