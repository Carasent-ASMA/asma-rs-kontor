//! Enabled composition through the production adapter, using real non-Git cwds.
use super::*;
use std::path::PathBuf;

struct DirectoryFixture {
    _directory: tempfile::TempDir,
    cwd: PathBuf,
    state: PathBuf,
    plane: Plane,
    request: HostedSeatLaunchRequest,
    agent: serde_json::Value,
}

impl DirectoryFixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().join("desk");
        let state = directory.path().join("realm");
        std::fs::create_dir(&cwd).unwrap();
        std::fs::create_dir(&state).unwrap();
        let cwd = std::fs::canonicalize(cwd).unwrap();
        let root = WorkspaceRoot::parse(cwd.to_str().unwrap()).unwrap();
        let binding_id = SeatBindingId::generate();
        let mut workspace = desk_workspace_listing();
        workspace["entries"][0]["workspaceDirectory"] = serde_json::json!(root.as_str());
        let mut agent = v(AGENT);
        agent["agent"]["cwd"] = serde_json::json!(root.as_str());
        agent["agent"]["title"] = serde_json::json!("LSA • ASMA-999911 • Qualification Alpha");
        agent["agent"]["labels"] = serde_json::json!({
            label::SEAT_BINDING: binding_id.to_string(), label::HOSTED_SEAT: "true",
            label::ROLE: "lsa", label::ROLE_SLOT: "lsa", label::WORKSPACE_ID: WORKSPACE_ID,
            label::WORKTREE: root.as_str(),
        });
        let recorded = RecordedPaseo::new()
            .answering(&PaseoCommand::version(), VERSION)
            .announcing(&v(SERVER_INFO))
            .answering_rpc("project.list.request", v(PROJECT_LIST))
            .answering_rpc("fetch_workspaces_request", workspace)
            .answering_rpc("fetch_agents_request", v(AGENT_LIST_EMPTY))
            .answering_rpc(
                "create_agent_request",
                serde_json::json!({"status":"agent_created","agent":{"id":AGENT_ID}}),
            )
            .answering_rpc("fetch_agent_request", agent.clone());
        let mut configured = config();
        configured.seat_mcp = Some(kontor_runtime_paseo::SeatMcp {
            command: "/owned/bin/kontor-mcp".into(),
            state_root: state.clone(),
        });
        let plane = Plane::build_with_config(
            recorded,
            PaseoCheckpoint::fresh(1, name(HOST_KEY)),
            configured,
        );
        let mut binding = bound_container_binding();
        binding.root = Some(root.clone());
        plane
            .adapter
            .inspect_container(&ContainerInspectRequest {
                binding: binding.clone(),
                native_parent: Some(bound_root(node(NODE_B)).identity),
                scope: None,
                epic_container: false,
                requested_at: at("2026-10-11T19:00:00Z"),
            })
            .await
            .unwrap();
        let container = plane
            .adapter
            .container_binding(binding.topology_node_id)
            .unwrap();
        let mut request =
            desk_seat_launch(binding_id, container, bound_root(node(NODE_B)).identity);
        request.cwd = root;
        request.model_rung.effort = Some(EffortLevel::High);
        agent["agent"]["thinkingOptionId"] = serde_json::json!("high");
        agent["agent"]["effectiveThinkingOptionId"] = serde_json::json!("high");
        plane
            .daemon
            .set_answer_rpc("fetch_agent_request", agent.clone());
        request.role_slot_id = slot("lsa");
        request.display_name = name("LSA • ASMA-999911 • Qualification Alpha");
        request.credential = kontor_runtime::adapter::ScopedSeatCredential::new(
            "directory-subject-alpha-secret".into(),
        );
        Self {
            _directory: directory,
            cwd,
            state,
            plane,
            request,
            agent,
        }
    }

    fn census(&self, agent: &serde_json::Value) {
        self.plane.daemon.set_answer_rpc("fetch_agents_request", serde_json::json!({"entries":[{"agent":agent["agent"],"project":agent["project"]}],"pageInfo":{"nextCursor":null,"prevCursor":null,"hasMore":false}}));
    }
    fn files(&self) -> (Vec<u8>, Vec<u8>) {
        (
            std::fs::read(self.cwd.join(".claude/settings.local.json")).unwrap(),
            std::fs::read(self.cwd.join(".mcp.json")).unwrap(),
        )
    }
    fn record(&self) -> PathBuf {
        self.state
            .join("hosted-directory-publications")
            .join(format!(
                "{}.json",
                ContentHash::of(self.cwd.as_os_str().as_encoded_bytes())
            ))
    }
    fn interrupted_pair(&self) {
        std::fs::create_dir_all(self.cwd.join(".claude")).unwrap();
        let mcp = serde_json::to_vec(
            &serde_json::json!({"mcpServers":{"kontor":{"command":"old-command"}}}),
        )
        .unwrap();
        let settings = b"{\"enableAllProjectMcpServers\":true}".to_vec();
        std::fs::write(self.cwd.join(".claude/settings.local.json"), &settings).unwrap();
        std::fs::create_dir_all(self.record().parent().unwrap()).unwrap();
        std::fs::write(self.record(), serde_json::to_vec(&serde_json::json!({
            "version":1,"cwd":self.cwd,"attempt":"01936f5a-0000-7000-8000-000000009999","created_claude_directory":true,
            "settings":{"original":null,"rendered":settings},"mcp":{"original":null,"rendered":mcp}
        })).unwrap()).unwrap();
    }
}

