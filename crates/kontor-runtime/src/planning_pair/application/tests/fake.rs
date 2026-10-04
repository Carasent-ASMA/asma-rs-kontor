//! The hypothetical owner the trace tests drive: it records every port call
//! in order and answers each scenario's facts. Any port a scenario must not
//! reach answers an error and appears in the trace.

use std::sync::Mutex;

use super::super::*;
use super::{at, id, pair_state, run};
use crate::capability::{RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade};
use crate::fake::ScriptedFakeRuntime;

/// Which facts the hypothetical owner answers with.
#[derive(Clone, Copy, Default)]
pub(super) struct Scenario {
    pub(super) hash_moved: bool,
    pub(super) unauthorized: bool,
    pub(super) replay: bool,
    pub(super) receipt_without_run: bool,
    /// The key froze a run: `Some(foreign)` names whether it is another one.
    pub(super) existing: Option<bool>,
    pub(super) epic_moved: bool,
    pub(super) running_already: bool,
    pub(super) routes_refused: bool,
    pub(super) advance_lost: Option<ConsultationRunState>,
    pub(super) inserted: bool,
}

pub(super) struct Fake {
    scenario: Scenario,
    trace: Mutex<Vec<String>>,
    intent: Mutex<Option<ContentHash>>,
}

impl Fake {
    pub(super) fn new(scenario: Scenario) -> Self {
        Self {
            scenario,
            trace: Mutex::new(Vec::new()),
            intent: Mutex::new(None),
        }
    }

    fn step(&self, name: &str) {
        self.trace.lock().expect("trace").push(name.to_owned());
    }

    pub(super) fn trace(&self) -> Vec<String> {
        self.trace.lock().expect("trace").clone()
    }

    fn frozen(&self) -> StoredConsultationRun {
        let intent = self
            .intent
            .lock()
            .expect("intent")
            .clone()
            .expect("an intent");
        let state = if self.scenario.running_already {
            ConsultationRunState::Running
        } else {
            ConsultationRunState::Materializing
        };
        run(&intent, state)
    }

    fn unexpected<T>(&self, name: &str) -> Result<T, String> {
        self.step(name);
        Err(format!("unexpected {name}"))
    }
}

fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL.iter().copied().collect(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 1024,
            max_history_page: 100,
            max_concurrent_sessions: 8,
            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
        },
    }
}

#[async_trait::async_trait]
impl InvokeOwner for Fake {
    type Error = String;
    type Activity = ();
    type Caller = u64;
    type Selection = serde_json::Value;
    type Output = (CommandReceiptId, Applied);

