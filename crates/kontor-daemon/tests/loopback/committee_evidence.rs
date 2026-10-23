use super::*;

/// This test reviews evidence, but every native admission still needs a real
/// fixture account and a fresh immutable observation through the production writer.
async fn refresh_committee_fixture_quota(world: &World, project: &str) {
    prepare_fake_provider_headroom(world, project).await;
    let project_id = ProjectId::parse(project).unwrap();
    let accounts = world
        .daemon
        .state()
        .with_store(|store| store.list_account_profiles(project_id).unwrap());
    for account in accounts {
        for provider in kontor_accounts::selectable_providers(&account).unwrap() {
            let family = provider.split('-').next().unwrap();
            kontor_daemon::usage::record_exact(
                &world.daemon.state(),
                &account,
                &provider,
                &UsageReading {
                    provider: family.to_owned(),
                    limit_reached: false,
                    windows: Vec::new(),
                    credits_exhausted: false,
                },
                None,
                None,
            )
            .expect("a fresh scripted provider report and immutable heartbeat");
        }
    }
}

#[tokio::test]
async fn scoped_committee_reads_actual_subject_evidence_and_verified_report_without_peer_leakage() {
    let (world, seed, tpm) = reopened_native_completion_fixture("committee-subject-evidence").await;
    let project = ProjectId::parse(&seed.project).unwrap();
    let epic = MiniProjectId::parse(&seed.epic).unwrap();
    let task = TaskId::parse(&seed.task).unwrap();
    let sibling = TaskId::generate();
    world.daemon.state().with_store(|store| {
        store
            .create_task(&NewTask {
                id: sibling,
                project_id: project,
                mini_project_id: Some(epic),
                title: ExternalName::parse("Sibling outside ticket review").unwrap(),
                module: None,
                state: kontor_core::state::TaskState::Todo,
                created_at: kontor_api::now(),
            })
            .unwrap();
    });
    // The native composer already confirmed the original epic/task before the sibling existed.
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store.confirmed_jira_epic_key(project, epic))
            .unwrap()
            .is_some()
    );
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store.confirmed_jira_task_key(project, task))
            .unwrap()
            .is_some()
    );
    let (commit, hash) = artifact_git_fixture(&world, &seed);
    let root = world
        .daemon
        .state()
        .with_store(|store| store.get_project(project).unwrap().unwrap().root_path);
    let git_dir = std::fs::canonicalize(std::path::Path::new(root.as_str()).join(".git")).unwrap();
    let artifact_id = kontor_policy::model::ArtifactEvidenceId::generate();
    let bad_artifact = kontor_policy::model::ArtifactEvidenceId::generate();
    world.daemon.state().with_store(|store| {
        let workflow = store.get_active_task_workflow(project, task).unwrap().unwrap();
        let gate = &workflow.snapshot.definition.gates[0];
        store.append_gate_evaluation(&NewGateEvaluation {
            project_id: project, workflow_id: workflow.id, gate: gate.id.clone(),
            verdict: GateVerdict::Rejected, evaluator_role: gate.evaluator_roles.first().unwrap().clone(),
            evaluator_account: AccountProfileId::parse(&seed.account).unwrap(), evidence: Vec::new(),
            agent_run_id: None, session_evidence: None, reviewer_principal: None,
            policy_evaluation_id: None, recorded_at: kontor_api::now(),
        }).unwrap();
        let key = workflow.snapshot.definition.artifacts[0].key.clone();
        for (id, sha256) in [(artifact_id, hash.clone()), (bad_artifact, ContentHash::of(b"wrong bytes").as_str().to_owned())] {
            store.record_artifact_evidence(&kontor_store::NewArtifactEvidence {
                id, binding: kontor_store::EvaluationBinding { project_id: project, task_id: task, workflow_id: workflow.id, team_run_id: None, agent_run_id: None },
                key: key.clone(), locator: CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"kind":"git_blob", "git_dir":git_dir, "commit":commit, "path":"evidence.md", "sha256":sha256})).unwrap(),
                producer_role: gate.evaluator_roles.first().unwrap().clone(), producer_account: Some(AccountProfileId::parse(&seed.account).unwrap()), recorded_at: kontor_api::now(),
            }).unwrap();
        }
    });
    // The working copy is intentionally absent; the immutable common Git directory is sufficient.
    std::fs::remove_file(std::path::Path::new(root.as_str()).join("evidence.md")).unwrap();
    let compiled =
        kontor_scheduler::compile(kontor_scheduler::operational_default().unwrap()).unwrap();
    let mut completion = kontor_scheduler::start(
        &compiled,
        tpm,
        vec![kontor_policy::completion::TicketRequirement {
            task_id: task,
            goals: [ExternalName::parse("review-goal").unwrap()].into(),
            evidence: [ExternalName::parse("review-report").unwrap()].into(),
        }],
    )
    .unwrap();
    completion.phase = kontor_scheduler::CompletionPhase::Integration;
    completion
        .integrations
        .push(kontor_scheduler::IntegrationRecord {
            receipt: ContentHash::of(b"real integration fixture"),
            repositories: vec![kontor_scheduler::RepositoryOutcome {
                repository: ExternalName::parse("module-a").unwrap(),
                pull_request: ExternalName::parse("https://github.com/example/module/pull/7")
                    .unwrap(),
                module_revision: ExternalName::parse("0123456789abcdef").unwrap(),
                root_pointer_revision: Some(ExternalName::parse("fedcba9876543210").unwrap()),
            }],
            decided_in: None,
        });
    world.daemon.state().with_store(|store| {
        store
            .create_epic_completion(&StoredEpicCompletion {
                project_id: project,
                mini_project_id: epic,
                profile_id: compiled.profile.id.clone(),
                profile_version: compiled.profile.version,
                definition_hash: compiled.definition_hash.clone(),
                state: serde_json::to_value(&completion).unwrap(),
                revision: completion.revision,
                updated_at: kontor_api::now(),
            })
            .unwrap()
    });
    let epic_read = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let invoke = |task_id: Option<TaskId>, topic: &str| {
        serde_json::json!({
            "profile":{"id":"01991c00-0000-7000-8000-000000000001","version":4},
            "topic":topic,"question":"Review the actual subject records and report bytes", "caller_seat_binding_id":tpm,
            "expected_revision":epic_read.json()["revision"],"task_id":task_id,
        })
    };
    refresh_committee_fixture_quota(&world, &seed.project).await;
    activate_through_the_registry(
        &world,
        &formal_review_fleet_yaml(),
        "subject-fixture-policy",
    )
    .await;
    let invoked = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &invoke(None, "Subject evidence"),
    )
    .signed_as(&world, "operator")
    .with_key("subject-evidence-invoke")
    .send(&world)
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.json()["committee_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reviewers: Vec<_> = invoked.json()["seats"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|seat| seat["committee_role"] == "reviewer")
        .cloned()
        .collect();
    let credential = |index: usize| {
        world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential_for_generation(
                SeatBindingId::parse(reviewers[index]["seat_binding_id"].as_str().unwrap())
                    .unwrap(),
                reviewers[index]["occupancy_generation"].as_u64().unwrap(),
            )
    };
    let read = Call::get(format!("/v1/projects/{project}/committee-runs/{run}"))
        .with_token(credential(0))
        .send(&world)
        .await;
    assert_eq!(read.status, 200, "{}", read.body);
    let evidence = read.json()["subject_evidence"].clone();
    assert_eq!(evidence["body"]["tasks"].as_array().unwrap().len(), 2);
    let task_evidence = evidence["body"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["task"]["task_id"] == seed.task)
        .unwrap();
    assert!(
        !task_evidence["task"]["gates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !task_evidence["task"]["required_artifacts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !task_evidence["work_profile"]["gates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(task_evidence["gate_evaluations"][0]["verdict"], "rejected");
    assert_eq!(
        task_evidence["producer_artifacts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let inline = task_evidence["producer_artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["evidence_id"] == artifact_id.to_string())
        .unwrap();
    assert_eq!(inline["content"]["status"], "verified");
    assert_eq!(
        inline["content"]["text"],
        "Verified fixture implementation and independent review evidence.\n"
    );
    assert!(
        task_evidence["native_closure_artifact_keys"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        evidence["body"]["completion"]["ticket_requirements"][0]["goals"][0],
        "review-goal"
    );
    assert_eq!(
        evidence["body"]["completion"]["integrations"][0]["repositories"][0]["module_revision"],
        "0123456789abcdef"
    );
    assert_eq!(
        evidence["body"]["completion"]["integrations"][0]["repositories"][0]["root_pointer_revision"],
        "fedcba9876543210"
    );
    let canonical = CanonicalDocument::from_value(&evidence["body"]).unwrap();
    assert_eq!(evidence["content_hash"], canonical.hash().as_str());
    assert!(evidence["body"]["completion"].get("rounds").is_none());
    assert!(evidence["body"]["completion"].get("remediations").is_none());
    assert_eq!(read.json()["seats"].as_array().unwrap().len(), 1);
    let artifact_url =
        format!("/v1/projects/{project}/committee-runs/{run}/artifacts/{artifact_id}");
    let report = Call::get(&artifact_url)
        .with_token(credential(0))
        .send(&world)
        .await;
    assert_eq!(report.status, 200, "{}", report.body);
    assert_eq!(
        report.json()["text"],
        "Verified fixture implementation and independent review evidence.\n"
    );
    assert_eq!(report.json()["sha256"], hash);
    let tampered = Call::get(format!(
        "/v1/projects/{project}/committee-runs/{run}/artifacts/{bad_artifact}"
    ))
    .with_token(credential(0))
    .send(&world)
    .await;
    assert_eq!(tampered.status, 409, "{}", tampered.body);
    let private_finding = Call::post(format!("/v1/projects/{project}/committee-runs/{run}/findings:record"), &serde_json::json!({
        "round":1,"verdict":"non_compliant","evidence_complete":false,"rationale":"private peer finding marker", "evidence_refs":["evidence:peer-only"], "expected_revision":read.json()["revision"],
    })).with_token(credential(1)).with_key("subject-peer-finding").send(&world).await;
    assert_eq!(private_finding.status, 200, "{}", private_finding.body);
    let isolated = Call::get(format!("/v1/projects/{project}/committee-runs/{run}"))
        .with_token(credential(0))
        .send(&world)
        .await;
    assert_eq!(isolated.status, 200, "{}", isolated.body);
    assert!(!isolated.body.contains("private peer finding marker"));
    assert!(isolated.json()["findings"].as_array().unwrap().is_empty());
    assert_eq!(
        isolated.json()["subject_evidence"]["body"]["completion"]["integrations"],
        evidence["body"]["completion"]["integrations"]
    );

    refresh_committee_fixture_quota(&world, &seed.project).await;
    let task_review = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &invoke(Some(task), "Ticket subject evidence"),
    )
    .signed_as(&world, "operator")
    .with_key("task-subject-evidence-invoke")
    .send(&world)
    .await;
    assert_eq!(task_review.status, 200, "{}", task_review.body);
    let other_run = task_review.json()["committee_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let task_seat = task_review.json()["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|seat| seat["committee_role"] == "reviewer")
        .unwrap()
        .clone();
    let task_credential = || {
        world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential_for_generation(
                SeatBindingId::parse(task_seat["seat_binding_id"].as_str().unwrap()).unwrap(),
                task_seat["occupancy_generation"].as_u64().unwrap(),
            )
    };
    let task_read = Call::get(format!("/v1/projects/{project}/committee-runs/{other_run}"))
        .with_token(task_credential())
        .send(&world)
        .await;
    assert_eq!(task_read.status, 200, "{}", task_read.body);
    assert_eq!(
        task_read.json()["subject_evidence"]["body"]["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        task_read.json()["subject_evidence"]["body"]["tasks"][0]["task"]["task_id"],
        seed.task
    );
    assert!(task_read.json()["subject_evidence"]["body"]["completion"].is_null());
    assert!(!task_read.body.contains(&sibling.to_string()));
    let cross_run = Call::get(&artifact_url)
        .with_token(task_credential())
        .send(&world)
        .await;
    assert_eq!(cross_run.status, 403, "{}", cross_run.body);
    let realm_read = Call::get(format!("/v1/projects/{project}/epics/{epic}/completion"))
        .with_token(credential(0))
        .send(&world)
        .await;
    assert_eq!(realm_read.status, 403, "{}", realm_read.body);
    let mutation = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision":1}),
    )
    .with_token(credential(0))
    .with_key("reviewer-cannot-advance")
    .send(&world)
    .await;
    assert_eq!(mutation.status, 403, "{}", mutation.body);
}

/// A fleet-only Judge without admission never represented a real legacy row.
/// Keep the pinned template and seat identities; refuse all new effects.
#[tokio::test]
async fn simulated_formal_committee_provenance_loss_refuses_recovery_and_findings_before_effects() {
    let realm = consultation_realm("/tmp/kontor-formal-provenance-loss", &[]).await;
    let world = &realm.world;
    let project = &realm.project;
    activate_through_the_registry(world, &formal_review_fleet_yaml(), "formal-fixture-policy")
        .await;
    let invoked = invoke_fleet_committee(
        &realm,
        "Simulated provenance loss",
        "provenance-loss-invoke",
    )
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.json()["committee_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let project_id = ProjectId::parse(project).unwrap();
    let run_id =
        ConsultationRunId::Committee(kontor_core::id::CommitteeRunId::parse(&run).unwrap());
    let frozen_invocation = world.daemon.state().with_store(|store| {
        store
            .get_consultation_run(project_id, run_id)
            .unwrap()
            .unwrap()
    });
    let seats_before = committee_readback(world, project, &run).await;
    let reviewer = seats_before["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|seat| seat["role_slot_id"] == "reviewer-a")
        .unwrap()
        .clone();
    let judge = seats_before["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|seat| seat["role_slot_id"] == "judge")
        .unwrap()
        .clone();
    assert!(judge["observed_binding"].is_null());
    assert_eq!(judge["model_route"]["provider"], "codex-b");
    let mut simulated_context = frozen_invocation.context.clone();
    simulated_context
        .as_object_mut()
        .expect("the frozen Committee context is an object")
        .remove("admission");
    let simulated_context_document = CanonicalDocument::from_value(&simulated_context)
        .expect("the legacy Committee context canonicalizes");
    let database = world.directory.path().join(kontor_daemon::DATABASE_FILE);
    let connection = rusqlite::Connection::open(database).expect("the Realm database opens");
    connection
        .execute_batch("DROP TRIGGER consultation_run_inputs_are_frozen;")
        .expect("the isolated fixture simulates provenance loss");
    connection
        .execute(
            "UPDATE consultation_runs
             SET context = ?1, context_hash = ?2
             WHERE project_id = ?3 AND run_id = ?4 AND family = 'committee'",
            rusqlite::params![
                simulated_context_document.json(),
                simulated_context_document.hash().as_str(),
                project,
                run,
            ],
        )
        .expect("the explicitly simulated provenance-loss row is reproduced");
    connection
        .execute_batch(
            "CREATE TRIGGER consultation_run_inputs_are_frozen
             BEFORE UPDATE ON consultation_runs
             WHEN OLD.project_id <> NEW.project_id
               OR OLD.mini_project_id <> NEW.mini_project_id
               OR OLD.family <> NEW.family
               OR OLD.profile_id <> NEW.profile_id
               OR OLD.profile_version <> NEW.profile_version
               OR OLD.definition_hash <> NEW.definition_hash
               OR OLD.question <> NEW.question
               OR OLD.question_hash <> NEW.question_hash
               OR OLD.context <> NEW.context
               OR OLD.context_hash <> NEW.context_hash
               OR OLD.caller_seat_binding_id <> NEW.caller_seat_binding_id
               OR OLD.topology_node_id <> NEW.topology_node_id
               OR OLD.invoke_key <> NEW.invoke_key
               OR OLD.invoke_intent_hash <> NEW.invoke_intent_hash
               OR OLD.created_at <> NEW.created_at
               OR OLD.result IS NOT NULL
             BEGIN
                 SELECT RAISE(ABORT, 'a consultation run cannot rewrite frozen input or settled evidence');
             END;",
        )
        .expect("the frozen-input guard is restored after fixture setup");
    drop(connection);

    let calls_before = world.fake.calls();
    let binding = SeatBindingId::parse(reviewer["seat_binding_id"].as_str().unwrap()).unwrap();
    let native =
        ExternalId::parse(reviewer["observed_binding"]["native_id"].as_str().unwrap()).unwrap();
    let recovery = Call::post(format!("/v1/projects/{project}/committee-runs/{run}/seats/{binding}/recover"),
        &serde_json::json!({"expected_revision": seats_before["revision"], "expected_native_id": native, "reason": "credential_propagation"}))
        .signed_as(world, "admin").with_key("provenance-loss-recover").send(world).await;
    assert_eq!(recovery.status, 409, "{}", recovery.body);
    assert_eq!(recovery.code(), "placement_blocked");
    assert_eq!(
        recovery.json()["rule"],
        "the active Committee route has no immutable template or reroute provenance"
    );
    let finding = Call::post(format!("/v1/projects/{project}/committee-runs/{run}/findings:record"),
        &serde_json::json!({"round": 1, "verdict": "compliant", "evidence_complete": true, "rationale": "Simulated row must not authorize a Judge", "evidence_refs": ["fixture:provenance-loss"], "expected_revision": seats_before["revision"]}))
        .with_token(world.daemon.state().credentials().consultation_seat_credential(binding))
        .with_key("provenance-loss-finding").send(world).await;
    assert_eq!(finding.status, 409, "{}", finding.body);
    assert_eq!(finding.code(), "placement_blocked");
    assert_eq!(
        world.fake.calls(),
        calls_before,
        "no recovery, container or Judge native effect is authorized"
    );
    let after = committee_readback(world, project, &run).await;
    assert_eq!(after["seats"], seats_before["seats"]);
    assert_eq!(after["revision"], seats_before["revision"]);
    assert_eq!(after["findings"], serde_json::json!([]));
    assert!(after["result"].is_null());
    world.daemon.state().with_store(|store| {
        let stored = store
            .get_consultation_run(project_id, run_id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.definition_hash, frozen_invocation.definition_hash);
        assert_eq!(stored.profile_id, frozen_invocation.profile_id);
        assert_eq!(stored.profile_version, frozen_invocation.profile_version);
        assert_eq!(
            stored.semantic_identity_hash,
            frozen_invocation.semantic_identity_hash
        );
        assert!(
            stored.context.get("admission").is_none(),
            "no provenance was invented"
        );
        assert!(
            store
                .get_consultation_recovery_attempt(
                    project_id,
                    run_id,
                    &RoleSlotId::parse("reviewer-a").unwrap(),
                    &native
                )
                .unwrap()
                .is_none()
        );
        for key in ["provenance-loss-recover", "provenance-loss-finding"] {
            assert!(
                store
                    .get_receipt_by_key(&IdempotencyKey::parse(key).unwrap())
                    .unwrap()
                    .is_none()
            );
        }
    });
    let connection =
        rusqlite::Connection::open(world.directory.path().join(kontor_daemon::DATABASE_FILE))
            .unwrap();
    assert!(
        connection
            .execute(
                "UPDATE consultation_runs SET context = '{}' WHERE project_id = ?1 AND run_id = ?2",
                rusqlite::params![project, run]
            )
            .is_err(),
        "the immutable-input trigger is restored"
    );
}

fn formal_seat(read: &serde_json::Value, slot: &str) -> serde_json::Value {
    read["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|seat| seat["role_slot_id"] == slot)
        .unwrap()
        .clone()
}

async fn formal_recover(
    realm: &ConsultationRealm,
    run: &str,
    read: &serde_json::Value,
    slot: &str,
    reason: &str,
    routes: serde_json::Value,
    key: &str,
) -> Answer {
    let world = &realm.world;
    let project = &realm.project;
    let seat = formal_seat(read, slot);
    Call::post(format!("/v1/projects/{project}/committee-runs/{run}/seats/{}/recover", seat["seat_binding_id"].as_str().unwrap()),
        &serde_json::json!({"expected_revision": read["revision"], "expected_native_id": seat["observed_binding"]["native_id"], "reason": reason, "recovery_profile": routes}))
        .signed_as(world, "admin").with_key(key).send(world).await
}

#[tokio::test]
async fn formal_committee_recovery_counts_the_deferred_judge_before_attempts_and_replays_lawfully()
{
    let realm =
        consultation_realm("/tmp/kontor-formal-recovery-cap", &[("Cursor", "cursor")]).await;
    let world = &realm.world;
    activate_through_the_registry(
        world,
        &permitted_committee_fleet_yaml(&[
            (&committee_fleet_key("reviewer-a"), "codex-sol"),
            (&committee_fleet_key("reviewer-b"), "cursor-grok"),
            (&committee_fleet_key("judge"), "claude-then-codex"),
        ]),
        "formal-permission-1",
    )
    .await;
    let invoked = invoke_fleet_committee(
        &realm,
        "Recovery cap including Judge",
        "formal-recovery-cap-invoke",
    )
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.json()["committee_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let read = committee_readback(world, &realm.project, &run).await;
    assert!(
        formal_seat(&read, "judge")["model_route"]["provider"]
            .as_str()
            .unwrap()
            .starts_with("claude-")
    );
    assert!(formal_seat(&read, "judge")["observed_binding"].is_null());
    // An activated chain change applies only to the proposed replacement.
    activate_through_the_registry(
        world,
        &permitted_committee_fleet_yaml(&[
            (&committee_fleet_key("reviewer-a"), "claude-then-codex"),
            (&committee_fleet_key("reviewer-b"), "cursor-grok"),
            (&committee_fleet_key("judge"), "codex-sol"),
        ]),
        "formal-permission-2",
    )
    .await;
    let before = world.fake.calls();
    let refused = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-a",
        "provider_unavailable",
        serde_json::json!([{"provider":"claude-b","model":"claude-opus-5","effort":"xhigh"}]),
        "formal-recovery-cap-refused",
    )
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "placement_blocked");
    assert_eq!(
        refused.json()["rule"],
        "no currently admissible whole-Committee allocation keeps the formal Independent Review to one Claude seat"
    );
    assert_eq!(world.fake.calls(), before);
    assert_eq!(committee_readback(world, &realm.project, &run).await, read);
    let a = formal_seat(&read, "reviewer-a");
    world.daemon.state().with_store(|store| {
        assert!(
            store
                .get_consultation_recovery_attempt(
                    project_id_of(&realm.project),
                    ConsultationRunId::Committee(
                        kontor_core::id::CommitteeRunId::parse(&run).unwrap()
                    ),
                    &RoleSlotId::parse("reviewer-a").unwrap(),
                    &ExternalId::parse(a["observed_binding"]["native_id"].as_str().unwrap())
                        .unwrap()
                )
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .get_receipt_by_key(&IdempotencyKey::parse("formal-recovery-cap-refused").unwrap())
                .unwrap()
                .is_none()
        );
    });
    // The same route replacement consumes one logical slot and keeps peer identities.
    let lawful = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-a",
        "credential_propagation",
        serde_json::json!([]),
        "formal-recovery-lawful",
    )
    .await;
    assert_eq!(lawful.status, 200, "{}", lawful.body);
    let after = committee_readback(world, &realm.project, &run).await;
    for slot in ["reviewer-b", "judge"] {
        assert_eq!(formal_seat(&after, slot), formal_seat(&read, slot));
    }
    assert_eq!(formal_seat(&after, "reviewer-a")["occupancy_generation"], 2);
    assert_eq!(
        formal_seat(&after, "reviewer-a")["model_route"],
        a["model_route"]
    );
    let calls = world.fake.calls();
    let replay = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-a",
        "credential_propagation",
        serde_json::json!([]),
        "formal-recovery-lawful",
    )
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["receipt"]["applied"], "unchanged");
    assert_eq!(world.fake.calls(), calls);
}

#[tokio::test]
async fn formal_committee_pending_recovery_refuses_peer_budget_and_resumes_exactly() {
    let realm = consultation_realm("/tmp/kontor-formal-pending-cap", &[("Cursor", "cursor")]).await;
    let world = &realm.world;
    let policy = permitted_committee_fleet_yaml(&[
        (&committee_fleet_key("reviewer-a"), "codex-sol"),
        (&committee_fleet_key("reviewer-b"), "cursor-grok"),
        (&committee_fleet_key("judge"), "codex-sol"),
    ]);
    activate_through_the_registry(world, &policy, "formal-permission-3").await;
    let invoked = invoke_fleet_committee(
        &realm,
        "Pending recovery reservation",
        "formal-pending-invoke",
    )
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.json()["committee_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let read = committee_readback(world, &realm.project, &run).await;
    let replacement_policy = policy
        .replace(
            "  codex-sol:\n    - [sol@xhigh]",
            "  codex-sol:\n    - [sol@xhigh]\n    - [opus@xhigh]",
        )
        .replace(
            "  cursor-grok:\n    - [grok@xhigh]",
            "  cursor-grok:\n    - [grok@xhigh]\n    - [opus@xhigh]",
        );
    activate_through_the_registry(world, &replacement_policy, "formal-permission-4").await;
    let route =
        serde_json::json!([{"provider":"claude-b","model":"claude-opus-5","effort":"xhigh"}]);
    world
        .fake
        .refusing_launch_of(&RoleSlotId::parse("reviewer-a").unwrap());
    let pending = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-a",
        "provider_unavailable",
        route.clone(),
        "formal-pending-a",
    )
    .await;
    assert_eq!(pending.status, 503, "{}", pending.body);
    let pending_read = committee_readback(world, &realm.project, &run).await;
    let before = world.fake.calls();
    let peer = formal_recover(
        &realm,
        &run,
        &pending_read,
        "reviewer-b",
        "provider_unavailable",
        route.clone(),
        "formal-pending-b",
    )
    .await;
    assert_eq!(peer.status, 409, "{}", peer.body);
    assert_eq!(peer.code(), "placement_blocked");
    assert_eq!(
        peer.json()["rule"],
        "the formal Committee has an unresolved peer recovery reservation"
    );
    assert_eq!(world.fake.calls(), before);
    assert_eq!(
        formal_seat(
            &committee_readback(world, &realm.project, &run).await,
            "reviewer-b"
        ),
        formal_seat(&read, "reviewer-b")
    );
    world
        .fake
        .allowing_launch_of(&RoleSlotId::parse("reviewer-a").unwrap());
    let resumed = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-a",
        "provider_unavailable",
        route.clone(),
        "formal-pending-a",
    )
    .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    let calls = world.fake.calls();
    let replay = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-a",
        "provider_unavailable",
        route,
        "formal-pending-a",
    )
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(world.fake.calls(), calls);
}

#[tokio::test]
async fn formal_committee_concurrent_recoveries_cannot_both_reserve_anthropic() {
    let realm =
        consultation_realm("/tmp/kontor-formal-concurrent-cap", &[("Cursor", "cursor")]).await;
    let world = &realm.world;
    let policy = permitted_committee_fleet_yaml(&[
        (&committee_fleet_key("reviewer-a"), "codex-sol"),
        (&committee_fleet_key("reviewer-b"), "cursor-grok"),
        (&committee_fleet_key("judge"), "codex-sol"),
    ]);
    activate_through_the_registry(world, &policy, "formal-permission-5").await;
    let invoked = invoke_fleet_committee(
        &realm,
        "Concurrent recovery proposals",
        "formal-concurrent-invoke",
    )
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.json()["committee_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let read = committee_readback(world, &realm.project, &run).await;
    activate_through_the_registry(
        world,
        &policy
            .replace(
                "  codex-sol:\n    - [sol@xhigh]",
                "  codex-sol:\n    - [sol@xhigh]\n    - [opus@xhigh]",
            )
            .replace(
                "  cursor-grok:\n    - [grok@xhigh]",
                "  cursor-grok:\n    - [grok@xhigh]\n    - [opus@xhigh]",
            ),
        "formal-permission-6",
    )
    .await;
    let route =
        serde_json::json!([{"provider":"claude-b","model":"claude-opus-5","effort":"xhigh"}]);
    let (a, b) = tokio::join!(
        formal_recover(
            &realm,
            &run,
            &read,
            "reviewer-a",
            "provider_unavailable",
            route.clone(),
            "formal-concurrent-a"
        ),
        formal_recover(
            &realm,
            &run,
            &read,
            "reviewer-b",
            "provider_unavailable",
            route,
            "formal-concurrent-b"
        )
    );
    assert_eq!(
        [a.status, b.status]
            .into_iter()
            .filter(|status| *status == 200)
            .count(),
        1,
        "{} {}",
        a.body,
        b.body
    );
    let after = committee_readback(world, &realm.project, &run).await;
    assert_eq!(
        after["seats"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|seat| seat["model_route"]["provider"] == "claude-b")
            .count(),
        1
    );
    assert_eq!(formal_seat(&after, "judge"), formal_seat(&read, "judge"));
}

#[tokio::test]
async fn formal_committee_native_less_judge_reroute_keeps_the_cap_before_persistence() {
    let realm =
        consultation_realm("/tmp/kontor-formal-judge-reroute", &[("Cursor", "cursor")]).await;
    let world = &realm.world;
    activate_through_the_registry(world, &formal_review_fleet_yaml(), "formal-fixture-policy")
        .await;
    world
        .fake
        .refusing_launch_of(&RoleSlotId::parse("reviewer-a").unwrap());
    let invoked = invoke_fleet_committee(
        &realm,
        "Native-less Judge cap",
        "formal-judge-reroute-invoke",
    )
    .await;
    assert_eq!(invoked.status, 503, "{}", invoked.body);
    let stored = world.daemon.state().with_store(|store| {
        store
            .get_consultation_run_by_key(
                project_id_of(&realm.project),
                &IdempotencyKey::parse("formal-judge-reroute-invoke").unwrap(),
            )
            .unwrap()
            .unwrap()
    });
    let run = stored.id.as_text();
    let read = committee_readback(world, &realm.project, &run).await;
    let judge = formal_seat(&read, "judge");
    refresh_committee_fixture_quota(world, &realm.project).await;
    activate_through_the_registry(
        world,
        &committee_fleet_yaml(&[(&committee_fleet_key("judge"), "claude-then-codex")]),
        "judge-reroute-policy",
    )
    .await;
    let reroute_body = |route: serde_json::Value| {
        serde_json::json!({
            "expected_revision": read["revision"], "expected_occupancy_generation": judge["occupancy_generation"],
            "expected_model_route": judge["model_route"], "reason":"permission_mode_unsupported", "recovery_profile": [route],
        })
    };
    let endpoint = format!(
        "/v1/projects/{}/committee-runs/{run}/seats/{}/reroute-unmaterialized",
        realm.project,
        judge["seat_binding_id"].as_str().unwrap()
    );
    let before = world.fake.calls();
    let refused = Call::post(
        &endpoint,
        &reroute_body(
            serde_json::json!({"provider":"claude-b","model":"claude-opus-5","effort":"xhigh"}),
        ),
    )
    .signed_as(world, "admin")
    .with_key("formal-judge-reroute-refused")
    .send(world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "placement_blocked");
    assert_eq!(
        refused.json()["rule"],
        "no currently admissible whole-Committee allocation keeps the formal Independent Review to one Claude seat"
    );
    assert_eq!(world.fake.calls(), before);
    assert_eq!(committee_readback(world, &realm.project, &run).await, read);
    world.daemon.state().with_store(|store| {
        assert!(
            store
                .get_consultation_materialization_route_provenance(
                    project_id_of(&realm.project),
                    stored.id,
                    &RoleSlotId::parse("judge").unwrap(),
                    2
                )
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .get_receipt_by_key(&IdempotencyKey::parse("formal-judge-reroute-refused").unwrap())
                .unwrap()
                .is_none()
        );
    });
    let lawful_policy = committee_fleet_yaml(&[(&committee_fleet_key("judge"), "terra-only")])
        .replace(
            "models:\n",
            "models:\n  terra: { domain: codex, id: gpt-5.6-terra, vendor: openai }\n",
        )
        .replace("bindings:\n", "  terra-only:\n    - [terra]\n\nbindings:\n");
    assert!(kontor_fleet::FleetSnapshot::parse(&lawful_policy).is_ok());
    activate_through_the_registry(world, &lawful_policy, "lawful-judge-policy").await;
    let body = reroute_body(serde_json::json!({"provider":"codex-b","model":"gpt-5.6-terra"}));
    let lawful = Call::post(&endpoint, &body)
        .signed_as(world, "admin")
        .with_key("formal-judge-reroute-lawful")
        .send(world)
        .await;
    assert_eq!(lawful.status, 200, "{}", lawful.body);
    let after = committee_readback(world, &realm.project, &run).await;
    assert_eq!(formal_seat(&after, "judge")["occupancy_generation"], 2);
    assert_eq!(
        formal_seat(&after, "judge")["model_route"]["provider"],
        "codex-b"
    );
    for slot in ["reviewer-a", "reviewer-b"] {
        assert_eq!(formal_seat(&after, slot), formal_seat(&read, slot));
    }
    assert_eq!(
        world.fake.calls(),
        before,
        "a native-less reroute has no native effect"
    );
    let replay = Call::post(&endpoint, &body)
        .signed_as(world, "admin")
        .with_key("formal-judge-reroute-lawful")
        .send(world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["receipt"]["applied"], "unchanged");
    assert_eq!(world.fake.calls(), before);
    let durable = world.daemon.state().with_store(|store| {
        store
            .get_consultation_run(project_id_of(&realm.project), stored.id)
            .unwrap()
            .unwrap()
    });
    assert_eq!(durable.context_hash, stored.context_hash);
    assert_eq!(durable.definition_hash, stored.definition_hash);
}

#[tokio::test]
async fn formal_committee_recovery_unknown_rank_or_vendor_refuses_before_fencing() {
    let realm = consultation_realm(
        "/tmp/kontor-formal-unknown-recovery",
        &[("Cursor", "cursor")],
    )
    .await;
    let world = &realm.world;
    activate_through_the_registry(world, &formal_review_fleet_yaml(), "formal-fixture-policy")
        .await;
    let invoked = invoke_fleet_committee(
        &realm,
        "Unknown recovery authority",
        "formal-unknown-invoke",
    )
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.json()["committee_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let read = committee_readback(world, &realm.project, &run).await;
    let policy = committee_fleet_yaml(&[(&committee_fleet_key("reviewer-a"), "claude-then-codex")]);
    for (key, yaml, route) in [
        (
            "formal-unknown-rank",
            policy.clone(),
            serde_json::json!({"provider":"claude-b", "model":"claude-opus-5", "effort":"xhigh"}),
        ),
        (
            "formal-unknown-vendor",
            formal_review_fleet_yaml().replace("vendor: anthropic", "vendor: unknown"),
            serde_json::json!({"provider":"claude-b", "model":"claude-opus-5","effort":"xhigh"}),
        ),
    ] {
        let parsed = kontor_fleet::FleetSnapshot::parse(&yaml)
            .expect("the explicit unknown authority fixture parses");
        if key == "formal-unknown-vendor" {
            assert_eq!(
                parsed.vendor_of(&ModelRung {
                    provider: ProviderRef("claude-b".to_owned()),
                    model: ModelRef("claude-opus-5".to_owned()),
                    effort: None
                }),
                Some("unknown")
            );
            activate_fleet_policy(world, &yaml);
        } else {
            write_fleet(world, &yaml);
        }
        let calls = world.fake.calls();
        let refused = formal_recover(
            &realm,
            &run,
            &read,
            "reviewer-a",
            "provider_unavailable",
            serde_json::json!([route]),
            key,
        )
        .await;
        assert_eq!(refused.status, 409, "{}", refused.body);
        assert_eq!(refused.code(), "placement_blocked");
        assert_eq!(world.fake.calls(), calls);
        assert_eq!(committee_readback(world, &realm.project, &run).await, read);
        world.daemon.state().with_store(|store| {
            let a = formal_seat(&read, "reviewer-a");
            assert!(
                store
                    .get_consultation_recovery_attempt(
                        project_id_of(&realm.project),
                        ConsultationRunId::Committee(
                            kontor_core::id::CommitteeRunId::parse(&run).unwrap()
                        ),
                        &RoleSlotId::parse("reviewer-a").unwrap(),
                        &ExternalId::parse(a["observed_binding"]["native_id"].as_str().unwrap())
                            .unwrap()
                    )
                    .unwrap()
                    .is_none()
            );
            assert!(
                store
                    .get_receipt_by_key(&IdempotencyKey::parse(key).unwrap())
                    .unwrap()
                    .is_none()
            );
        });
    }
}

/// The canonical recovery document carries permission authority separately from
/// credential-propagation route lineage, including a crash after retirement.
#[tokio::test]
async fn formal_committee_recovery_permission_pin_survives_restart_and_rejects_tamper() {
    let realm = consultation_realm(
        "/tmp/kontor-recovery-permission-pin",
        &[("Cursor", "cursor")],
    )
    .await;
    let policy = permitted_committee_fleet_yaml(&[
        (&committee_fleet_key("reviewer-a"), "codex-sol"),
        (&committee_fleet_key("reviewer-b"), "cursor-grok"),
        (&committee_fleet_key("judge"), "codex-sol"),
    ]);
    let hash =
        activate_through_the_registry(&realm.world, &policy, "recovery-permission-active").await;
    let invoked = invoke_fleet_committee(
        &realm,
        "Frozen recovery permission",
        "recovery-permission-invoke",
    )
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.json()["committee_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let read = committee_readback(&realm.world, &realm.project, &run).await;
    let slot = RoleSlotId::parse("reviewer-b").unwrap();
    realm.world.fake.refusing_launch_of(&slot);
    let pending = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-b",
        "credential_propagation",
        serde_json::json!([]),
        "recovery-permission-retry",
    )
    .await;
    assert_eq!(pending.status, 503, "{}", pending.body);
    let predecessor = formal_seat(&read, "reviewer-b");
    let profile = |realm: &ConsultationRealm| {
        realm
            .world
            .daemon
            .state()
            .with_store(|store| {
                store.get_consultation_recovery_attempt(
                    project_id_of(&realm.project),
                    ConsultationRunId::Committee(
                        kontor_core::id::CommitteeRunId::parse(&run).unwrap(),
                    ),
                    &slot,
                    &ExternalId::parse(
                        predecessor["observed_binding"]["native_id"]
                            .as_str()
                            .unwrap(),
                    )
                    .unwrap(),
                )
            })
            .unwrap()
            .unwrap()
    };
    let frozen = profile(&realm);
    assert_eq!(
        frozen.recovery_profile["consultation_permission_policy"],
        serde_json::json!({"source":"fleet_configuration","policy_hash":hash})
    );
    assert!(
        frozen
            .recovery_profile
            .get("consultation_route_source")
            .is_none(),
        "a fleet match must not relabel credential propagation"
    );
    let path = kontor_fleet_activation::policy_path(
        realm.world.directory.path(),
        &ContentHash::parse(&hash).unwrap(),
    );
    let before = realm.world.fake.calls();
    std::fs::write(&path, format!("{policy}\n# tampered\n")).unwrap();
    let tampered = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-b",
        "credential_propagation",
        serde_json::json!([]),
        "recovery-permission-retry",
    )
    .await;
    assert_ne!(tampered.status, 200, "{}", tampered.body);
    assert_eq!(realm.world.fake.calls(), before);
    std::fs::remove_file(&path).unwrap();
    let missing = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-b",
        "credential_propagation",
        serde_json::json!([]),
        "recovery-permission-retry",
    )
    .await;
    assert_ne!(missing.status, 200, "{}", missing.body);
    assert_eq!(realm.world.fake.calls(), before);
    std::fs::write(&path, &policy).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let changed = committee_fleet_yaml(&[
        (&committee_fleet_key("reviewer-a"), "codex-sol"),
        (&committee_fleet_key("reviewer-b"), "codex-sol"),
        (&committee_fleet_key("judge"), "codex-sol"),
    ]);
    activate_through_the_registry(&realm.world, &changed, "recovery-permission-next").await;
    write_fleet(&realm.world, &changed);
    realm.world.fake.allowing_launch_of(&slot);
    let ConsultationRealm {
        world,
        project,
        epic,
        caller,
    } = realm;
    let realm = ConsultationRealm {
        world: world.restart_native_consultations().await,
        project,
        epic,
        caller,
    };
    let active_path = realm
        .world
        .directory
        .path()
        .join(kontor_fleet_activation::FLEET_ACTIVATION_FILE);
    let active_bytes = std::fs::read(&active_path).unwrap();
    std::fs::write(&active_path, b"unreadable current activation").unwrap();
    assert!(kontor_fleet_activation::read_activation(realm.world.directory.path()).is_err());
    let resumed = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-b",
        "credential_propagation",
        serde_json::json!([]),
        "recovery-permission-retry",
    )
    .await;
    std::fs::write(&active_path, active_bytes).unwrap();
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_eq!(
        profile(&realm).recovery_profile_hash,
        frozen.recovery_profile_hash
    );
    assert_eq!(profile(&realm).recovery_profile, frozen.recovery_profile);
    let after = committee_readback(&realm.world, &realm.project, &run).await;
    assert_eq!(
        formal_seat(&after, "reviewer-b")["model_route"],
        predecessor["model_route"]
    );
    assert_ne!(
        formal_seat(&after, "reviewer-b")["observed_binding"]["native_id"],
        predecessor["observed_binding"]["native_id"]
    );
    for other in ["reviewer-a", "judge"] {
        assert_eq!(formal_seat(&after, other), formal_seat(&read, other));
    }
    let calls = realm.world.fake.calls();
    let replay = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-b",
        "credential_propagation",
        serde_json::json!([]),
        "recovery-permission-retry",
    )
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(realm.world.fake.calls(), calls);
}

