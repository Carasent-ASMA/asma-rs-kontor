//! Native Jira transport and configuration contract.

use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use kontor_accounts::{KeychainBackend, KeychainFailure, KeychainTarget};
use kontor_core::id::{
    AggregateRevision, CommandReceiptId, ConnectorKey, ContentHash, ExternalId,
    ExternalIssueTypeKey, ExternalName, ExternalProjectKey, IdempotencyKey, ProjectId,
    SCHEMA_VERSION, SemanticMilestoneKey, SpecVersion, WorkProfileKey, parse_utc_timestamp,
};
use kontor_core::ticket::{
    EpicCompletionEvidence, InternalPredicate, LiveTransition, OwnershipAction, SelectedTransition,
    StatusSelector, TransitionPlan,
};
use kontor_jira::jira::{
    ApplyAuthority, FieldSpecKey, IssueAmbiguityVerdict, JiraExchange, JiraIssueDelegation,
    JiraOperation, JiraOutcome, JiraRequest, JiraResponse, PinnedProfile, RequestedTransition,
    SpecCatalog, WireConfirmation, WireEffects, WireObservation, WireTransition, WorkflowSpecKey,
};
use kontor_jira::{
    JiraConnectors, JiraError, JiraIssueKind, JiraIssuePlan, SelectionConflict, UnavailableReason,
    WireTimestamp,
};
use secrecy::SecretString;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

#[derive(Default)]
struct FixtureKeychain {
    reads: AtomicUsize,
}

impl KeychainBackend for FixtureKeychain {
    fn secret(&self, target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
        assert_eq!(target.service(), "kontor-jira");
        assert_eq!(target.account(), "work");
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(SecretString::from(
            r#"{"email":"operator@example.test","api_token":"secret"}"#.to_owned(),
        ))
    }
}

struct MarkerSearch(Arc<AtomicUsize>);

impl Respond for MarkerSearch {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let issues = if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            serde_json::json!([])
        } else {
            serde_json::json!([{"key": "ASMA-1"}])
        };
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"issues": issues}))
    }
}

struct ExistingLinkedTask(Arc<AtomicUsize>);

impl Respond for ExistingLinkedTask {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let call = self.0.fetch_add(1, Ordering::SeqCst);
        let parent = if call < 4 { "ASMA-8049" } else { "ASMA-9999" };
        let labels = if call >= 2 {
            serde_json::json!(["kontor-task-link-fixture"])
        } else {
            serde_json::json!([])
        };
        let (summary, description) = if call >= 3 {
            (
                "Kontor-derived recovery summary",
                "Kontor-derived recovery description",
            )
        } else {
            (
                "Existing operator-owned Jira summary",
                "Existing Jira description",
            )
        };
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "key": "ASMA-8050",
            "fields": {
                "project": {"key": "ASMA"},
                "issuetype": {"name": "Task", "hierarchyLevel": 0},
                "parent": {"key": parent},
                "summary": summary,
                "description": {"type":"doc","version":1,"content":[{
                    "type":"paragraph","content":[{"type":"text","text":description}]
                }]},
                "labels": labels
            }
        }))
    }
}

struct RecordedExchange {
    requests: Mutex<Vec<JiraRequest>>,
    responses: Mutex<VecDeque<JiraResponse>>,
}

#[async_trait::async_trait]
impl JiraExchange for RecordedExchange {
    async fn execute(
        &self,
        _operation: &'static str,
        request: &JiraRequest,
    ) -> Result<JiraResponse, JiraError> {
        self.requests
            .lock()
            .expect("request ledger")
            .push(request.clone());
        Ok(self
            .responses
            .lock()
            .expect("response script")
            .pop_front()
            .expect("one scripted response per request"))
    }
}

fn wire_observation(status_id: &str, status_name: &str, token: &str) -> WireObservation {
    WireObservation {
        status_id: ExternalId::parse(status_id).expect("status id"),
        status_name: ExternalName::parse(status_name).expect("status name"),
        status_category: ExternalName::parse("In Progress").expect("status category"),
        issue_type: ExternalName::parse("Epic").expect("issue type"),
        assignee_account_id: None,
        assignee_display: None,
        update_token: Some(ExternalId::parse(token).expect("update token")),
        observation_hash: ContentHash::of(format!("{status_id}:{token}").as_bytes()),
    }
}

