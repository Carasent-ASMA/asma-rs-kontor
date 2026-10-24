//! KR-29: a seat kind selects exactly one serve profile.
//!
//! Delivery seats compose `worker`; Advisor and Committee seats compose
//! `consultation`; planning-pair members and hosted leadership keep their own
//! profiles. This module exercises every surface the adapter uses: Claude's
//! worktree `.mcp.json`, and the creation frame for Codex, OpenCode and every
//! provider a scoped consultation can select.

use super::*;

use kontor_core::id::AdvisorRunId;
use kontor_runtime_paseo::client::{DeliveryCreate, PaseoRpc};
use kontor_runtime_paseo::seat_mcp::{SeatKind, SeatMcp};

fn seat() -> SeatMcp {
    SeatMcp {
        command: "kontor-mcp".to_owned(),
        state_root: "/realm/state".into(),
    }
}

fn git_repository() -> tempfile::TempDir {
    let repository = tempfile::tempdir().expect("a temporary repository");
    let status = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(repository.path())
        .status()
        .expect("git runs");
    assert!(status.success(), "git init succeeds");
    repository
}

fn composed_profile(cwd: &std::path::Path) -> String {
    let mcp: serde_json::Value = serde_json::from_slice(
        &std::fs::read(cwd.join(".mcp.json")).expect("the composed file exists"),
    )
    .expect("the composed file is JSON");
    profile_from_config(&mcp)
}

fn profile_from_config(config: &serde_json::Value) -> String {
    let args = config["mcpServers"]["kontor"]["args"]
        .as_array()
        .expect("the kontor server carries its argv");
    args.windows(2)
        .find(|pair| pair[0] == "--serve-profile")
        .map(|pair| pair[1].as_str().expect("a profile string").to_owned())
        .expect("the argv names a serve profile")
}

fn rung(provider: &str, model: &str) -> ModelRung {
    ModelRung {
        provider: ProviderRef(provider.to_owned()),
        model: ModelRef(model.to_owned()),
        effort: None,
    }
}

/// Delivery seats on every provider the adapter admits: Claude reads its
/// worktree file, Codex, Cursor and OpenCode receive the creation frame.
#[test]
fn every_admitted_delivery_seat_composes_the_worker_profile() {
    for provider in ["claude", "claude-a", "claude-b"] {
        let repository = git_repository();
        kontor_runtime_paseo::seat_mcp::compose_for_delivery_seat(
            Some(&seat()),
            provider,
            &kontor_runtime_paseo::posture::SeatPosture::read_only(),
            repository.path(),
        )
        .unwrap_or_else(|error| panic!("{provider} composes: {error}"));
        assert_eq!(
            composed_profile(repository.path()),
            "worker",
            "{provider} is a delivery seat and must get the worker profile"
        );
    }

    for (provider, model) in [
        ("codex", "gpt-5.6-sol"),
        ("codex-a", "gpt-5.6-sol"),
        ("codex-b", "gpt-5.6-sol"),
        ("opencode", "deepseek/deepseek-v4-flash"),
        ("opencode-zai", "zai-coding-plan/glm-5.3"),
        ("cursor", "gpt-5.6-sol"),
    ] {
        let rung = rung(provider, model);
        let labels = BTreeMap::new();
        let create = PaseoRpc::delivery_agent_create(DeliveryCreate {
            resolved_route: None,
            request_id: "req-delivery-profile",
            workspace_id: WORKSPACE_ID,
            canonical_cwd: CWD,
            model_rung: &rung,
            autonomy: SeatAutonomy::Bounded,
            title: "Implement",
            labels: &labels,
            allowances: &[],
            seat_mcp: Some(&seat()),
            serve_profile: SeatKind::Delivery.serve_profile(),
            binding_id: "bind-delivery-profile",
            agent_run_id: "run-delivery-profile",
            role_slot_id: "implement-a",
            prompt: "go",
            idempotent: false,
        })
        .unwrap_or_else(|error| panic!("{provider} builds a delivery create: {error}"));
        assert_eq!(
            profile_from_config(&create.rpc.message["config"]),
            "worker",
            "{provider} is a delivery seat and must get the worker profile"
        );
    }
}

