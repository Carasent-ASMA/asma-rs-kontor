//! Durable quota-blocked seat succession: exact evidence, replay and restart.

use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, CanonicalDocument, ContentHash,
    CredentialAlias, ExternalId, ExternalName, IdempotencyKey, ProjectId,
    QuotaObservationProvenanceId, RoleKey, RuntimeBindingId, RuntimeKindKey, SuccessionAttemptId,
    SuccessionReceiptId, TaskId, TeamRunId, Timestamp,
};
use kontor_core::repository::{
    CapacityRepository, CredentialReference, CredentialReferenceKind, NewAccountProfile,
    NewObservation, NewProject, NewProviderQuotaState, NewQuotaObservationProvenance,
    NewRuntimeEvent, ProjectRepository, RunRepository, SuccessionRepository,
};
use kontor_core::spec::{ModelRef, ModelRung, ProviderQuotaKind, ProviderQuotaSource, ProviderRef};
use kontor_core::state::{Freshness, NativeRuntimeIdentity, ObservedRunState, RuntimeContact};
use kontor_core::succession::{
    NewSuccessionAttempt, SuccessionAttemptAdvance, SuccessionAttemptState, SuccessionConfirmation,
    SuccessionDeferredRefresh, SuccessionHandoff, SuccessionHandoffDegradedReason,
    SuccessionHandoffOutcome, SuccessionHandoffRecord, SuccessionReceipt, SuccessionRedactionPass,
    SuccessionRedactionReceipt, SuccessionRefusal, SuccessionRefusalReason,
    SuccessionSuccessorObservation, SuccessionSuccessorRecord,
};
use kontor_store::SqliteStore;
use kontor_store::backup::{ImportPlan, KontorExportV1, export_realm, import_export};
use rusqlite::{Connection, params};
use tempfile::TempDir;

const PROJECT: &str = "0193f000-0000-7000-8000-000000000001";
const TASK: &str = "0193f000-0000-7000-8000-000000000010";
const TEAM: &str = "0193f000-0000-7000-8000-000000000035";
const PREDECESSOR: &str = "0193f000-0000-7000-8000-000000000040";
const PREDECESSOR_BINDING: &str = "0193f000-0000-7000-8000-000000000050";

const BASE_SQL: &str = "
INSERT INTO projects (id, name, root_path, revision, created_at)
VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/succession', 1,
        '2026-09-04T08:00:00Z');
INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at)
VALUES ('0193f000-0000-7000-8000-000000000010',
        '0193f000-0000-7000-8000-000000000001', 'T', 'in_progress', 1,
        '2026-09-04T08:00:00Z', '2026-09-04T08:00:00Z');
INSERT INTO team_templates
       (project_id, template_id, version, name, definition, definition_hash,
        role_authority, created_at)
VALUES ('0193f000-0000-7000-8000-000000000001',
        '0193f000-0000-7000-8000-000000000020', 1, 'Team', '{}',
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '[]',
        '2026-09-04T08:00:00Z');
INSERT INTO team_runs
       (id, project_id, task_id, template_id, template_version, snapshot,
        snapshot_hash, lifecycle, revision, created_at)
VALUES ('0193f000-0000-7000-8000-000000000035',
        '0193f000-0000-7000-8000-000000000001',
        '0193f000-0000-7000-8000-000000000010',
        '0193f000-0000-7000-8000-000000000020', 1, '{}',
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
        'running', 1, '2026-09-04T08:00:00Z');";

struct Fixture {
    _directory: TempDir,
    path: std::path::PathBuf,
    store: SqliteStore,
    project: ProjectId,
    task: TaskId,
    team: TeamRunId,
    predecessor: AgentRunId,
    predecessor_binding: RuntimeBindingId,
    predecessor_account: AccountProfileId,
    successor_account: AccountProfileId,
    quota_provenance: QuotaObservationProvenanceId,
    quota_hash: ContentHash,
}

fn at(text: &str) -> Timestamp {
    text.parse().expect("a canonical timestamp")
}

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker,
    }))
    .expect("a canonical document")
}

fn identity(native_id: &str) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("generic.runtime").expect("runtime"),
        host: ExternalName::parse("host-1").expect("host"),
        generation: 1,
        native_id: ExternalId::parse(native_id).expect("native id"),
    }
}

fn redaction(pass: SuccessionRedactionPass) -> SuccessionRedactionReceipt {
    SuccessionRedactionReceipt {
        schema_version: 1,
        pass,
        source_hash: ContentHash::of(b"handoff-source"),
        redacted_hash: ContentHash::of(b"handoff-redacted"),
        policy_hash: ContentHash::of(b"handoff-policy"),
        redacted_at: at("2026-09-04T08:03:00Z"),
    }
}

