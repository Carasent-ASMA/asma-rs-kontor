//! External-ticket field, comment, workflow and ownership policy.
//!
//! Every reconciliation case below runs twice: once against a fixture whose
//! external statuses are spelled the way one deployment spells them, and once
//! against a fixture from a different project with entirely different status
//! names and ids. Identical inputs must produce identical outcomes, which is
//! what proves the evaluator has no name branch.
//!
//! The mutants this suite exists to kill:
//!
//! * hard-coding an external status name or a transition id;
//! * letting a model, an email, a display name or a team choose an assignee;
//! * clearing a self-held terminal assignee while the policy preserves it;
//! * transitioning into development before assignee confirmation, or retrying an
//!   already-applied transition instead of converging the assignee only;
//! * clearing an absent field instead of leaving it alone;
//! * adding an outbound comment, or mirroring one inbound comment twice.

use std::collections::BTreeSet;

use kontor_core::DomainError;
use kontor_core::id::{
    AggregateRevision, BoundedText, ContentHash, ExternalId, ExternalName, GateKey, IdempotencyKey,
    MiniProjectId, SemanticMilestoneKey, SpecVersion, TaskId, TicketLinkId, TicketObservationId,
    TicketProjectionId, Timestamp, parse_utc_timestamp,
};
use kontor_core::state::{Freshness, GateState, TaskState, TerminalOutcome};
use kontor_core::ticket::{
    AssigneeIdentitySource, CommentPolicy, EpicCompletionEvidence, EpicReconciliationInput,
    ExternalCommentRevision, ExternalEpicObservation, ExternalFieldMapping, ExternalFieldOption,
    ExternalFieldType, ExternalTicketObservation, ExternalWorkflowSpec, FieldDirection,
    FieldEncoding, FieldOwner, FieldValue, InternalEpicFacts, InternalPredicate, InternalTaskFacts,
    LiveTransition, OwnershipAction, OwnershipMismatchBehavior, ProjectedField,
    ReconciliationInput, ReconciliationOutcome, SelectedTransition, SemanticStatusClass,
    StatusConflictKind, StatusSelector, TicketFieldKey, TicketFieldMapping, TicketFieldSpec,
    TicketPrincipal, TicketSyncProjection, TransitionPlan, reconcile, reconcile_epic,
};

const WORKFLOW_ONE: &str = include_str!("fixtures/external_workflow_asma.json");
const WORKFLOW_TWO: &str = include_str!("fixtures/external_workflow_alternate.json");

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("fixture timestamp is canonical UTC")
}

fn external(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("valid external id")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("valid external name")
}

/// The two workflow fixtures, which differ in every external spelling.
fn workflows() -> Vec<ExternalWorkflowSpec> {
    vec![
        serde_json::from_str(WORKFLOW_ONE).expect("the first workflow fixture parses"),
        serde_json::from_str(WORKFLOW_TWO).expect("the second workflow fixture parses"),
    ]
}