    fn refuse(&self, refusal: InvokeRefusal) -> String {
        let name = format!("refuse:{refusal:?}");
        self.step(&name);
        name
    }
    fn domain_error(&self, error: &DomainError) -> String {
        format!("domain:{error}")
    }
    fn runtime_error(&self, error: &RuntimeError) -> String {
        format!("runtime:{error}")
    }
    fn now(&self) -> Timestamp {
        at()
    }
    fn realm_id(&self) -> Result<RealmId, String> {
        self.unexpected("realm_id")
    }
    fn begin_native_activity(&self) -> Result<(), String> {
        self.step("begin_native_activity");
        Ok(())
    }
    fn pinned_document(
        &self,
        _: ProjectId,
        _: &str,
        _: SpecVersion,
    ) -> Result<(StoredConsultationProfileRevision, PlanningPairSpec), String> {
        self.step("pinned_document");
        let mut state = pair_state(
            &run(&ContentHash::of(b"x"), ConsultationRunState::Running),
            true,
        );
        if self.scenario.hash_moved {
            state.revision.definition_hash = ContentHash::of(b"another document");
        }
        Ok((state.revision, state.spec))
    }
    fn authorize_caller(
        &self,
        _: ProjectId,
        _: MiniProjectId,
        _: Option<TaskId>,
        _: &PlanningPairSpec,
        caller: u64,
    ) -> Result<SeatBinding, String> {
        self.step("authorize_caller");
        if self.scenario.unauthorized {
            return Err("unauthorized".to_owned());
        }
        serde_json::from_value(serde_json::json!({
            "id": id("a1"), "project_id": id("f1"), "topology_node_id": id("c0"),
            "role_slot_id": "lsa",
            "role": {"catalog_id": id("d1"), "catalog_revision": 1, "role_code": "LSA",
                     "standard_title": "Lead Solution Architect"},
            "lifecycle": "active", "attach_deadline": "2026-10-02T10:00:00Z",
            "revision": caller, "created_at": "2026-10-02T10:00:00Z",
            "updated_at": "2026-10-02T10:00:00Z",
        }))
        .map_err(|error| error.to_string())
    }
    fn presented(&self, caller: u64) -> SeatGeneration {
        self.step("presented");
        SeatGeneration {
            seat_binding_id: SeatBindingId::parse(&id("a1")).expect("a caller"),
            occupancy_generation: caller,
        }
    }
    fn canonical(&self, document: &serde_json::Value) -> Result<CanonicalDocument, String> {
        self.step("canonical");
        let document = CanonicalDocument::from_value(document).map_err(|e| e.to_string())?;
        *self.intent.lock().expect("intent") = Some(document.hash().clone());
        Ok(document)
    }
    fn replayed(
        &self,
        _: &IdempotencyKey,
        _: &CanonicalDocument,
        _: &AggregateRef,
    ) -> Result<Option<CommandReceiptId>, String> {
        self.step("replayed");
        Ok(self
            .scenario
            .replay
            .then(|| CommandReceiptId::parse(&id("99")).expect("a receipt")))
    }
    fn run_by_key(
        &self,
        _: ProjectId,
        _: &IdempotencyKey,
    ) -> Result<Option<StoredConsultationRun>, String> {
        self.step("run_by_key");
        if self.scenario.receipt_without_run {
            return Ok(None);
        }
        Ok(match self.scenario.existing {
            Some(true) => Some(run(
                &ContentHash::of(b"another intent"),
                ConsultationRunState::Running,
            )),
            Some(false) => Some(self.frozen()),
            None => None,
        })
    }
    fn epic(&self, _: ProjectId, _: MiniProjectId) -> Result<MiniProject, String> {
        self.step("epic");
        let revision = if self.scenario.epic_moved { 2 } else { 1 };
        serde_json::from_value(serde_json::json!({
            "id": id("e1"), "project_id": id("f1"), "key": "PAIR", "name": "Pair",
            "description": null, "lifecycle": "active", "revision": revision,
            "created_at": "2026-10-02T10:00:00Z", "updated_at": "2026-10-02T10:00:00Z",
        }))
        .map_err(|error| format!("epic fixture: {error}"))
    }
    fn pair_state(&self, run: &StoredConsultationRun) -> Result<PairState, String> {
        self.step("pair_state");
        Ok(pair_state(
            run,
            self.scenario.existing == Some(false) && !self.scenario.routes_refused,
        ))
    }
    fn advance_to_running(
        &self,
        frozen: &StoredConsultationRun,
    ) -> Result<StoredConsultationRun, String> {
        self.step("advance_to_running");
        if self.scenario.advance_lost.is_some() {
            return Err("advance lost".to_owned());
        }
        let mut advanced = frozen.clone();
        advanced.state = ConsultationRunState::Running;
        Ok(advanced)
    }
    fn stored_run(
        &self,
        _: ProjectId,
        _: PlanningPairRunId,
    ) -> Result<StoredConsultationRun, String> {
        self.step("stored_run");
        let mut after = self.frozen();
        after.state = self
            .scenario
            .advance_lost
            .unwrap_or(ConsultationRunState::Running);
        Ok(after)
    }
    async fn receipt_hold_point(&self) {
        self.step("receipt_hold_point");
    }
    fn record(
        &self,
        _: &IdempotencyKey,
        _: ProjectId,
        kind: CommandKind,
        _: AggregateRef,
        _: AggregateRevision,
        _: &CanonicalDocument,
    ) -> Result<(CommandReceiptId, bool), String> {
        self.step(&format!("record:{}", kind.as_str()));
        Ok((
            CommandReceiptId::parse(&id("98")).expect("a receipt"),
            self.scenario.inserted,
        ))
    }
    fn render(
        &self,
        _: &PairState,
        receipt: CommandReceiptId,
        applied: Applied,
    ) -> Result<(CommandReceiptId, Applied), String> {
        self.step(&format!("render:{applied:?}"));
        Ok((receipt, applied))
    }
    fn pinned_team_definition(
        &self,
        _: ProjectId,
        _: MiniProjectId,
    ) -> Result<Option<TeamDefinitionSpec>, String> {
        self.step("pinned_team_definition");
        Ok(None)
    }
    fn epic_role_catalog(
        &self,
        _: ProjectId,
        _: MiniProjectId,
    ) -> Result<RoleCatalogRevision, String> {
        self.unexpected("epic_role_catalog")
    }
    fn member_catalog_role(
        &self,
        _: &RoleCatalogRevision,
        _: &RoleCode,
    ) -> Result<CatalogRoleRef, String> {
        self.unexpected("member_catalog_role")
    }
    fn semantic_identity(
        &self,
        _: ProjectId,
        _: MiniProjectId,
        _: Option<TaskId>,
        _: ConsultationFamily,
        _: &TopologyKindKey,
        _: &StoredConsultationProfileRevision,
        _: &TeamDefinitionSpec,
        _: &ExternalName,
    ) -> Result<ContentHash, String> {
        self.unexpected("semantic_identity")
    }
    fn run_by_semantic_identity(
        &self,
        _: ProjectId,
        _: &ContentHash,
    ) -> Result<Option<StoredConsultationRun>, String> {
        self.unexpected("run_by_semantic_identity")
    }
    fn place(&self, _: &[InvokeMember<'_>]) -> Result<FrozenPlacement<serde_json::Value>, String> {
        self.unexpected("place")
    }
    fn runtime(&self) -> Result<Arc<dyn RuntimeAdapter>, String> {
        self.step("runtime");
        Ok(Arc::new(ScriptedFakeRuntime::new(capabilities())))
    }
    fn require_member_routes(
        &self,
        _: &dyn RuntimeAdapter,
        _: &[PlanningPairMember],
    ) -> Result<(), String> {
        self.step("require_member_routes");
        if self.scenario.routes_refused {
            return Err("route refused".to_owned());
        }
        Ok(())
    }
    fn project_topology(&self, _: ProjectId) -> Result<TopologySnapshot, String> {
        self.unexpected("project_topology")
    }
    fn ensure_epic_node(
        &self,
        _: ProjectId,
        _: MiniProjectId,
    ) -> Result<SessionTopologyNode, String> {
        self.unexpected("ensure_epic_node")
    }
    fn attach_deadline(&self, now: Timestamp) -> Timestamp {
        now
    }
    fn create_run(
        &self,
        _: &StoredConsultationRun,
        _: &NewSessionTopologyNode,
        _: &[(&StoredConsultationSeat, &NewSeatBinding)],
        _: &StoredPlanningPairPlacement,
        _: &StoredPlanningPairRecord,
    ) -> Result<(), String> {
        self.unexpected("create_run")
    }
    fn project(&self, _: ProjectId) -> Result<Project, String> {
        self.step("project");
        serde_json::from_value(serde_json::json!({
            "id": id("f1"), "name": "Pair", "root_path": "/realm/project",
            "created_at": "2026-10-02T10:00:00Z", "updated_at": "2026-10-02T10:00:00Z",
            "revision": 1,
        }))
        .map_err(|error| format!("project fixture: {error}"))
    }
    fn topology_node(
        &self,
        _: ProjectId,
        _: TopologyNodeId,
    ) -> Result<Option<SessionTopologyNode>, String> {
        self.step("topology_node");
        serde_json::from_value(serde_json::json!({
            "id": id("c1"), "project_id": id("f1"), "mini_project_id": id("e1"),
            "topology": {"spec_id": id("c2"), "version": 1, "canonical_hash": "a".repeat(64)},
            "kind": "PPW", "lifecycle": "active", "placement": "bound", "revision": 1,
            "created_at": "2026-10-02T10:00:00Z", "updated_at": "2026-10-02T10:00:00Z",
        }))
        .map(Some)
        .map_err(|error| format!("node fixture: {error}"))
    }
    fn epic_tracker_key(
        &self,
        _: ProjectId,
        _: MiniProjectId,
    ) -> Result<Option<kontor_core::branch::TrackerKey>, String> {
        self.step("epic_tracker_key");
        Ok(None)
    }
    fn consultation_root(
        &self,
        _: &str,
        _: Option<&kontor_core::branch::TrackerKey>,
        _: TopologyNodeId,
    ) -> Result<WorkspaceRoot, String> {
        self.step("consultation_root");
        WorkspaceRoot::parse("/realm/pair").map_err(|error| error.to_string())
    }
    fn seat_binding(&self, _: ProjectId, _: SeatBindingId) -> Result<Option<SeatBinding>, String> {
        self.unexpected("seat_binding")
    }
    fn require_member_role(
        &self,
        _: &RoleCatalogRevision,
        _: &CatalogRoleRef,
        _: &RoleCode,
    ) -> Result<(), String> {
        self.unexpected("require_member_role")
    }
    async fn ensure_container(
        &self,
        _: ProjectId,
        _: &SessionTopologyNode,
        _: &WorkspaceRoot,
        _: &dyn RuntimeAdapter,
    ) -> Result<ContainerBindingSnapshot, String> {
        self.unexpected("ensure_container")
    }
    fn execution_scope(
        &self,
        _: ProjectId,
        _: MiniProjectId,
        _: Option<TaskId>,
        _: &dyn RuntimeAdapter,
    ) -> Result<ExecutionScope, String> {
        self.unexpected("execution_scope")
    }
    fn member_credential(
        &self,
        _: SeatBindingId,
        _: u64,
    ) -> Result<ConsultationCredential, String> {
        self.unexpected("member_credential")
    }
    fn seat_name(
        &self,
        _: ProjectId,
        _: &SessionTopologyNode,
        _: &ExecutionScope,
        _: &RoleCode,
        _: Option<&RoleSlotId>,
    ) -> Result<ExternalName, String> {
        self.unexpected("seat_name")
    }
    fn launch_context(
        &self,
        _: &StoredConsultationRun,
        _: &PairState,
        _: &StoredConsultationSeat,
        _: PlanningPairSlot,
        _: &SessionTopologyNode,
        _: &WorkspaceRoot,
        _: &RoleCatalogRevision,
        _: Option<&FleetLaunchProvenance>,
    ) -> Result<PlanningPairLaunchContext, String> {
        self.unexpected("launch_context")
    }
    fn record_launch_provenance(
        &self,
        _: SeatBindingId,
        _: &ExternalId,
        _: Option<&FleetLaunchProvenance>,
        _: &FleetProvenanceObservation,
    ) {
        self.step("record_launch_provenance");
    }
    fn record_member_launch(&self, _: &StoredPlanningPairKnownNative) -> Result<(), String> {
        self.unexpected("record_member_launch")
    }
    fn observe_member_attached(
        &self,
        _: ProjectId,
        _: SeatBindingId,
        _: Timestamp,
    ) -> Result<(), String> {
        self.unexpected("observe_member_attached")
    }
}