fn create_account(store: &SqliteStore, project_id: ProjectId, alias: &str) -> AccountProfileId {
    let id = AccountProfileId::generate();
    store
        .create_account_profile(&NewAccountProfile {
            id,
            project_id,
            label: ExternalName::parse(alias).expect("label"),
            external_account_id: None,
            harness: RuntimeKindKey::parse("generic.runtime").expect("runtime"),
            credential_ref: CredentialReference {
                kind: CredentialReferenceKind::ConfigHome,
                alias: CredentialAlias::parse(alias).expect("alias"),
            },
            environment: document("environment"),
            routing: document("routing"),
            capability: document("capability"),
            provider_identity: None,
            enabled: true,
            created_at: at("2026-09-04T08:00:00Z"),
        })
        .expect("account is created");
    id
}

fn fixture() -> Fixture {
    let directory = TempDir::new().expect("temporary directory");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    Connection::open(&path)
        .expect("raw fixture connection")
        .execute_batch(BASE_SQL)
        .expect("base fixture inserts");
    let project = ProjectId::parse(PROJECT).expect("project id");
    let task = TaskId::parse(TASK).expect("task id");
    let team = TeamRunId::parse(TEAM).expect("team id");
    let predecessor = AgentRunId::parse(PREDECESSOR).expect("run id");
    let predecessor_binding = RuntimeBindingId::parse(PREDECESSOR_BINDING).expect("binding id");
    let predecessor_account = create_account(&store, project, "blocked-account");
    let successor_account = create_account(&store, project, "successor-account");

    let connection = Connection::open(&path).expect("raw fixture connection");
    connection
        .execute(
            "INSERT INTO agent_runs
                (id, project_id, team_run_id, role_key, account_profile_id, lifecycle,
                 desired_state, observed_state, derived_state, revision, created_at)
             VALUES (?1, ?2, ?3, 'aud', ?4, 'running', 'run_requested', 'unknown',
                     'pending_confirmation', 1, '2026-09-04T08:00:00Z')",
            params![
                predecessor.to_string(),
                project.to_string(),
                team.to_string(),
                predecessor_account.to_string(),
            ],
        )
        .expect("predecessor inserts");
    connection
        .execute(
            "INSERT INTO runtime_bindings
                (id, project_id, agent_run_id, runtime_kind, host, generation, native_id, bound_at)
             VALUES (?1, ?2, ?3, 'generic.runtime', 'host-1', 1, 'predecessor',
                     '2026-09-04T08:00:00Z')",
            params![
                predecessor_binding.to_string(),
                project.to_string(),
                predecessor.to_string(),
            ],
        )
        .expect("predecessor binding inserts");

    let quota_hash = ContentHash::of(b"quota-refusal");
    let quota_provenance = QuotaObservationProvenanceId::generate();
    let blocked = store
        .record_observation(&NewObservation {
            event: NewRuntimeEvent {
                project_id: project,
                agent_run_id: predecessor,
                identity: identity("predecessor"),
                native_event_id: Some(ExternalId::parse("blocked-1").expect("event id")),
                native_sequence: 1,
                payload: document("blocked"),
                observed_at: at("2026-09-04T08:01:00Z"),
            },
            observed: ObservedRunState::Blocked,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            expected_revision: AggregateRevision::INITIAL,
            quota_state: Some(NewProviderQuotaState {
                project_id: project,
                account_profile_id: predecessor_account,
                provider: "openai".to_owned(),
                state: ProviderQuotaKind::Unknown,
                resets_at: None,
                windows: Vec::new(),
                credit: None,
                evidence_hash: quota_hash.clone(),
                provenance: Some(NewQuotaObservationProvenance {
                    id: quota_provenance,
                    project_id: project,
                    account_profile_id: predecessor_account,
                    provider: "openai".to_owned(),
                    signal_id: "runtime-quota".to_owned(),
                    signal_version: kontor_core::id::SpecVersion::FIRST,
                    signal_definition_hash: ContentHash::of(b"signal"),
                    agent_run_id: predecessor,
                    runtime_binding_id: predecessor_binding,
                    native_id: ExternalId::parse("predecessor").expect("native id"),
                    binding_generation: 1,
                    runtime_observation_cursor: None,
                    item_epoch: 1,
                    item_seq_start: 7,
                    item_seq_end: 7,
                    source_sequences: vec![(7, 7)],
                    item_kind: "assistant_message".to_owned(),
                    item_observed_at: at("2026-09-04T08:01:00Z"),
                    decision_basis: kontor_core::spec::QuotaDecisionBasis::RuntimeRefusal,
                    decided_state: ProviderQuotaKind::Unknown,
                    parsed_resets_at: None,
                    reset_zone: None,
                    evidence_digest: quota_hash.clone(),
                    recorded_at: at("2026-09-04T08:01:00Z"),
                }),
                source: ProviderQuotaSource::RuntimeObservation,
                observed_at: at("2026-09-04T08:01:00Z"),
                expected_revision: AggregateRevision::INITIAL,
                updated_at: at("2026-09-04T08:01:00Z"),
            }),
        })
        .expect("blocked observation and quota evidence commit together");
    assert!(blocked.last_cursor.is_some());

    Fixture {
        _directory: directory,
        path,
        store,
        project,
        task,
        team,
        predecessor,
        predecessor_binding,
        predecessor_account,
        successor_account,
        quota_provenance,
        quota_hash,
    }
}