fn wire_response(
    operation: JiraOperation,
    outcome: JiraOutcome,
    observation: WireObservation,
    confirmation: Option<WireObservation>,
) -> JiraResponse {
    JiraResponse {
        schema_version: SCHEMA_VERSION,
        operation,
        effective_operation: operation,
        issue_key: ExternalId::parse("ASMA-9").expect("issue key"),
        idempotency_key: IdempotencyKey::parse("epic-sync-1").expect("idempotency key"),
        intent_hash: None,
        requested_at: WireTimestamp::new(
            parse_utc_timestamp("2026-09-03T10:00:00Z").expect("request time"),
        ),
        completed_at: WireTimestamp::new(
            parse_utc_timestamp("2026-09-03T10:00:01Z").expect("completion time"),
        ),
        outcome,
        observation: Some(observation),
        principal_account_id: Some(ExternalId::parse("acct-1").expect("principal")),
        live_transitions: vec![WireTransition {
            transition_id: ExternalId::parse("31").expect("transition id"),
            to_status_id: ExternalId::parse("10214").expect("destination id"),
            to_status_name: ExternalName::parse("In Development").expect("destination name"),
            to_status_category: Some(
                ExternalName::parse("In Progress").expect("destination category"),
            ),
        }],
        effects: WireEffects {
            field_ids: Vec::new(),
            assignment: None,
            transition: matches!(operation, JiraOperation::DryRun | JiraOperation::Apply).then(
                || RequestedTransition {
                    transition_id: ExternalId::parse("31").expect("transition id"),
                    to_status_id: ExternalId::parse("10214").expect("destination id"),
                },
            ),
        },
        confirmation: confirmation.map(|observation| WireConfirmation {
            observation,
            confirmed_at: WireTimestamp::new(
                parse_utc_timestamp("2026-09-03T10:00:02Z").expect("confirmation time"),
            ),
        }),
        conflict: None,
        unavailable: None,
        notes: Vec::new(),
    }
}

fn collect_gate_keys(predicate: &InternalPredicate, keys: &mut BTreeSet<String>) {
    match predicate {
        InternalPredicate::GateStateIs { gate, .. } => {
            keys.insert(gate.as_str().to_owned());
        }
        InternalPredicate::All { of } | InternalPredicate::Any { of } => {
            for nested in of {
                collect_gate_keys(nested, keys);
            }
        }
        _ => {}
    }
}

#[test]
fn bundled_high_stakes_workflow_is_pinned_to_its_actual_gates() {
    let catalog = SpecCatalog::bundled().expect("the bundled catalogue is valid");
    let workflow = catalog
        .select_workflow_spec(&WorkflowSpecKey {
            connector: ConnectorKey::parse("connector.jira").expect("connector"),
            project: ExternalProjectKey::parse("asma").expect("project"),
            issue_type: ExternalIssueTypeKey::parse("task").expect("issue type"),
            version: SpecVersion::parse(2).expect("workflow revision"),
            work_profile: Some(PinnedProfile {
                key: WorkProfileKey::parse("asma-high-stakes-primary-20260829")
                    .expect("work profile"),
                version: SpecVersion::parse(1).expect("profile revision"),
            }),
        })
        .expect("the exact high-stakes mapping is selectable");

    let mut gates = BTreeSet::new();
    for rule in &workflow.spec().milestones {
        collect_gate_keys(&rule.predicate, &mut gates);
    }
    assert_eq!(
        gates,
        BTreeSet::from([
            "high-audit-gate".to_owned(),
            "high-verification-gate".to_owned(),
        ])
    );
}

