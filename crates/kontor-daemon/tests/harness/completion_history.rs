//! Completion-only historical dispositions never become delivery evidence.
use super::*;

struct HistoricalFixture {
    world: World,
    seed: Bootstrapped,
    request: serde_json::Value,
}

impl HistoricalFixture {
    fn uri(&self) -> String {
        format!(
            "/v1/projects/{}/epics/{}/completion:advance",
            self.seed.project, self.seed.epic
        )
    }
    fn completion(&self) -> StoredEpicCompletion {
        self.world
            .daemon
            .state()
            .with_store(|store| {
                store.get_epic_completion(
                    ProjectId::parse(&self.seed.project).unwrap(),
                    MiniProjectId::parse(&self.seed.epic).unwrap(),
                )
            })
            .unwrap()
            .unwrap()
    }
    fn sql(&self, sql: &str) {
        let connection = rusqlite::Connection::open(
            self.world
                .directory
                .path()
                .join(kontor_daemon::DATABASE_FILE),
        )
        .unwrap();
        // Restore exact historical/corrupt snapshots only in this isolated test
        // database, then restore the immutable-row guards before exercising API
        // or store behavior. Production has no raw SQL recovery surface.
        let triggers = connection.prepare("SELECT name, sql FROM sqlite_master WHERE type='trigger' AND name IN ('tasks_terminal_immutable','tasks_reopen_changes_only_the_state','task_gate_evaluations_no_update')")
            .unwrap().query_map([], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))).unwrap()
            .collect::<Result<Vec<_>,_>>().unwrap();
        connection.execute_batch("BEGIN IMMEDIATE").unwrap();
        for (name, _) in &triggers {
            connection
                .execute_batch(&format!("DROP TRIGGER {name}"))
                .unwrap();
        }
        connection.execute_batch(sql).unwrap();
        for (_, definition) in &triggers {
            connection.execute_batch(definition).unwrap();
        }
        connection.execute_batch("COMMIT").unwrap();
    }
    async fn offer(&self, request: &serde_json::Value, tier: &str, key: &str) -> Answer {
        Call::post(self.uri(), request)
            .signed_as(&self.world, tier)
            .with_key(key)
            .send(&self.world)
            .await
    }
    fn no_receipt(&self, key: &str) {
        assert!(
            self.world
                .daemon
                .state()
                .with_store(|store| store.get_receipt_by_key(&IdempotencyKey::parse(key).unwrap()))
                .unwrap()
                .is_none()
        );
    }
    fn unchanged_delivery(&self) {
        let project = ProjectId::parse(&self.seed.project).unwrap();
        let task = TaskId::parse(&self.seed.task).unwrap();
        self.world.daemon.state().with_store(|store| {
            assert!(
                store
                    .list_task_artifact_keys(project, task)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                store
                    .list_current_task_closure_artifact_keys(project, task)
                    .unwrap()
                    .is_empty()
            );
            let task = store.get_task(project, task).unwrap().unwrap();
            assert_eq!(task.state, kontor_core::state::TaskState::Done);
            assert_eq!(
                task.revision.get(),
                self.request["evidence"]["tasks"][0]["expected_task_revision"]
                    .as_u64()
                    .unwrap()
            );
        });
    }
}

