use super::*;

#[tokio::test]
async fn scoped_committee_reads_actual_subject_evidence_and_verified_report_without_peer_leakage() {
    let (world, seed, tpm) = reopened_completion_fixture("committee-subject-evidence").await;
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
    confirm_test_epic_identity(&world, &seed.project, &seed.epic);
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