#[test]
fn bundled_task_workflows_never_cross_profile_pins() {
    let catalog = SpecCatalog::bundled().expect("the bundled catalogue is valid");
    let task = ExternalIssueTypeKey::parse("task").expect("issue type");
    let connector = ConnectorKey::parse("connector.jira").expect("connector");
    let project = ExternalProjectKey::parse("asma").expect("project");

    let selected = catalog
        .select_workflow_spec(&WorkflowSpecKey {
            connector: connector.clone(),
            project: project.clone(),
            issue_type: task.clone(),
            version: SpecVersion::parse(1).expect("workflow revision"),
            work_profile: Some(PinnedProfile {
                key: WorkProfileKey::parse("code").expect("work profile"),
                version: SpecVersion::parse(1).expect("profile revision"),
            }),
        })
        .expect("code@1 has its exact task mapping");
    assert_eq!(
        selected
            .spec()
            .work_profile
            .as_ref()
            .map(WorkProfileKey::as_str),
        Some("code")
    );

    let wrong_profile = catalog
        .select_workflow_spec(&WorkflowSpecKey {
            connector,
            project,
            issue_type: task,
            version: SpecVersion::parse(2).expect("workflow revision"),
            work_profile: Some(PinnedProfile {
                key: WorkProfileKey::parse("code").expect("work profile"),
                version: SpecVersion::parse(1).expect("profile revision"),
            }),
        })
        .expect_err("code@1 never borrows the high-stakes mapping");
    assert!(matches!(
        wrong_profile,
        JiraError::Selection {
            conflict: SelectionConflict::ProfileRevisionMismatch,
            ..
        }
    ));
}

#[test]
fn bundled_epic_specs_are_generic_and_entity_specific() {
    let catalog = SpecCatalog::bundled().expect("the bundled catalogue is valid");
    let connector = ConnectorKey::parse("connector.jira").expect("connector");
    let project = ExternalProjectKey::parse("asma").expect("project");
    let epic = ExternalIssueTypeKey::parse("epic").expect("issue type");
    let field_version = SpecVersion::parse(1).expect("field spec revision");

    let field = catalog
        .select_field_spec(&FieldSpecKey {
            connector: connector.clone(),
            project: project.clone(),
            issue_type: epic.clone(),
            version: field_version,
        })
        .expect("the epic field mapping is selectable");
    assert_eq!(field.spec().issue_type.as_str(), "epic");

    let workflow = catalog
        .select_workflow_spec(&WorkflowSpecKey {
            connector,
            project,
            issue_type: epic,
            version: SpecVersion::parse(2).expect("workflow spec revision"),
            work_profile: None,
        })
        .expect("the generic epic workflow is selectable");
    assert!(workflow.spec().work_profile.is_none());
    assert!(workflow.spec().work_profile_version.is_none());
    assert_eq!(
        workflow.spec().ownership_milestone.as_str(),
        "terminal_done"
    );
    assert_eq!(workflow.document().schema_version(), SCHEMA_VERSION);
    assert_eq!(
        workflow.spec().version,
        SpecVersion::parse(2).expect("workflow spec revision")
    );
    assert_eq!(
        workflow.hash(),
        &ContentHash::parse("21b1a100d832d688fbf99c4140f63aac8c8f7d9980aa1e7174288a3c2cf0c40e")
            .expect("pinned epic v2 canonical hash")
    );

    let terminal = workflow
        .spec()
        .milestones
        .iter()
        .find(|rule| rule.milestone.as_str() == "terminal_done")
        .expect("terminal milestone");
    assert!(matches!(
        &terminal.predicate,
        InternalPredicate::All { of }
            if of.iter().any(|predicate| matches!(
                predicate,
                InternalPredicate::EpicCompletionIs {
                    state: EpicCompletionEvidence::Done
                }
            )) && of.iter().any(|predicate| matches!(
                predicate,
                InternalPredicate::AllChildTasksTerminal
            ))
    ));
    let hold = workflow
        .spec()
        .milestones
        .iter()
        .find(|rule| rule.milestone.as_str() == "terminal_hold")
        .expect("hold milestone");
    assert!(matches!(
        hold.predicate,
        InternalPredicate::EpicCompletionIs {
            state: EpicCompletionEvidence::NeedsHuman
        }
    ));
    let active = workflow
        .spec()
        .milestones
        .iter()
        .find(|rule| rule.milestone.as_str() == "epic_active")
        .expect("active milestone");
    let route: Vec<(&str, &str)> = active
        .route
        .iter()
        .map(|step| (step.from.status_id.as_str(), step.to.status_id.as_str()))
        .collect();
    assert_eq!(
        route,
        vec![
            ("10227", "10237"),
            ("10237", "10236"),
            ("10236", "10233"),
            ("10233", "10213"),
            ("10213", "10214"),
        ],
        "the bundled route is the verified ASMA Epic workflow, in order"
    );
    assert!(workflow.spec().milestones.iter().all(|rule| {
        !matches!(
            rule.predicate,
            InternalPredicate::TaskStateIs { .. }
                | InternalPredicate::PhaseCompleted { .. }
                | InternalPredicate::GateStateIs { .. }
                | InternalPredicate::AllRequiredGatesPassed
                | InternalPredicate::RunTerminal { .. }
        )
    }));
}