struct RefreshedEvidence {
    predecessor_revision: AggregateRevision,
    cursor: kontor_core::id::EventCursor,
    quota_provenance: QuotaObservationProvenanceId,
    quota_revision: AggregateRevision,
    quota_hash: ContentHash,
}

fn record_refreshed_blocked_evidence(
    fixture: &Fixture,
    observed_at: Timestamp,
    state: ProviderQuotaKind,
    resets_at: Option<Timestamp>,
) -> RefreshedEvidence {
    let predecessor = fixture
        .store
        .get_agent_run(fixture.project, fixture.predecessor)
        .expect("predecessor read")
        .expect("predecessor exists");
    let quota_revision = fixture
        .store
        .list_provider_quota_states(fixture.project)
        .expect("quota rows read")
        .into_iter()
        .find(|row| {
            row.account_profile_id == fixture.predecessor_account && row.provider == "openai"
        })
        .expect("predecessor quota exists")
        .revision;
    let quota_provenance = QuotaObservationProvenanceId::generate();
    let quota_hash = ContentHash::of(format!("quota-refresh-{observed_at}").as_bytes());
    let projection = fixture
        .store
        .record_observation(&NewObservation {
            event: NewRuntimeEvent {
                project_id: fixture.project,
                agent_run_id: fixture.predecessor,
                identity: identity("predecessor"),
                native_event_id: Some(ExternalId::parse("blocked-2").expect("event id")),
                native_sequence: 2,
                payload: document("blocked-again"),
                observed_at,
            },
            observed: ObservedRunState::Blocked,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            expected_revision: predecessor.revision,
            quota_state: Some(NewProviderQuotaState {
                project_id: fixture.project,
                account_profile_id: fixture.predecessor_account,
                provider: "openai".to_owned(),
                state,
                resets_at,
                windows: Vec::new(),
                credit: None,
                evidence_hash: quota_hash.clone(),
                provenance: Some(NewQuotaObservationProvenance {
                    id: quota_provenance,
                    project_id: fixture.project,
                    account_profile_id: fixture.predecessor_account,
                    provider: "openai".to_owned(),
                    signal_id: "runtime-quota".to_owned(),
                    signal_version: kontor_core::id::SpecVersion::FIRST,
                    signal_definition_hash: ContentHash::of(b"signal"),
                    agent_run_id: fixture.predecessor,
                    runtime_binding_id: fixture.predecessor_binding,
                    native_id: ExternalId::parse("predecessor").expect("native id"),
                    binding_generation: 1,
                    runtime_observation_cursor: None,
                    item_epoch: 1,
                    item_seq_start: 8,
                    item_seq_end: 8,
                    source_sequences: vec![(8, 8)],
                    item_kind: "assistant_message".to_owned(),
                    item_observed_at: observed_at,
                    decision_basis: kontor_core::spec::QuotaDecisionBasis::RuntimeRefusal,
                    decided_state: state,
                    parsed_resets_at: resets_at,
                    reset_zone: None,
                    evidence_digest: quota_hash.clone(),
                    recorded_at: observed_at,
                }),
                source: ProviderQuotaSource::RuntimeObservation,
                observed_at,
                expected_revision: quota_revision,
                updated_at: observed_at,
            }),
        })
        .expect("fresh blocked observation and quota evidence commit together");
    let predecessor = fixture
        .store
        .get_agent_run(fixture.project, fixture.predecessor)
        .expect("predecessor read")
        .expect("predecessor exists");
    let refreshed_quota_revision = fixture
        .store
        .list_provider_quota_states(fixture.project)
        .expect("quota rows read")
        .into_iter()
        .find(|row| {
            row.account_profile_id == fixture.predecessor_account && row.provider == "openai"
        })
        .expect("refreshed predecessor quota exists")
        .revision;
    RefreshedEvidence {
        predecessor_revision: predecessor.revision,
        cursor: projection.last_cursor.expect("blocked cursor"),
        quota_provenance,
        quota_revision: refreshed_quota_revision,
        quota_hash,
    }
}

