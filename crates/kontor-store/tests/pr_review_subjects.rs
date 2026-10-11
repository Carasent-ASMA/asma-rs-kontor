//! PR review subjects in the store (ASMA-8453): the subject row, the Adam and
//! room nodes that hang off an older desk, the receipt kinds, and the export
//! generation that carries the rows.
//!
//! Every topology and Team Definition revision is derived here from the
//! bundled defaults and published in the test, so these rules are pinned
//! independently of any lineage version a successor might ship at.

mod support;

use kontor_core::id::{
    ContentHash, DeskKey, ExternalId, ExternalName, MiniProjectId, ProjectId, RuntimeKindKey,
    TaskId, Timestamp, TopologyKindKey, TopologyNodeId, TopologySpecId, parse_utc_timestamp,
};
use kontor_core::pull_request::{ForgeRepository, PullRequestRef};
use kontor_core::repository::{
    NewMiniProject, NewNativeContainerBinding, NewProject, NewSessionTopologyNode, NewTask,
    ProjectRepository, RepositoryError, StoredDesk, StoredPrReviewSubject,
    TeamDefinitionRepository, TopologyRepository,
};
use kontor_core::spec::{
    ProjectSessionTopologySpec, Shareability, ShareabilityTier, TeamDefinitionSnapshot,
    TeamDefinitionSpec, TopologySnapshot,
};
use kontor_core::state::{NativeRuntimeIdentity, ObservedContainerKind, TaskState};
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

