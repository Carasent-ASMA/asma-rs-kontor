//! Atomic, append-only recovery contracts for legacy naming state.

use kontor_core::backlog_identity::EpicBacklogCode;
use kontor_core::id::{
    AggregateRevision, CanonicalDocument, CommandReceiptId, ExternalId, ExternalName,
    IdempotencyKey, MiniProjectId, ProjectId, RuntimeKindKey, Timestamp, TopologyKindKey,
    TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::realm::ReceiptEnvelope;
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    LegacyEpicBacklogCodeCorrection, MiniProjectTopologySnapshot, NewLocalCommand, NewMiniProject,
    NewNativeContainerBinding, NewProject, NewSessionTopologyNode, ProjectRepository,
    RealmRepository, TopologyContainerRecovery, TopologyRepository,
};
use kontor_core::spec::{Shareability, ShareabilityTier, TopologySnapshot};
use kontor_core::state::{NativeRuntimeIdentity, ObservedContainerKind};
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use rusqlite::{Connection, params};
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

fn identity(native_id: &str) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("a runtime kind"),
        host: name("paseo-local"),
        generation: 7,
        native_id: ExternalId::parse(native_id).expect("a native id"),
    }
}

fn local_command(
    project_id: ProjectId,
    key: &str,
    kind: CommandKind,
    target: AggregateRef,
    target_revision: AggregateRevision,
) -> NewLocalCommand {
    let intent = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "operation": kind.as_str(),
    }))
    .expect("the intent canonicalizes");
    NewLocalCommand {
        project_id,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse(key).expect("an idempotency key"),
        kind,
        target,
        target_revision,
        intent,
        created_at: at("2026-09-04T09:00:00Z"),
    }
}

#[test]
fn a_legacy_epic_code_correction_is_atomic_append_only_and_replay_safe() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = SqliteStore::open(&database).expect("the store opens");
    let project_id = ProjectId::generate();
    let epic_id = MiniProjectId::generate();
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Legacy naming project"),
            root_path: name("/tmp/legacy-naming-project"),
            created_at: at("2026-09-04T08:00:00Z"),
        })
        .expect("the project is created");
    let epic = store
        .create_mini_project(&NewMiniProject {
            id: epic_id,
            project_id,
            name: name("Kontor operational MVP"),
            created_at: at("2026-09-04T08:01:00Z"),
        })
        .expect("the epic is created");

    // The raw insert reproduces exactly the v72 migration's legacy row. There
    // is intentionally no public API for minting new legacy provenance.
    Connection::open(&database)
        .expect("the fixture connection opens")
        .execute(
            "INSERT INTO epic_backlog_codes
                 (project_id, mini_project_id, code, provenance, status, assigned_at)
             VALUES (?1, ?2, 'OP', 'legacy', 'active', '2026-09-04T08:02:00Z')",
            params![project_id.to_string(), epic_id.to_string()],
        )
        .expect("the migrated legacy code is reproduced");

    let correction = LegacyEpicBacklogCodeCorrection {
        project_id,
        mini_project_id: epic_id,
        expected_prior_code: EpicBacklogCode::parse("OP").expect("the prior code"),
        corrected_code: EpicBacklogCode::parse("KOP").expect("the corrected code"),
        reason: name("Restore the explicitly agreed Kontor epic backlog code"),
        corrected_at: at("2026-09-04T09:00:00Z"),
    };
    let command = local_command(
        project_id,
        "legacy-code-op-to-kop",
        CommandKind::CorrectEpicBacklogCode,
        AggregateRef::MiniProject {
            mini_project_id: epic_id,
        },
        epic.revision,
    );
    let envelope = ReceiptEnvelope::new(store.realm(), command);
    let (code, receipt, applied) = store
        .correct_legacy_epic_backlog_code_with_intent(&correction, epic.revision, &envelope)
        .expect("the legacy correction commits");
    assert_eq!(code.as_str(), "KOP");
    assert_eq!(applied, kontor_store::Applied::Created);
    assert_eq!(
        store
            .epic_backlog_code(project_id, epic_id)
            .expect("the effective code reads")
            .expect("the epic has a code")
            .as_str(),
        "KOP"
    );
    let origin = store
        .epic_backlog_code_origin(project_id, epic_id)
        .expect("the origin reads")
        .expect("the origin exists");
    assert_eq!(origin.0.as_str(), "OP", "the source row is retained");
    assert_eq!(origin.1, "legacy");
    assert_eq!(origin.2.expect("a correction").as_str(), "KOP");

    let (again, replayed_receipt, replayed) = store
        .correct_legacy_epic_backlog_code_with_intent(&correction, epic.revision, &envelope)
        .expect("the exact command replays");
    assert_eq!(again.as_str(), "KOP");
    assert_eq!(replayed_receipt.id, receipt.id);
    assert_eq!(replayed, kontor_store::Applied::Unchanged);
}