fn identical_refusal_observation(
    fixture: &Fixture,
    expected_predecessor_revision: AggregateRevision,
    expected_quota_revision: AggregateRevision,
    provenance_id: QuotaObservationProvenanceId,
) -> NewObservation {
    let observed_at = at("2026-09-04T08:02:00Z");
    NewObservation {
        event: NewRuntimeEvent {
            project_id: fixture.project,
            agent_run_id: fixture.predecessor,
            identity: identity("predecessor"),
            native_event_id: Some(ExternalId::parse("blocked-2").expect("event id")),
            native_sequence: 2,
            payload: document("blocked-again"),
            observed_at,
        },
        observed: ObservedRunState::Blocked,
        contact: RuntimeContact::Reachable,
        freshness: Freshness::Fresh,
        expected_revision: expected_predecessor_revision,
        quota_state: Some(NewProviderQuotaState {
            project_id: fixture.project,
            account_profile_id: fixture.predecessor_account,
            provider: "openai".to_owned(),
            state: ProviderQuotaKind::Unknown,
            resets_at: None,
            windows: Vec::new(),
            credit: None,
            evidence_hash: fixture.quota_hash.clone(),
            provenance: Some(NewQuotaObservationProvenance {
                id: provenance_id,
                project_id: fixture.project,
                account_profile_id: fixture.predecessor_account,
                provider: "openai".to_owned(),
                signal_id: "runtime-quota".to_owned(),
                signal_version: kontor_core::id::SpecVersion::FIRST,
                signal_definition_hash: ContentHash::of(b"signal"),
                agent_run_id: fixture.predecessor,
                runtime_binding_id: fixture.predecessor_binding,
                native_id: ExternalId::parse("predecessor").expect("native id"),
                binding_generation: 1,
                runtime_observation_cursor: None,
                item_epoch: 1,
                item_seq_start: 8,
                item_seq_end: 8,
                source_sequences: vec![(8, 8)],
                item_kind: "assistant_message".to_owned(),
                item_observed_at: observed_at,
                decision_basis: kontor_core::spec::QuotaDecisionBasis::RuntimeRefusal,
                decided_state: ProviderQuotaKind::Unknown,
                parsed_resets_at: None,
                reset_zone: None,
                evidence_digest: fixture.quota_hash.clone(),
                recorded_at: observed_at,
            }),
            source: ProviderQuotaSource::RuntimeObservation,
            observed_at,
            expected_revision: expected_quota_revision,
            updated_at: observed_at,
        }),
    }
}

#[test]
fn identical_runtime_refusal_refreshes_the_current_cursor_once_and_replay_is_inert() {
    let fixture = fixture();
    let before_run = fixture
        .store
        .get_agent_run(fixture.project, fixture.predecessor)
        .expect("run read")
        .expect("run exists");
    let before_rows = fixture
        .store
        .list_provider_quota_states(fixture.project)
        .expect("quota rows");
    assert_eq!(before_rows.len(), 1);
    let before = &before_rows[0];
    let fresh_provenance = QuotaObservationProvenanceId::generate();
    let observation = identical_refusal_observation(
        &fixture,
        before_run.revision,
        before.revision,
        fresh_provenance,
    );

    let projection = fixture
        .store
        .record_observation(&observation)
        .expect("new identical refusal commits atomically");
    let fresh_cursor = projection.last_cursor.expect("fresh cursor");
    assert!(fresh_cursor > before_run.projection.last_cursor.expect("old cursor"));
    let after_rows = fixture
        .store
        .list_provider_quota_states(fixture.project)
        .expect("quota rows");
    assert_eq!(
        after_rows.len(),
        1,
        "the projection remains one current row"
    );
    let after = &after_rows[0];
    assert_eq!(after.state, before.state);
    assert_eq!(after.evidence_hash, before.evidence_hash);
    assert!(after.revision > before.revision);
    assert_eq!(after.provenance_id, Some(fresh_provenance));
    let provenance = fixture
        .store
        .get_quota_observation_provenance(fixture.project, fresh_provenance)
        .expect("provenance read")
        .expect("fresh provenance exists");
    assert_eq!(
        provenance.record.runtime_observation_cursor,
        Some(fresh_cursor)
    );

    let current_run = fixture
        .store
        .get_agent_run(fixture.project, fixture.predecessor)
        .expect("run read")
        .expect("run exists");
    let replay_projection = fixture
        .store
        .record_observation(&NewObservation {
            expected_revision: current_run.revision,
            ..observation
        })
        .expect("exact runtime event replay is inert");
    assert_eq!(replay_projection.last_cursor, Some(fresh_cursor));
    let replay_rows = fixture
        .store
        .list_provider_quota_states(fixture.project)
        .expect("quota rows");
    assert_eq!(replay_rows, after_rows);
    let provenance_count: i64 = Connection::open(&fixture.path)
        .expect("raw connection")
        .query_row(
            "SELECT count(*) FROM provider_quota_observation_provenance",
            [],
            |row| row.get(0),
        )
        .expect("provenance count");
    assert_eq!(provenance_count, 2, "the replay appends no provenance");
}