#[tokio::test]
async fn enabled_claude_directory_launch_replacement_and_subjects_keep_distinct_authority() {
    let mut f = DirectoryFixture::new().await;
    let first = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    assert!(first.created);
    assert!(!f.cwd.join(".git").exists());
    let files = f.files();
    let disk: serde_json::Value = serde_json::from_slice(&files.1).unwrap();
    assert_eq!(disk["mcpServers"]["kontor"]["args"][5], "leadership");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&files.0).unwrap()["enableAllProjectMcpServers"],
        true
    );
    // Ad-hoc/PR/durable/epic subjects all reach this same Desk-scoped boundary;
    // the daemon suites separately prove their command/authority derivation.
    f.request.seat_binding_id = SeatBindingId::generate();
    f.request.display_name = name("LSA • PR • ASMA-999901 • #101");
    f.request.credential =
        kontor_runtime::adapter::ScopedSeatCredential::new("directory-subject-beta-secret".into());
    f.agent["agent"]["labels"][label::SEAT_BINDING] =
        serde_json::json!(f.request.seat_binding_id.to_string());
    f.agent["agent"]["title"] = serde_json::json!(f.request.display_name.as_str());
    f.agent["agent"]["id"] = serde_json::json!("agt_beta");
    f.plane.daemon.set_answer_rpc(
        "create_agent_request",
        serde_json::json!({"status":"agent_created","agent":{"id":"agt_beta"}}),
    );
    f.plane
        .daemon
        .set_answer_rpc("fetch_agent_request", f.agent.clone());
    let second = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    assert_ne!(second.identity.native_id, first.identity.native_id);
    assert_eq!(f.files(), files);
    let messages = f.plane.daemon.sent_messages("create_agent_request");
    assert_eq!(messages.len(), 2);
    assert_ne!(
        messages[0]["labels"][label::SEAT_BINDING],
        messages[1]["labels"][label::SEAT_BINDING]
    );
    for message in &messages {
        assert!(message["labels"].get(label::PROJECT_ID).is_none());
        assert!(message["labels"].get(label::JIRA_EPIC).is_none());
        assert_eq!(
            message["config"]["toolPolicy"]["preapproved"]
                .as_array()
                .unwrap()
                .len(),
            6
        );
        assert!(
            !message["config"]["mcpServers"]
                .to_string()
                .contains("-secret")
        );
    }
    assert!(!String::from_utf8(files.1).unwrap().contains("-secret"));
    assert!(!format!("{:?}", f.plane.adapter.checkpoint()).contains("-secret"));
    assert!(f.plane.adapter.project_binding().is_none());
    // A missing/fenced predecessor replacement rebuilds files at the same
    // shared cwd, with the same binding and a different exact native.
    f.request
        .fenced_predecessor_native_ids
        .push(second.identity.native_id);
    std::fs::remove_file(f.cwd.join(".mcp.json")).unwrap();
    f.agent["agent"]["id"] = serde_json::json!("agt_replacement");
    f.plane.daemon.set_answer_rpc(
        "create_agent_request",
        serde_json::json!({"status":"agent_created","agent":{"id":"agt_replacement"}}),
    );
    f.plane
        .daemon
        .set_answer_rpc("fetch_agent_request", f.agent.clone());
    let replacement = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    assert_eq!(replacement.identity.native_id.as_str(), "agt_replacement");
    assert!(replacement.created);
    assert!(f.cwd.join(".mcp.json").exists());
}