#[test]
fn an_installed_v1_epic_workflow_remains_selectable_and_hash_stable() {
    let source = include_str!("../fixtures/external-workflow-asma-epic.json");
    let source_value: serde_json::Value = serde_json::from_str(source).expect("v1 fixture JSON");
    let expected = kontor_core::id::CanonicalDocument::from_value(&source_value)
        .expect("v1 canonical document");
    let mut catalog = SpecCatalog::empty();
    catalog
        .load_workflow_spec(source)
        .expect("an installed v1 workflow remains compatible");
    let selected = catalog
        .select_workflow_spec(&WorkflowSpecKey {
            connector: ConnectorKey::parse("connector.jira").expect("connector"),
            project: ExternalProjectKey::parse("asma").expect("project"),
            issue_type: ExternalIssueTypeKey::parse("epic").expect("issue type"),
            version: SpecVersion::parse(1).expect("v1 workflow revision"),
            work_profile: None,
        })
        .expect("installed v1 workflow is selectable");
    assert_eq!(selected.document(), &expected);
    assert!(
        selected
            .spec()
            .milestones
            .iter()
            .all(|rule| rule.route.is_empty())
    );
}

#[tokio::test]
async fn entity_neutral_delegation_binds_apply_to_observation_route_and_readback() {
    let catalog = SpecCatalog::bundled().expect("the bundled catalogue is valid");
    let connector = ConnectorKey::parse("connector.jira").expect("connector");
    let project = ExternalProjectKey::parse("asma").expect("project");
    let epic = ExternalIssueTypeKey::parse("epic").expect("issue type");
    let field_spec = catalog
        .select_field_spec(&FieldSpecKey {
            connector: connector.clone(),
            project: project.clone(),
            issue_type: epic.clone(),
            version: SpecVersion::parse(1).expect("field spec revision"),
        })
        .expect("epic field specification");
    let workflow_spec = catalog
        .select_workflow_spec(&WorkflowSpecKey {
            connector,
            project,
            issue_type: epic,
            version: SpecVersion::parse(2).expect("workflow spec revision"),
            work_profile: None,
        })
        .expect("epic workflow specification");

    let before = wire_observation("10237", "DRAFT", "v1");
    let after = wire_observation("10214", "In Development", "v2");
    let exchange = RecordedExchange {
        requests: Mutex::new(Vec::new()),
        responses: Mutex::new(VecDeque::from([
            wire_response(
                JiraOperation::Observe,
                JiraOutcome::Observed,
                before.clone(),
                None,
            ),
            wire_response(
                JiraOperation::DryRun,
                JiraOutcome::Planned,
                before.clone(),
                None,
            ),
            wire_response(
                JiraOperation::Apply,
                JiraOutcome::Applied,
                before.clone(),
                Some(after.clone()),
            ),
            wire_response(
                JiraOperation::Apply,
                JiraOutcome::Applied,
                before.clone(),
                None,
            ),
        ])),
    };
    let issue_key = ExternalId::parse("ASMA-9").expect("issue key");
    let idempotency_key = IdempotencyKey::parse("epic-sync-1").expect("idempotency key");
    let delegation = JiraIssueDelegation {
        exchange: &exchange,
        field_spec,
        workflow_spec,
        issue_key: &issue_key,
        projection_revision: AggregateRevision::INITIAL,
        field_writes: &[],
        idempotency_key: &idempotency_key,
    };

    let observed = delegation.observe().await.expect("epic observation");
    assert_eq!(observed.observation, before);
    let target = StatusSelector {
        status_id: ExternalId::parse("10214").expect("target id"),
        status_name: ExternalName::parse("In Development").expect("target name"),
    };
    let plan = TransitionPlan {
        milestone: SemanticMilestoneKey::parse("epic_active").expect("milestone"),
        target: target.clone(),
        transition: Some(SelectedTransition {
            transition_id: ExternalId::parse("31").expect("transition id"),
            to: target,
        }),
        assignment: None,
        assignment_prerequisite: false,
    };

    let intermediate = StatusSelector {
        status_id: ExternalId::parse("10236").expect("intermediate id"),
        status_name: ExternalName::parse("TO BE GROOMED").expect("intermediate name"),
    };
    let intent_observation = kontor_jira::jira::ObservedIssue {
        live_transitions: vec![
            LiveTransition {
                transition_id: ExternalId::parse("31").expect("direct transition id"),
                to: plan.target.clone(),
            },
            LiveTransition {
                transition_id: ExternalId::parse("32").expect("hop transition id"),
                to: intermediate.clone(),
            },
        ],
        ..observed.clone()
    };
    let staged_plan = TransitionPlan {
        transition: Some(SelectedTransition {
            transition_id: ExternalId::parse("32").expect("hop transition id"),
            to: intermediate.clone(),
        }),
        ..plan.clone()
    };
    let direct_intent = delegation
        .intent(&intent_observation, &plan)
        .expect("direct intent");
    let staged_intent = delegation
        .intent(&intent_observation, &staged_plan)
        .expect("staged intent");
    assert_ne!(
        direct_intent.hash(),
        staged_intent.hash(),
        "authority must distinguish a direct transition from an intermediate hop"
    );
    let staged_json: serde_json::Value =
        serde_json::from_str(staged_intent.json()).expect("canonical intent JSON");
    assert_eq!(
        staged_json["destination"],
        serde_json::to_value(&intermediate).expect("selector JSON"),
        "canonical authority names this attempt's actual destination"
    );

    let renamed_route = kontor_jira::jira::ObservedIssue {
        live_transitions: vec![LiveTransition {
            transition_id: ExternalId::parse("31").expect("transition id"),
            to: StatusSelector {
                status_id: ExternalId::parse("10214").expect("destination id"),
                status_name: ExternalName::parse("Renamed destination").expect("destination name"),
            },
        }],
        ..observed.clone()
    };
    assert!(matches!(
        delegation.dry_run(&renamed_route, &plan).await,
        Err(JiraError::Refused { .. })
    ));

    delegation
        .dry_run(&observed, &plan)
        .await
        .expect("the exact apply document validates");
    let applied = delegation
        .apply(
            &observed,
            &plan,
            ApplyAuthority {
                authorized_by: CommandReceiptId::generate(),
            },
        )
        .await
        .expect("the authorized transition is confirmed");
    assert_eq!(
        applied
            .confirmation
            .as_ref()
            .expect("confirmed refetch")
            .observation,
        after
    );
    let unconfirmed = delegation
        .apply(
            &observed,
            &plan,
            ApplyAuthority {
                authorized_by: CommandReceiptId::generate(),
            },
        )
        .await
        .expect_err("an applied effect without refetch confirmation is refused");
    assert!(matches!(
        unconfirmed,
        JiraError::Unavailable {
            reason: UnavailableReason::MalformedResponse,
            ..
        }
    ));

    let renamed_after = wire_observation("10214", "Renamed destination", "v3");
    exchange
        .responses
        .lock()
        .expect("response script")
        .push_back(wire_response(
            JiraOperation::Refetch,
            JiraOutcome::Observed,
            renamed_after,
            None,
        ));
    assert!(matches!(
        delegation
            .reconcile_after_ambiguity(&observed, &plan)
            .await
            .expect("refetch is interpreted"),
        IssueAmbiguityVerdict::Contradictory(_)
    ));

    let requests = exchange.requests.lock().expect("request ledger");
    assert_eq!(requests.len(), 5);
    let dry_run = &requests[1];
    let apply = &requests[2];
    assert_eq!(
        dry_run
            .expected
            .as_ref()
            .expect("expected status")
            .status_id
            .as_str(),
        "10237"
    );
    assert_eq!(
        dry_run
            .expected
            .as_ref()
            .and_then(|expected| expected.update_token.as_ref())
            .map(ExternalId::as_str),
        Some("v1")
    );
    assert_eq!(dry_run.field_spec_hash.as_ref(), Some(field_spec.hash()));
    assert_eq!(
        dry_run.workflow_spec_hash.as_ref(),
        Some(workflow_spec.hash())
    );
    assert_eq!(
        dry_run
            .transition
            .as_ref()
            .map(|transition| transition.transition_id.as_str()),
        Some("31")
    );
    assert!(!dry_run.authorized_apply);
    assert_eq!(apply.intent_hash, dry_run.intent_hash);
    assert_eq!(apply.expected, dry_run.expected);
    assert!(apply.authorized_apply);
}

