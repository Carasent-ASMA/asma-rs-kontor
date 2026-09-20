//! The shared bound-container contract, as the fake implements it.
//!
//! A request carrying `bound_native_id` addresses one exact container and
//! nothing else. The fake has to answer it the way a wire plane does, because
//! every test that uses the fake in place of a runtime is only as honest as
//! that agreement — a fake that accepted what Paseo refuses would let a suite
//! pass against a container no real runtime would ever have bound.
//!
//! The predicate both planes share is
//! [`ContainerWorkspaceKind::is_applicable_to`]; these tests pin the fake's
//! half of it. The Paseo half is pinned in that crate's contract suite against
//! the same two semantics.

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
use kontor_runtime::container::{
    ContainerBinding, ContainerBindingId, ContainerProjection, ContainerRequest,
    ContainerWorkspaceKind,
};
use kontor_runtime::fake::ScriptedFakeRuntime;
use kontor_runtime::scope::{EpicScope, ExecutionScope, TaskScope};
use kontor_runtime::workspace::WorkspaceRoot;

const CWD: &str = "/w/epic/task-11";

fn at(text: &str) -> kontor_core::id::Timestamp {
    kontor_core::id::parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn every_capability() -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 4096,
            max_history_page: 64,
            max_concurrent_sessions: 8,
            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
        },
    }
}

fn topology() -> TopologySnapshot {
    TopologySnapshot {
        spec_id: TopologySpecId::parse("01936f5a-1000-7000-8000-000000000001")
            .expect("a canonical spec id"),
        version: SpecVersion::parse(1).expect("a version"),
        canonical_hash: ContentHash::of(b"topology"),
    }
}

fn epic_scope() -> EpicScope {
    EpicScope {
        mini_project_id: MiniProjectId::generate(),
        external_epic_key: ExternalId::parse("ASMA-8234").expect("epic key"),
        short_title: name("Bound container contract"),
    }
}

/// A ticket container's scope: it names the worktree the work happens in.
fn task_scope() -> ExecutionScope {
    ExecutionScope::for_task(
        epic_scope(),
        TaskScope {
            task_id: TaskId::generate(),
            external_issue_key: ExternalId::parse("ASMA-8234").expect("issue key"),
            short_code: None,
            worktree: WorkspaceRoot::parse(CWD).expect("an absolute path"),
        },
    )
}

/// An epic node's scope: it serves no ticket at all.
fn node_scope() -> ExecutionScope {
    ExecutionScope::for_epic(epic_scope())
}

fn parent_binding() -> ContainerBinding {
    ContainerBinding {
        id: ContainerBindingId::generate(),
        topology_node_id: TopologyNodeId::generate(),
        projection: ContainerProjection::NativeRoot,
        identity: kontor_core::state::NativeRuntimeIdentity {
            runtime_kind: kontor_core::id::RuntimeKindKey::parse("fake.runtime")
                .expect("a runtime kind"),
            host: name("fake-host"),
            generation: 1,
            native_id: ExternalId::parse("native-project-1").expect("a native id"),
        },
        root: None,
        bound_at: at("2026-08-17T09:00:00Z"),
    }
}

fn child_request(node_id: TopologyNodeId, scope: ExecutionScope) -> ContainerRequest {
    ContainerRequest {
        container_binding_id: ContainerBindingId::generate(),
        topology_node_id: node_id,
        topology: topology(),
        scope,
        capabilities: vec![NodeProjectionCapability::NativeChild],
        display_name: name("A bound container"),
        parent: Some(parent_binding()),
        cwd: Some(WorkspaceRoot::parse(CWD).expect("an absolute path")),
        bound_native_id: None,
        epic_container: false,
        task_id: None,
        team_run_id: None,
        requested_at: at("2026-08-17T09:05:00Z"),
    }
}

/// Create a container, then address it by the id the runtime minted.
async fn bound_after_create(
    fake: &ScriptedFakeRuntime,
    node_id: TopologyNodeId,
    scope: ExecutionScope,
) -> ContainerRequest {
    let created = fake
        .prepare_container(&child_request(node_id, scope.clone()))
        .await
        .expect("the container is created");
    ContainerRequest {
        bound_native_id: Some(created.snapshot.binding.identity.native_id.clone()),
        ..child_request(node_id, scope)
    }
}

/// The scope, not a flag and not tracker metadata, is what says which
/// semantics a container has.
#[test]
fn the_container_semantics_come_from_the_durable_scope() {
    let node_id = TopologyNodeId::generate();
    assert!(
        child_request(node_id, task_scope()).task_container(),
        "a ticket scope makes this a ticket container"
    );
    assert!(
        !child_request(node_id, node_scope()).task_container(),
        "an epic scope serves no ticket"
    );
}