#[tokio::test]
async fn enabled_claude_directory_refuses_invalid_configuration_and_incomplete_publication_without_create()
 {
    let f = DirectoryFixture::new().await;
    std::fs::write(f.cwd.join(".mcp.json"), "not json").unwrap();
    assert!(matches!(
        f.plane.adapter.launch_hosted_seat(&f.request).await,
        Err(RuntimeError::LaunchNotAdmitted { .. })
    ));
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 0);
    assert!(!f.cwd.join(".claude").exists());
    std::fs::remove_file(f.cwd.join(".mcp.json")).unwrap();
    f.interrupted_pair();
    // An external edit while publication was interrupted is not ours to undo.
    std::fs::write(f.cwd.join(".mcp.json"), "{\"foreign\":true}").unwrap();
    let refusal = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap_err();
    assert!(
        matches!(refusal, RuntimeError::LaunchNotAdmitted { rule } if rule.contains("cleanup is incomplete"))
    );
    assert!(f.record().exists());
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 0);
    assert_eq!(
        std::fs::read_to_string(f.cwd.join(".mcp.json")).unwrap(),
        "{\"foreign\":true}"
    );
}

#[tokio::test]
async fn enabled_claude_directory_lost_ack_recovers_same_native_and_reconciles_before_reload() {
    let f = DirectoryFixture::new().await;
    f.plane.daemon.lose_next_rpc("create_agent_request");
    assert!(
        f.plane
            .adapter
            .launch_hosted_seat(&f.request)
            .await
            .is_err()
    );
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 1);
    let pair = f.files();
    f.census(&f.agent);
    let recovered = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    assert!(!recovered.created);
    assert_eq!(recovered.identity.native_id.as_str(), AGENT_ID);
    assert_eq!(f.files(), pair);
    std::fs::remove_file(f.cwd.join(".mcp.json")).unwrap();
    f.interrupted_pair();
    let mut failed = f.agent.clone();
    failed["agent"]["status"] = serde_json::json!("error");
    failed["agent"]["runtimeInfo"] = serde_json::json!({});
    failed["agent"]["persistence"] = serde_json::json!({});
    f.census(&failed);
    f.plane
        .daemon
        .set_answer_rpc("fetch_agent_request", failed.clone());
    f.plane
        .daemon
        .queue_answer(&PaseoCommand::agent_reload(AGENT_ID), CLI_AGENT_RELOADED);
    std::fs::write(f.cwd.join(".mcp.json"), "{\"foreign\":true}").unwrap();
    let refusal = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap_err();
    assert!(
        matches!(refusal, RuntimeError::LaunchNotAdmitted { rule } if rule.contains("cleanup is incomplete"))
    );
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 1);
    assert_eq!(f.plane.daemon.count(&format!("agent reload {AGENT_ID}")), 0);
    std::fs::remove_file(f.cwd.join(".mcp.json")).unwrap();
    // Failed health persists through the refused recovery. Only the readback
    // after the successful reload reports an initialized provider thread.
    f.plane
        .daemon
        .queue_answer_rpc("fetch_agent_request", failed);
    f.plane
        .daemon
        .set_answer_rpc("fetch_agent_request", f.agent.clone());
    let recovered = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    assert!(!recovered.created);
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 1);
    assert_eq!(f.plane.daemon.count(&format!("agent reload {AGENT_ID}")), 1);
    assert!(!f.record().exists());
    assert_eq!(f.files(), pair);
}