#[tokio::test]
async fn create_is_marker_idempotent_and_credentials_are_resolved_per_request() {
    let server = MockServer::start().await;
    let searches = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(MarkerSearch(Arc::clone(&searches)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/createmeta/ASMA/issuetypes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issueTypes": [{"id": "10000", "hierarchyLevel": 1}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(serde_json::json!({"key": "ASMA-1"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "key": "ASMA-1",
            "fields": {
                "project": {"key": "ASMA"},
                "issuetype": {"name": "Epic", "hierarchyLevel": 1},
                "parent": null,
                "summary": "Operational MVP",
                "description": {"type":"doc","version":1,"content":[{
                    "type":"paragraph","content":[{"type":"text","text":"Server derived"}]
                }]},
                "labels": ["kontor-epic-fixture"]
            }
        })))
        .mount(&server)
        .await;

    let root = tempfile::tempdir().expect("a state root");
    let project_id = ProjectId::generate();
    std::fs::write(
        root.path().join("jira.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "projects": [{
                "project_id": project_id.to_string(),
                "endpoint": server.uri(),
                "project_key": "ASMA",
                "credential_alias": "work"
            }]
        }))
        .expect("configuration serializes"),
    )
    .expect("configuration is written");
    let keychain = Arc::new(FixtureKeychain::default());
    let connectors = JiraConnectors::read_with_keychain(
        root.path(),
        Arc::clone(&keychain) as Arc<dyn KeychainBackend>,
    )
    .expect("strict configuration loads");
    let connector = connectors
        .for_project(project_id)
        .expect("project is configured");
    let plan = JiraIssuePlan {
        kind: JiraIssueKind::Epic,
        requested_key: None,
        marker: ExternalId::parse("kontor-epic-fixture").expect("marker"),
        require_marker: false,
        summary: "Operational MVP".to_owned(),
        description: "Server derived".to_owned(),
        parent_key: None,
    };

    let first = connector
        .materialize(&plan)
        .await
        .expect("created and confirmed");
    let replay = connector
        .materialize(&plan)
        .await
        .expect("found and confirmed");
    assert_eq!(first, replay);
    assert_eq!(searches.load(Ordering::SeqCst), 2);
    assert_eq!(keychain.reads.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn explicit_link_confirms_existing_identity_without_claiming_summary_or_description() {
    let server = MockServer::start().await;
    let readbacks = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-8050"))
        .respond_with(ExistingLinkedTask(Arc::clone(&readbacks)))
        .expect(5)
        .mount(&server)
        .await;

    let root = tempfile::tempdir().expect("a state root");
    let project_id = ProjectId::generate();
    std::fs::write(
        root.path().join("jira.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "projects": [{
                "project_id": project_id.to_string(),
                "endpoint": server.uri(),
                "project_key": "ASMA",
                "credential_alias": "work"
            }]
        }))
        .expect("configuration serializes"),
    )
    .expect("configuration is written");
    let connector =
        JiraConnectors::read_with_keychain(root.path(), Arc::new(FixtureKeychain::default()))
            .expect("configuration loads");
    let plan = JiraIssuePlan {
        kind: JiraIssueKind::Task,
        requested_key: Some(ExternalId::parse("ASMA-8050").expect("issue key")),
        marker: ExternalId::parse("kontor-task-link-fixture").expect("marker"),
        require_marker: false,
        summary: "Kontor-derived recovery summary".to_owned(),
        description: "Kontor-derived recovery description".to_owned(),
        parent_key: Some(ExternalId::parse("ASMA-8049").expect("parent key")),
    };

    let confirmed = connector
        .for_project(project_id)
        .expect("project is configured")
        .materialize(&plan)
        .await
        .expect("the exact existing Jira identity is confirmed");
    assert_eq!(confirmed.issue_key.as_str(), "ASMA-8050");
    let mut recovery_plan = plan.clone();
    recovery_plan.require_marker = true;
    assert!(
        connector
            .for_project(project_id)
            .expect("project remains configured")
            .materialize(&recovery_plan)
            .await
            .is_err(),
        "pending-create recovery refuses an existing issue without its marker"
    );
    assert!(
        connector
            .for_project(project_id)
            .expect("project remains configured")
            .materialize(&recovery_plan)
            .await
            .is_err(),
        "a marker alone does not authorize mismatched recovery content"
    );
    connector
        .for_project(project_id)
        .expect("project remains configured")
        .materialize(&recovery_plan)
        .await
        .expect("exact marker, content, kind, project and parent recover in place");
    assert!(
        connector
            .for_project(project_id)
            .expect("project remains configured")
            .materialize(&recovery_plan)
            .await
            .is_err(),
        "an explicit link still refuses a task attached to another epic"
    );
}