/// The status a workflow uses for a milestone — read from the fixture, never
/// spelled in this file.
fn target_of(spec: &ExternalWorkflowSpec, milestone: &str) -> StatusSelector {
    let key = SemanticMilestoneKey::parse(milestone).expect("valid milestone key");
    spec.milestones
        .iter()
        .find(|rule| rule.milestone == key)
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

fn terminal_status(spec: &ExternalWorkflowSpec) -> StatusSelector {
    spec.statuses
        .iter()
        .find(|status| status.class.is_terminal())
        .expect("the fixture declares a terminal status")
        .selector
        .clone()
}

/// The status a workflow uses for one exact terminal class.
fn terminal_status_of(spec: &ExternalWorkflowSpec, class: SemanticStatusClass) -> StatusSelector {
    spec.statuses
        .iter()
        .find(|status| status.class == class)
        .expect("the fixture declares this terminal class")
        .selector
        .clone()
}

fn hold_status(spec: &ExternalWorkflowSpec) -> StatusSelector {
    spec.hold
        .clone()
        .expect("the fixture declares a hold status")
}

fn observation(
    status: &StatusSelector,
    assignee: Option<&ExternalId>,
) -> ExternalTicketObservation {
    ExternalTicketObservation {
        id: TicketObservationId::generate(),
        link_id: TicketLinkId::generate(),
        status: status.clone(),
        status_category: name("in progress"),
        issue_type: kontor_core::id::ExternalIssueTypeKey::parse("task").expect("valid issue type"),
        assignee_account_id: assignee.cloned(),
        assignee_display: None,
        external_version: None,
        observed_at: at("2026-08-09T10:00:00Z"),
        payload_hash: ContentHash::of(b"observation"),
    }
}

fn facts(
    state: TaskState,
    gates_passed: bool,
    outcome: Option<TerminalOutcome>,
) -> InternalTaskFacts {
    InternalTaskFacts {
        task_id: TaskId::generate(),
        task_state: state,
        task_revision: AggregateRevision::INITIAL,
        workflow_revision: AggregateRevision::INITIAL,
        projection_revision: AggregateRevision::INITIAL,
        completed_phases: BTreeSet::new(),
        gate_states: vec![(
            GateKey::parse("q7.attest.sign").expect("valid gate key"),
            GateState::Ready,
        )],
        all_required_gates_passed: gates_passed,
        run_outcome: outcome,
    }
}

fn principal() -> TicketPrincipal {
    TicketPrincipal {
        account_id: external("acct-kontor"),
    }
}

// ---------------------------------------------------------------------------
// Field specification and projection
// ---------------------------------------------------------------------------

fn mapping(
    key: TicketFieldKey,
    owner: FieldOwner,
    direction: Option<FieldDirection>,
) -> TicketFieldMapping {
    TicketFieldMapping {
        key,
        owner,
        direction,
        external: direction.map(|_| ExternalFieldMapping {
            field_id: external("customfield_1"),
            field_type: ExternalFieldType::Text,
            encoding: FieldEncoding::PlainText,
            options: Vec::new(),
        }),
        required: false,
    }
}

fn field_spec(mappings: Vec<TicketFieldMapping>) -> TicketFieldSpec {
    TicketFieldSpec {
        schema_version: kontor_core::id::SCHEMA_VERSION,
        connector: kontor_core::id::ConnectorKey::parse("connector.alpha").expect("valid"),
        project: kontor_core::id::ExternalProjectKey::parse("asma").expect("valid"),
        issue_type: kontor_core::id::ExternalIssueTypeKey::parse("task").expect("valid"),
        version: SpecVersion::FIRST,
        mappings,
    }
}

#[test]
fn the_field_key_set_is_closed() {
    assert_eq!(TicketFieldKey::ALL.len(), 8);
    for key in TicketFieldKey::ALL {
        assert_eq!(
            TicketFieldKey::parse(key.as_str()).expect("round-trips"),
            *key
        );
    }
    assert!(TicketFieldKey::parse("story_points").is_err());
    assert!(TicketFieldKey::parse("").is_err());
}

#[test]
fn owner_and_direction_must_be_a_permitted_pair() {
    let permitted = [
        (FieldOwner::Kontor, FieldDirection::Outbound),
        (FieldOwner::Kontor, FieldDirection::Bidirectional),
        (FieldOwner::Jira, FieldDirection::Inbound),
        (FieldOwner::Jira, FieldDirection::Bidirectional),
        (FieldOwner::MirrorOnly, FieldDirection::Outbound),
    ];
    for (owner, direction) in permitted {
        assert!(owner.allows(direction));
        mapping(TicketFieldKey::Summary, owner, Some(direction))
            .validate()
            .expect("a permitted pair validates");
    }
    let refused = [
        (FieldOwner::Kontor, FieldDirection::Inbound),
        (FieldOwner::Jira, FieldDirection::Outbound),
        (FieldOwner::MirrorOnly, FieldDirection::Inbound),
        (FieldOwner::MirrorOnly, FieldDirection::Bidirectional),
        (FieldOwner::Private, FieldDirection::Outbound),
        (FieldOwner::Private, FieldDirection::Inbound),
        (FieldOwner::Private, FieldDirection::Bidirectional),
    ];
    for (owner, direction) in refused {
        assert!(
            !owner.allows(direction),
            "{owner}/{direction} is not a pair"
        );
        assert!(
            mapping(TicketFieldKey::Summary, owner, Some(direction))
                .validate()
                .is_err()
        );
    }

    // A private field has no direction and no external mapping at all.
    mapping(TicketFieldKey::Severity, FieldOwner::Private, None)
        .validate()
        .expect("a private field validates");
}

#[test]
fn a_field_specification_is_accepted_or_refused_atomically() {
    field_spec(vec![
        mapping(
            TicketFieldKey::Summary,
            FieldOwner::Kontor,
            Some(FieldDirection::Outbound),
        ),
        mapping(
            TicketFieldKey::Product,
            FieldOwner::Jira,
            Some(FieldDirection::Inbound),
        ),
    ])
    .validate()
    .expect("a well-formed specification validates");

    assert!(
        field_spec(Vec::new()).validate().is_err(),
        "an empty specification is refused"
    );
    assert!(
        field_spec(vec![
            mapping(
                TicketFieldKey::Summary,
                FieldOwner::Kontor,
                Some(FieldDirection::Outbound)
            ),
            mapping(
                TicketFieldKey::Summary,
                FieldOwner::Jira,
                Some(FieldDirection::Inbound)
            ),
        ])
        .validate()
        .is_err(),
        "a duplicate field key is refused"
    );

    // A select field without options, and options on a non-select field.
    let mut select = mapping(
        TicketFieldKey::Severity,
        FieldOwner::Kontor,
        Some(FieldDirection::Outbound),
    );
    select.external = Some(ExternalFieldMapping {
        field_id: external("customfield_2"),
        field_type: ExternalFieldType::SingleSelect,
        encoding: FieldEncoding::PlainText,
        options: Vec::new(),
    });
    assert!(field_spec(vec![select.clone()]).validate().is_err());

    let mut duplicated = select.clone();
    duplicated.external = Some(ExternalFieldMapping {
        field_id: external("customfield_2"),
        field_type: ExternalFieldType::SingleSelect,
        encoding: FieldEncoding::PlainText,
        options: vec![
            ExternalFieldOption {
                id: external("opt-1"),
                name: name("High"),
            },
            ExternalFieldOption {
                id: external("opt-1"),
                name: name("Also high"),
            },
        ],
    });
    assert!(field_spec(vec![duplicated]).validate().is_err());
}

fn projection(fields: Vec<ProjectedField>) -> TicketSyncProjection {
    TicketSyncProjection {
        schema_version: kontor_core::id::SCHEMA_VERSION,
        id: TicketProjectionId::generate(),
        link_id: TicketLinkId::generate(),
        link_revision: AggregateRevision::INITIAL,
        connector: kontor_core::id::ConnectorKey::parse("connector.alpha").expect("valid"),
        field_spec_project: kontor_core::id::ExternalProjectKey::parse("asma").expect("valid"),
        field_spec_issue_type: kontor_core::id::ExternalIssueTypeKey::parse("task").expect("valid"),
        field_spec_version: SpecVersion::FIRST,
        external_issue_key: external("ABC-1"),
        fields,
        comment_policy: CommentPolicy::InboundOnly,
        external_comment_cursor: None,
        computed_at: at("2026-08-09T10:00:00Z"),
    }
}

#[test]
fn an_absent_field_means_no_write_never_a_clear() {
    let spec = field_spec(vec![mapping(
        TicketFieldKey::Summary,
        FieldOwner::Kontor,
        Some(FieldDirection::Outbound),
    )]);
    let absent = projection(vec![ProjectedField {
        key: TicketFieldKey::Summary,
        value: None,
    }]);
    absent
        .validate(&spec)
        .expect("an absent value is always legal: it means do not write");

    // There is no representation of "clear this field": `None` is absence, and a
    // present value is always a write.
    let written = projection(vec![ProjectedField {
        key: TicketFieldKey::Summary,
        value: Some(FieldValue::Text {
            body: BoundedText::parse("hello").expect("valid text"),
        }),
    }]);
    written.validate(&spec).expect("a present value validates");
}

#[test]
fn a_projection_may_not_contradict_its_pinned_specification() {
    let spec = field_spec(vec![
        mapping(
            TicketFieldKey::Summary,
            FieldOwner::Kontor,
            Some(FieldDirection::Outbound),
        ),
        mapping(
            TicketFieldKey::Product,
            FieldOwner::Jira,
            Some(FieldDirection::Inbound),
        ),
        mapping(TicketFieldKey::Severity, FieldOwner::Private, None),
    ]);
    let text = || FieldValue::Text {
        body: BoundedText::parse("value").expect("valid text"),
    };

    // A field the specification does not map at all.
    assert!(
        projection(vec![ProjectedField {
            key: TicketFieldKey::ReproSteps,
            value: Some(text()),
        }])
        .validate(&spec)
        .is_err()
    );
    // An inbound-owned field written outward.
    assert!(
        projection(vec![ProjectedField {
            key: TicketFieldKey::Product,
            value: Some(text()),
        }])
        .validate(&spec)
        .is_err()
    );
    // A private field written outward.
    assert!(
        projection(vec![ProjectedField {
            key: TicketFieldKey::Severity,
            value: Some(text()),
        }])
        .validate(&spec)
        .is_err()
    );
    // The same field twice.
    assert!(
        projection(vec![
            ProjectedField {
                key: TicketFieldKey::Summary,
                value: Some(text()),
            },
            ProjectedField {
                key: TicketFieldKey::Summary,
                value: None,
            },
        ])
        .validate(&spec)
        .is_err()
    );
    // A value whose type contradicts the mapping.
    assert!(
        projection(vec![ProjectedField {
            key: TicketFieldKey::Summary,
            value: Some(FieldValue::Number { value: 3 }),
        }])
        .validate(&spec)
        .is_err()
    );
}

#[test]
fn a_required_field_may_not_be_omitted() {
    let mut required = mapping(
        TicketFieldKey::Summary,
        FieldOwner::Kontor,
        Some(FieldDirection::Outbound),
    );
    required.required = true;
    let spec = field_spec(vec![required]);
    assert!(projection(Vec::new()).validate(&spec).is_err());
    assert!(
        projection(vec![ProjectedField {
            key: TicketFieldKey::Summary,
            value: None,
        }])
        .validate(&spec)
        .is_err()
    );
}

#[test]
fn a_select_value_must_be_one_the_specification_declares() {
    let mut select = mapping(
        TicketFieldKey::Severity,
        FieldOwner::Kontor,
        Some(FieldDirection::Outbound),
    );
    select.external = Some(ExternalFieldMapping {
        field_id: external("customfield_2"),
        field_type: ExternalFieldType::SingleSelect,
        encoding: FieldEncoding::PlainText,
        options: vec![ExternalFieldOption {
            id: external("opt-1"),
            name: name("High"),
        }],
    });
    let spec = field_spec(vec![select]);

    projection(vec![ProjectedField {
        key: TicketFieldKey::Severity,
        value: Some(FieldValue::Select {
            option: external("opt-1"),
        }),
    }])
    .validate(&spec)
    .expect("a declared option validates");

    assert!(
        projection(vec![ProjectedField {
            key: TicketFieldKey::Severity,
            value: Some(FieldValue::Select {
                option: external("opt-invented"),
            }),
        }])
        .validate(&spec)
        .is_err(),
        "an undeclared option must be refused"
    );
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

fn comment(link: TicketLinkId, id: &str, body: &str, observed: &str) -> ExternalCommentRevision {
    let body = BoundedText::parse(body).expect("valid body");
    ExternalCommentRevision {
        link_id: link,
        external_comment_id: external(id),
        author_account_id: external("acct-human"),
        author_display: Some(name("A Human")),
        external_created_at: at("2026-08-09T09:00:00Z"),
        external_updated_at: at("2026-08-09T09:30:00Z"),
        body_hash: ContentHash::of(body.as_str().as_bytes()),
        body,
        observed_at: at(observed),
        supersedes: None,
    }
}

#[test]
fn comments_are_inbound_only_and_identified_by_link_id_and_body() {
    // There is exactly one policy, and no outbound payload type exists to
    // construct: an outbound comment is unrepresentable, not merely disabled.
    assert_eq!(CommentPolicy::ALL.len(), 1);
    assert_eq!(CommentPolicy::ALL[0], CommentPolicy::InboundOnly);
    assert!(CommentPolicy::parse("outbound").is_err());
    assert!(CommentPolicy::parse("bidirectional").is_err());

    let link = TicketLinkId::generate();
    let first = comment(link, "c-1", "hello", "2026-08-09T10:00:00Z");
    let replay = comment(link, "c-1", "hello", "2026-08-09T11:00:00Z");
    let edited = comment(link, "c-1", "hello, corrected", "2026-08-09T11:00:00Z");

    first.verify().expect("the digest matches the body");
    assert!(
        first.is_same_revision(&replay),
        "a replay of the same body is the same revision"
    );
    assert!(
        !first.is_same_revision(&edited),
        "an edit is a new revision with its own provenance"
    );

    let other_link = comment(
        TicketLinkId::generate(),
        "c-1",
        "hello",
        "2026-08-09T10:00:00Z",
    );
    assert!(
        !first.is_same_revision(&other_link),
        "identity is scoped to the link"
    );

    // Provenance cannot be rewritten without detection.
    let tampered = ExternalCommentRevision {
        body: BoundedText::parse("something else").expect("valid body"),
        ..first
    };
    assert!(tampered.verify().is_err());
}

// ---------------------------------------------------------------------------
// Workflow specification
// ---------------------------------------------------------------------------

#[test]
fn both_workflow_fixtures_validate_and_use_different_spellings() {
    let specs = workflows();
    for spec in &specs {
        spec.validate().expect("the fixture workflow is valid");
        spec.canonicalize().expect("canonicalizes");
        assert_eq!(
            spec.ownership.identity_source,
            AssigneeIdentitySource::ExternalAccountId,
            "an assignee can only ever come from an external account id"
        );
    }
    let first: BTreeSet<&str> = specs[0]
        .statuses
        .iter()
        .map(|status| status.selector.status_id.as_str())
        .collect();
    let second: BTreeSet<&str> = specs[1]
        .statuses
        .iter()
        .map(|status| status.selector.status_id.as_str())
        .collect();
    assert!(
        first.is_disjoint(&second),
        "the two fixtures must share no external status id"
    );
}

#[test]
fn a_workflow_specification_is_checked_structurally() {
    let mut spec = workflows().remove(0);
    spec.statuses.clear();
    assert!(spec.validate().is_err());

    let mut spec = workflows().remove(0);
    spec.milestones[0].target = StatusSelector {
        status_id: external("not-declared"),
        status_name: name("Not declared"),
    };
    assert!(
        spec.validate().is_err(),
        "a milestone must target a declared status"
    );

    let mut spec = workflows().remove(0);
    spec.ownership_milestone =
        SemanticMilestoneKey::parse("milestone.absent").expect("valid milestone key");
    assert!(spec.validate().is_err());

    let mut spec = workflows().remove(0);
    spec.milestones[0].predicate = InternalPredicate::All { of: Vec::new() };
    assert!(
        spec.validate().is_err(),
        "an empty predicate group is neither true nor false"
    );
}

#[test]
fn predicates_read_kontor_state_and_nothing_else() {
    let ready = facts(TaskState::Ready, false, None);
    let running = facts(TaskState::InProgress, false, None);

    let predicate = InternalPredicate::TaskStateIs {
        state: TaskState::InProgress,
    };
    assert!(!predicate.evaluate(&ready));
    assert!(predicate.evaluate(&running));

    let gate = InternalPredicate::GateStateIs {
        gate: GateKey::parse("q7.attest.sign").expect("valid gate key"),
        state: GateState::Ready,
    };
    assert!(gate.evaluate(&running));

    let both = InternalPredicate::All {
        of: vec![predicate.clone(), gate.clone()],
    };
    assert!(both.evaluate(&running));
    assert!(!both.evaluate(&ready));

    let either = InternalPredicate::Any {
        of: vec![predicate, gate],
    };
    assert!(either.evaluate(&ready));

    let terminal = InternalPredicate::RunTerminal {
        outcome: TerminalOutcome::Succeeded,
    };
    assert!(!terminal.evaluate(&running));
    assert!(terminal.evaluate(&facts(
        TaskState::Done,
        true,
        Some(TerminalOutcome::Succeeded)
    )));
}

#[test]
fn epic_predicates_cannot_be_satisfied_by_task_facts() {
    let task = facts(TaskState::Done, true, Some(TerminalOutcome::Succeeded));
    let epic = InternalPredicate::EpicCompletionIs {
        state: EpicCompletionEvidence::Done,
    };
    let children = InternalPredicate::AllChildTasksTerminal;

    assert!(!epic.evaluate(&task));
    assert!(!children.evaluate(&task));
}

#[test]
fn an_epic_closes_only_from_completion_and_child_evidence() {
    let mut spec = workflows().remove(0);
    spec.milestones[0].predicate = InternalPredicate::All {
        of: vec![
            InternalPredicate::EpicCompletionIs {
                state: EpicCompletionEvidence::Done,
            },
            InternalPredicate::AllChildTasksTerminal,
        ],
    };
    let target = spec.milestones[0].target.clone();
    let current = first_inbound(&spec);
    let observation = ExternalEpicObservation {
        status: current,
        assignee_account_id: Some(external("acct-kontor")),
        external_version: Some(external("1")),
        observed_at: at("2026-08-09T10:00:00Z"),
        payload_hash: ContentHash::of(b"epic-observation"),
    };
    let live = [LiveTransition {
        transition_id: external("close-epic"),
        to: target,
    }];
    let base = InternalEpicFacts {
        epic_id: MiniProjectId::generate(),
        epic_revision: AggregateRevision::INITIAL,
        completion: EpicCompletionEvidence::Done,
        all_child_tasks_terminal: false,
    };
    let principal = principal();

    assert_eq!(
        reconcile_epic(&EpicReconciliationInput {
            spec: &spec,
            observation: &observation,
            freshness: Freshness::Fresh,
            facts: &base,
            live_transitions: &live,
            principal: &principal,
        }),
        ReconciliationOutcome::NoOp,
        "completion alone must not close an epic while a child remains open"
    );

    let complete = InternalEpicFacts {
        all_child_tasks_terminal: true,
        ..base
    };
    assert!(matches!(
        reconcile_epic(&EpicReconciliationInput {
            spec: &spec,
            observation: &observation,
            freshness: Freshness::Fresh,
            facts: &complete,
            live_transitions: &live,
            principal: &principal,
        }),
        ReconciliationOutcome::Transition(_)
    ));
}

// ---------------------------------------------------------------------------
// Reconciliation — run identically against both fixtures
// ---------------------------------------------------------------------------

#[test]
fn a_matching_milestone_selects_the_live_transition_by_destination() {
    for spec in workflows() {
        let target = target_of(&spec, "milestone.development-started");
        let current = first_inbound(&spec);
        let observed = observation(&current, Some(&principal().account_id));
        let transitions = vec![
            LiveTransition {
                transition_id: external("t-noise"),
                to: hold_status(&spec),
            },
            LiveTransition {
                transition_id: external("t-correct"),
                to: target.clone(),
            },
        ];
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observed,
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &transitions,
            principal: &principal(),
        });
        match outcome {
            ReconciliationOutcome::Transition(plan) => {
                assert_eq!(plan.target, target);
                assert_eq!(
                    plan.transition,
                    Some(SelectedTransition {
                        transition_id: external("t-correct"),
                        to: target,
                    }),
                    "the transition is chosen by destination, never remembered"
                );
                assert!(plan.assignment.is_none());
            }
            other => panic!("expected a transition plan, got {other:?}"),
        }
    }
}