async fn historical_fixture(slug: &'static str) -> HistoricalFixture {
    let (world, seed, tpm) = reopened_completion_fixture(slug).await;
    let workflow = active_workflow(&world, &seed);
    assert!(!workflow.snapshot.definition.gates.is_empty());
    let project = ProjectId::parse(&seed.project).unwrap();
    let task = TaskId::parse(&seed.task).unwrap();
    // Restore a legacy Done task with actual passed gates but no surviving
    // producer ledger or native closure certificate. This is test history,
    // deliberately not an authorized path for closing a live task.
    for gate in &workflow.snapshot.definition.gates {
        world
            .daemon
            .state()
            .with_store(|store| {
                store.append_gate_evaluation(&NewGateEvaluation {
                    project_id: project,
                    workflow_id: workflow.id,
                    gate: gate.id.clone(),
                    verdict: GateVerdict::Passed,
                    evaluator_role: gate.evaluator_roles[0].clone(),
                    evaluator_account: AccountProfileId::parse(&seed.account).unwrap(),
                    evidence: gate.required_evidence.clone(),
                    agent_run_id: None,
                    session_evidence: None,
                    reviewer_principal: None,
                    policy_evaluation_id: None,
                    recorded_at: kontor_api::now(),
                })
            })
            .unwrap();
    }
    let connection =
        rusqlite::Connection::open(world.directory.path().join(kontor_daemon::DATABASE_FILE))
            .unwrap();
    connection
        .execute(
            "UPDATE tasks SET state='done', imported_state=NULL, revision=revision+1 WHERE id=?1",
            [&seed.task],
        )
        .unwrap();
    drop(connection);
    let restored_task = world
        .daemon
        .state()
        .with_store(|store| store.get_task(project, task))
        .unwrap()
        .unwrap();
    assert_eq!(
        restored_task.mini_project_id,
        Some(MiniProjectId::parse(&seed.epic).unwrap())
    );
    assert!(restored_task.imported_state.is_none());
    let task_revision = world
        .daemon
        .state()
        .with_store(|store| store.get_task(project, task))
        .unwrap()
        .unwrap()
        .revision;
    let keys = workflow
        .snapshot
        .definition
        .artifacts
        .iter()
        .map(|artifact| artifact.key.as_str())
        .collect::<Vec<_>>();
    assert!(
        !keys.is_empty(),
        "fixture must have an actual missing evidence obligation"
    );
    let compiled =
        kontor_scheduler::compile(kontor_scheduler::operational_default().unwrap()).unwrap();
    let state = kontor_scheduler::start(
        &compiled,
        tpm,
        vec![kontor_policy::TicketRequirement {
            task_id: task,
            goals: workflow
                .snapshot
                .definition
                .gates
                .iter()
                .map(|gate| name(gate.id.as_str()))
                .collect(),
            evidence: keys.iter().map(|key| name(key)).collect(),
        }],
    )
    .unwrap();
    world
        .daemon
        .state()
        .with_store(|store| {
            store.create_epic_completion(&StoredEpicCompletion {
                project_id: project,
                mini_project_id: MiniProjectId::parse(&seed.epic).unwrap(),
                profile_id: compiled.profile.id.clone(),
                profile_version: compiled.profile.version,
                definition_hash: compiled.definition_hash.clone(),
                state: serde_json::to_value(&state).unwrap(),
                revision: state.revision,
                updated_at: kontor_api::now(),
            })
        })
        .unwrap();
    let (commit, sha256) = artifact_git_fixture(&world, &seed);
    let request = serde_json::json!({"expected_revision":state.revision,"evidence":{
        "phase":"historical_evidence", "disposition":"provenance_unavailable", "reason":"Historical producer provenance is unavailable; independent final review must verify current-source obligations.",
        "packet":{"commit":commit,"path":"evidence.md","sha256":sha256},
        "tasks":[{"task_id":task,"expected_task_revision":task_revision,
            "workflow_id":workflow.id,"expected_workflow_revision":workflow.revision,
            "workflow_hash":workflow.snapshot.definition_hash,"artifact_keys":keys}]
    }});
    HistoricalFixture {
        world,
        seed,
        request,
    }
}