#[tokio::test]
async fn native_observe_reads_issue_transitions_and_principal() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "fields": {
                "project": {"key": "ASMA"},
                "status": {"id": "3", "name": "In Progress", "statusCategory": {"name": "In Progress"}},
                "issuetype": {"name": "Task"},
                "assignee": {"accountId": "acct-1", "displayName": "Operator"},
                "updated": "2026-08-23T10:00:00.000+0000"
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-9/transitions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "transitions": [{"id":"31","to":{"id":"10000","name":"Done","statusCategory":{"name":"Done"}}}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "accountId": "acct-1"
        })))
        .mount(&server)
        .await;

    let root = tempfile::tempdir().expect("a state root");
    let project_id = ProjectId::generate();
    std::fs::write(
        root.path().join("jira.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "projects": [{
                "project_id": project_id.to_string(),
                "endpoint": server.uri(),
                "project_key": "ASMA",
                "credential_alias": "work"
            }]
        }))
        .expect("configuration serializes"),
    )
    .expect("configuration is written");
    let connector =
        JiraConnectors::read_with_keychain(root.path(), Arc::new(FixtureKeychain::default()))
            .expect("configuration loads");
    let response = connector
        .for_project(project_id)
        .expect("project is configured")
        .execute(
            "observe",
            &JiraRequest {
                schema_version: SCHEMA_VERSION,
                operation: JiraOperation::Observe,
                issue_key: ExternalId::parse("ASMA-9").expect("issue key"),
                idempotency_key: IdempotencyKey::parse("native-observe").expect("key"),
                intent_hash: None,
                field_spec_hash: None,
                workflow_spec_hash: None,
                expected: None,
                field_writes: Vec::new(),
                destination: None,
                ownership_action: OwnershipAction::Preserve,
                transition: None,
                authorized_apply: false,
            },
        )
        .await
        .expect("native observation succeeds");
    assert_eq!(response.outcome, JiraOutcome::Observed);
    assert_eq!(
        response
            .observation
            .expect("observation")
            .status_id
            .as_str(),
        "3"
    );
    assert_eq!(response.live_transitions[0].transition_id.as_str(), "31");
    assert_eq!(
        response
            .principal_account_id
            .as_ref()
            .map(ExternalId::as_str),
        Some("acct-1")
    );
}