#[test]
fn ownership_is_converged_before_the_status_moves() {
    for spec in workflows() {
        let current = first_inbound(&spec);
        // Nobody holds the ticket yet.
        let observed = observation(&current, None);
        let target = target_of(&spec, "milestone.development-started");
        let transitions = vec![LiveTransition {
            transition_id: external("t-correct"),
            to: target.clone(),
        }];
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observed,
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &transitions,
            principal: &principal(),
        });
        match outcome {
            ReconciliationOutcome::Transition(plan) => {
                assert!(
                    plan.assignment_prerequisite,
                    "assignment must precede the transition"
                );
                assert!(
                    plan.transition.is_none(),
                    "assignee-only convergence must not also dispatch the status move"
                );
                let assignment = plan.assignment.expect("an assignment is planned");
                assert_eq!(assignment.action, OwnershipAction::ReassignToPrincipal);
                assert_eq!(
                    assignment.assign_to,
                    Some(principal().account_id),
                    "the assignee can only be the authenticated principal's account id"
                );
            }
            other => panic!("expected an assignment plan, got {other:?}"),
        }
    }
}

#[test]
fn someone_else_holding_the_ticket_is_a_conflict_not_a_takeover() {
    for spec in workflows() {
        let current = first_inbound(&spec);
        let observed = observation(&current, Some(&external("acct-someone-else")));
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observed,
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &[],
            principal: &principal(),
        });
        assert_eq!(
            outcome,
            ReconciliationOutcome::Conflict(StatusConflictKind::OwnershipMismatch)
        );
    }
}