fn new_attempt(fixture: &Fixture, key: &str) -> NewSuccessionAttempt {
    let predecessor = fixture
        .store
        .get_agent_run(fixture.project, fixture.predecessor)
        .expect("predecessor read")
        .expect("predecessor exists");
    let team_revision: i64 = Connection::open(&fixture.path)
        .expect("raw connection")
        .query_row(
            "SELECT revision FROM team_runs WHERE id = ?1",
            params![fixture.team.to_string()],
            |row| row.get(0),
        )
        .expect("team revision");
    NewSuccessionAttempt {
        id: SuccessionAttemptId::generate(),
        project_id: fixture.project,
        task_id: fixture.task,
        team_run_id: fixture.team,
        role: RoleKey::parse("aud").expect("role"),
        predecessor_agent_run_id: fixture.predecessor,
        predecessor_runtime_binding_id: fixture.predecessor_binding,
        predecessor_native_identity: identity("predecessor"),
        expected_task_revision: AggregateRevision::INITIAL,
        expected_team_revision: AggregateRevision::parse(team_revision as u64)
            .expect("team revision"),
        expected_predecessor_revision: predecessor.revision,
        runtime_observation_cursor: predecessor.projection.last_cursor.expect("blocked cursor"),
        quota_provenance_id: fixture.quota_provenance,
        quota_state_revision: AggregateRevision::INITIAL,
        quota_evidence_hash: fixture.quota_hash.clone(),
        quota_provider: "openai".to_owned(),
        successor_model_rung: Some(ModelRung {
            provider: ProviderRef("anthropic".to_owned()),
            model: ModelRef("claude-sonnet".to_owned()),
            effort: None,
        }),
        successor_account_profile_id: Some(fixture.successor_account),
        idempotency_key: IdempotencyKey::parse(key).expect("idempotency key"),
        intent_hash: ContentHash::of(key.as_bytes()),
        deferred_until: None,
        created_at: at("2026-09-04T08:02:00Z"),
    }
}

#[test]
fn due_deferred_refresh_admits_at_the_elapsed_exhausted_reset_boundary() {
    let fixture = fixture();
    let mut request = new_attempt(&fixture, "succession:deferred");
    request.successor_model_rung = None;
    request.successor_account_profile_id = None;
    request.deferred_until = Some(at("2026-09-04T09:00:00Z"));
    let deferred = fixture
        .store
        .create_succession_attempt(&request)
        .expect("wait is durable without a fabricated route");
    assert_eq!(deferred.state, SuccessionAttemptState::Deferred);
    assert!(deferred.request.successor_model_rung.is_none());
    assert!(deferred.request.successor_account_profile_id.is_none());

    let original_id = deferred.request.id;
    let original_key = deferred.request.idempotency_key.clone();
    let original_intent_hash = deferred.request.intent_hash.clone();
    let evidence = record_refreshed_blocked_evidence(
        &fixture,
        at("2026-09-04T09:00:00Z"),
        ProviderQuotaKind::Exhausted,
        Some(at("2026-09-04T09:00:00Z")),
    );
    assert!(
        !fixture
            .store
            .list_provider_quota_states(fixture.project)
            .expect("quota rows")
            .into_iter()
            .find(|row| row.account_profile_id == fixture.predecessor_account)
            .expect("predecessor quota")
            .blocks_at(at("2026-09-04T09:00:00Z")),
        "an exhausted allowance stops blocking exactly at reset"
    );
    let refresh = SuccessionDeferredRefresh {
        project_id: fixture.project,
        attempt_id: deferred.request.id,
        expected_revision: deferred.revision,
        expected_predecessor_revision: evidence.predecessor_revision,
        runtime_observation_cursor: evidence.cursor,
        quota_provenance_id: evidence.quota_provenance,
        quota_state_revision: evidence.quota_revision,
        quota_evidence_hash: evidence.quota_hash,
        quota_provider: "openai".to_owned(),
        successor_model_rung: ModelRung {
            provider: ProviderRef("anthropic".to_owned()),
            model: ModelRef("claude-sonnet".to_owned()),
            effort: None,
        }
        .into(),
        successor_account_profile_id: Some(fixture.successor_account),
        deferred_until: None,
        refreshed_at: at("2026-09-04T09:00:00Z"),
    };
    assert!(
        fixture
            .store
            .refresh_deferred_succession_evidence(&SuccessionDeferredRefresh {
                refreshed_at: at("2026-09-04T08:59:59Z"),
                ..refresh.clone()
            })
            .is_err(),
        "refresh before the exact wait deadline is refused"
    );
    let planned = fixture
        .store
        .refresh_deferred_succession_evidence(&refresh)
        .expect("fresh same-binding blocked readback admits a successor");
    assert_eq!(planned.state, SuccessionAttemptState::Planned);
    assert_eq!(planned.request.id, original_id);
    assert_eq!(planned.request.idempotency_key, original_key);
    assert_eq!(planned.request.intent_hash, original_intent_hash);
    assert_eq!(planned.request.runtime_observation_cursor, evidence.cursor);
    assert_eq!(
        planned.request.quota_provenance_id,
        evidence.quota_provenance
    );
    assert!(planned.request.deferred_until.is_none());
    assert_eq!(
        planned.request.successor_account_profile_id,
        Some(fixture.successor_account)
    );
    assert!(
        fixture
            .store
            .refresh_deferred_succession_evidence(&refresh)
            .is_err(),
        "a stale CAS cannot replay over the refreshed attempt"
    );
}