#[tokio::test]
async fn native_requests_time_out_without_holding_the_daemon_open() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-9"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(31))
                .set_body_json(serde_json::json!({"fields": {}})),
        )
        .mount(&server)
        .await;

    let root = tempfile::tempdir().expect("a state root");
    let project_id = ProjectId::generate();
    std::fs::write(
        root.path().join("jira.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "projects": [{
                "project_id": project_id.to_string(),
                "endpoint": server.uri(),
                "project_key": "ASMA",
                "credential_alias": "work"
            }]
        }))
        .expect("configuration serializes"),
    )
    .expect("configuration is written");
    let connector =
        JiraConnectors::read_with_keychain(root.path(), Arc::new(FixtureKeychain::default()))
            .expect("configuration loads");
    let request = JiraRequest {
        schema_version: SCHEMA_VERSION,
        operation: JiraOperation::Observe,
        issue_key: ExternalId::parse("ASMA-9").expect("issue key"),
        idempotency_key: IdempotencyKey::parse("timeout-observe").expect("key"),
        intent_hash: None,
        field_spec_hash: None,
        workflow_spec_hash: None,
        expected: None,
        field_writes: Vec::new(),
        destination: None,
        ownership_action: OwnershipAction::Preserve,
        transition: None,
        authorized_apply: false,
    };

    let result = tokio::time::timeout(
        Duration::from_secs(31),
        connector
            .for_project(project_id)
            .expect("project is configured")
            .execute("observe", &request),
    )
    .await
    .expect("the connector owns the request timeout");
    assert!(result.is_err());
}

