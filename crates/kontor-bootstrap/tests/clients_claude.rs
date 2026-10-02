//! The Claude Code adapter matrix over its injected command boundary.
//!
//! Nothing here spawns `claude`: the boundary is a fake, and the exact argv a
//! production boundary would receive is asserted directly. Replacement is
//! race- and failure-safe, so a lost race or a failed re-add never deletes or
//! leaves a half-replaced entry.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use kontor_bootstrap::client::claude::{
    ClaudeBoundary, ClaudeBoundaryError, ClaudeCall, ENTRY_NAME, FakeClaudeBoundary, add_json_argv,
    get_argv, remove_argv,
};
use kontor_bootstrap::client::desired_entry;
use kontor_bootstrap::{
    AdapterError, AdapterResult, ClaudeAdapter, ClientAdapter, ClientId, ConflictReason,
    EntryState, NoFaults, ObservedHash, OwnershipStore, ServerSpec,
};

fn spec(state_root: &str) -> ServerSpec {
    ServerSpec {
        program: PathBuf::from("/opt/kontor tools/current/kontor-mcp"),
        args: vec![
            "--state-root".to_owned(),
            state_root.to_owned(),
            "--credential-tier".to_owned(),
            "admin".to_owned(),
        ],
    }
}

fn managed() -> ServerSpec {
    spec("/synthetic/realm")
}

struct Harness {
    _holder: tempfile::TempDir,
    ownership: OwnershipStore,
    adapter: ClaudeAdapter,
}

fn harness(boundary: impl ClaudeBoundary + 'static) -> Harness {
    let holder = tempfile::tempdir().expect("tempdir");
    let ownership = OwnershipStore::at(holder.path().join("state")).expect("ownership store");
    let adapter = ClaudeAdapter::new(Box::new(boundary), ownership.clone());
    Harness {
        _holder: holder,
        ownership,
        adapter,
    }
}

#[test]
fn the_exact_documented_argv_is_what_a_boundary_receives() {
    assert_eq!(
        add_json_argv("kontor", "{\"type\":\"stdio\"}"),
        vec![
            "mcp",
            "add-json",
            "kontor",
            "--scope",
            "user",
            "{\"type\":\"stdio\"}"
        ]
    );
    assert_eq!(get_argv("kontor"), vec!["mcp", "get", "kontor"]);
    assert_eq!(
        remove_argv("kontor"),
        vec!["mcp", "remove", "kontor", "--scope", "user"]
    );
}

#[test]
fn a_clean_install_writes_reads_back_and_records_ownership() {
    let mut harness = harness(FakeClaudeBoundary::new());
    assert_eq!(
        harness.adapter.inspect(&managed()).expect("inspect"),
        EntryState::EntryAbsent
    );
    assert_eq!(
        harness
            .adapter
            .install(&managed(), &mut NoFaults)
            .expect("install"),
        AdapterResult::Installed
    );
    assert!(harness.ownership.load().expect("ledger").owns(
        ClientId::ClaudeCode,
        &ObservedHash::of_value(&desired_entry(ClientId::ClaudeCode, &managed()))
    ));
    assert_eq!(
        harness
            .adapter
            .install(&managed(), &mut NoFaults)
            .expect("replay"),
        AdapterResult::AlreadyCurrent
    );
}

#[test]
fn an_identical_entry_without_provenance_is_current_and_never_rewritten() {
    let mut boundary = FakeClaudeBoundary::new();
    boundary.seed(ENTRY_NAME, desired_entry(ClientId::ClaudeCode, &managed()));
    let mut harness = harness(boundary);
    assert!(matches!(
        harness.adapter.inspect(&managed()).expect("inspect"),
        EntryState::Current { .. }
    ));
    assert_eq!(
        harness
            .adapter
            .install(&managed(), &mut NoFaults)
            .expect("install"),
        AdapterResult::AlreadyCurrent
    );
}