#[test]
fn an_already_converged_ticket_is_a_no_op() {
    for spec in workflows() {
        let target = target_of(&spec, "milestone.development-started");
        let observed = observation(&target, Some(&principal().account_id));
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observed,
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &[LiveTransition {
                transition_id: external("t-correct"),
                to: target,
            }],
            principal: &principal(),
        });
        assert_eq!(
            outcome,
            ReconciliationOutcome::NoOp,
            "an applied transition must never be retried"
        );
    }
}

#[test]
fn a_stale_observation_is_never_acted_on() {
    for spec in workflows() {
        let observed = observation(&first_inbound(&spec), Some(&principal().account_id));
        for freshness in [Freshness::Stale, Freshness::Unknown] {
            let outcome = reconcile(&ReconciliationInput {
                spec: &spec,
                observation: &observed,
                freshness,
                facts: &facts(TaskState::InProgress, false, None),
                live_transitions: &[],
                principal: &principal(),
            });
            assert_eq!(
                outcome,
                ReconciliationOutcome::Conflict(StatusConflictKind::StaleObservation)
            );
        }
    }
}

#[test]
fn zero_or_several_live_transitions_are_conflicts() {
    for spec in workflows() {
        let target = target_of(&spec, "milestone.development-started");
        let observed = observation(&first_inbound(&spec), Some(&principal().account_id));
        let input = |transitions: &'static [LiveTransition]| {
            reconcile(&ReconciliationInput {
                spec: &spec,
                observation: &observed,
                freshness: Freshness::Fresh,
                facts: &facts(TaskState::InProgress, false, None),
                live_transitions: transitions,
                principal: &principal(),
            })
        };
        // Nothing leads there: the workflow drifted.
        assert_eq!(
            input(&[]),
            ReconciliationOutcome::Conflict(StatusConflictKind::NoLiveTransition)
        );

        let ambiguous = vec![
            LiveTransition {
                transition_id: external("t-a"),
                to: target.clone(),
            },
            LiveTransition {
                transition_id: external("t-b"),
                to: target,
            },
        ];
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observed,
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &ambiguous,
            principal: &principal(),
        });
        assert_eq!(
            outcome,
            ReconciliationOutcome::Conflict(StatusConflictKind::MultipleLiveTransitions)
        );
    }
}

