//! Real stored membership and permanent prepared metadata, never live proof.
use super::support;

use kontor_core::consultation::{
    ConsultationFamily, ConsultationRunId, ConsultationRunState, ConsultationSubject,
};
use kontor_core::id::{
    AdvisorRunId, AggregateRevision, BoundedText, ContentHash, ExternalId, ExternalName,
    IdempotencyKey, MiniProjectId, ProjectId, RoleCode, RoleSlotId, RuntimeKindKey, SeatBindingId,
    SpecVersion, TaskId, Timestamp, TopologyKindKey, TopologyNodeId,
};
use kontor_core::repository::{
    AttestationAuthorityRepository, AttestationAuthorityScope, MiniProjectTopologySnapshot,
    NewMiniProject, NewProject, NewSeatBinding, NewSessionTopologyNode, NewTask,
    PrepareAttestationToken, ProjectRepository, RegisterAttestationKey, RepositoryError,
    StoredConsultationProfileRevision, StoredConsultationRun, StoredConsultationSeat,
    StoredHostedTopologySeat, TopologyRepository,
};
use kontor_core::spec::{
    CatalogRoleRef, ModelRef, ModelRung, ProviderRef, SeatAutonomy, Shareability, ShareabilityTier,
    TopologySnapshot,
};
use kontor_core::state::{NativeRuntimeIdentity, TaskState};
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use rusqlite::Connection;
use tempfile::TempDir;

pub(super) fn id(value: &str) -> ExternalId {
    ExternalId::parse(value).expect("id")
}
pub(super) fn name(value: &str) -> ExternalName {
    ExternalName::parse(value).expect("name")
}
pub(super) fn native(value: &str) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("kind"),
        host: name("fixture-host"),
        generation: 11,
        native_id: id(value),
    }
}
fn rung() -> ModelRung {
    ModelRung {
        provider: ProviderRef("codex".into()),
        model: ModelRef("fixture".into()),
        effort: None,
    }
}

fn fixture_project(store: &SqliteStore, at: Timestamp) -> (ProjectId, MiniProjectId, TaskId) {
    let project = ProjectId::generate();
    let epic = MiniProjectId::generate();
    let task = TaskId::generate();
    store
        .create_project(&NewProject {
            id: project,
            name: name(&format!("Fixture {project}")),
            root_path: name(&format!("/tmp/{project}")),
            created_at: at,
        })
        .expect("project");
    store
        .create_mini_project(&NewMiniProject {
            id: epic,
            project_id: project,
            name: name("Token epic"),
            created_at: at,
        })
        .expect("epic");
    store
        .create_task(&NewTask {
            id: task,
            project_id: project,
            mini_project_id: Some(epic),
            title: name("Token task"),
            module: None,
            state: TaskState::Ready,
            created_at: at,
        })
        .expect("task");
    (project, epic, task)
}

fn fixture_nodes(
    store: &SqliteStore,
    project: ProjectId,
    epic: MiniProjectId,
    topology: &TopologySnapshot,
    at: Timestamp,
) -> (TopologyNodeId, TopologyNodeId) {
    let root = TopologyNodeId::generate();
    let esw = TopologyNodeId::generate();
    let node = TopologyNodeId::generate();
    for (node_id, kind, parent, mini) in [
        (root, "PSW", None, None),
        (esw, "ESW", Some(root), Some(epic)),
        (node, "ECP", Some(esw), Some(epic)),
    ] {
        store
            .create_topology_node(&NewSessionTopologyNode {
                id: node_id,
                project_id: project,
                mini_project_id: mini,
                topology: topology.clone(),
                kind: TopologyKindKey::parse(kind).expect("kind"),
                parent_id: parent,
                task_id: None,
                created_at: at,
            })
            .expect("node");
    }
    (esw, node)
}

fn fixture_key(store: &SqliteStore, scope: &AttestationAuthorityScope) {
    store
        .register_attestation_key(&RegisterAttestationKey {
            scope: scope.clone(),
            expected_head_revision: None,
            issuer: id("issuer"),
            key_id: id("key-1"),
            public_key_der: vec![1, 2, 3],
            not_before: 10,
            expires_at: 1000,
        })
        .expect("key");
}

