//! The container-retitle contract.
//!
//! A native container's title is the one thing about it that humans read, and
//! on most runtimes it is fixed at creation. This is the operation that
//! corrects one *without* touching the identity every binding resolves by —
//! and the capability a runtime declares when it genuinely can.

use std::collections::BTreeSet;

use kontor_core::id::{ContentHash, ExternalName, SpecVersion, TopologyNodeId, TopologySpecId};
use kontor_core::spec::{NodeProjectionCapability, TopologySnapshot};
use kontor_runtime::adapter::{RuntimeAdapter, RuntimeError};
use kontor_runtime::capability::{
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::container::{ContainerBindingId, ContainerRequest, RetitleContainerRequest};
use kontor_runtime::fake::ScriptedFakeRuntime;

fn at(text: &str) -> kontor_core::id::Timestamp {
    kontor_core::id::parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn capabilities(declared: &[RuntimeCapability]) -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: declared.iter().copied().collect::<BTreeSet<_>>(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 4096,
            max_history_page: 64,
            max_concurrent_sessions: 8,
            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
        },
    }
}

fn every_capability() -> RuntimeCapabilities {
    capabilities(RuntimeCapability::ALL)
}

fn topology() -> TopologySnapshot {
    TopologySnapshot {
        spec_id: TopologySpecId::parse("01936f5a-1000-7000-8000-000000000001")
            .expect("a canonical spec id"),
        version: SpecVersion::parse(1).expect("a version"),
        canonical_hash: ContentHash::of(b"topology"),
    }
}

fn container_request(node_id: TopologyNodeId, title: &str) -> ContainerRequest {
    ContainerRequest {
        container_binding_id: ContainerBindingId::generate(),
        topology_node_id: node_id,
        topology: topology(),
        capabilities: vec![NodeProjectionCapability::NativeRoot],
        display_name: name(title),
        parent: None,
        cwd: None,
        bound_native_id: None,
        task_id: None,
        team_run_id: None,
        requested_at: at("2026-08-17T09:00:00Z"),
    }
}

fn retitle(
    node_id: TopologyNodeId,
    native_id: &str,
    generation: u64,
    title: &str,
) -> RetitleContainerRequest {
    RetitleContainerRequest {
        topology_node_id: node_id,
        bound_native_id: kontor_core::id::ExternalId::parse(native_id).expect("a native id"),
        generation,
        desired_title: name(title),
        requested_at: at("2026-08-17T09:05:00Z"),
    }
}

/// A retitle changes the name and keeps the identity, and it says which
/// happened.
#[tokio::test]
async fn a_retitle_keeps_the_identity_and_reports_what_it_read_back() {
    let runtime = ScriptedFakeRuntime::new(every_capability());
    let node_id = TopologyNodeId::generate();
    let prepared = runtime
        .prepare_container(&container_request(
            node_id,
            "Project Session Workspace · 0189",
        ))
        .await
        .expect("the container is prepared");
    let native = prepared.snapshot.binding.identity.clone();

    let outcome = runtime
        .retitle_container(&retitle(
            node_id,
            native.native_id.as_str(),
            native.generation,
            "PSW · Kontor",
        ))
        .await
        .expect("the container is retitled");

    assert!(outcome.changed, "the title actually differed");
    assert_eq!(
        outcome.observed_title, "PSW · Kontor",
        "the title is read back, not echoed"
    );
    // The whole point: the handle every binding resolves by is untouched.
    assert_eq!(outcome.snapshot.binding.identity, native);
    assert_eq!(
        outcome.snapshot.binding.root,
        prepared.snapshot.binding.root
    );
    assert_eq!(
        outcome.snapshot.binding.projection,
        prepared.snapshot.binding.projection
    );
    assert_eq!(outcome.snapshot.topology_node_id(), node_id);

    // Replaying it is the goal already met, not a second change.
    let replay = runtime
        .retitle_container(&retitle(
            node_id,
            native.native_id.as_str(),
            native.generation,
            "PSW · Kontor",
        ))
        .await
        .expect("a replay is answered");
    assert!(!replay.changed, "the container was already correct");
    assert_eq!(replay.observed_title, "PSW · Kontor");
    assert_eq!(replay.snapshot.binding.identity, native);
}

/// The container is addressed by its exact native id, never by anything else.
#[tokio::test]
async fn a_retitle_refuses_an_id_or_generation_it_did_not_bind() {
    let runtime = ScriptedFakeRuntime::new(every_capability());
    let node_id = TopologyNodeId::generate();
    let prepared = runtime
        .prepare_container(&container_request(node_id, "before"))
        .await
        .expect("the container is prepared");
    let native = prepared.snapshot.binding.identity.clone();

    // Another container's id under this node.
    let wrong_id = runtime
        .retitle_container(&retitle(
            node_id,
            "native-container-99",
            native.generation,
            "after",
        ))
        .await;
    assert!(
        matches!(wrong_id, Err(RuntimeError::StaleBinding { .. })),
        "a native id this node does not hold must be refused: {wrong_id:?}"
    );

    // The right id in a generation that is not the one it was bound in — which
    // after a restart names whatever replaced it.
    let wrong_generation = runtime
        .retitle_container(&retitle(
            node_id,
            native.native_id.as_str(),
            native.generation + 1,
            "after",
        ))
        .await;
    assert!(
        matches!(wrong_generation, Err(RuntimeError::StaleBinding { .. })),
        "an id from another generation must be refused: {wrong_generation:?}"
    );

    // A node holding no container at all has nothing to retitle.
    let unplaced = runtime
        .retitle_container(&retitle(
            TopologyNodeId::generate(),
            native.native_id.as_str(),
            native.generation,
            "after",
        ))
        .await;
    assert!(matches!(unplaced, Err(RuntimeError::StaleBinding { .. })));
}

/// A runtime that does not declare the capability refuses, and says which one.
#[tokio::test]
async fn a_runtime_that_does_not_declare_the_capability_refuses_by_name() {
    let declared: Vec<RuntimeCapability> = RuntimeCapability::ALL
        .iter()
        .copied()
        .filter(|capability| *capability != RuntimeCapability::RetitleContainer)
        .collect();
    let runtime = ScriptedFakeRuntime::new(capabilities(&declared));
    let node_id = TopologyNodeId::generate();
    runtime
        .prepare_container(&container_request(node_id, "before"))
        .await
        .expect("the container is prepared");

    let refused = runtime
        .retitle_container(&retitle(node_id, "native-container-1", 1, "after"))
        .await;
    assert!(
        matches!(
            refused,
            Err(RuntimeError::UnsupportedCapability {
                capability: RuntimeCapability::RetitleContainer
            })
        ),
        "the refusal must name the capability that is missing: {refused:?}"
    );
}

/// The capability is a change to the runtime, so a read-only grade cannot drive
/// it.
#[test]
fn retitling_is_a_change_and_is_declared_as_one() {
    assert!(
        RuntimeCapability::RetitleContainer.changes_runtime(),
        "a retitle writes to the runtime and must be graded as one"
    );
    assert!(
        RuntimeCapability::ALL.contains(&RuntimeCapability::RetitleContainer),
        "the capability must be in the closed set a runtime can declare"
    );
    assert_eq!(
        RuntimeCapability::RetitleContainer.as_str(),
        "retitle_container"
    );
}