#[test]
fn an_unknown_status_or_an_incompatible_human_move_is_a_conflict() {
    for spec in workflows() {
        let unknown = StatusSelector {
            status_id: external("not-in-this-workflow"),
            status_name: name("Unknown"),
        };
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observation(&unknown, Some(&principal().account_id)),
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &[],
            principal: &principal(),
        });
        assert_eq!(
            outcome,
            ReconciliationOutcome::Conflict(StatusConflictKind::UnknownStatusClass)
        );

        // A human parked the ticket somewhere Kontor cannot start from.
        let target = target_of(&spec, "milestone.development-started");
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observation(&hold_status(&spec), Some(&principal().account_id)),
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &[LiveTransition {
                transition_id: external("t-correct"),
                to: target,
            }],
            principal: &principal(),
        });
        assert_eq!(
            outcome,
            ReconciliationOutcome::Conflict(StatusConflictKind::IncompatibleHumanMove)
        );
    }
}

#[test]
fn an_externally_closed_ticket_without_internal_evidence_is_a_conflict() {
    for spec in workflows() {
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observation(&terminal_status(&spec), Some(&principal().account_id)),
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &[],
            principal: &principal(),
        });
        assert_eq!(
            outcome,
            ReconciliationOutcome::Conflict(
                StatusConflictKind::ExternalTerminalBeforeInternalEvidence
            )
        );
    }
}

#[test]
fn preserve_never_clears_a_terminal_assignee() {
    // All three holders of a closed ticket — nobody, the principal, a stranger
    // — converge to the same nothing. `preserve` means the assignee is not
    // Kontor's to write, so there is no assignment, no clear, and no conflict
    // about a value the policy already promised not to touch.
    for spec in workflows() {
        assert_eq!(spec.ownership.terminal_action, OwnershipAction::Preserve);

        let holders = [
            (None, "an unassigned closed ticket"),
            (Some(principal().account_id), "a self-held closed ticket"),
            (Some(external("acct-other")), "an other-held closed ticket"),
        ];
        for (holder, described) in holders {
            let outcome = reconcile(&ReconciliationInput {
                spec: &spec,
                observation: &observation(&terminal_status(&spec), holder.as_ref()),
                freshness: Freshness::Fresh,
                facts: &facts(TaskState::Done, true, Some(TerminalOutcome::Succeeded)),
                live_transitions: &[],
                principal: &principal(),
            });
            assert_eq!(
                outcome,
                ReconciliationOutcome::NoOp,
                "{described} must plan no assignment at all, least of all a clear"
            );
        }
    }
}

/// The same specification with `accept_external` instead of `raise_conflict`.
///
/// The mismatch behavior is the *only* thing that changes, so a difference in
/// outcome can only come from that policy value and never from a status name.
fn accepting_workflows() -> Vec<ExternalWorkflowSpec> {
    workflows()
        .into_iter()
        .map(|mut spec| {
            spec.ownership.mismatch = OwnershipMismatchBehavior::AcceptExternal;
            spec.validate().expect("only the mismatch behavior changed");
            spec
        })
        .collect()
}