#[tokio::test]
async fn historical_completion_requires_exact_admin_disposition_and_preserves_raw_gaps() {
    let fixture = historical_fixture("historical-completion-exact").await;
    let before = fixture.completion();
    let normal = fixture
        .offer(
            &serde_json::json!({"expected_revision":before.revision}),
            "operator",
            "historical-normal-missing",
        )
        .await;
    assert_ne!(
        normal.status, 200,
        "ordinary completion must still require evidence"
    );
    fixture.no_receipt("historical-normal-missing");
    for tier in ["observer", "operator"] {
        let key = format!("historical-denied-{tier}");
        let denied = fixture.offer(&fixture.request, tier, &key).await;
        assert_eq!(denied.status, 403, "{}", denied.body);
        fixture.no_receipt(&key);
    }
    let pin = &fixture.request["evidence"]["tasks"][0];
    let cases = [
        (
            "wrong-task",
            "/evidence/tasks/0/task_id",
            serde_json::json!(TaskId::generate()),
        ),
        (
            "stale-task",
            "/evidence/tasks/0/expected_task_revision",
            serde_json::json!(99),
        ),
        (
            "wrong-workflow",
            "/evidence/tasks/0/workflow_id",
            serde_json::json!(TaskWorkflowId::generate()),
        ),
        (
            "stale-workflow",
            "/evidence/tasks/0/expected_workflow_revision",
            serde_json::json!(99),
        ),
        (
            "wrong-workflow-hash",
            "/evidence/tasks/0/workflow_hash",
            serde_json::json!("a".repeat(64)),
        ),
        (
            "arbitrary-key",
            "/evidence/tasks/0/artifact_keys",
            serde_json::json!(["not-a-declared-artifact"]),
        ),
        (
            "duplicate-key",
            "/evidence/tasks/0/artifact_keys",
            serde_json::json!([pin["artifact_keys"][0], pin["artifact_keys"][0]]),
        ),
        (
            "wrong-packet",
            "/evidence/packet/sha256",
            serde_json::json!("b".repeat(64)),
        ),
        (
            "uncommitted-packet",
            "/evidence/packet/path",
            serde_json::json!("missing.md"),
        ),
        (
            "path-traversal",
            "/evidence/packet/path",
            serde_json::json!("../evidence.md"),
        ),
        ("no-reason", "/evidence/reason", serde_json::json!(" ")),
    ];
    for (case, pointer, value) in cases {
        let mut request = fixture.request.clone();
        *request.pointer_mut(pointer).unwrap() = value;
        let key = format!("historical-refuse-{case}");
        let refused = fixture.offer(&request, "admin", &key).await;
        assert!(
            [400, 409].contains(&refused.status.as_u16()),
            "{case}: {} {}",
            refused.status,
            refused.body
        );
        fixture.no_receipt(&key);
        assert_eq!(fixture.completion(), before, "{case} must write nothing");
    }
    for state in ["ready", "blocked"] {
        fixture.sql(&format!(
            "UPDATE tasks SET state='{state}' WHERE id='{}'",
            fixture.seed.task
        ));
        let key = format!("historical-non-done-{state}");
        let refused = fixture.offer(&fixture.request, "admin", &key).await;
        assert_eq!(refused.status, 409, "{}", refused.body);
        fixture.no_receipt(&key);
        fixture.sql(&format!(
            "UPDATE tasks SET state='done' WHERE id='{}'",
            fixture.seed.task
        ));
    }
    // A wrong owning epic cannot be hidden behind a correct task/workflow id.
    fixture.sql(&format!(
        "UPDATE tasks SET mini_project_id=NULL WHERE id='{}'",
        fixture.seed.task
    ));
    let refused = fixture
        .offer(&fixture.request, "admin", "historical-wrong-subject")
        .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    fixture.no_receipt("historical-wrong-subject");
    fixture.sql(&format!(
        "UPDATE tasks SET mini_project_id='{}' WHERE id='{}'",
        fixture.seed.epic, fixture.seed.task
    ));
    let workflow = active_workflow(&fixture.world, &fixture.seed);
    for verdict in ["rejected", "waived"] {
        fixture.sql(&format!(
            "UPDATE task_gate_evaluations SET verdict='{verdict}' WHERE workflow_id='{}'",
            workflow.id
        ));
        let key = format!("historical-failed-gate-{verdict}");
        let refused = fixture.offer(&fixture.request, "admin", &key).await;
        assert_eq!(refused.status, 409, "{}", refused.body);
        fixture.no_receipt(&key);
        fixture.sql(&format!(
            "UPDATE task_gate_evaluations SET verdict='passed' WHERE workflow_id='{}'",
            workflow.id
        ));
    }
    fixture.sql(&format!(
        "UPDATE tasks SET imported_state='completed' WHERE id='{}'",
        fixture.seed.task
    ));
    let imported = fixture
        .offer(&fixture.request, "admin", "historical-imported-done")
        .await;
    assert_eq!(imported.status, 409, "{}", imported.body);
    fixture.no_receipt("historical-imported-done");
    fixture.sql(&format!(
        "UPDATE tasks SET imported_state=NULL WHERE id='{}'",
        fixture.seed.task
    ));
    assert_eq!(fixture.completion(), before);
    let accepted = fixture
        .offer(&fixture.request, "admin", "historical-accept")
        .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
    assert_eq!(accepted.json()["state"]["phase"]["phase"], "integration");
    let after = fixture.completion();
    assert_eq!(
        after.state["historical_evidence"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        after.state["historical_evidence"][0]["packet"],
        "Verified fixture implementation and independent review evidence.\n"
    );
    assert!(
        after.state["ticket_evidence"][0]["evidence"]
            .as_array()
            .unwrap()
            .is_empty(),
        "the actual missing producer evidence is retained"
    );
    fixture.unchanged_delivery();
    let replay = fixture
        .offer(&fixture.request, "admin", "historical-accept")
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["receipt"]["applied"], "unchanged");
    assert_eq!(fixture.completion(), after);
    let mut changed = fixture.request.clone();
    changed["evidence"]["reason"] = serde_json::json!("Different disposition under the old key");
    let refused = fixture.offer(&changed, "admin", "historical-accept").await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(fixture.completion(), after);
}