/// Test-only pre-boundary intent: old writers persisted a profile and constraint
/// identity without permission authority. New preparation cannot produce this grant.
#[tokio::test]
async fn formal_committee_legacy_profile_only_recovery_refuses_without_effects() {
    let realm =
        consultation_realm("/tmp/kontor-recovery-legacy-pin", &[("Cursor", "cursor")]).await;
    let policy = permitted_committee_fleet_yaml(&[
        (&committee_fleet_key("reviewer-a"), "codex-sol"),
        (&committee_fleet_key("reviewer-b"), "cursor-grok"),
        (&committee_fleet_key("judge"), "codex-sol"),
    ]);
    let hash =
        activate_through_the_registry(&realm.world, &policy, "legacy-permission-active").await;
    let invoked = invoke_fleet_committee(
        &realm,
        "Legacy recovery evidence",
        "legacy-permission-invoke",
    )
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.json()["committee_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let read = committee_readback(&realm.world, &realm.project, &run).await;
    let project = project_id_of(&realm.project);
    let seat = formal_seat(&read, "reviewer-b");
    let binding = SeatBindingId::parse(seat["seat_binding_id"].as_str().unwrap()).unwrap();
    let predecessor = realm
        .world
        .daemon
        .state()
        .with_store(|s| s.get_consultation_seat_by_binding(project, binding))
        .unwrap()
        .unwrap();
    let native = predecessor
        .native_identity
        .as_ref()
        .unwrap()
        .native_id
        .clone();
    let intent = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version":1,"operation":"recover_consultation_seat","project":realm.project,
        "committee_run":run,"seat_binding_id":binding.to_string(),"expected_native_id":native,
        "reason":"credential_propagation","recovery_profile":[],
    }))
    .unwrap();
    let profile = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version":1,"ordered_rungs":[predecessor.model_rung.canonical_rung()],"constraint_policy_hash":hash,
        "constraint_candidate":{"rung":predecessor.model_rung.canonical_rung(),"step":1,"sub_step":1,"vendor":"xai","independence":"xai"},
    })).unwrap();
    let attempt = realm
        .world
        .daemon
        .state()
        .with_store(|s| {
            s.prepare_consultation_recovery_attempt(
                &kontor_core::repository::NewConsultationRecoveryAttempt {
                    project_id: project,
                    predecessor: predecessor.clone(),
                    expected_revision: AggregateRevision::parse(read["revision"].as_u64().unwrap())
                        .unwrap(),
                    recovery_reason: "credential_propagation".into(),
                    request_intent_hash: intent.hash().clone(),
                    recovery_profile: profile.clone(),
                    selected_model_rung: predecessor.model_rung.clone(),
                    prepared_at: kontor_api::now(),
                },
            )
        })
        .unwrap();
    let before = committee_readback(&realm.world, &realm.project, &run).await;
    let calls = realm.world.fake.calls();
    let refused = formal_recover(
        &realm,
        &run,
        &read,
        "reviewer-b",
        "credential_propagation",
        serde_json::json!([]),
        "legacy-permission-retry",
    )
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert!(
        refused.body.contains("consultation_permission_unpinned"),
        "{}",
        refused.body
    );
    assert_eq!(realm.world.fake.calls(), calls);
    assert_eq!(
        committee_readback(&realm.world, &realm.project, &run).await,
        before
    );
    realm.world.daemon.state().with_store(|s| {
        let unchanged = s
            .get_consultation_recovery_attempt(
                project,
                predecessor.run_id,
                &predecessor.role_slot_id,
                &native,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            unchanged.recovery_profile_hash,
            attempt.recovery_profile_hash
        );
        assert_eq!(unchanged.recovery_profile, attempt.recovery_profile);
        assert!(
            unchanged
                .recovery_profile
                .get("consultation_permission_policy")
                .is_none()
        );
        assert!(
            s.get_consultation_recovery_successor(
                project,
                predecessor.run_id,
                &predecessor.role_slot_id,
                &native
            )
            .unwrap()
            .is_none()
        );
        assert!(
            s.get_receipt_by_key(&IdempotencyKey::parse("legacy-permission-retry").unwrap())
                .unwrap()
                .is_none()
        );
    });
}