fn pull_request(repository: &str, number: u64) -> PullRequestRef {
    PullRequestRef::new(
        ForgeRepository::parse(repository).expect("a repository"),
        number,
    )
    .expect("a pull request")
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

fn review_kind(code: &str, parents: &[&str], capabilities: &[&str]) -> Value {
    json!({"kind": code, "allowed_parents": parents, "cardinality": {"minimum": 0},
           "projection_capabilities": capabilities, "read_only": false,
           "name_template": area(), "seat_name_template": area(),
           "code_help": help("PR Review")})
}

fn review_kinds() -> Vec<Value> {
    vec![
        review_kind("PRA", &["DWS"], &["parent_session_host"]),
        review_kind("RSW", &["DESK"], &["native_child", "session_host"]),
    ]
}

fn with_kinds(mut kinds: Vec<Value>, more: Vec<Value>) -> Vec<Value> {
    kinds.extend(more);
    kinds
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

fn token(value: &str) -> Value {
    json!({"kind": "token", "value": value})
}

/// The bundled default definition extended to `topology` with the two desks
/// and, when asked, the PR review on the `pr-review` desk.
fn definition(
    version: u32,
    topology: &ProjectSessionTopologySpec,
    review: bool,
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
    if review {
        containers.push(json!({
            "kind": "PRA", "parent": "DWS", "prefix": "PRA",
            "projection_capabilities": ["parent_session_host"], "read_only": false,
            "name_template": {"segments": [token("PREFIX")]},
            "seat_name_template": {"segments": [
                token("ROLE_CODE"), token("SCOPE_JIRA_KEY"), token("PULL_REQUEST")]},
            "slots": [{"slot_id": "tpm", "role_code": "TPM",
                       "capability_profile": "epic-leadership"}]}));
        containers.push(json!({
            "kind": "RSW", "parent": "DESK", "prefix": "RSW",
            "projection_capabilities": ["native_child", "session_host"], "read_only": false,
            "name_template": {"segments": [
                token("PREFIX"), token("SCOPE_JIRA_KEY"), token("PULL_REQUEST")]},
            "seat_name_template": {"segments": [token("SLOT_DISPLAY_NAME")]},
            "slots": [
                {"slot_id": "rev-a", "display_name": "REV A",
                 "capability_profile": "independent-reviewer"},
                {"slot_id": "rev-b", "display_name": "REV B",
                 "capability_profile": "independent-reviewer"},
                {"slot_id": "judge", "display_name": "JUDGE",
                 "capability_profile": "committee-judge"}]}));
        value["pr_review"] = json!({
            "desk_key": "pr-review", "adam_kind": "PRA", "room_kind": "RSW",
            "room_roles": [
                {"slot_id": "rev-a", "role_code": "AUD"},
                {"slot_id": "rev-b", "role_code": "AUD"},
                {"slot_id": "judge", "role_code": "AUD"}]});
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
    /// The successor of the desks' revision that introduces the review kinds.
    review: TopologySnapshot,
    review_definition: TeamDefinitionSpec,
    adam: StoredDesk,
    desk: StoredDesk,
    epic: MiniProjectId,
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
                name: name("PR review project"),
                root_path: name("/tmp/pr-review-project"),
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
        let review = revision(base_version + 2, with_kinds(desk_kinds(), review_kinds()));
        for topology in [&base, &desks, &review] {
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
        let review_definition = definition(default_version + 2, &review, true);
        for definition in [&desk_definition, &review_definition] {
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
        let desk = place("pr-review");
        let epic = MiniProjectId::generate();
        store
            .create_mini_project(&NewMiniProject {
                id: epic,
                project_id,
                name: name("An epic"),
                created_at,
            })
            .expect("the epic is created");
        Self {
            home,
            store,
            project_id,
            base_version,
            review: snapshot_of(&review),
            review_definition,
            adam,
            desk,
            epic,
            created_at,
        }
    }

    fn publish(&self, topology: &ProjectSessionTopologySpec) -> TopologySnapshot {
        self.store
            .publish_topology_spec(self.project_id, topology, &default_stamp(), self.created_at)
            .expect("the topology publishes");
        snapshot_of(topology)
    }

    fn task(&self, epic: MiniProjectId) -> TaskId {
        let task = TaskId::generate();
        self.store
            .create_task(&NewTask {
                id: task,
                project_id: self.project_id,
                mini_project_id: Some(epic),
                title: name("A task"),
                module: None,
                state: TaskState::Ready,
                created_at: self.created_at,
            })
            .expect("the task is created");
        task
    }

    fn planned(&self, desk: &StoredDesk, repository: &str, number: u64) -> StoredPrReviewSubject {
        StoredPrReviewSubject {
            project_id: self.project_id,
            desk_key: desk.desk_key.clone(),
            pull_request: pull_request(repository, number),
            jira_key: ExternalId::parse("ASMA-8453").expect("a key"),
            mini_project_id: self.epic,
            task_id: None,
            adam_node_id: TopologyNodeId::generate(),
            room_node_id: TopologyNodeId::generate(),
            team_definition: TeamDefinitionSnapshot::from_revision(&self.review_definition)
                .expect("a snapshot"),
            created_at: self.created_at,
        }
    }

    fn node(
        &self,
        id: TopologyNodeId,
        code: &str,
        parent: TopologyNodeId,
        topology: &TopologySnapshot,
    ) -> Result<(), RepositoryError> {
        self.store
            .create_topology_node(&NewSessionTopologyNode {
                id,
                project_id: self.project_id,
                mini_project_id: None,
                topology: topology.clone(),
                kind: kind(code),
                parent_id: Some(parent),
                task_id: None,
                created_at: self.created_at,
            })
            .map(|_| ())
    }

    /// One whole subject on the review revision: the row, then both nodes.
    fn open(&self, repository: &str, number: u64) -> StoredPrReviewSubject {
        let subject = self.planned(&self.desk, repository, number);
        self.store
            .create_pr_review_subject(&subject)
            .expect("the subject row is written");
        self.node(
            subject.adam_node_id,
            "PRA",
            self.desk.workspace_node_id,
            &self.review,
        )
        .expect("the Adam hangs off its desk's workspace");
        self.node(
            subject.room_node_id,
            "RSW",
            self.desk.topology_node_id,
            &self.review,
        )
        .expect("the room hangs off its desk");
        subject
    }

    fn container(&self, node: TopologyNodeId, native_id: &str) -> NewNativeContainerBinding {
        NewNativeContainerBinding {
            topology_node_id: node,
            project_id: self.project_id,
            container_binding_id: ExternalId::parse(native_id).expect("an id"),
            identity: native(native_id),
            observed_kind: ObservedContainerKind::Workspace,
            canonical_cwd: None,
            readback: None,
            bound_at: self.created_at,
            observed_at: self.created_at,
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
fn two_pull_requests_of_one_key_and_one_number_in_two_repositories_are_distinct_subjects() {
    let f = Fixture::build();
    let first = f.open("Carasent-ASMA/asma-rs-kontor", 297);
    let second = f.open("Carasent-ASMA/asma-rs-kontor", 298);
    let elsewhere = f.open("Carasent-ASMA/asma-modules", 297);

    let subjects = [&first, &second, &elsewhere];
    for (index, subject) in subjects.iter().enumerate() {
        for other in &subjects[index + 1..] {
            assert_ne!(subject.adam_node_id, other.adam_node_id);
            assert_ne!(subject.room_node_id, other.room_node_id);
        }
        let adam = f
            .store
            .get_topology_node(f.project_id, subject.adam_node_id)
            .expect("reads")
            .expect("the Adam exists");
        assert_eq!(adam.parent_id, Some(f.desk.workspace_node_id));
        assert_eq!(adam.mini_project_id, None, "the Adam is the desk's");
        assert_eq!(adam.topology, f.review, "placed under the review revision");
        let room = f
            .store
            .get_topology_node(f.project_id, subject.room_node_id)
            .expect("reads")
            .expect("the room exists");
        assert_eq!(room.parent_id, Some(f.desk.topology_node_id));
        assert_eq!(room.mini_project_id, None, "the room is the desk's");
    }
    let workspace = f
        .store
        .get_topology_node(f.project_id, f.desk.workspace_node_id)
        .expect("reads")
        .expect("the workspace exists");
    assert_ne!(
        workspace.topology, f.review,
        "positive control: the desk is on an older revision"
    );

    let path = f.home.path().join("kontor.db");
    drop(f.store);
    let reopened = SqliteStore::open(&path).expect("the store reopens");
    for subject in subjects {
        assert_eq!(
            reopened
                .get_pr_review_subject(f.project_id, &f.desk.desk_key, &subject.pull_request)
                .expect("reads")
                .as_ref(),
            Some(subject),
            "the repository and number find exactly their own subject"
        );
        for node in [subject.adam_node_id, subject.room_node_id] {
            assert_eq!(
                reopened
                    .get_pr_review_subject_by_node(f.project_id, node)
                    .expect("reads")
                    .as_ref(),
                Some(subject)
            );
        }
    }
    assert_eq!(
        reopened
            .get_pr_review_subject(
                f.project_id,
                &f.desk.desk_key,
                &pull_request("Carasent-ASMA/asma-rs-kontor", 299)
            )
            .expect("reads"),
        None
    );
    assert_eq!(
        reopened
            .list_pr_review_subjects(f.project_id, &f.desk.desk_key)
            .expect("lists")
            .len(),
        3
    );
    assert!(
        reopened
            .list_pr_review_subjects(f.project_id, &f.adam.desk_key)
            .expect("lists")
            .is_empty(),
        "another desk holds none of them"
    );
}

#[test]
fn one_pull_request_per_desk_each_node_serves_one_and_every_row_is_immutable() {
    let f = Fixture::build();
    let subject = f.open("Carasent-ASMA/asma-rs-kontor", 297);

    assert!(
        conflict(f.store.create_pr_review_subject(&f.planned(
            &f.desk,
            "Carasent-ASMA/asma-rs-kontor",
            297
        ))),
        "one pull request has one subject on a desk"
    );
    let mut shared_adam = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 300);
    shared_adam.adam_node_id = subject.adam_node_id;
    assert!(conflict(f.store.create_pr_review_subject(&shared_adam)));
    let mut shared_room = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 301);
    shared_room.room_node_id = subject.room_node_id;
    assert!(conflict(f.store.create_pr_review_subject(&shared_room)));
    let mut crossed = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 302);
    crossed.room_node_id = subject.adam_node_id;
    assert!(
        conflict(f.store.create_pr_review_subject(&crossed)),
        "an Adam is never another pull request's room"
    );
    let mut reversed = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 302);
    reversed.adam_node_id = subject.room_node_id;
    assert!(conflict(f.store.create_pr_review_subject(&reversed)));
    let mut same = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 303);
    same.room_node_id = same.adam_node_id;
    assert!(conflict(f.store.create_pr_review_subject(&same)));
    let mut unknown_epic = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 304);
    unknown_epic.mini_project_id = MiniProjectId::generate();
    assert!(conflict(f.store.create_pr_review_subject(&unknown_epic)));
    let mut unknown_desk = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 305);
    unknown_desk.desk_key = desk_key("nowhere");
    assert!(conflict(f.store.create_pr_review_subject(&unknown_desk)));
    let mut unconfirmed = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 306);
    unconfirmed.jira_key = ExternalId::parse("not-a-key").expect("an id");
    assert!(conflict(f.store.create_pr_review_subject(&unconfirmed)));
    f.store
        .create_pr_review_subject(&f.planned(&f.adam, "Carasent-ASMA/asma-rs-kontor", 297))
        .expect("positive control: another desk is another subject");

    let connection = f.raw();
    let updated = connection.execute(
        "UPDATE pr_review_subjects SET jira_key = 'ASMA-1' WHERE pull_request_number = 297",
        [],
    );
    assert!(
        updated
            .expect_err("a subject is immutable")
            .to_string()
            .contains("a PR review subject keeps its pull request")
    );
    let deleted = connection.execute("DELETE FROM pr_review_subjects", []);
    assert!(
        deleted
            .expect_err("a subject is permanent")
            .to_string()
            .contains("a PR review subject is never deleted")
    );
    let repository = connection.execute(
        "INSERT INTO pr_review_subjects
             SELECT project_id, desk_key, 'owner/name/extra', 1, jira_key, mini_project_id,
                    task_id, ?1, ?2, team_definition_id, team_definition_version,
                    team_definition_hash, created_at
               FROM pr_review_subjects WHERE pull_request_number = 297 LIMIT 1",
        rusqlite::params![
            TopologyNodeId::generate().to_string(),
            TopologyNodeId::generate().to_string()
        ],
    );
    assert!(repository.is_err(), "the schema holds one owner/name");
}

