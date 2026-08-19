//! The delegation contract: pinned policy, one writer, confirmed effects.
//!
//! Two fixtures drive every workflow assertion — the ASMA seed this build ships
//! and a synthetic project whose statuses share not one id or name with it. The
//! same internal facts go into both and must produce the same *decision shape*
//! while producing each project's own external targets. That is what proves the
//! evaluator has no name branch, and it is why loading a third project needs no
//! code change.
//!
//! Everything that reaches the world goes through a real temporary executable.
//! It records its argv and its stdin, so the suite can assert on what actually
//! crossed the boundary rather than on what a mock was told to expect.
//!
//! The mutants this suite exists to kill:
//!
//! * hard-coding an ASMA status name or id in shipped source;
//! * selecting the first live transition, or reusing a remembered transition id;
//! * treating the internal `qa_ready` fact as active external QA;
//! * clearing or replacing an existing holder under `preserve`;
//! * transitioning before assignment confirmation;
//! * serializing an absent field as null;
//! * accepting an apply without a final refetch;
//! * replaying a transition after a lost acknowledgement;
//! * reading fleet state or calling the ticket system directly from Rust.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kontor_core::id::{
    AggregateRevision, BoundedText, CommandReceiptId, ConnectorKey, ContentHash, ExternalId,
    ExternalIssueTypeKey, ExternalName, ExternalProjectKey, GateKey, IdempotencyKey,
    SemanticMilestoneKey, SpecVersion, TaskId, TicketLinkId, TicketObservationId,
    TicketProjectionId, Timestamp, WorkProfileKey, parse_utc_timestamp,
};
use kontor_core::state::{GateState, TaskState, TerminalOutcome};
use kontor_core::ticket::{
    AssignmentPlan, CommentPolicy, ExternalTicketObservation, ExternalWorkflowSpec, FieldOwner,
    FieldValue, InternalTaskFacts, LiveTransition, OwnershipAction, ProjectedField,
    ReconciliationInput, ReconciliationOutcome, SelectedTransition, StatusConflictKind,
    StatusSelector, TicketFieldKey, TicketPrincipal, TicketSyncProjection, TransitionPlan,
    reconcile,
};
use kontor_integrations_asma::jira::{
    AmbiguityVerdict, ApplyAuthority, CompiledFieldSpec, CompiledWorkflowSpec, FieldSpecKey,
    JiraOperation, JiraOutcome, JiraRequest, JiraResponse, Observed, PinnedProfile, SpecCatalog,
    TicketDelegation, WireConfirmation, WireEffects, WireFailure, WireObservation, WireTransition,
    WorkflowSpecKey, compile_field_writes,
};
use kontor_integrations_asma::{
    AsmaError, AsmaExecutable, SelectionConflict, UnavailableReason, WIRE_SCHEMA_VERSION,
    WireTimestamp,
};

const ALTERNATE_WORKFLOW: &str = include_str!("fixtures/external-workflow-alternate.json");

const IMPLEMENTATION_ACTIVE: &str = "implementation_active";
const QA_READY: &str = "qa_ready";
const QA_ACTIVE: &str = "qa_active";
const TERMINAL_DONE: &str = "terminal_done";

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn external(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("valid external id")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("valid external name")
}

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("canonical UTC timestamp")
}

fn wire_at(text: &str) -> WireTimestamp {
    WireTimestamp::new(at(text))
}

fn milestone(text: &str) -> SemanticMilestoneKey {
    SemanticMilestoneKey::parse(text).expect("valid milestone key")
}

/// The catalogue this build ships plus the synthetic second project.
fn catalog() -> SpecCatalog {
    let mut catalog = SpecCatalog::bundled().expect("the bundled specifications load");
    catalog
        .load_workflow_spec(ALTERNATE_WORKFLOW)
        .expect("the alternate workflow fixture loads");
    // A second project needs a field specification of its own. It is the ASMA
    // one re-keyed, because the field contract is not what this fixture varies.
    let mut alternate_fields = asma_field_spec().spec().clone();
    alternate_fields.project = alternate_workflow_spec().project.clone();
    alternate_fields.issue_type = alternate_workflow_spec().issue_type.clone();
    catalog
        .load_field_spec(
            &serde_json::to_string(&alternate_fields).expect("the re-keyed spec serializes"),
        )
        .expect("the re-keyed field specification loads");
    catalog
}

fn asma_workflow_key() -> WorkflowSpecKeyOwned {
    WorkflowSpecKeyOwned {
        project: "asma",
        issue_type: "task",
        profile: Some(("code", 1)),
    }
}

fn alternate_workflow_key() -> WorkflowSpecKeyOwned {
    WorkflowSpecKeyOwned {
        project: "nordlys",
        issue_type: "sak",
        profile: None,
    }
}

/// The three coordinates a test varies, spelled once.
struct WorkflowSpecKeyOwned {
    project: &'static str,
    issue_type: &'static str,
    profile: Option<(&'static str, u32)>,
}

impl WorkflowSpecKeyOwned {
    fn workflow(&self) -> WorkflowSpecKey {
        WorkflowSpecKey {
            connector: ConnectorKey::parse("connector.jira").expect("valid connector"),
            project: ExternalProjectKey::parse(self.project).expect("valid project"),
            issue_type: ExternalIssueTypeKey::parse(self.issue_type).expect("valid issue type"),
            version: SpecVersion::FIRST,
            work_profile: self.profile.map(|(key, version)| PinnedProfile {
                key: WorkProfileKey::parse(key).expect("valid profile key"),
                version: SpecVersion::parse(version).expect("valid profile version"),
            }),
        }
    }