#[test]
fn due_deferred_refresh_can_record_another_exact_future_wait() {
    let fixture = fixture();
    let mut request = new_attempt(&fixture, "succession:wait-again");
    request.successor_model_rung = None;
    request.successor_account_profile_id = None;
    request.deferred_until = Some(at("2026-09-04T09:00:00Z"));
    let deferred = fixture
        .store
        .create_succession_attempt(&request)
        .expect("first Wait is durable");
    let evidence = record_refreshed_blocked_evidence(
        &fixture,
        at("2026-09-04T09:00:00Z"),
        ProviderQuotaKind::Exhausted,
        Some(at("2026-09-04T10:00:00Z")),
    );
    let renewed = fixture
        .store
        .refresh_deferred_succession_evidence(&SuccessionDeferredRefresh {
            project_id: fixture.project,
            attempt_id: deferred.request.id,
            expected_revision: deferred.revision,
            expected_predecessor_revision: evidence.predecessor_revision,
            runtime_observation_cursor: evidence.cursor,
            quota_provenance_id: evidence.quota_provenance,
            quota_state_revision: evidence.quota_revision,
            quota_evidence_hash: evidence.quota_hash,
            quota_provider: "openai".to_owned(),
            successor_model_rung: None,
            successor_account_profile_id: None,
            deferred_until: Some(at("2026-09-04T10:00:00Z")),
            refreshed_at: at("2026-09-04T09:00:00Z"),
        })
        .expect("another exact Wait is durable");
    assert_eq!(renewed.state, SuccessionAttemptState::Deferred);
    assert_eq!(
        renewed.request.deferred_until,
        Some(at("2026-09-04T10:00:00Z"))
    );
    assert_eq!(
        fixture
            .store
            .list_due_succession_attempts(at("2026-09-04T09:59:59Z"), 10)
            .expect("due inventory")
            .len(),
        0
    );
}

#[test]
fn schema_rejects_route_only_mutation_that_bypasses_the_due_refresh_cas() {
    let fixture = fixture();
    let mut request = new_attempt(&fixture, "succession:raw-route");
    request.successor_model_rung = None;
    request.successor_account_profile_id = None;
    request.deferred_until = Some(at("2026-09-04T09:00:00Z"));
    let deferred = fixture
        .store
        .create_succession_attempt(&request)
        .expect("Wait is durable");
    let route = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "model_rung": {
            "provider": "anthropic",
            "model": "claude-sonnet",
            "effort": null
        }
    }))
    .expect("route document");
    let error = Connection::open(&fixture.path)
        .expect("raw connection")
        .execute(
            "UPDATE succession_attempts
             SET state = 'planned', successor_model_rung = ?1,
                 successor_model_rung_hash = ?2, successor_account_profile_id = ?3,
                 deferred_until = NULL, successor_planned_at = ?4,
                 revision = revision + 1, updated_at = ?4
             WHERE project_id = ?5 AND id = ?6",
            params![
                route.json(),
                route.hash().as_str(),
                fixture.successor_account.to_string(),
                "2026-09-04T09:00:00Z",
                fixture.project.to_string(),
                deferred.request.id.to_string(),
            ],
        )
        .expect_err("route-only SQL cannot evade the deferred authority trigger");
    assert!(error.to_string().contains("deferred succession authority"));
}

#[test]
fn succession_attempts_round_trip_only_as_non_executable_lineage() {
    let fixture = fixture();
    let attempt = fixture
        .store
        .create_succession_attempt(&new_attempt(&fixture, "succession:export"))
        .expect("attempt is durable");
    let export = export_realm(&fixture.store, at("2026-09-04T08:03:00Z"))
        .expect("succession evidence exports");
    assert_eq!(export.records.succession_attempts.len(), 1);
    assert_eq!(
        export.records.succession_attempts[0]
            .successor_account_profile_id
            .as_deref(),
        Some(fixture.successor_account.to_string().as_str())
    );
    let parsed = KontorExportV1::parse(&export.canonical_bytes().expect("canonical export"))
        .expect("succession export parses");

    let destination_home = TempDir::new().expect("destination directory");
    let destination =
        SqliteStore::open(&destination_home.path().join("kontor.db")).expect("destination store");
    let destination_project = ProjectId::generate();
    destination
        .create_project(&NewProject {
            id: destination_project,
            name: ExternalName::parse("Succession lineage destination").expect("name"),
            root_path: ExternalName::parse("/tmp/succession-lineage").expect("path"),
            created_at: at("2026-09-04T08:04:00Z"),
        })
        .expect("destination project");
    let report = import_export(
        &destination,
        &parsed,
        &ImportPlan::redacted_import_into(destination_project),
        at("2026-09-04T08:05:00Z"),
    )
    .expect("succession export imports as lineage");
    assert!(
        destination
            .list_nonterminal_succession_attempts(10)
            .expect("destination attempts")
            .is_empty(),
        "foreign succession authority is never materialized"
    );
    let lineage = destination
        .imported_records(&report.import_id.as_hyphenated().to_string())
        .expect("import lineage");
    assert!(lineage.iter().any(|record| {
        record.record_kind == "succession_attempts"
            && record.source_identity == attempt.request.id.to_string()
            && record.disposition == "recorded"
    }));
}