#[test]
fn a_stale_container_recovery_cas_preserves_logical_identity_and_history() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = SqliteStore::open(&database).expect("the store opens");
    let project_id = ProjectId::generate();
    let epic_id = MiniProjectId::generate();
    let created_at = at("2026-09-04T08:00:00Z");
    let project = store
        .create_project(&NewProject {
            id: project_id,
            name: name("Container recovery project"),
            root_path: name("/tmp/container-recovery-project"),
            created_at,
        })
        .expect("the project is created");
    store
        .create_mini_project(&NewMiniProject {
            id: epic_id,
            project_id,
            name: name("Container recovery epic"),
            created_at,
        })
        .expect("the epic is created");
    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let spec = domain.topology_specs.first().expect("a topology").clone();
    let stamp = Shareability::default_for(ShareabilityTier::ProjectKnowledge)
        .expect("project knowledge classifies");
    let canonical_hash = store
        .publish_topology_spec(project_id, &spec, &stamp, created_at)
        .expect("the topology publishes");
    let topology = TopologySnapshot {
        spec_id: spec.spec_id,
        version: spec.version,
        canonical_hash,
    };
    store
        .pin_mini_project_topology(&MiniProjectTopologySnapshot {
            project_id,
            mini_project_id: epic_id,
            topology: topology.clone(),
            pinned_at: created_at,
        })
        .expect("the epic pins the topology");
    let project_root_id = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: project_root_id,
            project_id,
            mini_project_id: None,
            topology: topology.clone(),
            kind: TopologyKindKey::parse("PSW").expect("a kind"),
            parent_id: None,
            task_id: None,
            created_at,
        })
        .expect("the project root node is created");
    let epic_root_id = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: epic_root_id,
            project_id,
            mini_project_id: Some(epic_id),
            topology: topology.clone(),
            kind: TopologyKindKey::parse("ESW").expect("a kind"),
            parent_id: Some(project_root_id),
            task_id: None,
            created_at,
        })
        .expect("the epic root node is created");
    let node_id = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: node_id,
            project_id,
            mini_project_id: Some(epic_id),
            topology,
            kind: TopologyKindKey::parse("ECP").expect("a kind"),
            parent_id: Some(epic_root_id),
            task_id: None,
            created_at,
        })
        .expect("the ECP node is created");
    let original = store
        .bind_topology_node_container(&NewNativeContainerBinding {
            topology_node_id: node_id,
            project_id,
            container_binding_id: ExternalId::parse("binding-ecp").expect("a binding id"),
            identity: identity("wks_stale"),
            observed_kind: ObservedContainerKind::Workspace,
            canonical_cwd: Some(name("/tmp/container-recovery-project/epic")),
            observed_at: created_at,
        })
        .expect("the stale identity is initially bound");
    let replacement = NewNativeContainerBinding {
        topology_node_id: node_id,
        project_id,
        container_binding_id: original.container_binding_id.clone(),
        identity: identity("wks_live"),
        observed_kind: ObservedContainerKind::Workspace,
        canonical_cwd: original.canonical_cwd.clone(),
        observed_at: at("2026-09-04T09:00:00Z"),
    };
    let recovery = TopologyContainerRecovery {
        expected: original.clone(),
        replacement,
        parent_native_id: ExternalId::parse("prj_epic").expect("a parent id"),
        observed_title: name("ECP • KOP-8001"),
    };
    let command = local_command(
        project_id,
        "recover-stale-ecp",
        CommandKind::RecoverTopologyContainer,
        AggregateRef::Project { project_id },
        project.revision,
    );
    let envelope = ReceiptEnvelope::new(store.realm(), command);
    let (evidence, receipt, applied) = store
        .recover_topology_container_with_intent(&recovery, project.revision, &envelope)
        .expect("the exact recovery commits");
    assert_eq!(applied, kontor_store::Applied::Created);
    assert_eq!(evidence.prior_identity.native_id.as_str(), "wks_stale");
    assert_eq!(evidence.replacement_identity.native_id.as_str(), "wks_live");
    assert_eq!(evidence.container_binding_id, original.container_binding_id);
    let current = store
        .get_topology_node_container(project_id, node_id)
        .expect("the binding reads")
        .expect("the binding remains");
    assert_eq!(current.identity.native_id.as_str(), "wks_live");
    assert_eq!(current.container_binding_id, original.container_binding_id);

    let (again, replayed_receipt, replayed) = store
        .recover_topology_container_with_intent(&recovery, project.revision, &envelope)
        .expect("the exact command replays from immutable history");
    assert_eq!(again, evidence);
    assert_eq!(replayed_receipt.id, receipt.id);
    assert_eq!(replayed, kontor_store::Applied::Unchanged);
}