#[test]
fn a_task_key_names_a_task_of_its_own_epic() {
    let f = Fixture::build();
    let other_epic = MiniProjectId::generate();
    f.store
        .create_mini_project(&NewMiniProject {
            id: other_epic,
            project_id: f.project_id,
            name: name("Another epic"),
            created_at: f.created_at,
        })
        .expect("the other epic is created");
    let foreign = f.task(other_epic);
    let mut crossing = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 1);
    crossing.task_id = Some(foreign);
    assert!(conflict(f.store.create_pr_review_subject(&crossing)));

    let own = f.task(f.epic);
    let mut owned = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 1);
    owned.task_id = Some(own);
    owned.jira_key = ExternalId::parse("ASMA-8454").expect("a key");
    f.store
        .create_pr_review_subject(&owned)
        .expect("positive control: a task of its own epic");
    assert_eq!(
        f.store
            .get_pr_review_subject(f.project_id, &f.desk.desk_key, &owned.pull_request)
            .expect("reads"),
        Some(owned)
    );
}

#[test]
fn the_bridge_admits_only_the_registered_parents_from_a_newer_revision() {
    let f = Fixture::build();

    // Unregistered: no subject row names either node.
    let unregistered = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 1);
    assert!(conflict(f.node(
        unregistered.adam_node_id,
        "PRA",
        f.desk.workspace_node_id,
        &f.review
    )));
    assert!(conflict(f.node(
        unregistered.room_node_id,
        "RSW",
        f.desk.topology_node_id,
        &f.review
    )));

    let subject = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 2);
    f.store
        .create_pr_review_subject(&subject)
        .expect("the row is written");
    // Another desk's workspace and root are not the registered parents.
    assert!(conflict(f.node(
        subject.adam_node_id,
        "PRA",
        f.adam.workspace_node_id,
        &f.review
    )));
    assert!(conflict(f.node(
        subject.room_node_id,
        "RSW",
        f.adam.topology_node_id,
        &f.review
    )));
    // Each node has exactly its own parent: the Adam is not the room's.
    assert!(conflict(f.node(
        subject.room_node_id,
        "PRA",
        f.desk.workspace_node_id,
        &f.review
    )));
    assert!(conflict(f.node(
        subject.adam_node_id,
        "RSW",
        f.desk.topology_node_id,
        &f.review
    )));

    // A desk whose declaration changed between the two revisions.
    let mut changed = revision(f.base_version + 3, with_kinds(desk_kinds(), review_kinds()));
    for declared in &mut changed.node_kinds {
        if declared.kind == kind("DWS") {
            declared.read_only = true;
        }
    }
    let changed = f.publish(&changed);
    let moved = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 3);
    f.store.create_pr_review_subject(&moved).expect("row");
    assert!(
        conflict(f.node(
            moved.adam_node_id,
            "PRA",
            f.desk.workspace_node_id,
            &changed
        )),
        "the workspace changed, so the Adam edge is refused"
    );
    f.node(moved.room_node_id, "RSW", f.desk.topology_node_id, &changed)
        .expect("the desk itself did not change, so the room edge is admitted");

    // Another lineage entirely, with every kind and a higher version.
    let mut foreign = revision(f.base_version + 4, with_kinds(desk_kinds(), review_kinds()));
    foreign.spec_id = TopologySpecId::generate();
    let foreign = f.publish(&foreign);
    let crossing = f.planned(&f.desk, "Carasent-ASMA/asma-rs-kontor", 4);
    f.store.create_pr_review_subject(&crossing).expect("row");
    assert!(conflict(f.node(
        crossing.adam_node_id,
        "PRA",
        f.desk.workspace_node_id,
        &foreign
    )));
    assert!(conflict(f.node(
        crossing.room_node_id,
        "RSW",
        f.desk.topology_node_id,
        &foreign
    )));

    // Positive control: the exact registered parents.
    f.node(
        subject.adam_node_id,
        "PRA",
        f.desk.workspace_node_id,
        &f.review,
    )
    .expect("the registered workspace hosts its Adam");
    f.node(
        subject.room_node_id,
        "RSW",
        f.desk.topology_node_id,
        &f.review,
    )
    .expect("the registered desk holds its room");
    assert!(
        f.store
            .get_topology_node(f.project_id, unregistered.adam_node_id)
            .expect("reads")
            .is_none(),
        "a refused node leaves nothing"
    );
}