#[tokio::test]
async fn enabled_claude_directory_wrong_placement_stays_refused_without_files() {
    let f = DirectoryFixture::new().await;
    let mut workspace = desk_workspace_listing();
    workspace["entries"][0]["workspaceDirectory"] = serde_json::json!(f.cwd);
    workspace["entries"][0]["workspaceKind"] = serde_json::json!("worktree");
    f.plane
        .daemon
        .set_answer_rpc("fetch_workspaces_request", workspace);
    assert!(matches!(
        f.plane.adapter.launch_hosted_seat(&f.request).await,
        Err(RuntimeError::WorkspaceMismatch { .. })
    ));
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 0);
    assert!(!f.cwd.join(".mcp.json").exists());
}

#[tokio::test]
async fn enabled_claude_directory_retitle_keeps_identity_and_files() {
    let f = DirectoryFixture::new().await;
    let native = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    // Metadata operations leave even an invalid config untouched.
    std::fs::write(f.cwd.join(".mcp.json"), "malformed metadata-only fixture").unwrap();
    let pair = f.files();
    let desired = name("LSA • ASMA-999911 • Renamed");
    let mut after = f.agent.clone();
    after["agent"]["title"] = serde_json::json!(desired.as_str());
    f.plane
        .daemon
        .queue_answer_rpc("fetch_agent_request", f.agent.clone());
    f.plane.daemon.set_answer_rpc("fetch_agent_request", after);
    f.plane.daemon.queue_answer(
        &PaseoCommand::agent_update(AGENT_ID, Some(desired.as_str()), &BTreeMap::new()),
        r#"{"agentId":"agt_implement"}"#,
    );
    let result = f
        .plane
        .adapter
        .retitle_seat(&RetitleSeatRequest {
            identity: native.identity.clone(),
            provider_session_id: native.provider_session_id.clone(),
            container_native_id: external(WORKSPACE_ID),
            desired_title: desired,
            requested_at: at("2026-10-11T19:05:00Z"),
        })
        .await
        .unwrap();
    assert_eq!(result.identity, native.identity);
    assert_eq!(result.provider_session_id, native.provider_session_id);
    assert_eq!(f.files(), pair);
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 1);
}

#[tokio::test]
async fn enabled_claude_directory_claim_is_metadata_only_and_retains_exact_scope() {
    let f = DirectoryFixture::new().await;
    let native = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    std::fs::write(f.cwd.join(".mcp.json"), "malformed metadata-only fixture").unwrap();
    let files = f.files();
    f.census(&f.agent);
    let claimed = f
        .plane
        .adapter
        .claim_hosted_seat(&HostedSeatClaimRequest {
            resolved_route: None,
            seat_binding_id: f.request.seat_binding_id,
            role_slot_id: f.request.role_slot_id.clone(),
            display_name: f.request.display_name.clone(),
            container_native_id: external(WORKSPACE_ID),
            cwd: f.request.cwd.clone(),
            scope: f.request.scope.clone(),
            claimant_native_id: native.identity.native_id.clone(),
            expected_claimant_provider_session_id: native.provider_session_id.clone(),
            expected_predecessor: None,
            requested_at: at("2026-10-11T19:05:00Z"),
        })
        .await
        .unwrap();
    assert_eq!(claimed.claim.identity, native.identity);
    assert_eq!(
        claimed.claim.provider_session_id,
        native.provider_session_id
    );
    assert!(claimed.claim.already_claimed);
    assert!(!claimed.changed);
    assert_eq!(f.files(), files);
    assert!(f.plane.adapter.project_binding().is_none());
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 1);
}