struct BlockingKeychain {
    entered: std::sync::mpsc::Sender<()>,
    release: Arc<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>,
}

impl KeychainBackend for BlockingKeychain {
    fn secret(&self, _target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
        let _ = self.entered.send(());
        let _ = self.release.lock().expect("the release lock").recv();
        Ok(SecretString::from(
            r#"{"email":"operator@example.test","api_token":"secret"}"#.to_owned(),
        ))
    }
}

#[tokio::test]
async fn a_blocking_keychain_read_is_bounded_by_the_credential_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"fields": {}})))
        .mount(&server)
        .await;

    let root = tempfile::tempdir().expect("a state root");
    let project_id = ProjectId::generate();
    std::fs::write(
        root.path().join("jira.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "projects": [{
                "project_id": project_id.to_string(),
                "endpoint": server.uri(),
                "project_key": "ASMA",
                "credential_alias": "work"
            }]
        }))
        .expect("configuration serializes"),
    )
    .expect("configuration is written");

    let (entered, started) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let connector = JiraConnectors::read_with_keychain(
        root.path(),
        Arc::new(BlockingKeychain {
            entered,
            release: Arc::new(std::sync::Mutex::new(release_rx)),
        }),
    )
    .expect("configuration loads");
    let request = JiraRequest {
        schema_version: SCHEMA_VERSION,
        operation: JiraOperation::Observe,
        issue_key: ExternalId::parse("ASMA-9").expect("issue key"),
        idempotency_key: IdempotencyKey::parse("blocked-keychain-observe").expect("key"),
        intent_hash: None,
        field_spec_hash: None,
        workflow_spec_hash: None,
        expected: None,
        field_writes: Vec::new(),
        destination: None,
        ownership_action: OwnershipAction::Preserve,
        transition: None,
        authorized_apply: false,
    };

    let result = tokio::time::timeout(
        Duration::from_secs(11),
        connector
            .for_project(project_id)
            .expect("project is configured")
            .execute("observe", &request),
    )
    .await
    .expect("the connector owns the credential timeout");
    started.recv().expect("the keychain read began");
    let error = result.expect_err("a blocked keychain read must time out");
    assert!(error.to_string().contains("exceeded its bound"), "{error}");
    let _ = release_tx.send(());
}

#[test]
fn strict_configuration_rejects_inline_credentials() {
    let root = tempfile::tempdir().expect("a state root");
    std::fs::write(
        root.path().join("jira.json"),
        r#"{"schema_version":1,"projects":[{"project_id":"0198fb22-056d-7de0-a8b2-777e719c83fd","endpoint":"https://user:secret@example.test","project_key":"ASMA","credential_alias":"work"}]}"#,
    )
    .expect("configuration is written");
    let error =
        JiraConnectors::read_with_keychain(root.path(), Arc::new(FixtureKeychain::default()))
            .expect_err("inline credentials are refused");
    assert!(!error.to_string().contains("secret"));
}
