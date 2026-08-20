//! The container-retitle contract.
//!
//! A native container's title is the one thing about it that humans read, and
//! on most runtimes it is fixed at creation. This is the operation that
//! corrects one *without* touching the identity every binding resolves by —
//! and the capability a runtime declares when it genuinely can.

use std::collections::BTreeSet;

use kontor_core::id::{
    ContentHash, ExternalId, ExternalName, MiniProjectId, SpecVersion, TaskId, TopologyNodeId,
    TopologySpecId,
};
use kontor_core::spec::{NodeProjectionCapability, TopologySnapshot};
use kontor_runtime::adapter::{RuntimeAdapter, RuntimeError};
use kontor_runtime::capability::{
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::container::{ContainerBindingId, ContainerRequest, RetitleContainerRequest};
use kontor_runtime::fake::{AdapterCall, ScriptedFakeRuntime};
use kontor_runtime::scope::{EpicScope, ExecutionScope};

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
        scope: ExecutionScope::for_epic(EpicScope {
            mini_project_id: MiniProjectId::generate(),
            external_epic_key: ExternalId::parse("ASMA-RETITLE").expect("epic key"),
            short_title: name("Retitle contract"),
        }),
        capabilities: vec![NodeProjectionCapability::NativeRoot],
        display_name: name(title),
        parent: None,
        cwd: None,
        bound_native_id: None,
        epic_container: true,
        task_id: None,
        team_run_id: None,
        requested_at: at("2026-08-17T09:00:00Z"),
    }
}

fn retitle(
    node_id: TopologyNodeId,
    native_id: &str,
    generation: u64,
    desired_title: &str,
) -> RetitleContainerRequest {
    RetitleContainerRequest {
        topology_node_id: node_id,
        container_binding_id: ContainerBindingId::generate(),
        projection: kontor_runtime::container::ContainerProjection::NativeChild,
        bound_native_id: kontor_core::id::ExternalId::parse(native_id).expect("a native id"),
        bound_project_native_id: Some(
            kontor_core::id::ExternalId::parse("native-project-1").expect("a native project id"),
        ),
        generation,
        desired_title: name(desired_title),
        requested_at: at("2026-08-17T09:05:00Z"),
    }
}

/// The same request, for a container that belongs to a delivery task.
fn retitle_for_task(
    node_id: TopologyNodeId,
    native_id: &str,
    generation: u64,
    desired_title: &str,
    _task_id: TaskId,
) -> RetitleContainerRequest {
    retitle(node_id, native_id, generation, desired_title)
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
        outcome.desired_title.as_str(),
        "PSW · Kontor",
        "the runtime answers with the title it derived"
    );
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

/// A task-scoped container consumes the daemon-rendered title verbatim.
#[tokio::test]
async fn a_task_scoped_container_is_titled_from_the_planes_scope() {
    let runtime = ScriptedFakeRuntime::new(every_capability());
    let node_id = TopologyNodeId::generate();
    let task_id = TaskId::generate();
    runtime.scope_task_title(task_id, "ASMA-7872 · OP-03");
    let prepared = runtime
        .prepare_container(&container_request(node_id, "before"))
        .await
        .expect("the container is prepared");
    let native = prepared.snapshot.binding.identity.clone();

    let outcome = runtime
        .retitle_container(&retitle_for_task(
            node_id,
            native.native_id.as_str(),
            native.generation,
            "TSW • ASMA-7872 • OP-03",
            task_id,
        ))
        .await
        .expect("the container is retitled");

    assert_eq!(
        outcome.desired_title.as_str(),
        "TSW • ASMA-7872 • OP-03",
        "the runtime must not recompute the caller-rendered title"
    );
    assert_eq!(outcome.observed_title, "TSW • ASMA-7872 • OP-03");
    assert_eq!(outcome.snapshot.binding.identity, native);
}

/// A runtime needs no local naming scope once the daemon supplies the title.
#[tokio::test]
async fn a_task_with_no_scope_on_this_plane_uses_the_supplied_title() {
    let runtime = ScriptedFakeRuntime::new(every_capability());
    let node_id = TopologyNodeId::generate();
    let prepared = runtime
        .prepare_container(&container_request(node_id, "before"))
        .await
        .expect("the container is prepared");
    let native = prepared.snapshot.binding.identity.clone();

    let outcome = runtime
        .retitle_container(&retitle_for_task(
            node_id,
            native.native_id.as_str(),
            native.generation,
            "TSW • ASMA-7872 • OP-03",
            TaskId::generate(),
        ))
        .await
        .expect("the finished title needs no adapter-side scope");
    assert_eq!(outcome.observed_title, "TSW • ASMA-7872 • OP-03");
    assert_eq!(
        runtime.container_title(node_id).as_deref(),
        Some("TSW • ASMA-7872 • OP-03"),
        "the supplied title is applied verbatim"
    );
}