/// Advisor and Committee seats on every provider a consultation route admits
/// compose the consultation profile, and nothing wider.
#[test]
fn every_admitted_consultation_and_committee_seat_composes_the_consultation_profile() {
    for provider in ["claude", "claude-a", "claude-b"] {
        let repository = git_repository();
        seat()
            .compose_consultation(repository.path())
            .unwrap_or_else(|error| panic!("{provider} composes: {error}"));
        assert_eq!(
            composed_profile(repository.path()),
            "consultation",
            "{provider} is an Advisor/Committee seat and must get the consultation profile"
        );
        let settings: serde_json::Value = serde_json::from_slice(
            &std::fs::read(repository.path().join(".claude/settings.local.json"))
                .expect("the composed settings exist"),
        )
        .expect("the composed settings are JSON");
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "'kontor-mcp' --consultation-tool-guard || exit 2",
            "the consultation guard is the one without the planning-pair profile"
        );
    }

    for (provider, model) in [
        ("claude", "claude-opus-5"),
        ("claude-a", "claude-opus-5"),
        ("claude-b", "claude-opus-5"),
        ("codex", "gpt-5.6-sol"),
        ("codex-a", "gpt-5.6-sol"),
        ("codex-b", "gpt-5.6-sol"),
        ("opencode", "deepseek/deepseek-v4-flash"),
        ("opencode-zai", "zai-coding-plan/glm-5.3"),
        ("cursor", "gpt-5.6-sol"),
    ] {
        let rung = rung(provider, model);
        let mut provenance =
            kontor_runtime::adapter::ConsultationRouteProvenance::fleet_configuration(
                ContentHash::of(b"a fleet policy"),
            );
        let family = match provider {
            "cursor" => Some("cursor"),
            "opencode" | "opencode-zai" => Some("opencode"),
            _ => None,
        };
        if let Some(family) = family {
            let policy = consultation_permission_policy(family, &rung);
            provenance = kontor_runtime::adapter::ConsultationRouteProvenance::fleet_configuration(
                policy.hash().clone(),
            );
            provenance.fleet_permissions = Some(policy.consultation_permissions());
        }
        for (family, run) in [
            (
                "advisor",
                ConsultationRunId::Advisor(AdvisorRunId::generate()),
            ),
            (
                "committee",
                ConsultationRunId::Committee(CommitteeRunId::generate()),
            ),
        ] {
            let labels = [(
                label::CONSULTATION_RUN.to_owned(),
                format!("{}/{}", run.family().as_str(), run.as_text()),
            )]
            .into_iter()
            .collect::<BTreeMap<_, _>>();
            let mut frame = PaseoRpc::consultation_agent_create(
                "req-consultation-profile".to_owned(),
                WORKSPACE_ID,
                CWD,
                &rung,
                &provenance,
                "Reviewer A",
                &labels,
                "review the change",
                "consultation-secret",
            )
            .unwrap_or_else(|error| {
                panic!("{provider}/{family} builds a consultation create: {error}")
            });
            frame.with_consultation_mcp(&seat());
            assert_eq!(
                profile_from_config(&frame.message["config"]),
                "consultation",
                "{provider}/{family} is a consultation seat and must get the consultation profile"
            );
        }
    }
}

/// The seat-kind mapping itself, enumerated so a new kind cannot be added
/// without stating its profile.
#[test]
fn every_composed_seat_kind_names_one_profile() {
    assert_eq!(
        SeatKind::ALL.map(SeatKind::serve_profile),
        [
            "worker",
            "consultation",
            "planning_pair_member",
            "leadership",
        ]
    );
}

/// Frame-delivered delivery providers must not keep the CLI path that carries
/// no MCP, and their frames must name the worker profile.
#[tokio::test]
async fn frame_delivered_delivery_launches_compose_the_worker_profile() {
    for (provider, model, autonomy, mode) in [
        (
            "codex",
            "gpt-5.6-sol",
            SeatAutonomy::Supervised,
            "auto-review",
        ),
        ("cursor", "gpt-5.6-sol", SeatAutonomy::Bounded, "agent"),
    ] {
        let configured_seat = seat();
        let mut configured = config();
        configured.seat_mcp = Some(configured_seat.clone());
        let plane = Plane::build_with_config(
            daemon(),
            PaseoCheckpoint::fresh(1, name(HOST_KEY)),
            configured,
        );
        plane
            .adapter
            .prepare_project("cmd-seat-profiles", &project_name())
            .await
            .expect("the epic project is prepared");
        let workspace = plane.prepare_workspace().await.expect("a task workspace");
        let request = plane
            .launch_request_for_with_autonomy(
                run(RUN_IMPLEMENT),
                &slot("implement-a"),
                &workspace,
                rung(provider, model),
                autonomy,
            )
            .await
            .unwrap_or_else(|error| panic!("{provider} admits a delivery seat: {error}"));

        let binding_id = request.binding_id().to_string();
        let agent_run_id = request.agent_run_id().to_string();
        let intent = PaseoRpc::delivery_agent_create(DeliveryCreate {
            resolved_route: None,
            request_id: "req-frame-intent",
            workspace_id: WORKSPACE_ID,
            canonical_cwd: CWD,
            model_rung: request.model_rung(),
            autonomy: request.autonomy(),
            title: request.display_name().as_str(),
            labels: &BTreeMap::new(),
            allowances: &[],
            seat_mcp: Some(&configured_seat),
            serve_profile: SeatKind::Delivery.serve_profile(),
            binding_id: &binding_id,
            agent_run_id: &agent_run_id,
            role_slot_id: request.role_slot_id().as_str(),
            prompt: request.prompt().as_str(),
            idempotent: false,
        })
        .expect("the same create the adapter builds")
        .intent_digest;

        let mut agent = v(AGENT);
        let body = agent["agent"].as_object_mut().expect("an agent object");
        body.insert("provider".to_owned(), serde_json::json!(provider));
        body.insert("model".to_owned(), serde_json::json!(model));
        body.insert("currentModeId".to_owned(), serde_json::json!(mode));
        body["labels"]
            .as_object_mut()
            .expect("the fixture labels")
            .insert(label::LAUNCH_INTENT.to_owned(), serde_json::json!(intent));
        plane
            .daemon
            .set_answer_rpc("create_agent_request", created_answer(&agent));

        let outcome = plane
            .adapter
            .launch(&request)
            .await
            .unwrap_or_else(|error| panic!("the {provider} delivery seat launches: {error}"));
        assert_eq!(
            outcome.snapshot.binding.identity.native_id.as_str(),
            AGENT_ID,
            "the launched seat is the frame's own native"
        );

        let sent = plane.daemon.sent_messages("create_agent_request");
        assert_eq!(sent.len(), 1, "{provider}: one frame create: {sent:?}");
        assert_eq!(
            profile_from_config(&sent[0]["config"]),
            "worker",
            "the {provider} delivery create must carry the worker profile"
        );
        assert_eq!(
            plane.daemon.count("agent run"),
            0,
            "{provider}: the CLI path cannot carry a per-seat MCP surface"
        );
    }
}