    fn fields(&self) -> FieldSpecKey {
        FieldSpecKey {
            connector: ConnectorKey::parse("connector.jira").expect("valid connector"),
            project: ExternalProjectKey::parse(self.project).expect("valid project"),
            issue_type: ExternalIssueTypeKey::parse(self.issue_type).expect("valid issue type"),
            version: SpecVersion::FIRST,
        }
    }
}

fn asma_field_spec() -> CompiledFieldSpec {
    SpecCatalog::bundled()
        .expect("the bundled specifications load")
        .select_field_spec(&asma_workflow_key().fields())
        .expect("the bundled field specification is selectable")
        .clone()
}

fn alternate_workflow_spec() -> ExternalWorkflowSpec {
    serde_json::from_str(ALTERNATE_WORKFLOW).expect("the alternate workflow fixture parses")
}

/// Both projects, as (workflow, field) pairs a test can loop over.
fn projects() -> Vec<(CompiledWorkflowSpec, CompiledFieldSpec)> {
    let catalog = catalog();
    [asma_workflow_key(), alternate_workflow_key()]
        .iter()
        .map(|key| {
            (
                catalog
                    .select_workflow_spec(&key.workflow())
                    .expect("the workflow specification is selectable")
                    .clone(),
                catalog
                    .select_field_spec(&key.fields())
                    .expect("the field specification is selectable")
                    .clone(),
            )
        })
        .collect()
}

/// The status a workflow uses for a milestone — read from the fixture, never
/// spelled in this file.
fn target_of(spec: &ExternalWorkflowSpec, key: &str) -> StatusSelector {
    let wanted = milestone(key);
    spec.milestones
        .iter()
        .find(|rule| rule.milestone == wanted)
        .expect("the fixture declares this milestone")
        .target
        .clone()
}

fn first_inbound(spec: &ExternalWorkflowSpec) -> StatusSelector {
    spec.inbound_compatible
        .first()
        .expect("the fixture declares an inbound status")
        .clone()
}

fn terminal_of(spec: &ExternalWorkflowSpec) -> StatusSelector {
    spec.statuses
        .iter()
        .find(|status| status.class.is_terminal())
        .expect("the fixture declares a terminal status")
        .selector
        .clone()
}

fn principal() -> TicketPrincipal {
    TicketPrincipal {
        account_id: external("acct-kontor"),
    }
}

fn facts(state: TaskState, gate: GateState, outcome: Option<TerminalOutcome>) -> InternalTaskFacts {
    InternalTaskFacts {
        task_id: TaskId::generate(),
        task_state: state,
        task_revision: AggregateRevision::INITIAL,
        workflow_revision: AggregateRevision::INITIAL,
        projection_revision: AggregateRevision::INITIAL,
        completed_phases: BTreeSet::new(),
        gate_states: vec![(GateKey::parse("qa-gate").expect("valid gate key"), gate)],
        all_required_gates_passed: outcome == Some(TerminalOutcome::Succeeded),
        run_outcome: outcome,
    }
}

/// Facts that select the implementation milestone and nothing more specific.
fn implementing() -> InternalTaskFacts {
    facts(TaskState::InProgress, GateState::NotReady, None)
}

fn projection(field_spec: &CompiledFieldSpec, fields: Vec<ProjectedField>) -> TicketSyncProjection {
    let spec = field_spec.spec();
    TicketSyncProjection {
        schema_version: WIRE_SCHEMA_VERSION,
        id: TicketProjectionId::generate(),
        link_id: TicketLinkId::generate(),
        link_revision: AggregateRevision::INITIAL,
        connector: spec.connector.clone(),
        field_spec_project: spec.project.clone(),
        field_spec_issue_type: spec.issue_type.clone(),
        field_spec_version: spec.version,
        external_issue_key: external("ASMA-1"),
        fields,
        comment_policy: CommentPolicy::InboundOnly,
        external_comment_cursor: None,
        computed_at: at("2026-08-11T10:00:00Z"),
    }
}

fn text_value(body: &str) -> FieldValue {
    FieldValue::Text {
        body: BoundedText::parse(body).expect("valid bounded text"),
    }
}

fn wire_observation(status: &StatusSelector, holder: Option<&ExternalId>) -> WireObservation {
    WireObservation {
        status_id: status.status_id.clone(),
        status_name: status.status_name.clone(),
        status_category: name("In Progress"),
        issue_type: name("User Story"),
        assignee_account_id: holder.cloned(),
        assignee_display: holder.map(|_| name("A Human")),
        update_token: Some(external("12345")),
        observation_hash: ContentHash::of(status.status_id.as_str().as_bytes()),
    }
}

fn core_observation(
    link_id: TicketLinkId,
    status: &StatusSelector,
    holder: Option<&ExternalId>,
) -> ExternalTicketObservation {
    wire_observation(status, holder)
        .to_core(link_id, wire_at("2026-08-11T10:00:00Z"))
        .expect("the wire observation converts")
}

/// One observation, assembled without going through a subprocess.
fn observed(
    link_id: TicketLinkId,
    status: &StatusSelector,
    holder: Option<&ExternalId>,
    live: Vec<LiveTransition>,
) -> Observed {
    Observed {
        response: response_for(
            JiraOperation::Observe,
            JiraOutcome::Observed,
            status,
            holder,
        ),
        observation: core_observation(link_id, status, holder),
        live_transitions: live,
        principal: principal(),
    }
}

fn response_for(
    operation: JiraOperation,
    outcome: JiraOutcome,
    status: &StatusSelector,
    holder: Option<&ExternalId>,
) -> JiraResponse {
    JiraResponse {
        schema_version: WIRE_SCHEMA_VERSION,
        operation,
        effective_operation: operation,
        issue_key: external("ASMA-1"),
        idempotency_key: idempotency(),
        intent_hash: None,
        requested_at: wire_at("2026-08-11T10:00:00Z"),
        completed_at: wire_at("2026-08-11T10:00:01Z"),
        outcome,
        observation: Some(wire_observation(status, holder)),
        principal_account_id: Some(principal().account_id),
        live_transitions: Vec::new(),
        effects: WireEffects::default(),
        confirmation: None,
        conflict: None,
        unavailable: None,
        notes: Vec::new(),
    }
}

fn idempotency() -> IdempotencyKey {
    IdempotencyKey::parse("kon-mvp-14-delegation-1").expect("valid idempotency key")
}

fn route(transition_id: &str, to: &StatusSelector) -> LiveTransition {
    LiveTransition {
        transition_id: external(transition_id),
        to: to.clone(),
    }
}

fn wire_route(transition_id: &str, to: &StatusSelector) -> WireTransition {
    WireTransition {
        transition_id: external(transition_id),
        to_status_id: to.status_id.clone(),
        to_status_name: to.status_name.clone(),
        to_status_category: Some(name("In Progress")),
    }
}

// ---------------------------------------------------------------------------
// A real executable, recording what crossed the boundary
// ---------------------------------------------------------------------------

/// A temporary executable standing in for `asma`.
///
/// A shell script rather than a mocked trait: it exercises real argv quoting,
/// real pipes, a real exit status and a real timeout, which is where the bugs
/// that matter at a process boundary actually live. A trait double would prove
/// only that this crate calls its own abstraction.
#[cfg(unix)]
struct FakeAsma {
    directory: PathBuf,
    executable: PathBuf,
    record: PathBuf,
}

#[cfg(unix)]
impl FakeAsma {
    /// A fake whose body is arbitrary shell after the argv/stdin recorder.
    fn scripted(body: &str) -> Self {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = std::env::temp_dir().join(format!(
            "kontor-asma-fake-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&directory).expect("the temporary directory is creatable");
        let executable = directory.join("asma");
        let record = directory.join("record");
        // The record path is baked in rather than passed through the environment:
        // tests share one process, and mutating process environment variables
        // from several threads is a race, not a fixture.
        let request = directory.join("request");
        let script = format!(
            "#!/bin/sh\nfor arg in \"$@\"; do printf 'arg:%s\\n' \"$arg\" >> '{record}'; done\n\
             REQUEST='{request}'\ncat > \"$REQUEST\"\n\
             printf 'stdin-begin\\n' >> '{record}'\ncat \"$REQUEST\" >> '{record}'\n\
             printf '\\nstdin-end\\n' >> '{record}'\n{body}\n",
            record = record.display(),
            request = request.display(),
        );
        let temporary = directory.join("asma.tmp");
        std::fs::write(&temporary, script).expect("the fake executable is writable");
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755))
            .expect("the fake executable is markable executable");
        std::fs::rename(temporary, &executable).expect("the fake executable is installable");
        Self {
            directory,
            executable,
            record,
        }
    }

    /// A fake that answers with one document and exits zero.
    fn answering<T: serde::Serialize>(response: &T) -> Self {
        let json = serde_json::to_string(response).expect("the response serializes");
        Self::scripted(&heredoc(&json))
    }

    /// A fake that answers a dry run and an apply differently.
    ///
    /// It branches on the request it was handed, not on argv: both operations
    /// cross the boundary as the same public command, which is the point.
    fn answering_each<T: serde::Serialize>(dry_run: &T, apply: &T) -> Self {
        let planned = serde_json::to_string(dry_run).expect("serializes");
        let applied = serde_json::to_string(apply).expect("serializes");
        Self::scripted(&format!(
            "if grep -q '\"operation\":\"apply\"' \"$REQUEST\"; then\n{}\nelse\n{}\nfi",
            heredoc(&applied),
            heredoc(&planned),
        ))
    }

    fn resolved(&self) -> AsmaExecutable {
        AsmaExecutable::with_budgets(&self.executable, Duration::from_secs(10), 1 << 20)
            .expect("the fake resolves")
    }

    fn resolved_with(&self, timeout: Duration, max_stdout_bytes: usize) -> AsmaExecutable {
        AsmaExecutable::with_budgets(&self.executable, timeout, max_stdout_bytes)
            .expect("the fake resolves")
    }

    fn transcript(&self) -> String {
        std::fs::read_to_string(&self.record).unwrap_or_default()
    }

    /// Every argument, one entry per real argv slot.
    fn argv(&self) -> Vec<String> {
        self.transcript()
            .lines()
            .filter_map(|line| line.strip_prefix("arg:").map(str::to_owned))
            .collect()
    }

    /// Everything written to stdin, verbatim.
    fn stdin(&self) -> String {
        let transcript = self.transcript();
        let Some(start) = transcript.find("stdin-begin\n") else {
            return String::new();
        };
        let body = &transcript[start + "stdin-begin\n".len()..];
        body.split("\nstdin-end").next().unwrap_or("").to_owned()
    }
}

#[cfg(unix)]
impl Drop for FakeAsma {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Emit a document from a shell script without any interpolation.
#[cfg(unix)]
fn heredoc(json: &str) -> String {
    format!("cat <<'KONTOR_FAKE_EOF'\n{json}\nKONTOR_FAKE_EOF")
}

#[cfg(unix)]
fn delegate<'a>(
    asma: &'a AsmaExecutable,
    workflow: &'a CompiledWorkflowSpec,
    field: &'a CompiledFieldSpec,
    projection: &'a TicketSyncProjection,
    facts: &'a InternalTaskFacts,
    link_id: TicketLinkId,
    key: &'a IdempotencyKey,
) -> TicketDelegation<'a> {
    TicketDelegation {
        asma,
        field_spec: field,
        workflow_spec: workflow,
        projection,
        facts,
        link_id,
        idempotency_key: key,
    }
}