#[tokio::test]
async fn enabled_composition_restore_bindings_keeps_snapshot_and_does_not_publish_files() {
    let f = DirectoryFixture::new().await;
    f.plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    std::fs::write(f.cwd.join(".mcp.json"), "malformed metadata-only fixture").unwrap();
    let files = f.files();
    // This API restores delivery RuntimeBinding snapshots. Hosted desk intents
    // recover at launch_hosted_seat, covered by the lost-ack test above.
    let (_, binding) = launched().await;
    let recorded = daemon();
    recorded.forget_queued_rpc("fetch_workspaces_request");
    recorded.set_answer_rpc("fetch_workspaces_request", v(WORKSPACE_LIST_ONE));
    let mut configured = config();
    configured.seat_mcp = Some(kontor_runtime_paseo::SeatMcp {
        command: "/owned/bin/kontor-mcp".into(),
        state_root: f.state.clone(),
    });
    let restarted = Plane::build_with_config(
        recorded,
        PaseoCheckpoint::fresh(1, name(HOST_KEY)),
        configured,
    );
    let restored = restarted
        .adapter
        .restore_bindings(std::slice::from_ref(&binding))
        .await
        .unwrap();
    assert_eq!(restored, vec![binding]);
    assert_eq!(f.files(), files);
    assert!(restarted.daemon.mutations().is_empty());
}

#[tokio::test]
async fn enabled_directory_cross_vendor_replacement_and_route_correction_keep_shared_files() {
    let mut f = DirectoryFixture::new().await;
    let claude = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    let files = f.files();
    f.request
        .fenced_predecessor_native_ids
        .push(claude.identity.native_id);
    f.request.model_rung = ModelRung {
        provider: ProviderRef("codex".into()),
        model: ModelRef("gpt-5.6-sol".into()),
        effort: None,
    };
    f.request.autonomy = SeatAutonomy::Bounded;
    f.agent["agent"]["id"] = serde_json::json!("agt_codex_replacement");
    f.agent["agent"]["provider"] = serde_json::json!("codex");
    f.agent["agent"]["model"] = serde_json::json!("gpt-5.6-sol");
    f.agent["agent"]["currentModeId"] = serde_json::json!("full-access");
    f.agent["agent"]["runtimeInfo"] = serde_json::json!({"provider":"codex","sessionId":"codex_session_2","model":"gpt-5.6-sol","modeId":"full-access"});
    f.agent["agent"]["persistence"] =
        serde_json::json!({"provider":"codex","sessionId":"codex_session_2"});
    f.plane
        .daemon
        .set_answer_rpc("fetch_agent_request", f.agent.clone());
    f.plane.daemon.set_answer_rpc(
        "create_agent_request",
        serde_json::json!({"status":"agent_created","agent":{"id":"agt_codex_replacement"}}),
    );
    let codex = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    assert!(codex.created);
    assert_eq!(f.files(), files);
    f.request
        .fenced_predecessor_native_ids
        .push(codex.identity.native_id);
    f.request.model_rung = model_rung();
    f.request.model_rung.provider = ProviderRef("claude-b".into());
    f.request.autonomy = SeatAutonomy::standard();
    f.agent["agent"]["id"] = serde_json::json!("agt_claude_corrected");
    f.agent["agent"]["provider"] = serde_json::json!("claude-b");
    f.agent["agent"]["model"] = serde_json::json!("claude-opus-5");
    f.agent["agent"]["currentModeId"] = serde_json::json!("auto");
    f.agent["agent"]["runtimeInfo"] = serde_json::json!({"provider":"claude","sessionId":"claude_session_3","model":"claude-opus-5","modeId":"auto"});
    f.agent["agent"]["persistence"] =
        serde_json::json!({"provider":"claude-b","sessionId":"claude_session_3"});
    f.plane
        .daemon
        .set_answer_rpc("fetch_agent_request", f.agent.clone());
    f.plane.daemon.set_answer_rpc(
        "create_agent_request",
        serde_json::json!({"status":"agent_created","agent":{"id":"agt_claude_corrected"}}),
    );
    std::fs::remove_file(f.cwd.join(".mcp.json")).unwrap();
    let corrected = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    assert!(corrected.created);
    assert_eq!(f.files(), files);
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 3);
    assert!(f.plane.adapter.project_binding().is_none());
}