#[test]
fn an_owned_entry_is_updated_through_revalidate_remove_add_and_readback() {
    let mut harness = harness(FakeClaudeBoundary::new());
    let first = spec("/synthetic/one");
    let second = spec("/synthetic/two");
    assert_eq!(
        harness
            .adapter
            .install(&first, &mut NoFaults)
            .expect("install"),
        AdapterResult::Installed
    );
    assert_eq!(
        harness
            .adapter
            .install(&second, &mut NoFaults)
            .expect("update"),
        AdapterResult::Updated
    );
    assert!(harness.ownership.load().expect("ledger").owns(
        ClientId::ClaudeCode,
        &ObservedHash::of_value(&desired_entry(ClientId::ClaudeCode, &second))
    ));
}

#[test]
fn a_same_name_unrelated_entry_is_conflict_and_repairs_only_with_its_hash() {
    let mut boundary = FakeClaudeBoundary::new();
    boundary.seed(
        ENTRY_NAME,
        json!({"type": "stdio", "command": "/usr/bin/other-mcp", "args": ["--own"]}),
    );
    let mut harness = harness(boundary);
    let observed = match harness.adapter.inspect(&managed()).expect("inspect") {
        EntryState::Unrelated { observed } => observed,
        other => panic!("expected unrelated, got {other:?}"),
    };
    assert_eq!(
        harness
            .adapter
            .install(&managed(), &mut NoFaults)
            .expect("install"),
        AdapterResult::Conflict {
            reason: ConflictReason::SameNameUnrelated,
            observed: Some(observed.clone()),
        }
    );
    assert_eq!(
        harness
            .adapter
            .repair(
                &managed(),
                &ObservedHash::parse(&"a".repeat(64)).expect("hash"),
                &mut NoFaults
            )
            .expect("repair"),
        AdapterResult::Conflict {
            reason: ConflictReason::ObservedHashMismatch,
            observed: Some(observed.clone()),
        }
    );
    assert_eq!(
        harness
            .adapter
            .repair(&managed(), &observed, &mut NoFaults)
            .expect("repair"),
        AdapterResult::Repaired
    );
}

#[test]
fn a_basename_does_not_claim_ownership() {
    let mut boundary = FakeClaudeBoundary::new();
    boundary.seed(
        ENTRY_NAME,
        json!({"type": "stdio", "command": "/elsewhere/kontor-mcp", "args": []}),
    );
    let mut harness = harness(boundary);
    assert!(matches!(
        harness.adapter.inspect(&managed()).expect("inspect"),
        EntryState::Unrelated { .. }
    ));
    assert!(matches!(
        harness
            .adapter
            .install(&managed(), &mut NoFaults)
            .expect("install"),
        AdapterResult::Conflict {
            reason: ConflictReason::SameNameUnrelated,
            ..
        }
    ));
}

/// A boundary that changes its answer between the observation and the
/// revalidation that guards removal.
struct RaceBoundary {
    inner: FakeClaudeBoundary,
    phase: std::sync::Arc<AtomicUsize>,
    reads: AtomicUsize,
    first: Value,
    second: Value,
}

impl ClaudeBoundary for RaceBoundary {
    fn get(&mut self, name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
        if self.phase.load(Ordering::SeqCst) == 0 {
            return self.inner.get(name);
        }
        let read = self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(Some(if read == 0 {
            self.first.clone()
        } else {
            self.second.clone()
        }))
    }

    fn add_json(&mut self, name: &str, json: &str) -> Result<(), ClaudeBoundaryError> {
        self.inner.add_json(name, json)
    }

    fn remove(&mut self, name: &str) -> Result<(), ClaudeBoundaryError> {
        self.inner.remove(name)
    }
}