#[tokio::test]
async fn historical_completion_is_atomic_append_only_and_invalidated_by_task_or_workflow_drift() {
    let fixture = historical_fixture("historical-completion-drift").await;
    let accepted = fixture
        .offer(&fixture.request, "admin", "historical-drift-accept")
        .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
    let stored = fixture.completion();
    let project = ProjectId::parse(&fixture.seed.project).unwrap();
    let epic = MiniProjectId::parse(&fixture.seed.epic).unwrap();
    let command = |key: &str| NewLocalCommand {
        project_id: project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse(key).unwrap(),
        kind: CommandKind::AdvanceCompletion,
        target: kontor_core::receipt::AggregateRef::MiniProject {
            mini_project_id: epic,
        },
        target_revision: AggregateRevision::INITIAL,
        intent: CanonicalDocument::from_value(
            &serde_json::json!({"schema_version":1,"operation":"atomic-history-test","key":key}),
        )
        .unwrap(),
        created_at: kontor_api::now(),
    };
    let mut proposed = stored.clone();
    proposed.revision = AggregateRevision::parse(stored.revision.get() + 1).unwrap();
    proposed.state["revision"] = serde_json::json!(proposed.revision);
    for (case, state) in [
        ("remove", serde_json::json!([])),
        ("rewrite", {
            let mut records = stored.state["historical_evidence"].clone();
            records[0]["reason"] = serde_json::json!("rewritten history");
            records
        }),
    ] {
        let mut changed = proposed.clone();
        changed.state["historical_evidence"] = state;
        let key = format!("historical-atomic-{case}");
        assert!(
            fixture
                .world
                .daemon
                .state()
                .with_store(|store| store.commit_epic_completion(
                    &changed,
                    kontor_core::repository::CompletionWrite::Advance(stored.revision),
                    &[],
                    &command(&key)
                ))
                .is_err()
        );
        fixture.no_receipt(&key);
        assert_eq!(fixture.completion(), stored);
    }
    // Direct commit simulates a mutation after daemon validation but before the
    // commit's transaction, proving the persistence fence independently.
    for (case, table, column, id) in [
        ("task", "tasks", "revision", fixture.seed.task.clone()),
        (
            "workflow",
            "task_workflows",
            "revision",
            fixture.request["evidence"]["tasks"][0]["workflow_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        ),
    ] {
        fixture.sql(&format!(
            "UPDATE {table} SET {column}={column}+1 WHERE id='{id}'"
        ));
        let key = format!("historical-atomic-drift-{case}");
        assert!(
            fixture
                .world
                .daemon
                .state()
                .with_store(|store| store.commit_epic_completion(
                    &proposed,
                    kontor_core::repository::CompletionWrite::Advance(stored.revision),
                    &[],
                    &command(&key)
                ))
                .is_err(),
            "atomic {case} drift fence"
        );
        fixture.no_receipt(&key);
        assert_eq!(fixture.completion(), stored);
        let read = Call::get(fixture.uri().replace(":advance", ""))
            .signed_as(&fixture.world, "observer")
            .send(&fixture.world)
            .await;
        assert_eq!(read.status, 200, "{}", read.body);
        assert_eq!(
            read.json()["historical_evidence"][0]["currently_applicable"],
            false
        );
        let refused=fixture.offer(&serde_json::json!({"expected_revision":stored.revision,"evidence":{"phase":"integration","repositories":[{"repository":"fixture","pull_request":"PR-1","module_revision":"source"}]}}),"operator",&format!("historical-next-drift-{case}")).await;
        assert_eq!(refused.status, 409, "{}", refused.body);
        fixture.sql(&format!(
            "UPDATE {table} SET {column}={column}-1 WHERE id='{id}'"
        ));
    }
    fixture.sql(&format!(
        "UPDATE tasks SET state='ready',revision=revision+1 WHERE id='{}'",
        fixture.seed.task
    ));
    // Automatic completion reopening retains the entire disposition but its era
    // cannot satisfy the returned delivery work.
    fixture.world.daemon.reconcile().await;
    let reopened = fixture.completion();
    assert_eq!(reopened.state["phase"], "Tickets");
    assert_eq!(reopened.state["generation"], 2);
    assert_eq!(
        reopened.state["historical_evidence"],
        stored.state["historical_evidence"]
    );
    let replay = fixture
        .offer(&fixture.request, "admin", "historical-drift-accept")
        .await;
    assert_eq!(
        replay.status, 200,
        "exact replay does not repeat the historical mutation: {}",
        replay.body
    );
    assert_eq!(fixture.completion(), reopened);
}

#[tokio::test]
async fn historical_completion_packet_is_visible_to_the_existing_independent_committee() {
    let fixture = historical_fixture("historical-completion-committee").await;
    let accepted = fixture
        .offer(&fixture.request, "admin", "historical-committee-accept")
        .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
    let integrated=fixture.offer(&serde_json::json!({"expected_revision":accepted.json()["state"]["revision"],
        "evidence":{"phase":"integration","repositories":[{"repository":"fixture","pull_request":"PR-fixture","module_revision":fixture.request["evidence"]["packet"]["commit"]}]}}),"operator","historical-committee-integration").await;
    assert_eq!(integrated.status, 200, "{}", integrated.body);
    assert_eq!(integrated.json()["state"]["phase"]["phase"], "verdict");
    let stored = fixture.completion();
    let project = &fixture.seed.project;
    let epic = &fixture.seed.epic;
    confirm_test_epic_identity(&fixture.world, project, epic);
    prepare_fake_provider_headroom(&fixture.world, project).await;
    let invoked=Call::post(format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),&serde_json::json!({
        "profile":{"id":COMMITTEE_PRESET,"version":4},"topic":"Current-source verification",
        "question":"Independently assess actual obligations and disclosed unavailable historical provenance; the operator disposition is not an acceptance pass.",
        "caller_seat_binding_id":stored.state["tpm_seat_id"],"expected_revision":1
    })).signed_as(&fixture.world,"operator").with_key("historical-final-review").send(&fixture.world).await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let committee = invoked.json()["committee_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let invoked_body = invoked.json();
    let reviewer = invoked_body["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|seat| {
            seat["role_slot_id"]
                .as_str()
                .unwrap()
                .starts_with("reviewer")
        })
        .unwrap()["seat_binding_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let evidence = Call::get(format!("/v1/projects/{project}/committee-runs/{committee}"))
        .with_token(
            fixture
                .world
                .daemon
                .state()
                .credentials()
                .consultation_seat_credential(SeatBindingId::parse(&reviewer).unwrap()),
        )
        .send(&fixture.world)
        .await;
    assert_eq!(evidence.status, 200, "{}", evidence.body);
    let body = evidence.json();
    let history = &body["subject_evidence"]["body"]["completion"]["historical_evidence"][0];
    assert_eq!(history["disposition"], "provenance_unavailable");
    assert_eq!(
        history["packet"],
        stored.state["historical_evidence"][0]["packet"]
    );
    assert_eq!(
        history["sha256"],
        fixture.request["evidence"]["packet"]["sha256"]
    );
    assert_eq!(history["currently_applicable"], true);
    assert!(
        body["subject_evidence"]["body"]["tasks"][0]["producer_artifacts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fixture.unchanged_delivery();
}

#[tokio::test]
async fn historical_completion_accepts_partial_exact_gaps_but_refuses_the_same_gap_twice() {
    let fixture = historical_fixture("historical-completion-partial").await;
    let mut partial = fixture.request.clone();
    let keys = partial["evidence"]["tasks"][0]["artifact_keys"]
        .as_array()
        .unwrap()
        .clone();
    assert!(keys.len() > 1);
    partial["evidence"]["tasks"][0]["artifact_keys"] = serde_json::json!([keys[0]]);
    let first = fixture
        .offer(&partial, "admin", "historical-partial-first")
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["state"]["phase"]["phase"], "ticket_gate");
    partial["expected_revision"] = first.json()["state"]["revision"].clone();
    let before = fixture.completion();
    let repeated = fixture
        .offer(&partial, "admin", "historical-partial-duplicate")
        .await;
    assert_eq!(repeated.status, 400, "{}", repeated.body);
    assert_eq!(fixture.completion(), before);
    fixture.no_receipt("historical-partial-duplicate");
    partial["evidence"]["tasks"][0]["artifact_keys"] = serde_json::json!(&keys[1..]);
    let second = fixture
        .offer(&partial, "admin", "historical-partial-rest")
        .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(second.json()["state"]["phase"]["phase"], "integration");
    let after = fixture.completion();
    assert_eq!(
        after.state["historical_evidence"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        after.state["historical_evidence"][0],
        before.state["historical_evidence"][0]
    );
    fixture.unchanged_delivery();
}

#[tokio::test]
async fn historical_completion_partial_drift_reopens_the_era_without_stranding_the_ticket_gate() {
    let fixture = historical_fixture("historical-completion-partial-drift").await;
    let mut partial = fixture.request.clone();
    let keys = partial["evidence"]["tasks"][0]["artifact_keys"]
        .as_array()
        .unwrap()
        .clone();
    assert!(keys.len() > 1);
    partial["evidence"]["tasks"][0]["artifact_keys"] = serde_json::json!([keys[0]]);
    let first = fixture
        .offer(&partial, "admin", "historical-partial-drift-first")
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let before = fixture.completion();
    assert_eq!(before.state["phase"], "Tickets");
    fixture.sql(&format!(
        "UPDATE tasks SET revision=revision+1 WHERE id='{}'",
        fixture.seed.task
    ));
    fixture.world.daemon.reconcile().await;
    let reopened = fixture.completion();
    assert_eq!(reopened.state["phase"], "Tickets");
    assert_eq!(
        reopened.state["generation"], 2,
        "the invalid partial record must not trap Tickets in its old era"
    );
    assert_eq!(
        reopened.state["historical_evidence"],
        before.state["historical_evidence"]
    );
    let mut fresh = fixture.request.clone();
    fresh["expected_revision"] = serde_json::json!(reopened.revision);
    fresh["evidence"]["tasks"][0]["expected_task_revision"] = serde_json::json!(
        fresh["evidence"]["tasks"][0]["expected_task_revision"]
            .as_u64()
            .unwrap()
            + 1
    );
    let accepted = fixture
        .offer(&fresh, "admin", "historical-partial-drift-new-era")
        .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
    assert_eq!(accepted.json()["state"]["phase"]["phase"], "integration");
    assert_eq!(
        accepted.json()["state"]["historical_evidence"][0]["currently_applicable"],
        false
    );
    assert_eq!(
        accepted.json()["state"]["historical_evidence"][1]["currently_applicable"],
        true
    );
}