#[tokio::test]
async fn formal_committee_materialization_profile_freezes_permission_policy() {
    let policy = permitted_committee_fleet_yaml(&[
        (&committee_fleet_key("reviewer-a"), "codex-sol"),
        (&committee_fleet_key("reviewer-b"), "claude-then-codex"),
        (&committee_fleet_key("judge"), "codex-sol"),
    ]);
    let policy = policy
        .replace("provider: cursor", "provider: opencode")
        .replace("id: grok-4.7", "id: example/recovery")
        .replace("accounts: [cursor]", "accounts: [opencode-recovery]")
        .replace("account: cursor,", "account: opencode-recovery,");
    let realm = consultation_realm_with_catalog(
        "/tmp/kontor-materialization-policy-pin",
        &[("Recovery account", "opencode-recovery")],
        &policy,
    )
    .await;
    activate_through_the_registry(&realm.world, &policy, "materialization-permission-active").await;
    let slot = RoleSlotId::parse("reviewer-a").unwrap();
    realm.world.fake.refusing_launch_of(&slot);
    let invoked = invoke_fleet_committee(
        &realm,
        "Materialization permission",
        "materialization-permission-invoke",
    )
    .await;
    assert_eq!(invoked.status, 503, "{}", invoked.body);
    let project = project_id_of(&realm.project);
    let run = realm
        .world
        .daemon
        .state()
        .with_store(|s| {
            s.get_consultation_run_by_key(
                project,
                &IdempotencyKey::parse("materialization-permission-invoke").unwrap(),
            )
        })
        .unwrap()
        .unwrap();
    let read = committee_readback(&realm.world, &realm.project, &run.id.as_text()).await;
    let seat = formal_seat(&read, "reviewer-b");
    assert!(seat["observed_binding"].is_null());
    let next = policy.replace(
        &format!("{}: claude-then-codex", committee_fleet_key("reviewer-b")),
        &format!("{}: cursor-grok", committee_fleet_key("reviewer-b")),
    );
    let hash =
        activate_through_the_registry(&realm.world, &next, "materialization-permission-next").await;
    refresh_committee_fixture_quota(&realm.world, &realm.project).await;
    let endpoint = format!(
        "/v1/projects/{}/committee-runs/{}/seats/{}/reroute-unmaterialized",
        realm.project,
        run.id.as_text(),
        seat["seat_binding_id"].as_str().unwrap()
    );
    let body = serde_json::json!({"expected_revision":read["revision"],"expected_occupancy_generation":seat["occupancy_generation"],
        "expected_model_route":seat["model_route"],"reason":"permission_mode_unsupported","recovery_profile":[{"provider":"opencode-recovery","model":"example/recovery","effort":"xhigh"}]});
    let before = realm.world.fake.calls();
    let rerouted = Call::post(&endpoint, &body)
        .signed_as(&realm.world, "admin")
        .with_key("materialization-permission-reroute")
        .send(&realm.world)
        .await;
    assert_eq!(rerouted.status, 200, "{}", rerouted.body);
    assert_eq!(realm.world.fake.calls(), before);
    let (profile, _) = realm
        .world
        .daemon
        .state()
        .with_store(|s| {
            let seat = s
                .get_consultation_seat_by_binding(
                    project,
                    SeatBindingId::parse(seat["seat_binding_id"].as_str().unwrap()).unwrap(),
                )?
                .unwrap();
            s.get_consultation_generation_profile(project, &seat)
        })
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(profile.json()).unwrap();
    assert_eq!(value["consultation_permission_policy"]["policy_hash"], hash);
    assert!(value.get("consultation_route_source").is_none());
    let changed = committee_fleet_yaml(&[]);
    activate_through_the_registry(&realm.world, &changed, "materialization-permission-later").await;
    realm.world.fake.allowing_launch_of(&slot);
    let resumed = invoke_fleet_committee(
        &realm,
        "Materialization permission",
        "materialization-permission-invoke",
    )
    .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_eq!(
        formal_seat(
            &committee_readback(&realm.world, &realm.project, &run.id.as_text()).await,
            "reviewer-b"
        )["model_route"]["provider"],
        "opencode-recovery"
    );
    let calls = realm.world.fake.calls();
    let replay = Call::post(&endpoint, &body)
        .signed_as(&realm.world, "admin")
        .with_key("materialization-permission-reroute")
        .send(&realm.world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(realm.world.fake.calls(), calls);
}