#[test]
fn a_race_between_observation_and_removal_never_deletes_the_current_entry() {
    let first = desired_entry(ClientId::ClaudeCode, &spec("/synthetic/one"));
    let second = desired_entry(ClientId::ClaudeCode, &spec("/synthetic/two"));
    let phase = std::sync::Arc::new(AtomicUsize::new(0));
    let holder = tempfile::tempdir().expect("tempdir");
    let ownership = OwnershipStore::at(holder.path().join("state")).expect("store");
    let mut adapter = ClaudeAdapter::new(
        Box::new(RaceBoundary {
            inner: FakeClaudeBoundary::new(),
            phase: phase.clone(),
            reads: AtomicUsize::new(0),
            first: first.clone(),
            second: second.clone(),
        }),
        ownership,
    );
    // Phase one: a clean install records ownership of the first snapshot.
    assert_eq!(
        adapter
            .install(&spec("/synthetic/one"), &mut NoFaults)
            .expect("install"),
        AdapterResult::Installed
    );
    // Phase two: the first read reports the owned snapshot, the revalidation
    // reports different bytes. The adapter must refuse without removing.
    phase.store(1, Ordering::SeqCst);
    let result = adapter
        .install(&spec("/synthetic/two"), &mut NoFaults)
        .expect("install");
    match result {
        AdapterResult::Conflict {
            reason: ConflictReason::ConcurrentChange,
            ..
        } => {}
        other => panic!("a lost race must be a typed conflict, got {other:?}"),
    }
}

/// A boundary that fails one specific add and accepts restores.
struct SelectiveBoundary {
    inner: FakeClaudeBoundary,
    fail_contains: String,
    fail_all_after: Option<usize>,
    adds: usize,
}

impl ClaudeBoundary for SelectiveBoundary {
    fn get(&mut self, name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
        self.inner.get(name)
    }

    fn add_json(&mut self, name: &str, json: &str) -> Result<(), ClaudeBoundaryError> {
        self.adds += 1;
        let fail = json.contains(&self.fail_contains)
            || self.fail_all_after.is_some_and(|limit| self.adds > limit);
        if fail {
            return Err(ClaudeBoundaryError::Failed);
        }
        self.inner.add_json(name, json)
    }

    fn remove(&mut self, name: &str) -> Result<(), ClaudeBoundaryError> {
        self.inner.remove(name)
    }
}

#[test]
fn a_failed_replacement_restores_the_exact_original_entry() {
    let original = json!({"type": "stdio", "command": "/old/kontor-mcp", "args": []});
    let mut inner = FakeClaudeBoundary::new();
    inner.seed(ENTRY_NAME, original.clone());
    let boundary = SelectiveBoundary {
        inner,
        fail_contains: "/synthetic/two".to_owned(),
        fail_all_after: None,
        adds: 0,
    };
    let mut harness = harness(boundary);
    // Own the seeded entry through an explicit repair of its exact digest.
    let hash = ObservedHash::of_value(&original);
    assert_eq!(
        harness
            .adapter
            .repair(&spec("/synthetic/one"), &hash, &mut NoFaults)
            .expect("repair"),
        AdapterResult::Repaired
    );
    let owned = desired_entry(ClientId::ClaudeCode, &spec("/synthetic/one"));
    assert_eq!(
        harness
            .adapter
            .install(&spec("/synthetic/two"), &mut NoFaults)
            .expect("install"),
        AdapterResult::Conflict {
            reason: ConflictReason::ReplacementFailed,
            observed: Some(ObservedHash::of_value(&owned)),
        }
    );
    // The exact original entry was restored.
    assert!(
        matches!(
            harness
                .adapter
                .inspect(&spec("/synthetic/one"))
                .expect("inspect"),
            EntryState::Current { .. }
        ),
        "the original entry must be restored after a failed replacement"
    );
}

#[test]
fn a_failed_replacement_that_cannot_restore_is_a_typed_error() {
    let original = json!({"type": "stdio", "command": "/old/kontor-mcp", "args": []});
    let mut inner = FakeClaudeBoundary::new();
    inner.seed(ENTRY_NAME, original);
    let boundary = SelectiveBoundary {
        inner,
        fail_contains: "/synthetic/two".to_owned(),
        fail_all_after: Some(1),
        adds: 0,
    };
    let mut harness = harness(boundary);
    let hash =
        ObservedHash::of_value(&json!({"type": "stdio", "command": "/old/kontor-mcp", "args": []}));
    assert_eq!(
        harness
            .adapter
            .repair(&spec("/synthetic/one"), &hash, &mut NoFaults)
            .expect("repair"),
        AdapterResult::Repaired
    );
    assert_eq!(
        harness
            .adapter
            .install(&spec("/synthetic/two"), &mut NoFaults)
            .expect_err("restore failure is typed"),
        AdapterError::RestoreFailed
    );
}