#[test]
fn accept_external_preserves_an_existing_owner_and_still_converges_the_status() {
    for spec in accepting_workflows() {
        let target = target_of(&spec, "milestone.development-started");
        let current = first_inbound(&spec);
        let stranger = external("acct-someone-else");
        let observed = observation(&current, Some(&stranger));
        let transitions = vec![
            LiveTransition {
                transition_id: external("t-noise"),
                to: hold_status(&spec),
            },
            LiveTransition {
                transition_id: external("t-correct"),
                to: target.clone(),
            },
        ];
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observed,
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &transitions,
            principal: &principal(),
        });
        match outcome {
            ReconciliationOutcome::Transition(plan) => {
                assert!(
                    plan.assignment.is_none(),
                    "accept_external must not replan the assignee onto the principal"
                );
                assert!(
                    !plan.assignment_prerequisite,
                    "there is no assignment to wait for"
                );
                assert_eq!(
                    plan.transition,
                    Some(SelectedTransition {
                        transition_id: external("t-correct"),
                        to: target,
                    }),
                    "the status still converges under the external owner"
                );
            }
            other => panic!("expected a transition plan under accept_external, got {other:?}"),
        }
    }
}

#[test]
fn accept_external_still_assigns_an_unassigned_ticket_to_the_principal() {
    // `accept_external` is about an owner that already exists. With nobody
    // holding the ticket there is no external value to accept, so the ownership
    // milestone still takes ownership — and still waits for confirmation.
    for spec in accepting_workflows() {
        let observed = observation(&first_inbound(&spec), None);
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observed,
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &[],
            principal: &principal(),
        });
        match outcome {
            ReconciliationOutcome::Transition(plan) => {
                let assignment = plan.assignment.expect("an assignment is planned");
                assert_eq!(assignment.action, OwnershipAction::ReassignToPrincipal);
                assert_eq!(assignment.assign_to, Some(principal().account_id));
                assert!(plan.assignment_prerequisite);
                assert!(plan.transition.is_none());
            }
            other => panic!("expected an assignment plan, got {other:?}"),
        }
    }
}

#[test]
fn accept_external_never_writes_an_assignee_it_did_not_have_to() {
    // Every non-terminal ownership shape under `accept_external`, in one place:
    // an assignment is planned for exactly one of them, and its value can only
    // ever be the principal's own account id.
    for spec in accepting_workflows() {
        let target = target_of(&spec, "milestone.development-started");
        let transitions = vec![LiveTransition {
            transition_id: external("t-correct"),
            to: target,
        }];
        let planned: Vec<Option<Option<ExternalId>>> = [
            None,
            Some(principal().account_id),
            Some(external("acct-someone-else")),
        ]
        .into_iter()
        .map(|holder| {
            let outcome = reconcile(&ReconciliationInput {
                spec: &spec,
                observation: &observation(&first_inbound(&spec), holder.as_ref()),
                freshness: Freshness::Fresh,
                facts: &facts(TaskState::InProgress, false, None),
                live_transitions: &transitions,
                principal: &principal(),
            });
            match outcome {
                ReconciliationOutcome::Transition(plan) => {
                    plan.assignment.map(|assignment| assignment.assign_to)
                }
                other => panic!("expected a plan for every ownership shape, got {other:?}"),
            }
        })
        .collect();
        assert_eq!(
            planned,
            vec![Some(Some(principal().account_id)), None, None],
            "only the unassigned ticket may be assigned, and only to the principal"
        );
    }
}

#[test]
fn no_matching_milestone_does_nothing() {
    for spec in workflows() {
        let outcome = reconcile(&ReconciliationInput {
            spec: &spec,
            observation: &observation(&first_inbound(&spec), Some(&principal().account_id)),
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::Ready, false, None),
            live_transitions: &[],
            principal: &principal(),
        });
        assert_eq!(outcome, ReconciliationOutcome::NoOp);
    }
}

#[test]
fn the_two_fixtures_produce_the_same_decisions() {
    // The same logical situation, expressed in two entirely different external
    // vocabularies, must produce the same *kind* of outcome every time.
    let specs = workflows();
    let mut shapes = Vec::new();
    for spec in &specs {
        let outcome = reconcile(&ReconciliationInput {
            spec,
            observation: &observation(&first_inbound(spec), None),
            freshness: Freshness::Fresh,
            facts: &facts(TaskState::InProgress, false, None),
            live_transitions: &[],
            principal: &principal(),
        });
        shapes.push(match outcome {
            ReconciliationOutcome::NoOp => "no_op".to_owned(),
            ReconciliationOutcome::Conflict(kind) => format!("conflict:{kind}"),
            ReconciliationOutcome::Transition(plan) => format!(
                "transition:prerequisite={}:has_transition={}",
                plan.assignment_prerequisite,
                plan.transition.is_some()
            ),
        });
    }
    assert_eq!(shapes[0], shapes[1]);
}

// ---------------------------------------------------------------------------
// Staged multi-hop convergence
// ---------------------------------------------------------------------------

/// One live transition, built from what a workflow actually offers.
fn route(id: &str, to: &StatusSelector) -> LiveTransition {
    LiveTransition {
        transition_id: external(id),
        to: to.clone(),
    }
}

/// The input shape every staged-hop case shares: ownership already converged, so
/// the assignment prerequisite is behind us and the status is the only question.
fn held_input<'a>(
    spec: &'a ExternalWorkflowSpec,
    observation: &'a ExternalTicketObservation,
    facts: &'a InternalTaskFacts,
    principal: &'a TicketPrincipal,
    live: &'a [LiveTransition],
) -> ReconciliationInput<'a> {
    ReconciliationInput {
        spec,
        observation,
        freshness: Freshness::Fresh,
        facts,
        live_transitions: live,
        principal,
    }
}