#[tokio::test]
async fn healthy_directory_replay_retains_session_and_validates_without_publication() {
    let f = DirectoryFixture::new().await;
    let first = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    f.census(&f.agent);
    let paths = [
        f.cwd.join(".claude/settings.local.json"),
        f.cwd.join(".mcp.json"),
    ];
    let before = f.files();
    let metadata = paths.each_ref().map(|p| std::fs::metadata(p).unwrap());
    let replay = f
        .plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    assert_eq!(replay.identity, first.identity);
    assert_eq!(replay.provider_session_id, first.provider_session_id);
    assert!(!replay.created);
    assert_eq!(f.files(), before);
    for (path, before) in paths.iter().zip(metadata) {
        let after = std::fs::metadata(path).unwrap();
        assert_eq!(after.modified().unwrap(), before.modified().unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(after.ino(), before.ino());
        }
    }
    // Valid differences in unrelated content/format are retained byte-exact.
    let mut settings: serde_json::Value = serde_json::from_slice(&before.0).unwrap();
    settings["foreign"] = serde_json::json!("retained");
    let changed = serde_json::to_vec(&settings).unwrap();
    std::fs::write(&paths[0], &changed).unwrap();
    f.plane
        .adapter
        .launch_hosted_seat(&f.request)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&paths[0]).unwrap(), changed);
    // Missing or different required config refuses, never repairs a healthy native.
    std::fs::remove_file(&paths[1]).unwrap();
    assert!(matches!(
        f.plane.adapter.launch_hosted_seat(&f.request).await,
        Err(RuntimeError::LaunchNotAdmitted { .. })
    ));
    assert!(!paths[1].exists());
    std::fs::write(&paths[1], &before.1).unwrap();
    settings["enableAllProjectMcpServers"] = serde_json::json!(false);
    let invalid = serde_json::to_vec(&settings).unwrap();
    std::fs::write(&paths[0], &invalid).unwrap();
    assert!(matches!(
        f.plane.adapter.launch_hosted_seat(&f.request).await,
        Err(RuntimeError::LaunchNotAdmitted { .. })
    ));
    assert_eq!(std::fs::read(&paths[0]).unwrap(), invalid);
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 1);
    assert_eq!(f.plane.daemon.count(&format!("agent reload {AGENT_ID}")), 0);
    assert!(!f.record().exists());
}

#[tokio::test]
async fn healthy_directory_replay_recovers_pending_originals_without_republishing() {
    let f = DirectoryFixture::new().await;
    f.census(&f.agent);
    f.interrupted_pair();
    assert!(matches!(
        f.plane.adapter.launch_hosted_seat(&f.request).await,
        Err(RuntimeError::LaunchNotAdmitted { .. })
    ));
    // Pending partial pair is rolled back to absence, then read-only validation
    // refuses missing required configuration without composing a fresh pair.
    assert!(!f.cwd.join(".claude").exists());
    assert!(!f.cwd.join(".mcp.json").exists());
    assert!(!f.record().exists());
    assert_eq!(f.plane.daemon.count("rpc create_agent_request"), 0);
    assert_eq!(f.plane.daemon.count(&format!("agent reload {AGENT_ID}")), 0);
}