#[test]
fn exact_quota_cursor_and_one_active_slot_are_durable_across_restart() {
    let fixture = fixture();
    let provenance = fixture
        .store
        .get_quota_observation_provenance(fixture.project, fixture.quota_provenance)
        .expect("provenance read")
        .expect("provenance exists");
    let expected_cursor = fixture
        .store
        .get_agent_run(fixture.project, fixture.predecessor)
        .expect("run read")
        .expect("run exists")
        .projection
        .last_cursor;
    assert_eq!(
        provenance.record.runtime_observation_cursor,
        expected_cursor
    );

    let request = new_attempt(&fixture, "succession:one");
    let stored = fixture
        .store
        .create_succession_attempt(&request)
        .expect("attempt is planned");
    assert_eq!(stored.state, SuccessionAttemptState::Planned);

    let replay = fixture
        .store
        .create_succession_attempt(&NewSuccessionAttempt {
            id: SuccessionAttemptId::generate(),
            ..new_attempt(&fixture, "succession:one")
        })
        .expect("same key and intent replay");
    assert_eq!(replay.request.id, stored.request.id);

    let occupied = fixture
        .store
        .create_succession_attempt(&new_attempt(&fixture, "succession:two"));
    assert!(
        occupied.is_err(),
        "one active attempt occupies one exact slot"
    );

    let path = fixture.path.clone();
    drop(fixture.store);
    let reopened = SqliteStore::open(&path).expect("store reopens");
    assert_eq!(
        reopened
            .list_nonterminal_succession_attempts(10)
            .expect("startup inventory")
            .len(),
        1
    );
    assert_eq!(
        reopened
            .list_due_succession_attempts(at("2026-09-04T08:03:00Z"), 10)
            .expect("due inventory")
            .len(),
        1
    );
}

