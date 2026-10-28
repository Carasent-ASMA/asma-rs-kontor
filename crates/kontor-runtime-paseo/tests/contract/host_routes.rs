//! Real adapter entry points consume the persisted binding, including after restart.
use super::*;
use kontor_core::launch_route::ResolvedRoute;

fn frozen_route() -> ResolvedRoute {
    ResolvedRoute {
        schema_version: 1,
        slot: "logical-primary".into(),
        route_provider: "codex".into(),
        route_harness: "paseo".into(),
        native_alias: "opaque-host-a".into(),
        qualification: Some("qualified-route@r1".into()),
        route_model_id: "private-native-model".into(),
        canonical_model: "gpt-5.6-sol".into(),
        canonical_effort: None,
        canonical_vendor: "openai".into(),
        policy_hash: ContentHash::of(b"policy"),
        host_route_hash: ContentHash::of(b"original host bytes"),
        resolved_at: at("2026-08-16T09:10:00Z"),
    }
}

#[tokio::test]
async fn frozen_hosted_route_survives_restart_and_refuses_native_substitution() {
    let route = frozen_route();
    let seat = SeatBindingId::generate();
    let mut workspace = v(WORKSPACE_ROOT_LOCAL);
    for field in ["name", "title"] {
        workspace["entries"][0][field] = serde_json::json!("ECP · ASMA-7744 · Kontor MVP");
    }
    let mut agent = v(AGENT);
    agent["agent"]["provider"] = serde_json::json!(route.native_alias);
    agent["agent"]["model"] = serde_json::json!(route.route_model_id);
    agent["agent"]["currentModeId"] = serde_json::json!("full-access");
    agent["agent"]["labels"] = serde_json::json!({
        "jira.epic": "ASMA-7744", "kontor.project_id": MINI_PROJECT,
        "kontor.seat_binding_id": seat.to_string(), "kontor.hosted_seat": "true",
        "kontor.role": "lsa", "kontor.role_slot_id": "lsa",
        "kontor.workspace_id": WORKSPACE_ID, "kontor.worktree": CWD,
        "kontor.launch_intent": ContentHash::of(&serde_json::to_vec(&route).unwrap()).as_str(),
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
    let plane = Plane::fresh(recorded);
    plane
        .adapter
        .prepare_project("frozen-route-project", &project_name())
        .await
        .unwrap();
    let container = plane
        .adapter
        .prepare_container(&ecp_request(node(NODE_A), bound_root(node(NODE_B))))
        .await
        .unwrap()
        .snapshot;
    let request = HostedSeatLaunchRequest {
        resolved_route: Some(route.clone()),
        seat_binding_id: seat,
        role_slot_id: slot("lsa"),
        display_name: name("LSA"),
        container,
        cwd: root(),
        scope: HostedSeatScope::Epic(epic_execution_scope()),
        prompt: text("continue governed leadership"),
        role_prompt: None,
        credential: kontor_runtime::adapter::ScopedSeatCredential::new(
            "synthetic-seat-credential".into(),
        ),
        fenced_predecessor_native_ids: Vec::new(),
        model_rung: route.canonical_rung(),
        fleet_provenance: None,
        autonomy: SeatAutonomy::Bounded,
        context_policy: standard_context_policy(),
        requested_at: route.resolved_at,
    };
    let outcome = plane.adapter.launch_hosted_seat(&request).await.unwrap();
    let sent = plane.daemon.sent_messages("create_agent_request");
    assert_eq!(sent[0]["config"]["provider"], route.native_alias);
    assert_eq!(sent[0]["config"]["model"], route.route_model_id);
    let public_labels = sent[0]["labels"].to_string();
    for private in [&route.native_alias, &route.route_model_id] {
        assert!(!public_labels.contains(private));
    }
    assert!(!format!("{request:?}").contains(&route.native_alias));

    // Persist only the authorized carrier, then start a fresh adapter. A changed
    // host candidate is deliberately never supplied as recovery authority.
    let bytes = serde_json::to_vec(&request.resolved_route).unwrap();
    let stored: Option<ResolvedRoute> = serde_json::from_slice(&bytes).unwrap();
    let mut changed_host = route.clone();
    changed_host.native_alias = "opaque-host-b".into();
    let checkpoint = plane.adapter.checkpoint();
    let restarted = Plane {
        daemon: Arc::clone(&plane.daemon),
        adapter: PaseoAdapter::new(config(), Box::new(Arc::clone(&plane.daemon)), checkpoint)
            .unwrap(),
    };
    let inspect = HostedSeatInspectRequest {
        resolved_route: stored,
        seat_binding_id: seat,
        identity: outcome.identity,
        model_rung: route.canonical_rung(),
        autonomy: request.autonomy,
        requested_at: request.requested_at,
    };
    restarted
        .adapter
        .inspect_hosted_seat(&inspect)
        .await
        .unwrap();
    let claim = HostedSeatClaimRequest {
        resolved_route: Some(route.clone()),
        seat_binding_id: seat,
        role_slot_id: slot("lsa"),
        display_name: name("LSA"),
        container_native_id: external(WORKSPACE_ID),
        cwd: root(),
        scope: HostedSeatScope::Epic(epic_execution_scope()),
        claimant_native_id: external(AGENT_ID),
        expected_claimant_provider_session_id: None,
        expected_predecessor: None,
        requested_at: route.resolved_at,
    };
    let preview = restarted
        .adapter
        .preview_hosted_seat_claim(&claim)
        .await
        .unwrap();
    assert_eq!(preview.model_rung, route.canonical_rung());
    assert!(!format!("{preview:?}").contains(&route.native_alias));
    let before = restarted.daemon.mutations().len();
    agent["agent"]["provider"] = serde_json::json!(changed_host.native_alias);
    restarted
        .daemon
        .set_answer_rpc("fetch_agent_request", agent);
    let error = restarted
        .adapter
        .inspect_hosted_seat(&inspect)
        .await
        .unwrap_err();
    assert!(matches!(error, RuntimeError::CorrelationFailed));
    assert!(matches!(
        restarted.adapter.preview_hosted_seat_claim(&claim).await,
        Err(RuntimeError::CorrelationFailed)
    ));
    assert_eq!(restarted.daemon.mutations().len(), before);
    for private in [
        &route.native_alias,
        &changed_host.native_alias,
        &route.route_model_id,
    ] {
        assert!(!format!("{error}").contains(private));
    }
    restarted
        .daemon
        .set_answer_rpc("fetch_agent_request", serde_json::json!({"agent":null}));
    restarted
        .daemon
        .set_answer_rpc("fetch_agents_request", v(AGENT_LIST_EMPTY));
    assert!(matches!(
        restarted
            .adapter
            .retire_hosted_seat(&HostedSeatRetireRequest {
                seat_binding_id: seat,
                identity: inspect.identity.clone(),
                model_rung: route.canonical_rung(),
                resolved_route: Some(route.clone()),
                autonomy: request.autonomy,
                requested_at: request.requested_at,
                placement: None,
            })
            .await,
        Err(RuntimeError::ReplacementNotEvidenced { .. })
    ));
    assert_eq!(
        restarted.daemon.mutations().len(),
        before,
        "missing is not archive evidence"
    );
}

#[tokio::test]
async fn frozen_delivery_uses_native_transport_and_restarts_without_remapping() {
    let (plane, workspace) = Plane::prepared(daemon()).await;
    let base = plane
        .launch_request(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .unwrap();
    let mut route = frozen_route();
    route.slot = base.model_rung().provider.0.clone();
    route.canonical_model = base.model_rung().model.0.clone();
    route.canonical_effort = base.model_rung().effort;
    let request = base.with_resolved_route(Some(route.clone()));
    let mut agent = v(AGENT);
    let labels = serde_json::from_value(agent["agent"]["labels"].clone()).unwrap();
    let built = kontor_runtime_paseo::client::PaseoRpc::delivery_agent_create(
        kontor_runtime_paseo::client::DeliveryCreate {
            request_id: "synthetic-create",
            workspace_id: WORKSPACE_ID,
            canonical_cwd: CWD,
            model_rung: request.model_rung(),
            resolved_route: Some(&route),
            autonomy: request.autonomy(),
            title: request.display_name().as_str(),
            labels: &labels,
            allowances: &[],
            seat_mcp: None,
            serve_profile: "worker",
            binding_id: &request.binding_id().to_string(),
            agent_run_id: &request.agent_run_id().to_string(),
            role_slot_id: request.role_slot_id().as_str(),
            prompt: request.prompt().as_str(),
            idempotent: false,
        },
    )
    .unwrap();
    agent["agent"]["labels"] = serde_json::json!(built.labels);
    agent["agent"]["provider"] = serde_json::json!(route.native_alias);
    agent["agent"]["model"] = serde_json::json!(route.route_model_id);
    agent["agent"]["currentModeId"] = serde_json::json!("auto-review");
    plane
        .daemon
        .set_answer_rpc("create_agent_request", created_answer(&agent));
    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", agent.clone());
    let outcome = plane.adapter.launch(&request).await.unwrap();
    assert_eq!(plane.daemon.count("rpc create_agent_request"), 1);
    assert_eq!(plane.daemon.count("agent run"), 0);
    let create = plane.daemon.sent_messages("create_agent_request");
    assert_eq!(create[0]["config"]["provider"], route.native_alias);
    assert_eq!(create[0]["config"]["model"], route.route_model_id);
    assert!(
        !create[0]["labels"]
            .to_string()
            .contains(&route.native_alias)
    );

    let mut unavailable = config();
    unavailable
        .unavailable_providers
        .insert(route.native_alias.clone());
    let restarted = PaseoAdapter::new(
        unavailable,
        Box::new(Arc::clone(&plane.daemon)),
        plane.adapter.checkpoint(),
    )
    .unwrap();
    let inspect = InspectRequest {
        binding: outcome.snapshot.clone(),
        requested_at: request.requested_at(),
        resolved_route: Some(route.clone()),
    };
    restarted
        .inspect(&inspect)
        .await
        .expect("unavailability cannot hide exact readback");
    restarted
        .resume(&ResumeRequest {
            binding: outcome.snapshot.clone(),
            requested_at: request.requested_at(),
            resolved_route: Some(route.clone()),
        })
        .await
        .unwrap();
    let before = plane.daemon.mutations().len();
    agent["agent"]["provider"] = serde_json::json!("opaque-host-b");
    plane.daemon.set_answer_rpc("fetch_agent_request", agent);
    assert!(matches!(
        restarted.inspect(&inspect).await,
        Err(RuntimeError::CorrelationFailed)
    ));
    assert!(matches!(
        restarted
            .resume(&ResumeRequest {
                binding: outcome.snapshot,
                requested_at: request.requested_at(),
                resolved_route: Some(route)
            })
            .await,
        Err(RuntimeError::CorrelationFailed)
    ));
    assert_eq!(
        plane.daemon.mutations().len(),
        before,
        "substitution refuses before reload"
    );
}