// ---------------------------------------------------------------------------
// AC-1 — project-configured workflows
// ---------------------------------------------------------------------------

#[test]
fn identical_specification_bytes_reload_to_an_identical_hash() {
    // Canonicalization is what lets a receipt cite a specification by digest. If
    // reloading the same file produced different bytes, every receipt would cite
    // a hash nobody could reproduce.
    for _ in 0..2 {
        let first = catalog();
        let second = catalog();
        for key in [asma_workflow_key(), alternate_workflow_key()] {
            let one = first
                .select_workflow_spec(&key.workflow())
                .expect("selectable");
            let two = second
                .select_workflow_spec(&key.workflow())
                .expect("selectable");
            assert_eq!(one.document().json(), two.document().json());
            assert_eq!(one.hash(), two.hash());
        }
    }
}

#[test]
fn the_two_projects_share_no_external_status() {
    let asma = catalog();
    let ids = |key: &WorkflowSpecKeyOwned| -> BTreeSet<String> {
        asma.select_workflow_spec(&key.workflow())
            .expect("selectable")
            .spec()
            .statuses
            .iter()
            .map(|status| status.selector.status_id.as_str().to_owned())
            .collect()
    };
    let first = ids(&asma_workflow_key());
    let second = ids(&alternate_workflow_key());
    assert!(!first.is_empty());
    assert!(
        first.is_disjoint(&second),
        "the fixtures must not share a status id, or the matrix proves nothing"
    );
}

#[test]
fn the_same_internal_facts_target_each_project_own_status() {
    // One fact set, two vocabularies. The decision shape must be identical and
    // the external target must be each project's own.
    let mut targets = Vec::new();
    let mut shapes = Vec::new();
    for (workflow, field) in projects() {
        let spec = workflow.spec();
        let target = target_of(spec, IMPLEMENTATION_ACTIVE);
        let current = first_inbound(spec);
        let link_id = TicketLinkId::generate();
        let projection = projection(&field, Vec::new());
        let facts = implementing();
        let key = idempotency();
        let asma = AsmaExecutable::with_budgets(
            std::env::current_exe().expect("the test binary has a path"),
            Duration::from_secs(1),
            1 << 10,
        )
        .expect("any real file resolves; this delegation never spawns it");
        let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
        let seen = observed(
            link_id,
            &current,
            Some(&principal().account_id),
            vec![route("t-1", &target)],
        );
        match delegation.plan(&seen) {
            ReconciliationOutcome::Transition(plan) => {
                targets.push(plan.target.clone());
                shapes.push(format!(
                    "transition:prerequisite={}:has_transition={}:has_assignment={}",
                    plan.assignment_prerequisite,
                    plan.transition.is_some(),
                    plan.assignment.is_some()
                ));
            }
            other => panic!("expected a plan for every project, got {other:?}"),
        }
    }
    assert_eq!(shapes[0], shapes[1], "the decision shape must not vary");
    assert_ne!(
        targets[0], targets[1],
        "each project must converge to its own external status"
    );
}

#[test]
fn selection_refuses_to_guess() {
    let catalog = catalog();

    // Nothing configured for this project at all.
    let mut unknown = asma_workflow_key().workflow();
    unknown.project = ExternalProjectKey::parse("not-configured").expect("valid project");
    assert!(matches!(
        catalog.select_workflow_spec(&unknown),
        Err(AsmaError::Selection {
            conflict: SelectionConflict::NoMatch,
            ..
        })
    ));

    // A revision nobody loaded. "Use the newest" is not an option.
    let mut stale = asma_workflow_key().workflow();
    stale.version = SpecVersion::parse(2).expect("valid version");
    assert!(matches!(
        catalog.select_workflow_spec(&stale),
        Err(AsmaError::Selection {
            conflict: SelectionConflict::NoMatch,
            ..
        })
    ));

    // The project is configured, but the pinned profile revision moved on.
    let mut wrong_revision = asma_workflow_key().workflow();
    wrong_revision.work_profile = Some(PinnedProfile {
        key: WorkProfileKey::parse("code").expect("valid profile"),
        version: SpecVersion::parse(9).expect("valid version"),
    });
    assert!(
        matches!(
            catalog.select_workflow_spec(&wrong_revision),
            Err(AsmaError::Selection {
                conflict: SelectionConflict::ProfileRevisionMismatch,
                ..
            })
        ),
        "a stale pin must not read as an unconfigured project"
    );

    // A profile-specific specification is not eligible for an unpinned item.
    let mut unpinned = asma_workflow_key().workflow();
    unpinned.work_profile = None;
    assert!(matches!(
        catalog.select_workflow_spec(&unpinned),
        Err(AsmaError::Selection {
            conflict: SelectionConflict::ProfileRevisionMismatch | SelectionConflict::NoMatch,
            ..
        })
    ));

    // Two identical revisions is an ambiguous catalogue, never a coin toss.
    let mut duplicated = catalog.clone();
    duplicated
        .load_workflow_spec(ALTERNATE_WORKFLOW)
        .expect("loads");
    assert!(matches!(
        duplicated.select_workflow_spec(&alternate_workflow_key().workflow()),
        Err(AsmaError::Selection {
            conflict: SelectionConflict::Ambiguous,
            ..
        })
    ));
}