/// A target that needs two moves is reached by two planned moves, not by one
/// forced one.
///
/// This is the shape a real Jira workflow produced: the milestone wanted
/// `In Development`, the ticket was standing somewhere that offered no direct
/// route there, and the only route on offer led to the status the specification
/// already declares as its reopen selector. Kontor plans that hop, keeps citing
/// the milestone it is converging to, and lets the next observation finish the
/// job.
#[test]
fn a_target_two_moves_away_is_reached_through_the_declared_intermediate() {
    let spec = workflows().remove(0);
    let target = target_of(&spec, "milestone.development-started");
    let hop = spec
        .reopen
        .clone()
        .expect("the fixture declares a reopen status");
    let standing = spec
        .inbound_compatible
        .iter()
        .find(|status| status.status_id != hop.status_id && status.status_id != target.status_id)
        .cloned()
        .expect("the fixture declares a third inbound status to stand on");

    let principal = principal();
    let standing_at = observation(&standing, Some(&principal.account_id));
    let facts = facts(TaskState::InProgress, false, None);
    // Only the hop is on offer. Nothing reaches the milestone in one move.
    let live = vec![route("15", &hop)];

    let outcome = reconcile(&held_input(&spec, &standing_at, &facts, &principal, &live));

    let ReconciliationOutcome::Transition(plan) = outcome else {
        panic!("a reachable intermediate is a plan, not a conflict: {outcome:?}");
    };
    assert_eq!(
        plan.target.status_id, target.status_id,
        "the plan still names the milestone it is converging to"
    );
    assert_eq!(
        plan.destination().status_id,
        hop.status_id,
        "this attempt goes to the declared intermediate"
    );
    assert_eq!(
        plan.transition
            .as_ref()
            .expect("a hop invokes a transition")
            .transition_id,
        external("15"),
        "the hop uses the route the observation offered, not a remembered id"
    );
    assert!(
        plan.is_staged_hop(),
        "a hop is progress, not convergence, and has to say so"
    );
    assert!(
        !plan.assignment_prerequisite,
        "ownership already converged; this attempt is about the status"
    );

    // The next observation stands on the hop, and now the milestone is reachable.
    let arrived = observation(&hop, Some(&principal.account_id));
    let onward = vec![route("21", &target)];
    let outcome = reconcile(&held_input(&spec, &arrived, &facts, &principal, &onward));
    let ReconciliationOutcome::Transition(plan) = outcome else {
        panic!("the second move is an ordinary convergence: {outcome:?}");
    };
    assert_eq!(plan.destination().status_id, target.status_id);
    assert!(
        !plan.is_staged_hop(),
        "the second move reaches the milestone, so it is not a hop"
    );
}

/// Every shape that is not "the declared intermediate, directly reachable, once"
/// stays a typed conflict.
///
/// The hop is a narrow allowance, not a search. Each case below would be a route
/// Kontor invented rather than one the specification declared, so each one keeps
/// the conflict a human resolves.
#[test]
fn an_intermediate_kontor_was_not_given_is_refused_rather_than_invented() {
    let base = workflows().remove(0);
    let target = target_of(&base, "milestone.development-started");
    let hop = base
        .reopen
        .clone()
        .expect("the fixture declares a reopen status");
    let standing = base
        .inbound_compatible
        .iter()
        .find(|status| status.status_id != hop.status_id && status.status_id != target.status_id)
        .cloned()
        .expect("the fixture declares a third inbound status");
    let principal = principal();
    let facts = facts(TaskState::InProgress, false, None);

    // No reopen selector at all: nothing is declared safe to route through.
    let mut undeclared = base.clone();
    undeclared.reopen = None;
    let standing_at = observation(&standing, Some(&principal.account_id));
    let live = vec![route("15", &hop)];
    assert_eq!(
        reconcile(&held_input(
            &undeclared,
            &standing_at,
            &facts,
            &principal,
            &live
        )),
        ReconciliationOutcome::Conflict(StatusConflictKind::NoLiveTransition),
        "with no declared intermediate the offered route is not Kontor's to take"
    );

    // The intermediate is declared but not currently offered.
    let elsewhere = vec![route("99", &hold_status(&base))];
    assert_eq!(
        reconcile(&held_input(
            &base,
            &standing_at,
            &facts,
            &principal,
            &elsewhere
        )),
        ReconciliationOutcome::Conflict(StatusConflictKind::NoLiveTransition),
        "a declared intermediate that is not on offer is still not reachable"
    );

    // Already standing on the intermediate, and the milestone is still not
    // reachable. Hopping again would be a loop between two statuses.
    let on_hop = observation(&hop, Some(&principal.account_id));
    let back_to_hop = vec![route("15", &hop)];
    assert_eq!(
        reconcile(&held_input(
            &base,
            &on_hop,
            &facts,
            &principal,
            &back_to_hop
        )),
        ReconciliationOutcome::Conflict(StatusConflictKind::NoLiveTransition),
        "a hop to where the ticket already stands makes no progress"
    );

    // Two routes reach the intermediate: which one runs is not determined.
    let ambiguous = vec![route("15", &hop), route("16", &hop)];
    assert_eq!(
        reconcile(&held_input(
            &base,
            &standing_at,
            &facts,
            &principal,
            &ambiguous
        )),
        ReconciliationOutcome::Conflict(StatusConflictKind::MultipleLiveTransitions),
        "an ambiguous hop is a conflict, not a coin flip"
    );
}

// ---------------------------------------------------------------------------
// Transition receipts
// ---------------------------------------------------------------------------

#[test]
fn a_transition_receipt_must_record_something_it_actually_did() {
    let spec = workflows().remove(0);
    let target = target_of(&spec, "milestone.development-started");
    let mut receipt = kontor_core::ticket::StatusTransitionReceipt {
        id: kontor_core::id::StatusTransitionReceiptId::generate(),
        link_id: TicketLinkId::generate(),
        task_id: TaskId::generate(),
        task_revision: AggregateRevision::INITIAL,
        workflow_revision: AggregateRevision::INITIAL,
        projection_revision: AggregateRevision::INITIAL,
        spec_version: SpecVersion::FIRST,
        prior_observation_id: TicketObservationId::generate(),
        plan: TransitionPlan {
            milestone: SemanticMilestoneKey::parse("milestone.development-started")
                .expect("valid milestone key"),
            target: target.clone(),
            transition: Some(SelectedTransition {
                transition_id: external("t-correct"),
                to: target,
            }),
            assignment: None,
            assignment_prerequisite: false,
        },
        principal: principal(),
        assignment_result: None,
        idempotency_key: IdempotencyKey::parse("sync-1").expect("valid key"),
        dispatched_at: at("2026-08-09T10:00:00Z"),
        acknowledged_at: None,
        confirmed_at: None,
        refetched_observation_id: None,
    };
    receipt
        .validate()
        .expect("a dispatched transition is valid");

    receipt.plan.transition = None;
    assert!(
        receipt.validate().is_err(),
        "a receipt with neither a transition nor an assignment did nothing"
    );

    receipt.confirmed_at = Some(at("2026-08-09T10:01:00Z"));
    receipt.plan.assignment = Some(kontor_core::ticket::AssignmentPlan {
        assign_to: Some(principal().account_id),
        action: OwnershipAction::ReassignToPrincipal,
    });
    let error = receipt
        .validate()
        .expect_err("confirmation without a refetch is an assumption");
    assert!(matches!(error, DomainError::MissingEvidence { .. }));

    receipt.refetched_observation_id = Some(TicketObservationId::generate());
    receipt
        .validate()
        .expect("an evidenced confirmation is valid");
}