pub(super) struct Fixture {
    pub(super) home: TempDir,
    pub(super) store: SqliteStore,
    pub(super) scope: AttestationAuthorityScope,
    pub(super) epic: MiniProjectId,
    pub(super) task: TaskId,
    pub(super) esw: TopologyNodeId,
    pub(super) node: TopologyNodeId,
    pub(super) seat: SeatBindingId,
    pub(super) topology: TopologySnapshot,
    pub(super) role: CatalogRoleRef,
}
impl Fixture {
    pub(super) fn build() -> Self {
        let home = support::state_root();
        let store = SqliteStore::open(&home.path().join("kontor.db")).expect("store");
        let at = Timestamp::now();
        let (project, epic, task) = fixture_project(&store, at);
        let domain = bundled_operational_domain().expect("domain");
        let spec = &domain.topology_specs[0];
        let catalog = &domain.role_catalogs[0];
        let stamp = Shareability::default_for(ShareabilityTier::ProjectKnowledge).expect("stamp");
        let hash = store
            .publish_topology_spec(project, spec, &stamp, at)
            .expect("topology");
        store
            .publish_role_catalog(catalog, &stamp, at)
            .expect("catalog");
        let topology = TopologySnapshot {
            spec_id: spec.spec_id,
            version: spec.version,
            canonical_hash: hash,
        };
        store
            .pin_mini_project_topology(&MiniProjectTopologySnapshot {
                project_id: project,
                mini_project_id: epic,
                topology: topology.clone(),
                pinned_at: at,
            })
            .expect("pin");
        let (esw, node) = fixture_nodes(&store, project, epic, &topology, at);
        let entry = catalog
            .role(&RoleCode::parse("LSA").expect("role"))
            .expect("entry");
        let role = CatalogRoleRef {
            catalog_id: catalog.catalog_id,
            catalog_revision: catalog.version,
            role_code: entry.role_code.clone(),
            standard_title: entry.standard_title.clone(),
            custom_display_name: None,
        };
        let seat = SeatBindingId::generate();
        store
            .create_seat_binding(&NewSeatBinding {
                id: seat,
                project_id: project,
                topology_node_id: node,
                role_slot_id: RoleSlotId::parse("epic.lsa").expect("slot"),
                role: role.clone(),
                task_id: None,
                team_run_id: None,
                attach_deadline: at,
                parent_seat_binding_id: None,
                created_at: at,
            })
            .expect("binding");
        store
            .bind_hosted_topology_seat(&StoredHostedTopologySeat {
                project_id: project,
                seat_binding_id: seat,
                model_rung: rung(),
                native_identity: native("hosted-current"),
                autonomy: SeatAutonomy::Supervised,
                provider_session_id: None,
                observed_at: at,
            })
            .expect("hosted");
        let scope = AttestationAuthorityScope {
            realm_id: store.realm_id(),
            project_id: project,
            application: id("asma.planning-pair.application.v1"),
        };
        fixture_key(&store, &scope);
        Self {
            home,
            store,
            scope,
            epic,
            task,
            esw,
            node,
            seat,
            topology,
            role,
        }
    }
    pub(super) fn sql(&self) -> Connection {
        let c = Connection::open(self.home.path().join("kontor.db")).expect("sql");
        c.execute_batch("PRAGMA foreign_keys=ON").expect("FK");
        c
    }
    pub(super) fn request(&self) -> PrepareAttestationToken {
        PrepareAttestationToken {
            scope: self.scope.clone(),
            expected_key_head_revision: 1,
            expected_token_head_revision: None,
            issuer: id("issuer"),
            key_id: id("key-1"),
            token_id: id("token-1"),
            mini_project_id: self.epic,
            task_id: None,
            seat_binding_id: self.seat,
            expected_occupancy_generation: 1,
            expected_native_identity: native("hosted-current"),
            not_before: 10,
            expires_at: 1000,
            payload_digest: ContentHash::of(b"unqualified payload"),
        }
    }
    pub(super) fn counts(&self) -> (i64, i64, i64) {
        self.sql().query_row("SELECT (SELECT count(*) FROM prepared_attestation_tokens),(SELECT count(*) FROM attestation_token_heads),COALESCE((SELECT max(revision) FROM attestation_token_heads),0)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).expect("counts")
    }
    fn frozen_run(
        &self,
        run_id: ConsultationRunId,
        profile: String,
        task: Option<TaskId>,
        node: TopologyNodeId,
        at: Timestamp,
    ) -> StoredConsultationRun {
        let question = BoundedText::parse("Check tokens").expect("question");
        StoredConsultationRun {
            id: run_id,
            project_id: self.scope.project_id,
            mini_project_id: self.epic,
            profile_id: profile,
            profile_version: SpecVersion::FIRST,
            definition_hash: ContentHash::of(br#"{"schema_version":1}"#),
            semantic_identity_hash: Some(ContentHash::of(run_id.as_text().as_bytes())),
            subject: Some(task.map_or(ConsultationSubject::Epic, ConsultationSubject::Task)),
            topic: Some(name("Token review")),
            question_hash: ContentHash::of(question.as_str().as_bytes()),
            question,
            context: serde_json::json!({"schema_version":1}),
            context_hash: ContentHash::of(br#"{"schema_version":1}"#),
            caller_seat_binding_id: self.seat,
            topology_node_id: node,
            invoke_key: IdempotencyKey::parse(&run_id.as_text()).expect("invoke"),
            invoke_intent_hash: ContentHash::of(b"intent"),
            state: ConsultationRunState::Materializing,
            round: 1,
            result: None,
            result_hash: None,
            revision: AggregateRevision::INITIAL,
            created_at: at,
            updated_at: at,
            settled_at: None,
        }
    }

    pub(super) fn consultation(
        &self,
        task: Option<TaskId>,
    ) -> (PrepareAttestationToken, ConsultationRunId, TopologyNodeId) {
        self.consultation_family(task, ConsultationFamily::Advisor)
    }

    // Registry/native metadata fixture only. This intentionally makes no claim
    // of profile/placement/protocol qualification or a completed native flow.
    pub(super) fn consultation_family(
        &self,
        task: Option<TaskId>,
        family: ConsultationFamily,
    ) -> (PrepareAttestationToken, ConsultationRunId, TopologyNodeId) {
        let at = Timestamp::now();
        let profile = AdvisorRunId::generate().to_string();
        let digest = ContentHash::of(br#"{"schema_version":1}"#);
        self.store
            .publish_consultation_profile_revision(&StoredConsultationProfileRevision {
                project_id: self.scope.project_id,
                family,
                profile_id: profile.clone(),
                version: SpecVersion::FIRST,
                name: name("Advisor"),
                definition: r#"{"schema_version":1}"#.into(),
                definition_hash: digest.clone(),
                published_at: at,
            })
            .expect("profile");
        let run_id = match family {
            ConsultationFamily::Advisor => ConsultationRunId::Advisor(AdvisorRunId::generate()),
            ConsultationFamily::Committee => {
                ConsultationRunId::Committee(kontor_core::id::CommitteeRunId::generate())
            }
            ConsultationFamily::PlanningPair => {
                ConsultationRunId::PlanningPair(kontor_core::id::PlanningPairRunId::generate())
            }
        };
        let node = TopologyNodeId::generate();
        let seat = SeatBindingId::generate();
        let slot = RoleSlotId::parse("advisor.sa").expect("slot");
        let run = self.frozen_run(run_id, profile, task, node, at);
        let binding = NewSeatBinding {
            id: seat,
            project_id: self.scope.project_id,
            topology_node_id: node,
            role_slot_id: slot.clone(),
            role: self.role.clone(),
            task_id: None,
            team_run_id: None,
            attach_deadline: at,
            parent_seat_binding_id: Some(self.seat),
            created_at: at,
        };
        let mut observed = StoredConsultationSeat {
            run_id,
            role_slot_id: slot,
            committee_role: (family == ConsultationFamily::Committee)
                .then_some(kontor_core::consultation::CommitteeRole::Judge),
            logical_role: kontor_core::id::RoleKey::parse("sa").expect("role"),
            seat_binding_id: seat,
            model_rung: rung(),
            occupancy_generation: 7,
            native_identity: None,
            provider_session_id: None,
            observed_at: None,
        };
        self.store
            .create_consultation_run(
                &run,
                &NewSessionTopologyNode {
                    id: node,
                    project_id: self.scope.project_id,
                    mini_project_id: Some(self.epic),
                    topology: self.topology.clone(),
                    kind: TopologyKindKey::parse("ASW").expect("kind"),
                    parent_id: Some(self.esw),
                    task_id: None,
                    created_at: at,
                },
                &[(&observed, &binding)],
            )
            .expect("consultation");
        observed.native_identity = Some(native(&run_id.as_text()));
        observed.observed_at = Some(at);
        self.store
            .bind_consultation_seat(self.scope.project_id, &observed)
            .expect("native binding");
        let mut request = self.request();
        request.task_id = task;
        request.seat_binding_id = seat;
        request.expected_occupancy_generation = 7;
        request.expected_native_identity = observed.native_identity.expect("native");
        request.token_id = id(&run_id.as_text());
        (request, run_id, node)
    }
    pub(super) fn refuse_unchanged(&self, request: &PrepareAttestationToken) -> RepositoryError {
        let before = self.counts();
        let error = self
            .store
            .prepare_attestation_token(request)
            .expect_err("must refuse");
        assert_eq!(self.counts(), before, "refusal changed token state");
        error
    }
}