/// A preview answers what an apply would do and writes nothing.
#[tokio::test]
async fn a_preview_answers_the_same_thing_and_changes_nothing() {
    let runtime = ScriptedFakeRuntime::new(every_capability());
    let node_id = TopologyNodeId::generate();
    let task_id = TaskId::generate();
    runtime.scope_task_title(task_id, "ASMA-7872 · OP-03");
    let prepared = runtime
        .prepare_container(&container_request(
            node_id,
            "Ticket Session Workspace · 0189",
        ))
        .await
        .expect("the container is prepared");
    let native = prepared.snapshot.binding.identity.clone();
    let request = retitle_for_task(
        node_id,
        native.native_id.as_str(),
        native.generation,
        "TSW • ASMA-7872 • OP-03",
        task_id,
    );

    let preview = runtime
        .preview_retitle_container(&request)
        .await
        .expect("the preview is answered");
    assert!(preview.changed, "the container is not correct yet");
    assert_eq!(preview.desired_title.as_str(), "TSW • ASMA-7872 • OP-03");
    assert_eq!(
        preview.observed_title, "Ticket Session Workspace · 0189",
        "a preview reports what the container carries now"
    );
    assert_eq!(
        runtime.container_title(node_id).as_deref(),
        Some("Ticket Session Workspace · 0189"),
        "a preview must not have written anything"
    );
    assert!(
        runtime
            .calls()
            .contains(&AdapterCall::PreviewRetitleContainer(node_id)),
        "the preview is recorded as a preview"
    );
    assert!(
        !runtime
            .calls()
            .contains(&AdapterCall::RetitleContainer(node_id)),
        "a preview is not an apply"
    );

    let applied = runtime
        .retitle_container(&request)
        .await
        .expect("the apply is answered");
    assert_eq!(applied.desired_title, preview.desired_title);
    assert_eq!(applied.observed_title, preview.desired_title.as_str());

    // And now the preview says so, which is what makes it worth reading twice.
    let settled = runtime
        .preview_retitle_container(&request)
        .await
        .expect("the second preview is answered");
    assert!(!settled.changed, "the container is already correct");
    assert_eq!(settled.observed_title, "TSW • ASMA-7872 • OP-03");
}

/// A preview refuses for every reason an apply refuses, including a runtime that
/// cannot rename at all.
#[tokio::test]
async fn a_preview_refuses_what_an_apply_would_refuse() {
    let declared: Vec<RuntimeCapability> = RuntimeCapability::ALL
        .iter()
        .copied()
        .filter(|capability| *capability != RuntimeCapability::RetitleContainer)
        .collect();
    let runtime = ScriptedFakeRuntime::new(capabilities(&declared));
    let node_id = TopologyNodeId::generate();
    let prepared = runtime
        .prepare_container(&container_request(node_id, "before"))
        .await
        .expect("the container is prepared");
    let native = prepared.snapshot.binding.identity.clone();

    let refused = runtime
        .preview_retitle_container(&retitle(
            node_id,
            native.native_id.as_str(),
            native.generation,
            "after",
        ))
        .await;
    assert!(
        matches!(
            refused,
            Err(RuntimeError::UnsupportedCapability {
                capability: RuntimeCapability::RetitleContainer
            })
        ),
        "a preview against a runtime that cannot rename promises nothing: {refused:?}"
    );

    let stale = ScriptedFakeRuntime::new(every_capability());
    stale
        .prepare_container(&container_request(node_id, "before"))
        .await
        .expect("the container is prepared");
    let wrong_generation = stale
        .preview_retitle_container(&retitle(
            node_id,
            native.native_id.as_str(),
            native.generation + 1,
            "after",
        ))
        .await;
    assert!(
        matches!(wrong_generation, Err(RuntimeError::StaleBinding { .. })),
        "a preview addresses the container the same way an apply does: {wrong_generation:?}"
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