#[test]
fn handoff_retirement_successor_readback_and_receipt_advance_once() {
    let fixture = fixture();
    let mut attempt = fixture
        .store
        .create_succession_attempt(&new_attempt(&fixture, "succession:complete"))
        .expect("attempt is planned");
    let handoff = SuccessionHandoff {
        schema_version: 1,
        attempt_id: attempt.request.id,
        predecessor_agent_run_id: fixture.predecessor,
        predecessor_runtime_binding_id: fixture.predecessor_binding,
        predecessor_native_identity: identity("predecessor"),
        outcome: SuccessionHandoffOutcome::Degraded {
            timeline: None,
            reason: SuccessionHandoffDegradedReason::TimelineUnavailable,
            input_redaction: redaction(SuccessionRedactionPass::Input),
            output_redaction: redaction(SuccessionRedactionPass::Output),
        },
        produced_at: at("2026-09-04T08:03:00Z"),
    };
    attempt = fixture
        .store
        .record_succession_handoff(&SuccessionHandoffRecord {
            project_id: fixture.project,
            attempt_id: attempt.request.id,
            expected_revision: attempt.revision,
            handoff,
            recorded_at: at("2026-09-04T08:03:00Z"),
        })
        .expect("handoff is durable");
    attempt = fixture
        .store
        .mark_succession_predecessor_retired(&SuccessionAttemptAdvance {
            project_id: fixture.project,
            attempt_id: attempt.request.id,
            expected_revision: attempt.revision,
            occurred_at: at("2026-09-04T08:04:00Z"),
        })
        .expect("retirement advances once");

    let successor = AgentRunId::generate();
    let successor_binding = RuntimeBindingId::generate();
    let connection = Connection::open(&fixture.path).expect("raw connection");
    connection
        .execute(
            "INSERT INTO agent_runs
                (id, project_id, team_run_id, parent_agent_run_id, role_key,
                 account_profile_id, lifecycle, desired_state, observed_state,
                 derived_state, revision, created_at)
             VALUES (?1, ?2, ?3, ?4, 'aud', ?5, 'running', 'run_requested', 'unknown',
                     'pending_confirmation', 1, '2026-09-04T08:05:00Z')",
            params![
                successor.to_string(),
                fixture.project.to_string(),
                fixture.team.to_string(),
                fixture.predecessor.to_string(),
                fixture.successor_account.to_string(),
            ],
        )
        .expect("successor run inserts");
    connection
        .execute(
            "INSERT INTO runtime_bindings
                (id, project_id, agent_run_id, runtime_kind, host, generation, native_id, bound_at)
             VALUES (?1, ?2, ?3, 'generic.runtime', 'host-1', 1, 'successor',
                     '2026-09-04T08:05:00Z')",
            params![
                successor_binding.to_string(),
                fixture.project.to_string(),
                successor.to_string(),
            ],
        )
        .expect("successor binding inserts");
    let successor_projection = fixture
        .store
        .record_observation(&NewObservation {
            event: NewRuntimeEvent {
                project_id: fixture.project,
                agent_run_id: successor,
                identity: identity("successor"),
                native_event_id: Some(ExternalId::parse("successor-1").expect("event id")),
                native_sequence: 1,
                payload: document("successor-running"),
                observed_at: at("2026-09-04T08:06:00Z"),
            },
            observed: ObservedRunState::Running,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            expected_revision: AggregateRevision::INITIAL,
            quota_state: None,
        })
        .expect("successor readback reduces");
    let observation = SuccessionSuccessorObservation {
        agent_run_id: successor,
        runtime_binding_id: successor_binding,
        native_identity: identity("successor"),
        runtime_observation_cursor: successor_projection.last_cursor.expect("successor cursor"),
        observed_at: at("2026-09-04T08:06:00Z"),
    };
    attempt = fixture
        .store
        .mark_succession_successor_observed(&SuccessionSuccessorRecord {
            project_id: fixture.project,
            attempt_id: attempt.request.id,
            expected_revision: attempt.revision,
            observation: observation.clone(),
        })
        .expect("successor observation advances once");

    let handoff_hash = attempt.handoff_hash.clone().expect("handoff hash");
    let receipt = SuccessionReceipt {
        schema_version: 1,
        id: SuccessionReceiptId::generate(),
        attempt_id: attempt.request.id,
        project_id: fixture.project,
        task_id: fixture.task,
        team_run_id: fixture.team,
        role: RoleKey::parse("aud").expect("role"),
        predecessor_agent_run_id: fixture.predecessor,
        predecessor_runtime_binding_id: fixture.predecessor_binding,
        predecessor_native_identity: identity("predecessor"),
        successor_agent_run_id: successor,
        successor_runtime_binding_id: successor_binding,
        successor_native_identity: identity("successor"),
        successor_runtime_observation_cursor: observation.runtime_observation_cursor,
        authorizing_runtime_observation_cursor: attempt.request.runtime_observation_cursor,
        quota_provenance_id: fixture.quota_provenance,
        quota_state_revision: AggregateRevision::INITIAL,
        quota_evidence_hash: fixture.quota_hash.clone(),
        handoff_hash,
        summary_hash: None,
        intent_hash: attempt.request.intent_hash.clone(),
        confirmed_at: at("2026-09-04T08:07:00Z"),
    };
    let stored_receipt = fixture
        .store
        .confirm_succession(&SuccessionConfirmation {
            expected_revision: attempt.revision,
            receipt: receipt.clone(),
        })
        .expect("confirmation commits receipt atomically");
    assert_eq!(stored_receipt, receipt);
    assert_eq!(
        fixture
            .store
            .confirm_succession(&SuccessionConfirmation {
                expected_revision: attempt.revision,
                receipt: receipt.clone(),
            })
            .expect("receipt replay is idempotent"),
        receipt
    );
    assert!(
        fixture
            .store
            .list_nonterminal_succession_attempts(10)
            .expect("startup inventory")
            .is_empty()
    );
    assert_eq!(
        fixture
            .store
            .succession_receipt_for_attempt(fixture.project, attempt.request.id)
            .expect("receipt readback"),
        Some(receipt)
    );
}

#[test]
fn a_typed_refusal_frees_the_slot_without_a_handoff_unavailable_escape() {
    let fixture = fixture();
    let attempt = fixture
        .store
        .create_succession_attempt(&new_attempt(&fixture, "succession:refuse"))
        .expect("attempt is planned");
    let refused = fixture
        .store
        .refuse_succession(&SuccessionRefusal {
            project_id: fixture.project,
            attempt_id: attempt.request.id,
            expected_revision: attempt.revision,
            reason: SuccessionRefusalReason::QuotaNoLongerBlocking,
            refused_at: at("2026-09-04T08:03:00Z"),
        })
        .expect("typed refusal is terminal");
    assert_eq!(refused.state, SuccessionAttemptState::Refused);
    fixture
        .store
        .create_succession_attempt(&new_attempt(&fixture, "succession:after-refusal"))
        .expect("terminal refusal frees the exact slot");
}