#[test]
fn external_terminal_success_with_incomplete_gates_is_a_conflict_even_when_run_outcome_exists() {
    for spec in workflows() {
        let terminal = terminal_status(&spec);
        let held = observation(&terminal, Some(&principal().account_id));

        // The run succeeded, so a run outcome genuinely exists — but a required
        // gate never passed. This is the exact shape that must not collapse into
        // `NoOp`.
        let incomplete = facts(TaskState::Done, false, Some(TerminalOutcome::Succeeded));
        assert_eq!(
            reconcile(&ReconciliationInput {
                spec: &spec,
                observation: &held,
                freshness: Freshness::Fresh,
                facts: &incomplete,
                live_transitions: &[],
                principal: &principal(),
            }),
            ReconciliationOutcome::Conflict(
                StatusConflictKind::ExternalTerminalBeforeInternalEvidence
            ),
            "external terminal success with incomplete gates must be a conflict"
        );

        // A run that failed cannot evidence an externally successful close
        // either, gates or no gates.
        for outcome in [
            TerminalOutcome::Failed,
            TerminalOutcome::Cancelled,
            TerminalOutcome::Parked,
            TerminalOutcome::Abandoned,
        ] {
            assert_eq!(
                reconcile(&ReconciliationInput {
                    spec: &spec,
                    observation: &held,
                    freshness: Freshness::Fresh,
                    facts: &facts(TaskState::Done, true, Some(outcome)),
                    live_transitions: &[],
                    principal: &principal(),
                }),
                ReconciliationOutcome::Conflict(
                    StatusConflictKind::ExternalTerminalBeforeInternalEvidence
                ),
                "{outcome} must not evidence an externally successful close"
            );
        }

        // Only a succeeded run with complete gates agrees, and even then the
        // preserve policy plans nothing.
        assert_eq!(
            reconcile(&ReconciliationInput {
                spec: &spec,
                observation: &held,
                freshness: Freshness::Fresh,
                facts: &facts(TaskState::Done, true, Some(TerminalOutcome::Succeeded)),
                live_transitions: &[],
                principal: &principal(),
            }),
            ReconciliationOutcome::NoOp
        );

        // The precondition is evaluated before the ownership branch too: a
        // terminal ticket held by somebody else with incomplete gates reports
        // the missing evidence rather than the ownership violation.
        let stranger = observation(&terminal, Some(&external("acct-other")));
        assert_eq!(
            reconcile(&ReconciliationInput {
                spec: &spec,
                observation: &stranger,
                freshness: Freshness::Fresh,
                facts: &incomplete,
                live_transitions: &[],
                principal: &principal(),
            }),
            ReconciliationOutcome::Conflict(
                StatusConflictKind::ExternalTerminalBeforeInternalEvidence
            )
        );
    }
}

#[test]
fn each_external_terminal_class_requires_its_own_corresponding_internal_outcome() {
    // The full cross-product of external terminal class against internal
    // terminal outcome. Exactly one cell per row agrees; everything else is a
    // conflict. `abandoned` and `parked` agree with nothing: an operator closing
    // a run without a runtime verdict has produced no evidence about what the
    // external system should say.
    let agrees = |class: SemanticStatusClass| match class {
        SemanticStatusClass::TerminalSuccess => Some(TerminalOutcome::Succeeded),
        SemanticStatusClass::TerminalCancelled => Some(TerminalOutcome::Cancelled),
        SemanticStatusClass::TerminalRejected => Some(TerminalOutcome::Failed),
        SemanticStatusClass::Active | SemanticStatusClass::Hold => None,
    };

    for spec in workflows() {
        for class in [
            SemanticStatusClass::TerminalSuccess,
            SemanticStatusClass::TerminalCancelled,
            SemanticStatusClass::TerminalRejected,
        ] {
            let held = observation(
                &terminal_status_of(&spec, class),
                Some(&principal().account_id),
            );
            for outcome in [
                TerminalOutcome::Succeeded,
                TerminalOutcome::Failed,
                TerminalOutcome::Cancelled,
                TerminalOutcome::Parked,
                TerminalOutcome::Abandoned,
            ] {
                // Gates are complete throughout, so the only variable under test
                // is whether the outcome *corresponds* to the class.
                let outcome_agrees = agrees(class) == Some(outcome);
                let result = reconcile(&ReconciliationInput {
                    spec: &spec,
                    observation: &held,
                    freshness: Freshness::Fresh,
                    facts: &facts(TaskState::Done, true, Some(outcome)),
                    live_transitions: &[],
                    principal: &principal(),
                });
                if outcome_agrees {
                    assert_eq!(
                        result,
                        ReconciliationOutcome::NoOp,
                        "{outcome} is the corresponding outcome for {class} and must agree"
                    );
                } else {
                    assert_eq!(
                        result,
                        ReconciliationOutcome::Conflict(
                            StatusConflictKind::ExternalTerminalBeforeInternalEvidence
                        ),
                        "{outcome} must not evidence an externally {class} ticket"
                    );
                }
            }

            // A missing outcome never evidences any terminal class either.
            assert_eq!(
                reconcile(&ReconciliationInput {
                    spec: &spec,
                    observation: &held,
                    freshness: Freshness::Fresh,
                    facts: &facts(TaskState::InProgress, true, None),
                    live_transitions: &[],
                    principal: &principal(),
                }),
                ReconciliationOutcome::Conflict(
                    StatusConflictKind::ExternalTerminalBeforeInternalEvidence
                ),
                "a run with no outcome cannot evidence an externally {class} ticket"
            );
        }
    }
}