#[test]
fn no_shipped_source_file_names_a_fixture_status() {
    // Status ids and names are data. A quoted occurrence in shipped source is
    // what a comparison or a match arm looks like; prose spells them in
    // backticks.
    let mut vocabulary: BTreeSet<String> = BTreeSet::new();
    for (workflow, _) in projects() {
        for status in &workflow.spec().statuses {
            vocabulary.insert(status.selector.status_id.as_str().to_owned());
            vocabulary.insert(status.selector.status_name.as_str().to_owned());
        }
    }
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let core_source = crate_root
        .parent()
        .expect("the crate lives in the workspace")
        .join("kontor-core")
        .join("src");

    let mut offenders = Vec::new();
    for directory in [crate_root.join("src"), core_source] {
        for entry in std::fs::read_dir(&directory).expect("the source directory is readable") {
            let path = entry.expect("a readable directory entry").path();
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("the source file is readable");
            for spelling in &vocabulary {
                if text.contains(&format!("\"{spelling}\"")) {
                    offenders.push(format!("{} mentions \"{spelling}\"", path.display()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "shipped source must not name project status data: {offenders:?}"
    );
}

// ---------------------------------------------------------------------------
// AC-2 — no model choice, no hardcoded transition
// ---------------------------------------------------------------------------

#[test]
fn the_transition_is_selected_by_destination_and_never_by_position() {
    // The id is varied deliberately, including a run where the *first* offered
    // route is a decoy: "take the first transition" and "reuse the id we saw
    // last time" both die here.
    for (workflow, field) in projects() {
        let spec = workflow.spec();
        let target = target_of(spec, QA_ACTIVE);
        let current = first_inbound(spec);
        let hold = spec.hold.clone().expect("the fixture declares a hold");
        for wanted in ["t-1", "zz-9", "0", "transition-omega"] {
            let link_id = TicketLinkId::generate();
            let projection = projection(&field, Vec::new());
            let facts = facts(TaskState::InProgress, GateState::Active, None);
            let key = idempotency();
            let asma = unspawned();
            let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
            let seen = observed(
                link_id,
                &current,
                Some(&principal().account_id),
                vec![route("decoy-first", &hold), route(wanted, &target)],
            );
            match delegation.plan(&seen) {
                ReconciliationOutcome::Transition(plan) => assert_eq!(
                    plan.transition,
                    Some(SelectedTransition {
                        transition_id: external(wanted),
                        to: target.clone(),
                    }),
                    "the route must be chosen by destination, whatever its id"
                ),
                other => panic!("expected a transition plan, got {other:?}"),
            }
        }
    }
}

#[test]
fn zero_and_several_matching_routes_are_conflicts_never_a_fallback() {
    for (workflow, field) in projects() {
        let spec = workflow.spec();
        let target = target_of(spec, QA_ACTIVE);
        let current = first_inbound(spec);
        let hold = spec.hold.clone().expect("the fixture declares a hold");
        let link_id = TicketLinkId::generate();
        let projection = projection(&field, Vec::new());
        let facts = facts(TaskState::InProgress, GateState::Active, None);
        let key = idempotency();
        let asma = unspawned();
        let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);

        // Only a decoy is offered: nothing leads to the target.
        let nothing = observed(
            link_id,
            &current,
            Some(&principal().account_id),
            vec![route("decoy", &hold)],
        );
        assert_eq!(
            delegation.plan(&nothing),
            ReconciliationOutcome::Conflict(StatusConflictKind::NoLiveTransition),
        );

        // Two routes lead there: the workflow is ambiguous, not "pick one".
        let ambiguous = observed(
            link_id,
            &current,
            Some(&principal().account_id),
            vec![route("a", &target), route("b", &target)],
        );
        assert_eq!(
            delegation.plan(&ambiguous),
            ReconciliationOutcome::Conflict(StatusConflictKind::MultipleLiveTransitions),
        );
    }
}

#[test]
fn the_internal_qa_ready_fact_never_targets_the_external_qa_status() {
    // `qa_ready` is Kontor's own distinction. Letting it move the ticket into
    // the external QA status would tell every human that review had started.
    for (workflow, field) in projects() {
        let spec = workflow.spec();
        let qa_active_target = target_of(spec, QA_ACTIVE);
        let qa_ready_target = target_of(spec, QA_READY);
        assert_ne!(
            qa_ready_target, qa_active_target,
            "qa_ready must not resolve to the active QA status"
        );
        assert_eq!(
            qa_ready_target,
            target_of(spec, IMPLEMENTATION_ACTIVE),
            "qa_ready keeps the ticket where active work sits"
        );

        // A ticket already there, with QA merely ready, plans nothing at all.
        let link_id = TicketLinkId::generate();
        let projection = projection(&field, Vec::new());
        let facts = facts(TaskState::InProgress, GateState::Ready, None);
        let key = idempotency();
        let asma = unspawned();
        let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
        let seen = observed(
            link_id,
            &qa_ready_target,
            Some(&principal().account_id),
            vec![route("t-qa", &qa_active_target)],
        );
        assert_eq!(delegation.plan(&seen), ReconciliationOutcome::NoOp);
    }
}

/// An executable that resolves but is never spawned, for pure-planning tests.
fn unspawned() -> AsmaExecutable {
    AsmaExecutable::with_budgets(
        std::env::current_exe().expect("the test binary has a path"),
        Duration::from_secs(1),
        1 << 10,
    )
    .expect("any real file resolves")
}

// ---------------------------------------------------------------------------
// AC-3 — ownership ordering and terminal preserve
// ---------------------------------------------------------------------------

#[test]
fn an_unassigned_ticket_converges_the_assignee_before_the_status() {
    for (workflow, field) in projects() {
        let spec = workflow.spec();
        let target = target_of(spec, IMPLEMENTATION_ACTIVE);
        let link_id = TicketLinkId::generate();
        let projection = projection(&field, Vec::new());
        let facts = implementing();
        let key = idempotency();
        let asma = unspawned();
        let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
        let seen = observed(
            link_id,
            &first_inbound(spec),
            None,
            vec![route("t-dev", &target)],
        );
        match delegation.plan(&seen) {
            ReconciliationOutcome::Transition(plan) => {
                assert!(plan.assignment_prerequisite);
                assert!(
                    plan.transition.is_none(),
                    "the status must wait for a confirmed assignee"
                );
                let assignment = plan.assignment.expect("an assignment is planned");
                assert_eq!(assignment.action, OwnershipAction::ReassignToPrincipal);
                assert_eq!(assignment.assign_to, Some(principal().account_id));
            }
            other => panic!("expected an assignment-first plan, got {other:?}"),
        }
    }
}

#[test]
fn an_issue_already_at_the_target_but_unassigned_converges_the_assignee_only() {
    for (workflow, field) in projects() {
        let spec = workflow.spec();
        let target = target_of(spec, IMPLEMENTATION_ACTIVE);
        let link_id = TicketLinkId::generate();
        let projection = projection(&field, Vec::new());
        let facts = implementing();
        let key = idempotency();
        let asma = unspawned();
        let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
        // The status move already happened; only the owner is missing.
        let seen = observed(link_id, &target, None, vec![route("t-dev", &target)]);
        match delegation.plan(&seen) {
            ReconciliationOutcome::Transition(plan) => {
                assert!(plan.transition.is_none(), "a landed move is never retried");
                assert!(plan.assignment.is_some());
            }
            other => panic!("expected assignee-only convergence, got {other:?}"),
        }
    }
}

#[test]
fn an_existing_different_owner_is_preserved_and_the_status_still_moves() {
    for (workflow, field) in projects() {
        let spec = workflow.spec();
        assert_eq!(spec.ownership.terminal_action, OwnershipAction::Preserve);
        let target = target_of(spec, IMPLEMENTATION_ACTIVE);
        let link_id = TicketLinkId::generate();
        let projection = projection(&field, Vec::new());
        let facts = implementing();
        let key = idempotency();
        let asma = unspawned();
        let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
        let stranger = external("acct-a-human-being");
        let seen = observed(
            link_id,
            &first_inbound(spec),
            Some(&stranger),
            vec![route("t-dev", &target)],
        );
        match delegation.plan(&seen) {
            ReconciliationOutcome::Transition(plan) => {
                assert!(
                    plan.assignment.is_none(),
                    "the existing owner must never be written over"
                );
                assert!(plan.transition.is_some());
            }
            other => panic!("expected a transition under the external owner, got {other:?}"),
        }
    }
}

#[test]
fn a_terminal_ticket_plans_nothing_whoever_holds_it() {
    for (workflow, field) in projects() {
        let spec = workflow.spec();
        let terminal = terminal_of(spec);
        for holder in [
            None,
            Some(principal().account_id),
            Some(external("acct-a-human-being")),
        ] {
            let link_id = TicketLinkId::generate();
            let projection = projection(&field, Vec::new());
            let facts = facts(
                TaskState::Done,
                GateState::Passed,
                Some(TerminalOutcome::Succeeded),
            );
            let key = idempotency();
            let asma = unspawned();
            let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
            let seen = observed(link_id, &terminal, holder.as_ref(), Vec::new());
            assert_eq!(
                delegation.plan(&seen),
                ReconciliationOutcome::NoOp,
                "a preserved terminal holder is never written, cleared or reported"
            );
        }
    }
}

#[tokio::test]
#[cfg(unix)]
async fn a_plan_the_boundary_cannot_perform_never_reaches_it() {
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let target = target_of(spec, IMPLEMENTATION_ACTIVE);
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = implementing();
    let key = idempotency();
    let fake = FakeAsma::answering(&response_for(
        JiraOperation::DryRun,
        JiraOutcome::Planned,
        &target,
        None,
    ));
    let asma = fake.resolved();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
    let seen = observed(
        link_id,
        &first_inbound(spec),
        None,
        vec![route("t", &target)],
    );

    // A plan that says "preserve" while carrying an assignee value contradicts
    // itself. It must never reach the boundary at all.
    let contradictory = TransitionPlan {
        milestone: milestone(IMPLEMENTATION_ACTIVE),
        target: target.clone(),
        transition: None,
        assignment: Some(AssignmentPlan {
            assign_to: Some(principal().account_id),
            action: OwnershipAction::Preserve,
        }),
        assignment_prerequisite: false,
    };
    assert!(matches!(
        delegation.dry_run(&seen, &contradictory).await,
        Err(AsmaError::Refused { .. })
    ));

    // An assignee that is not the principal's own account id, likewise.
    let stolen = TransitionPlan {
        assignment: Some(AssignmentPlan {
            assign_to: Some(external("acct-somebody-else")),
            action: OwnershipAction::ReassignToPrincipal,
        }),
        ..contradictory.clone()
    };
    assert!(matches!(
        delegation.dry_run(&seen, &stolen).await,
        Err(AsmaError::Refused { .. })
    ));

    // A clear is a domain action the `asma` boundary has no write for. Refusing
    // it here keeps the failure a policy answer; sending it and reading the
    // rejection back would arrive as a transport error instead.
    let cleared = TransitionPlan {
        assignment: Some(AssignmentPlan {
            assign_to: None,
            action: OwnershipAction::Unassign,
        }),
        ..contradictory
    };
    assert!(matches!(
        delegation.dry_run(&seen, &cleared).await,
        Err(AsmaError::Refused { .. })
    ));

    assert_eq!(
        fake.argv(),
        Vec::<String>::new(),
        "a plan the boundary cannot perform must not reach it"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn a_transition_the_observation_did_not_offer_is_refused() {
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let target = target_of(spec, IMPLEMENTATION_ACTIVE);
    let hold = spec.hold.clone().expect("the fixture declares a hold");
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = implementing();
    let key = idempotency();
    let fake = FakeAsma::answering(&response_for(
        JiraOperation::DryRun,
        JiraOutcome::Planned,
        &target,
        None,
    ));
    let asma = fake.resolved();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);

    let plan = TransitionPlan {
        milestone: milestone(IMPLEMENTATION_ACTIVE),
        target: target.clone(),
        transition: Some(SelectedTransition {
            transition_id: external("remembered-from-last-week"),
            to: target.clone(),
        }),
        assignment: None,
        assignment_prerequisite: false,
    };

    // The observation offers nothing at all: a remembered id is not evidence.
    let empty = observed(
        link_id,
        &first_inbound(spec),
        Some(&principal().account_id),
        Vec::new(),
    );
    assert!(matches!(
        delegation.dry_run(&empty, &plan).await,
        Err(AsmaError::Refused { .. })
    ));

    // The id is offered, but the workflow was rewired and it now leads elsewhere.
    let rewired = observed(
        link_id,
        &first_inbound(spec),
        Some(&principal().account_id),
        vec![route("remembered-from-last-week", &hold)],
    );
    assert!(matches!(
        delegation.dry_run(&rewired, &plan).await,
        Err(AsmaError::Refused { .. })
    ));
    assert!(
        fake.argv().is_empty(),
        "a route this observation cannot vouch for must not reach the boundary"
    );
}

// ---------------------------------------------------------------------------
// AC-4 — fields, absent values, and the option-id source of truth
// ---------------------------------------------------------------------------

#[test]
fn an_absent_projection_field_is_omitted_and_never_nulled() {
    let field = asma_field_spec();
    let written = projection(
        &field,
        vec![
            ProjectedField {
                key: TicketFieldKey::Summary,
                value: Some(text_value("A real title")),
            },
            ProjectedField {
                key: TicketFieldKey::Description,
                value: None,
            },
        ],
    );
    let writes = compile_field_writes(&written, &field).expect("the projection validates");
    assert_eq!(writes.len(), 1, "only the present field is written");

    let encoded = serde_json::to_string(&writes).expect("the writes serialize");
    assert!(
        !encoded.contains("null"),
        "an absent field must have no wire representation at all: {encoded}"
    );
    let description_id = field
        .spec()
        .mapping(TicketFieldKey::Description)
        .and_then(|mapping| mapping.external.as_ref())
        .map(|external| external.field_id.clone())
        .expect("the specification maps the description");
    assert!(
        !encoded.contains(description_id.as_str()),
        "an absent field's id must not appear either"
    );
}

#[test]
fn a_select_field_is_written_by_option_id_not_by_label() {
    let field = asma_field_spec();
    let mapping = field
        .spec()
        .mapping(TicketFieldKey::Product)
        .expect("the specification maps the product field");
    let mapped = mapping
        .external
        .as_ref()
        .expect("the product field has an external mapping");
    let option = mapped
        .options
        .first()
        .expect("the product field declares options")
        .clone();

    let written = projection(
        &field,
        vec![ProjectedField {
            key: TicketFieldKey::Product,
            value: Some(FieldValue::Select {
                option: option.id.clone(),
            }),
        }],
    );
    let writes = compile_field_writes(&written, &field).expect("a declared option validates");
    let encoded = serde_json::to_string(&writes).expect("serializes");
    assert!(encoded.contains(option.id.as_str()));
    assert!(
        !encoded.contains(option.name.as_str()),
        "a display label is never the value on the wire: {encoded}"
    );

    // An option the specification does not declare is refused before the wire.
    let invented = projection(
        &field,
        vec![ProjectedField {
            key: TicketFieldKey::Product,
            value: Some(FieldValue::Select {
                option: external("an-invented-option"),
            }),
        }],
    );
    assert!(matches!(
        compile_field_writes(&invented, &field),
        Err(AsmaError::Domain(_))
    ));
}

#[test]
fn a_field_kontor_does_not_own_never_reaches_the_wire() {
    // The canary stands in for anything Kontor holds but must not push. A field
    // the specification hands to the external system is readable, not writable.
    const CANARY: &str = "CANARY-INTERNAL-ONLY-4f1c";
    let mut spec = asma_field_spec().spec().clone();
    for mapping in &mut spec.mappings {
        if mapping.key == TicketFieldKey::Summary {
            mapping.owner = FieldOwner::Jira;
            mapping.direction = Some(kontor_core::ticket::FieldDirection::Bidirectional);
        }
    }
    let mut catalog = SpecCatalog::empty();
    catalog
        .load_field_spec(&serde_json::to_string(&spec).expect("serializes"))
        .expect("the re-owned specification loads");
    let field = catalog
        .select_field_spec(&asma_workflow_key().fields())
        .expect("selectable")
        .clone();

    let written = projection(
        &field,
        vec![ProjectedField {
            key: TicketFieldKey::Summary,
            value: Some(text_value(CANARY)),
        }],
    );
    let writes = compile_field_writes(&written, &field).expect("a bidirectional read validates");
    let encoded = serde_json::to_string(&writes).expect("serializes");
    assert!(
        !encoded.contains(CANARY),
        "a field Kontor does not own must not be pushed: {encoded}"
    );
}

#[test]
fn the_fixture_and_the_cli_agree_on_every_option_id() {
    // The CLI's field module is the verified source of truth for option ids. The
    // check is skipped rather than faked when the sibling module is not checked
    // out, so a standalone build of this repository still passes honestly.
    let Some(python) = cli_field_source() else {
        eprintln!("skipped: the CLI field module is not present in this checkout");
        return;
    };
    let field = asma_field_spec();
    let mut compared = 0_usize;
    for mapping in &field.spec().mappings {
        let Some(external) = mapping.external.as_ref() else {
            continue;
        };
        if external.options.is_empty() {
            continue;
        }
        // Both halves of every pair must appear as literals in the CLI's table.
        // The pairing itself is not re-parsed here — the CLI names one of its ids
        // through a named constant — but an id or a label drifting on either side
        // still fails, which is the drift this check exists to catch.
        for option in &external.options {
            assert!(
                python.contains(&format!("\"{}\"", option.id.as_str())),
                "the CLI does not record option id {}",
                option.id
            );
            assert!(
                python.contains(&format!("\"{}\"", option.name.as_str().to_lowercase())),
                "the CLI does not record option label {}",
                option.name
            );
            compared += 1;
        }
        assert!(
            python.contains(external.field_id.as_str()),
            "the CLI does not record field id {}",
            external.field_id
        );
    }
    assert!(
        compared >= 20,
        "the parity check covered only {compared} ids"
    );
}

/// The CLI's verified field module, when this checkout contains it.
fn cli_field_source() -> Option<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)?
        .join("asma-cli")
        .join("src")
        .join("asma_cli")
        .join("jira_ticket_fields.py");
    std::fs::read_to_string(path).ok()
}

// ---------------------------------------------------------------------------
// AC-5 — one writer, typed unavailability, idempotent receipts
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn observe_response(status: &StatusSelector, routes: Vec<WireTransition>) -> JiraResponse {
    JiraResponse {
        live_transitions: routes,
        ..response_for(
            JiraOperation::Observe,
            JiraOutcome::Observed,
            status,
            Some(&principal().account_id),
        )
    }
}

#[tokio::test]
#[cfg(unix)]
async fn an_observation_crosses_the_boundary_as_argv_and_one_json_document() {
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let current = first_inbound(spec);
    let target = target_of(spec, IMPLEMENTATION_ACTIVE);
    let fake = FakeAsma::answering(&observe_response(
        &current,
        vec![wire_route("t-dev", &target)],
    ));
    let asma = fake.resolved();
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = implementing();
    let key = idempotency();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);

    let seen = delegation.observe().await.expect("the observation parses");
    assert_eq!(seen.observation.status, current);
    assert_eq!(seen.principal, principal());
    assert_eq!(seen.live_transitions, vec![route("t-dev", &target)]);

    assert_eq!(
        fake.argv(),
        vec!["jira", "sync", "--request-json", "-"],
        "the boundary is one public command, not a shell line"
    );
    let request: JiraRequest =
        serde_json::from_str(fake.stdin().trim()).expect("stdin carried one request document");
    assert_eq!(request.operation, JiraOperation::Observe);
    assert!(!request.authorized_apply, "a read never claims authority");
    assert!(request.field_writes.is_empty());
}

#[tokio::test]
#[cfg(unix)]
async fn argv_never_carries_a_url_a_state_path_or_an_interpreted_metacharacter() {
    // The one-writer rule, asserted on what actually crossed: no endpoint, no
    // fleet directory, and nothing a shell could reinterpret. The vehicle is
    // the jira delegation because it is the only boundary this crate has left
    // — capacity no longer crosses one at all.
    let (workflow, _) = projects().remove(0);
    let spec = workflow.spec();
    let current = first_inbound(spec);
    let fake = FakeAsma::answering(&observe_response(&current, Vec::new()));
    let asma = fake.resolved();
    probe_boundary(&asma).await.expect("the observation parses");

    let argv = fake.argv();
    assert!(
        !Path::new("/tmp/kontor-should-not-exist").exists(),
        "argv must never be interpreted by a shell"
    );
    for argument in &argv {
        let lowered = argument.to_lowercase();
        for forbidden in [
            "http://",
            "https://",
            "atlassian",
            "/rest/api",
            ".asma/fleet",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "argv must not carry {forbidden}: {argument}"
            );
        }
    }
}

#[tokio::test]
#[cfg(unix)]
async fn every_boundary_failure_becomes_a_typed_unavailable_result() {
    let cases: Vec<(&str, UnavailableReason, Duration, usize)> = vec![
        (
            "sleep 5",
            UnavailableReason::Timeout,
            Duration::from_millis(150),
            1 << 20,
        ),
        (
            "printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'",
            UnavailableReason::OversizedOutput,
            Duration::from_secs(5),
            8,
        ),
        (
            "printf 'boom' >&2\nexit 3",
            UnavailableReason::ExitStatus,
            Duration::from_secs(5),
            1 << 20,
        ),
        (
            "printf 'not json at all'",
            UnavailableReason::MalformedResponse,
            Duration::from_secs(5),
            1 << 20,
        ),
    ];
    for (body, expected, timeout, max_stdout_bytes) in cases {
        let fake = FakeAsma::scripted(body);
        let asma = fake.resolved_with(timeout, max_stdout_bytes);
        let error = probe_boundary(&asma).await.expect_err("the case must fail");
        match error {
            AsmaError::Unavailable { reason, .. } => assert_eq!(reason, expected, "for {body:?}"),
            other => panic!("expected an unavailable result for {body:?}, got {other:?}"),
        }
    }
}

#[tokio::test]
#[cfg(unix)]
async fn a_schema_this_build_does_not_speak_is_refused() {
    // Deserialization of a future schema fails at the version field itself,
    // which is the earliest possible refusal: no field of an unknown generation
    // is ever interpreted.
    let fake = FakeAsma::scripted(&heredoc(
        "{\"schema_version\": 99, \"operation\": \"jira.observe\", \
         \"observed_at\": \"2026-08-11T10:00:00Z\", \"records\": [], \"errors\": []}",
    ));
    let asma = fake.resolved();
    assert!(matches!(
        probe_boundary(&asma).await,
        Err(AsmaError::Unavailable {
            reason: UnavailableReason::MalformedResponse | UnavailableReason::SchemaMismatch,
            ..
        })
    ));
}

#[tokio::test]
#[cfg(unix)]
async fn a_credential_in_a_child_diagnostic_never_reaches_the_error() {
    const SENTINEL: &str = "ghp_0123456789abcdefghijklmnopqrstuvwx";
    let fake = FakeAsma::scripted(&format!("printf '%s' '{SENTINEL}' >&2\nexit 1"));
    let asma = fake.resolved();
    let error = probe_boundary(&asma)
        .await
        .expect_err("a non-zero exit fails");
    let rendered = format!("{error:?} {error}");
    assert!(
        !rendered.contains(SENTINEL),
        "a credential in the child's diagnostic must not be carried: {rendered}"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn an_apply_that_reports_no_refetch_is_not_believed() {
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let target = target_of(spec, QA_ACTIVE);
    let current = first_inbound(spec);
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = facts(TaskState::InProgress, GateState::Active, None);
    let key = idempotency();

    let mut applied = response_for(
        JiraOperation::Apply,
        JiraOutcome::Applied,
        &target,
        Some(&principal().account_id),
    );
    // An acknowledgement without a refetch is an assumption.
    applied.confirmation = None;
    let fake = FakeAsma::answering(&applied);
    let asma = fake.resolved();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
    let seen = observed(
        link_id,
        &current,
        Some(&principal().account_id),
        vec![route("t-qa", &target)],
    );
    let plan = expect_plan(&delegation, &seen);
    let error = delegation
        .apply(&seen, &plan, authority())
        .await
        .expect_err("an unconfirmed apply must not be accepted");
    assert!(matches!(
        error,
        AsmaError::Unavailable {
            reason: UnavailableReason::MalformedResponse,
            ..
        }
    ));
}

fn authority() -> ApplyAuthority {
    ApplyAuthority {
        authorized_by: CommandReceiptId::generate(),
    }
}

fn expect_plan(delegation: &TicketDelegation<'_>, seen: &Observed) -> TransitionPlan {
    match delegation.plan(seen) {
        ReconciliationOutcome::Transition(plan) => *plan,
        other => panic!("expected a transition plan, got {other:?}"),
    }
}

#[tokio::test]
#[cfg(unix)]
async fn a_confirmed_apply_produces_a_receipt_citing_the_refetch() {
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let target = target_of(spec, QA_ACTIVE);
    let current = first_inbound(spec);
    let link_id = TicketLinkId::generate();
    let projection = projection(
        &field,
        vec![ProjectedField {
            key: TicketFieldKey::Summary,
            value: Some(text_value("Converged title")),
        }],
    );
    let facts = facts(TaskState::InProgress, GateState::Active, None);
    let key = idempotency();

    let mut applied = response_for(
        JiraOperation::Apply,
        JiraOutcome::Applied,
        &target,
        Some(&principal().account_id),
    );
    applied.confirmation = Some(WireConfirmation {
        observation: wire_observation(&target, Some(&principal().account_id)),
        confirmed_at: wire_at("2026-08-11T10:00:02Z"),
    });
    applied.effects = WireEffects {
        field_ids: vec![external("summary")],
        assignment: None,
        transition: Some(kontor_integrations_asma::jira::RequestedTransition {
            transition_id: external("t-qa"),
            to_status_id: target.status_id.clone(),
        }),
    };
    let planned = response_for(
        JiraOperation::DryRun,
        JiraOutcome::Planned,
        &current,
        Some(&principal().account_id),
    );
    let fake = FakeAsma::answering_each(&planned, &applied);
    let asma = fake.resolved();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
    let seen = observed(
        link_id,
        &current,
        Some(&principal().account_id),
        vec![route("t-qa", &target)],
    );
    let plan = expect_plan(&delegation, &seen);

    // The dry run must send byte-identical effects to the apply, or the review
    // was of a different request.
    let dry = delegation.dry_run(&seen, &plan).await;
    assert!(dry.is_ok(), "the dry run must validate: {dry:?}");
    let dry_request = fake.stdin();

    let response = delegation
        .apply(&seen, &plan, authority())
        .await
        .expect("the confirmed apply is accepted");
    let receipt = delegation
        .receipt(&seen, &plan, &response)
        .expect("the receipt validates");
    assert!(receipt.confirmed_at.is_some());
    assert!(receipt.refetched_observation_id.is_some());
    assert_eq!(receipt.prior_observation_id, seen.observation.id);
    assert_eq!(receipt.spec_version, workflow.spec().version);

    let requests: Vec<JiraRequest> = fake
        .stdin()
        .split("stdin-end")
        .filter_map(|chunk| serde_json::from_str(chunk.trim()).ok())
        .collect();
    assert!(!requests.is_empty(), "at least one request was recorded");
    assert!(
        dry_request.contains("\"dry_run\""),
        "the reviewed request must say what it is"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn a_lost_acknowledgement_is_reconciled_and_never_replayed() {
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let target = target_of(spec, QA_ACTIVE);
    let current = first_inbound(spec);
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = facts(TaskState::InProgress, GateState::Active, None);
    let key = idempotency();
    let holder = principal().account_id;

    // Case 1 — the refetch finds the target: the effect landed, so no retry.
    let arrived = FakeAsma::answering(&observe_response(&target, Vec::new()));
    let asma = arrived.resolved();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
    let before = observed(
        link_id,
        &current,
        Some(&holder),
        vec![route("t-qa", &target)],
    );
    let plan = expect_plan(&delegation, &before);
    assert!(
        matches!(
            delegation
                .reconcile_after_ambiguity(&before, &plan)
                .await
                .expect("the refetch answers"),
            AmbiguityVerdict::AlreadyConfirmed(_)
        ),
        "an effect already in place must suppress the retry"
    );

    // Case 2 — the refetch finds the original state: nothing happened, so one
    // retry under the same idempotency key is permitted.
    let unchanged = FakeAsma::answering(&observe_response(&current, Vec::new()));
    let asma = unchanged.resolved();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
    assert!(
        matches!(
            delegation
                .reconcile_after_ambiguity(&before, &plan)
                .await
                .expect("the refetch answers"),
            AmbiguityVerdict::NoEffect(_)
        ),
        "proven absence of effect is the only thing that permits a retry"
    );

    // Case 3 — the ticket is somewhere else entirely: a human decides.
    let hold = spec.hold.clone().expect("the fixture declares a hold");
    let elsewhere = FakeAsma::answering(&observe_response(&hold, Vec::new()));
    let asma = elsewhere.resolved();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
    assert!(
        matches!(
            delegation
                .reconcile_after_ambiguity(&before, &plan)
                .await
                .expect("the refetch answers"),
            AmbiguityVerdict::Contradictory(_)
        ),
        "contested state is a conflict, not permission to replay"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn an_assignment_that_landed_without_the_transition_is_contradictory() {
    // The precise partial state a naive retry would replay a transition over.
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let target = target_of(spec, IMPLEMENTATION_ACTIVE);
    let current = first_inbound(spec);
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = implementing();
    let key = idempotency();

    let before = observed(link_id, &current, None, vec![route("t-dev", &target)]);
    let plan = expect_plan(
        &delegate(
            &unspawned(),
            &workflow,
            &field,
            &projection,
            &facts,
            link_id,
            &key,
        ),
        &before,
    );
    assert!(plan.assignment_prerequisite);

    // The assignee write landed; the status never moved.
    let partial = FakeAsma::answering(&observe_response(&current, Vec::new()));
    let asma = partial.resolved();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
    let verdict = delegation
        .reconcile_after_ambiguity(&before, &plan)
        .await
        .expect("the refetch answers");
    assert!(
        matches!(verdict, AmbiguityVerdict::AlreadyConfirmed(_)),
        "an assignee-only plan is complete once the assignee is in place, got {verdict:?}"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn a_reported_conflict_becomes_a_typed_conflict() {
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let current = first_inbound(spec);
    let mut conflicted = response_for(
        JiraOperation::Observe,
        JiraOutcome::Conflict,
        &current,
        None,
    );
    conflicted.conflict = Some(WireFailure {
        reason: StatusConflictKind::IncompatibleHumanMove
            .as_str()
            .to_owned(),
        detail: "a human parked it".to_owned(),
    });
    let fake = FakeAsma::answering(&conflicted);
    let asma = fake.resolved();
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = implementing();
    let key = idempotency();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
    assert!(matches!(
        delegation.observe().await,
        Err(AsmaError::Conflict {
            kind: StatusConflictKind::IncompatibleHumanMove,
            ..
        })
    ));
}

#[tokio::test]
#[cfg(unix)]
async fn an_observation_without_a_principal_is_never_guessed_at() {
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let current = first_inbound(spec);
    let mut anonymous = observe_response(&current, Vec::new());
    anonymous.principal_account_id = None;
    let fake = FakeAsma::answering(&anonymous);
    let asma = fake.resolved();
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = implementing();
    let key = idempotency();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);
    assert!(
        matches!(
            delegation.observe().await,
            Err(AsmaError::Conflict {
                kind: StatusConflictKind::OwnershipUnresolved,
                ..
            })
        ),
        "without a principal, 'is the holder me?' has no answer"
    );
}

#[test]
fn the_intent_digest_ignores_ids_timestamps_and_attempts() {
    // Two attempts against the same logical situation must hash identically, or
    // replay detection can never recognize a retry as the same command.
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let target = target_of(spec, QA_ACTIVE);
    let current = first_inbound(spec);
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = facts(TaskState::InProgress, GateState::Active, None);
    let key = idempotency();
    let asma = unspawned();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);

    let first = observed(
        link_id,
        &current,
        Some(&principal().account_id),
        vec![route("t-qa", &target)],
    );
    let plan = expect_plan(&delegation, &first);
    let mut second = first.clone();
    // A different observation identity and a later moment: evidence, not intent.
    second.observation.id = TicketObservationId::generate();
    second.observation.observed_at = at("2026-08-12T23:59:59Z");

    let one = delegation.intent(&first, &plan).expect("canonicalizes");
    let two = delegation.intent(&second, &plan).expect("canonicalizes");
    assert_eq!(one.hash(), two.hash());
    assert_eq!(one.json(), two.json());

    // A different destination is a different command, and must hash differently.
    let moved = TransitionPlan {
        target: spec.hold.clone().expect("the fixture declares a hold"),
        ..plan
    };
    let different = delegation.intent(&first, &moved).expect("canonicalizes");
    assert_ne!(one.hash(), different.hash());
}

#[test]
fn this_crate_has_no_ticket_client_and_no_state_writer() {
    // The one-writer rule, asserted against the manifest rather than intent: an
    // HTTP client, a database handle or a directory resolver appearing here would
    // mean a second path to the world exists.
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("the manifest is readable");
    for forbidden in [
        "reqwest",
        "rusqlite",
        "kontor-store",
        "directories",
        "fs4",
        "keyring",
        "url",
        "hyper",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "this crate must not depend on {forbidden}"
        );
    }
    // And no source file may open a path or a socket of its own.
    for entry in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .expect("the source directory is readable")
    {
        let path = entry.expect("a readable entry").path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("readable");
        for forbidden in ["std::fs::", "TcpStream", "http://", "https://"] {
            assert!(
                !text.contains(forbidden),
                "{} must not use {forbidden}",
                path.display()
            );
        }
    }
}

#[test]
fn the_bundled_specifications_are_the_seed_this_build_ships() {
    let catalog = SpecCatalog::bundled().expect("the bundled data loads");
    let workflow = catalog
        .select_workflow_spec(&asma_workflow_key().workflow())
        .expect("the bundled workflow is selectable");
    let spec = workflow.spec();

    // The ownership contract the seed exists to state.
    assert_eq!(
        spec.ownership_milestone,
        milestone(IMPLEMENTATION_ACTIVE),
        "ownership is taken at the implementation milestone"
    );
    assert_eq!(
        spec.ownership.mismatch,
        kontor_core::ticket::OwnershipMismatchBehavior::AcceptExternal
    );
    assert_eq!(spec.ownership.terminal_action, OwnershipAction::Preserve);
    assert_eq!(
        spec.ownership.identity_source,
        kontor_core::ticket::AssigneeIdentitySource::ExternalAccountId
    );

    // Every semantic fact the plan names has a rule, and the terminal one is
    // classified terminal rather than merely named so.
    for key in [
        IMPLEMENTATION_ACTIVE,
        QA_READY,
        QA_ACTIVE,
        TERMINAL_DONE,
        "terminal_hold",
    ] {
        let target = target_of(spec, key);
        assert!(
            spec.class_of(&target.status_id).is_some(),
            "{key} targets a declared status"
        );
    }
    let done = target_of(spec, TERMINAL_DONE);
    assert!(
        spec.class_of(&done.status_id)
            .expect("declared")
            .is_terminal()
    );
    let hold = target_of(spec, "terminal_hold");
    assert_eq!(
        spec.class_of(&hold.status_id),
        Some(kontor_core::ticket::SemanticStatusClass::Hold)
    );
    assert_eq!(
        spec.class_of(&external("10237")),
        Some(kontor_core::ticket::SemanticStatusClass::Active),
        "the live ASMA Jira workflow's DRAFT state is known even when no Kontor milestone applies"
    );

    // The seed contains no transition id: routes are discovered, never declared.
    let json = workflow.document().json();
    assert!(!json.contains("transition"), "the seed declares no route");
}

#[test]
fn the_live_draft_status_can_converge_to_closed() {
    let catalog = SpecCatalog::bundled().expect("the bundled data loads");
    let workflow = catalog
        .select_workflow_spec(&asma_workflow_key().workflow())
        .expect("the bundled workflow is selectable");
    let field = asma_field_spec();
    let spec = workflow.spec();
    let draft = spec
        .statuses
        .iter()
        .find(|status| status.selector.status_id == external("10237"))
        .expect("the live DRAFT status is declared")
        .selector
        .clone();
    assert!(
        spec.inbound_compatible.contains(&draft),
        "a known live starting status must be safe to converge"
    );

    let closed = target_of(spec, TERMINAL_DONE);
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = facts(
        TaskState::Done,
        GateState::Passed,
        Some(TerminalOutcome::Succeeded),
    );
    let key = idempotency();
    let asma = unspawned();
    let delegation = delegate(&asma, workflow, &field, &projection, &facts, link_id, &key);
    let seen = observed(link_id, &draft, None, vec![route("close", &closed)]);

    match delegation.plan(&seen) {
        ReconciliationOutcome::Transition(plan) => assert_eq!(plan.target, closed),
        other => panic!("expected DRAFT to converge to Closed, got {other:?}"),
    }
}

/// Drive the process boundary through the one delegation this crate still has.
///
/// The properties below — timeout, oversized output, exit status, malformed
/// response, credential redaction, schema mismatch — belong to
/// [`AsmaExecutable`] and not to any one command. They used to be driven
/// through `fleet::status` because it was the cheapest call to set up; that
/// module is gone, so they are driven through the jira observe instead. The
/// vehicle changed, the properties did not.
#[cfg(unix)]
async fn probe_boundary(asma: &AsmaExecutable) -> Result<Observed, AsmaError> {
    let (workflow, field) = projects().remove(0);
    let projection = projection(&field, Vec::new());
    let facts = implementing();
    let key = idempotency();
    let link_id = TicketLinkId::generate();
    delegate(asma, &workflow, &field, &projection, &facts, link_id, &key)
        .observe()
        .await
}

/// The exact live failure ASMA-7877 hit, against the specification this build
/// ships.
///
/// A reconcile plan targeted `In Development` for a ticket standing in `DRAFT`.
/// The assignment prerequisite applied cleanly, and the follow-up then had
/// nowhere to go: this Jira workflow offers no direct `DRAFT -> In Development`
/// route. It offers `DRAFT -> READY FOR DEVELOPMENT`, which is the status the
/// shipped specification already declares as its reopen selector.
///
/// Forcing the move or reporting convergence would both be lies. The plan hops
/// to the declared intermediate, keeps naming `In Development` as the milestone,
/// and the next observation finishes. The ids here are the ones the live run
/// reported, read from the shipped specification rather than retyped.
#[test]
fn a_draft_ticket_reaches_in_development_through_ready_for_development() {
    let workflow = SpecCatalog::bundled()
        .expect("the bundled specifications load")
        .select_workflow_spec(&asma_workflow_key().workflow())
        .expect("the shipped workflow specification is selectable")
        .clone();
    let spec = workflow.spec();

    let draft = spec
        .inbound_compatible
        .iter()
        .find(|status| status.status_name.as_str() == "DRAFT")
        .cloned()
        .expect("the shipped specification accepts DRAFT as a starting point");
    let ready = spec
        .reopen
        .clone()
        .expect("the shipped specification declares a reopen selector");
    let in_development = spec
        .milestones
        .iter()
        .find(|rule| rule.milestone == milestone("implementation_active"))
        .expect("the shipped specification declares implementation_active")
        .target
        .clone();

    assert_eq!(draft.status_id.as_str(), "10237", "the live DRAFT id");
    assert_eq!(
        ready.status_id.as_str(),
        "10213",
        "the live READY FOR DEVELOPMENT id"
    );
    assert_eq!(
        in_development.status_id.as_str(),
        "10214",
        "the live In Development id"
    );

    let principal = TicketPrincipal {
        account_id: external("acct-igor"),
    };
    let facts = implementing();
    // What Jira actually offered from DRAFT: transition 15, to 10213. Nothing
    // reaches 10214 in one move.
    let offered = vec![route("15", &ready)];

    let outcome = reconcile(&ReconciliationInput {
        spec,
        observation: &core_observation(
            TicketLinkId::generate(),
            &draft,
            Some(&principal.account_id),
        ),
        freshness: kontor_core::state::Freshness::Fresh,
        facts: &facts,
        live_transitions: &offered,
        principal: &principal,
    });

    let ReconciliationOutcome::Transition(plan) = outcome else {
        panic!("DRAFT has a declared route onward; this is a plan: {outcome:?}");
    };
    assert_eq!(plan.target.status_id, in_development.status_id);
    assert_eq!(plan.destination().status_id, ready.status_id);
    assert!(plan.is_staged_hop());
    assert_eq!(
        plan.transition
            .as_ref()
            .expect("the hop invokes a transition")
            .transition_id,
        external("15"),
        "the hop uses transition 15, the one this observation offered"
    );
}

/// A staged hop's request names the status it is actually going to.
///
/// `destination` and `transition` travel together in one document, so a request
/// that named the milestone while carrying a route to the intermediate would be
/// internally inconsistent — it tells the connector to reach `10214` and hands it
/// the transition that lands on `10213`. That inconsistency is how a hop becomes
/// the false-success receipt this checkpoint exists to prevent, and it is
/// asserted on the bytes that actually cross the boundary.
#[tokio::test]
#[cfg(unix)]
async fn a_staged_hop_request_declares_the_hop_not_the_milestone() {
    let (workflow, field) = projects().remove(0);
    let spec = workflow.spec();
    let target = target_of(spec, IMPLEMENTATION_ACTIVE);
    let hop = spec
        .reopen
        .clone()
        .expect("the fixture declares a reopen selector");
    let standing = spec
        .inbound_compatible
        .iter()
        .find(|status| status.status_id != hop.status_id && status.status_id != target.status_id)
        .cloned()
        .expect("the fixture declares a third inbound status");

    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = implementing();
    let key = idempotency();
    let fake = FakeAsma::answering(&response_for(
        JiraOperation::DryRun,
        JiraOutcome::Planned,
        &hop,
        None,
    ));
    let asma = fake.resolved();
    let delegation = delegate(&asma, &workflow, &field, &projection, &facts, link_id, &key);

    // Exactly what `reconcile` produces for a target two moves away: the plan
    // still names the milestone, the attempt goes to the declared intermediate.
    let plan = TransitionPlan {
        milestone: milestone(IMPLEMENTATION_ACTIVE),
        target: target.clone(),
        transition: Some(SelectedTransition {
            transition_id: external("15"),
            to: hop.clone(),
        }),
        assignment: None,
        assignment_prerequisite: false,
    };
    assert!(plan.is_staged_hop(), "the fixture plan is a hop");

    let seen = observed(
        link_id,
        &standing,
        Some(&principal().account_id),
        vec![route("15", &hop)],
    );
    delegation
        .dry_run(&seen, &plan)
        .await
        .expect("the offered route is planned");

    let sent: serde_json::Value =
        serde_json::from_str(&fake.stdin()).expect("the recorded request is one JSON document");
    assert_eq!(
        sent["destination"]["status_id"],
        serde_json::Value::from(hop.status_id.as_str()),
        "the request declares the hop it is performing"
    );
    assert_eq!(
        sent["transition"]["to_status_id"],
        serde_json::Value::from(hop.status_id.as_str()),
        "the route and the declared destination agree"
    );
    assert_ne!(
        sent["destination"]["status_id"],
        serde_json::Value::from(target.status_id.as_str()),
        "a hop never claims the milestone it has not reached"
    );
}