#[test]
fn the_room_owns_a_container_and_the_adam_owns_none() {
    let f = Fixture::build();
    let subject = f.open("Carasent-ASMA/asma-rs-kontor", 297);
    f.store
        .bind_topology_node_container(&f.container(subject.room_node_id, "wks_room"))
        .expect("the room is a native workspace of its own");
    assert!(
        f.store
            .bind_topology_node_container(&f.container(subject.adam_node_id, "wks_adam"))
            .expect_err("the Adam borrows the desk workspace")
            .to_string()
            .contains("owns no native container")
    );
}

#[test]
fn the_receipt_kinds_admit_pr_review_commands_and_stay_closed() {
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
        "open_pr_review",
        "close_pr_review",
        // The earlier kinds survive the rebuild.
        "open_ad_hoc_desk_adam",
        "hand_off_desk_adam",
        "declare_durable_seat",
        "reconcile_durable_seats",
    ]
    .into_iter()
    .enumerate()
    {
        receipt(
            &format!("0193f000-0000-7000-8000-00000000014{index}"),
            kind,
            kind,
        )
        .expect("the kind is admitted");
    }
    assert!(
        receipt(
            "0193f000-0000-7000-8000-000000000149",
            "unknown",
            "merge_pr_review",
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
fn a_verified_snapshot_restore_keeps_every_subject() {
    let f = Fixture::build();
    let subject = f.open("Carasent-ASMA/asma-rs-kontor", 297);
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
            .list_pr_review_subjects(f.project_id, &f.desk.desk_key)
            .expect("lists"),
        vec![subject]
    );
}

#[test]
fn the_typed_export_carries_subjects_and_no_older_generation_can_carry_or_conceal_them() {
    let f = Fixture::build();
    let subject = f.open("Carasent-ASMA/asma-rs-kontor", 297);
    let export = export_realm(&f.store, at("2026-10-11T10:00:00Z")).expect("the Realm exports");
    assert_eq!(export.records.pr_review_subjects.len(), 1);
    let row = &export.records.pr_review_subjects[0];
    assert_eq!(row.repository, "Carasent-ASMA/asma-rs-kontor");
    assert_eq!(row.pull_request_number, 297);
    assert_eq!(row.adam_node_id, subject.adam_node_id.to_string());
    assert_eq!(row.room_node_id, subject.room_node_id.to_string());
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

    // Generation 16 (schema 133) is the last one before PR review subjects:
    // it proves desk Adam subjects, so only the PR review check can refuse it.
    let mut carrying = base.clone();
    carrying["schema_version"] = json!(16);
    carrying["database_schema_version"] = json!(133);
    rehash(&mut carrying);
    assert_eq!(
        refusal(carrying),
        "the legacy export generation carries PR review subjects it did not define"
    );

    let mut concealing = base.clone();
    concealing["schema_version"] = json!(16);
    concealing["records"]["pr_review_subjects"] = json!([]);
    concealing["continuity_summary"]["record_counts"]["pr_review_subjects"] = json!(0);
    rehash(&mut concealing);
    assert_eq!(
        refusal(concealing),
        "the legacy export generation cannot prove PR review subject completeness"
    );

    let mut understated = base;
    understated["continuity_summary"]["record_counts"]["pr_review_subjects"] = json!(0);
    rehash(&mut understated);
    assert!(
        KontorExportV1::parse(&serde_json::to_vec(&understated).expect("bytes")).is_err(),
        "the continuity summary must disclose every subject"
    );
}