#[test]
fn an_unavailable_client_is_absent_and_never_a_silent_success() {
    let mut harness = harness(FakeClaudeBoundary::unavailable());
    assert_eq!(
        harness.adapter.inspect(&managed()).expect("inspect"),
        EntryState::ClientAbsent
    );
    assert_eq!(
        harness
            .adapter
            .install(&managed(), &mut NoFaults)
            .expect("install"),
        AdapterResult::Absent
    );
}

#[test]
fn a_write_that_does_not_read_back_is_failed_readback_without_ownership() {
    struct Vanish;

    impl ClaudeBoundary for Vanish {
        fn get(&mut self, _name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
            Ok(None)
        }

        fn add_json(&mut self, _name: &str, _json: &str) -> Result<(), ClaudeBoundaryError> {
            Ok(())
        }

        fn remove(&mut self, _name: &str) -> Result<(), ClaudeBoundaryError> {
            Ok(())
        }
    }

    let mut harness = harness(Vanish);
    assert_eq!(
        harness
            .adapter
            .install(&managed(), &mut NoFaults)
            .expect("install"),
        AdapterResult::FailedReadback
    );
    assert!(!harness.ownership.load().expect("ledger").owns(
        ClientId::ClaudeCode,
        &ObservedHash::of_value(&desired_entry(ClientId::ClaudeCode, &managed()))
    ));
}

#[test]
fn a_wrong_transport_is_never_current() {
    let mut boundary = FakeClaudeBoundary::new();
    boundary.seed(
        ENTRY_NAME,
        json!({"type": "sse", "url": "https://example.invalid"}),
    );
    let mut harness = harness(boundary);
    assert!(!matches!(
        harness.adapter.inspect(&managed()).expect("inspect"),
        EntryState::Current { .. }
    ));
    assert!(matches!(
        harness
            .adapter
            .install(&managed(), &mut NoFaults)
            .expect("install"),
        AdapterResult::Conflict { .. }
    ));
}

#[test]
fn a_spec_that_does_not_point_at_kontor_mcp_is_refused() {
    let mut harness = harness(FakeClaudeBoundary::new());
    let mut invalid = managed();
    invalid.program = PathBuf::from("/usr/bin/other-mcp");
    assert_eq!(
        harness.adapter.inspect(&invalid).expect_err("invalid spec"),
        AdapterError::InvalidSpec
    );
    assert_eq!(
        harness
            .adapter
            .install(&invalid, &mut NoFaults)
            .expect_err("invalid"),
        AdapterError::InvalidSpec
    );
}

#[test]
fn the_adapter_never_spawns_a_process() {
    let source = include_str!("../src/client/claude.rs");
    for forbidden in [concat!("std::", "process"), concat!("Command", "::new")] {
        assert!(
            !source.contains(forbidden),
            "claude adapter must not reference {forbidden}"
        );
    }
}

#[test]
fn the_fake_records_typed_calls_only() {
    let mut boundary = FakeClaudeBoundary::new();
    let _ = boundary.get("kontor").expect("get");
    boundary
        .add_json("kontor", "{\"type\":\"stdio\"}")
        .expect("add");
    boundary.remove("kontor").expect("remove");
    assert_eq!(
        boundary.calls(),
        &[
            ClaudeCall::Get {
                name: "kontor".to_owned()
            },
            ClaudeCall::AddJson {
                name: "kontor".to_owned(),
                json: "{\"type\":\"stdio\"}".to_owned()
            },
            ClaudeCall::Remove {
                name: "kontor".to_owned()
            }
        ]
    );
}

#[test]
fn the_claude_client_identifier_is_stable() {
    assert_eq!(ClientId::ClaudeCode.as_str(), "claude-code");
}