/// The shared predicate itself, stated once as a table.
#[test]
fn the_shared_predicate_admits_exactly_one_shape_per_semantics() {
    use ContainerWorkspaceKind as K;
    for (kind, ticket, epic) in [
        (K::Worktree, true, false),
        (K::Directory, false, true),
        (K::LocalCheckout, false, true),
        (K::Checkout, false, false),
        (K::Other, false, false),
    ] {
        assert_eq!(kind.is_applicable_to(true), ticket, "{kind:?} for a ticket");
        assert_eq!(
            kind.is_applicable_to(false),
            epic,
            "{kind:?} for an epic node"
        );
    }
}

/// A container this runtime made takes a shape its own semantics allow, so a
/// bound re-preparation of it is answered rather than refused.
#[tokio::test]
async fn a_bound_container_of_either_semantics_reconciles_by_exact_id() {
    for scope in [task_scope(), node_scope()] {
        let fake = ScriptedFakeRuntime::new(every_capability());
        let node_id = TopologyNodeId::generate();
        let bound = bound_after_create(&fake, node_id, scope).await;

        let outcome = fake
            .prepare_container(&bound)
            .await
            .expect("the exact bound container is re-proved");

        assert!(!outcome.created, "reconciling creates nothing");
        assert_eq!(
            Some(&outcome.snapshot.binding.identity.native_id),
            bound.bound_native_id.as_ref()
        );
    }
}

/// Lockstep: a ticket container whose native is a plain directory is refused
/// by the fake exactly as the wire plane refuses it.
#[tokio::test]
async fn the_fake_refuses_a_ticket_container_that_is_not_a_worktree() {
    for kind in [
        ContainerWorkspaceKind::Directory,
        ContainerWorkspaceKind::LocalCheckout,
        ContainerWorkspaceKind::Checkout,
        ContainerWorkspaceKind::Other,
    ] {
        let fake = ScriptedFakeRuntime::new(every_capability());
        let node_id = TopologyNodeId::generate();
        let bound = bound_after_create(&fake, node_id, task_scope()).await;
        fake.seed_container_kind(node_id, kind);

        let error = fake
            .prepare_container(&bound)
            .await
            .expect_err("a ticket container must be a worktree");

        assert!(
            matches!(error, RuntimeError::StaleBinding { .. }),
            "{kind:?} is stale for a ticket container: {error:?}"
        );
    }
}

/// Lockstep: an epic container is a directory or a local checkout, and is
/// refused when it is somebody's worktree.
#[tokio::test]
async fn the_fake_accepts_an_epic_directory_and_refuses_an_epic_worktree() {
    for kind in [
        ContainerWorkspaceKind::Directory,
        ContainerWorkspaceKind::LocalCheckout,
    ] {
        let fake = ScriptedFakeRuntime::new(every_capability());
        let node_id = TopologyNodeId::generate();
        let bound = bound_after_create(&fake, node_id, node_scope()).await;
        fake.seed_container_kind(node_id, kind);

        fake.prepare_container(&bound)
            .await
            .unwrap_or_else(|error| panic!("an epic container may be {kind:?}: {error:?}"));
    }

    for kind in [
        ContainerWorkspaceKind::Worktree,
        ContainerWorkspaceKind::Checkout,
        ContainerWorkspaceKind::Other,
    ] {
        let fake = ScriptedFakeRuntime::new(every_capability());
        let node_id = TopologyNodeId::generate();
        let bound = bound_after_create(&fake, node_id, node_scope()).await;
        fake.seed_container_kind(node_id, kind);

        let error = fake
            .prepare_container(&bound)
            .await
            .expect_err("an epic container is not a worktree");
        assert!(
            matches!(error, RuntimeError::StaleBinding { .. }),
            "{kind:?} is stale for an epic container: {error:?}"
        );
    }
}

/// Lockstep, item 4: a bound id this runtime does not hold is stale, and never
/// a licence to mint a replacement beside the one that went missing.
#[tokio::test]
async fn the_fake_refuses_a_bound_id_it_does_not_hold() {
    let fake = ScriptedFakeRuntime::new(every_capability());
    let node_id = TopologyNodeId::generate();
    let bound = ContainerRequest {
        bound_native_id: Some(ExternalId::parse("native-never-made").expect("a native id")),
        ..child_request(node_id, task_scope())
    };

    let error = fake
        .prepare_container(&bound)
        .await
        .expect_err("an absent bound container is stale");

    assert!(
        matches!(error, RuntimeError::StaleBinding { .. }),
        "{error:?}"
    );
}

/// Lockstep: a bound id that does not match the one this runtime holds is
/// stale rather than silently adopted.
#[tokio::test]
async fn the_fake_refuses_a_bound_id_that_does_not_match_what_it_holds() {
    let fake = ScriptedFakeRuntime::new(every_capability());
    let node_id = TopologyNodeId::generate();
    let _ = bound_after_create(&fake, node_id, task_scope()).await;

    let mismatched = ContainerRequest {
        bound_native_id: Some(ExternalId::parse("native-someone-else").expect("a native id")),
        ..child_request(node_id, task_scope())
    };

    let error = fake
        .prepare_container(&mismatched)
        .await
        .expect_err("a mismatched bound id is stale");

    assert!(
        matches!(error, RuntimeError::StaleBinding { .. }),
        "{error:?}"
    );
}
