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
    ClaudeBoundary, ClaudeBoundaryError, ClaudeCall, ENTRY_NAME, FakeClaudeBoundary, Removal,
    add_json_argv, get_argv, remove_argv,
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

    fn remove_if(&mut self, name: &str, expected: &Value) -> Result<Removal, ClaudeBoundaryError> {
        self.inner.remove_if(name, expected)
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

/// A boundary whose conditional removal can be toggled between the real
/// comparison, a mismatch and an unavailable capability.
struct ConditionalBoundary {
    inner: FakeClaudeBoundary,
    mode: std::sync::Arc<AtomicUsize>,
}

impl ClaudeBoundary for ConditionalBoundary {
    fn get(&mut self, name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
        self.inner.get(name)
    }

    fn add_json(&mut self, name: &str, json: &str) -> Result<(), ClaudeBoundaryError> {
        self.inner.add_json(name, json)
    }

    fn remove(&mut self, name: &str) -> Result<(), ClaudeBoundaryError> {
        self.inner.remove(name)
    }

    fn remove_if(&mut self, name: &str, expected: &Value) -> Result<Removal, ClaudeBoundaryError> {
        match self.mode.load(Ordering::SeqCst) {
            0 => self.inner.remove_if(name, expected),
            1 => Ok(Removal::Mismatch),
            _ => Ok(Removal::Unavailable),
        }
    }
}

#[test]
fn a_concurrent_edit_between_revalidate_and_remove_is_refused() {
    let mode = std::sync::Arc::new(AtomicUsize::new(0));
    let holder = tempfile::tempdir().expect("tempdir");
    let ownership = OwnershipStore::at(holder.path().join("state")).expect("store");
    let mut adapter = ClaudeAdapter::new(
        Box::new(ConditionalBoundary {
            inner: FakeClaudeBoundary::new(),
            mode: mode.clone(),
        }),
        ownership,
    );
    assert_eq!(
        adapter
            .install(&spec("/synthetic/one"), &mut NoFaults)
            .expect("install"),
        AdapterResult::Installed
    );
    mode.store(1, Ordering::SeqCst);
    let result = adapter
        .install(&spec("/synthetic/two"), &mut NoFaults)
        .expect("install");
    match result {
        AdapterResult::Conflict {
            reason: ConflictReason::ConcurrentChange,
            ..
        } => {}
        other => panic!("a conditional mismatch must be a typed conflict, got {other:?}"),
    }
    assert!(matches!(
        adapter.inspect(&spec("/synthetic/one")).expect("inspect"),
        EntryState::Current { .. }
    ));
}

#[test]
fn a_production_claude_cas_gap_is_a_typed_refusal_and_recorded_as_a_goal_gap() {
    let mode = std::sync::Arc::new(AtomicUsize::new(0));
    let holder = tempfile::tempdir().expect("tempdir");
    let ownership = OwnershipStore::at(holder.path().join("state")).expect("store");
    let mut adapter = ClaudeAdapter::new(
        Box::new(ConditionalBoundary {
            inner: FakeClaudeBoundary::new(),
            mode: mode.clone(),
        }),
        ownership,
    );
    assert_eq!(
        adapter
            .install(&spec("/synthetic/one"), &mut NoFaults)
            .expect("install"),
        AdapterResult::Installed
    );
    mode.store(2, Ordering::SeqCst);
    let result = adapter
        .install(&spec("/synthetic/two"), &mut NoFaults)
        .expect("install");
    match result {
        AdapterResult::Conflict {
            reason: ConflictReason::CasUnavailable,
            ..
        } => {}
        other => panic!("an unavailable conditional capability is a typed refusal, got {other:?}"),
    }
    assert!(matches!(
        adapter.inspect(&spec("/synthetic/one")).expect("inspect"),
        EntryState::Current { .. }
    ));
}

#[test]
fn an_unavailable_boundary_is_unsupported_and_never_a_silent_success() {
    let mut harness = harness(FakeClaudeBoundary::unavailable());
    assert_eq!(
        harness.adapter.inspect(&managed()).expect("inspect"),
        EntryState::Unsupported
    );
    assert_eq!(
        harness
            .adapter
            .install(&managed(), &mut NoFaults)
            .expect("install"),
        AdapterResult::Unsupported
    );
}

/// A boundary whose next `add_json` calls fail a fixed number of times.
struct FailingAddBoundary {
    inner: FakeClaudeBoundary,
    failures: std::sync::Arc<AtomicUsize>,
}

impl ClaudeBoundary for FailingAddBoundary {
    fn get(&mut self, name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
        self.inner.get(name)
    }

    fn add_json(&mut self, name: &str, json: &str) -> Result<(), ClaudeBoundaryError> {
        if self.failures.load(Ordering::SeqCst) > 0 {
            self.failures.fetch_sub(1, Ordering::SeqCst);
            return Err(ClaudeBoundaryError::Failed);
        }
        self.inner.add_json(name, json)
    }

    fn remove(&mut self, name: &str) -> Result<(), ClaudeBoundaryError> {
        self.inner.remove(name)
    }

    fn remove_if(&mut self, name: &str, expected: &Value) -> Result<Removal, ClaudeBoundaryError> {
        self.inner.remove_if(name, expected)
    }
}

fn failing_add_harness() -> (Harness, std::sync::Arc<AtomicUsize>) {
    let failures = std::sync::Arc::new(AtomicUsize::new(0));
    let holder = tempfile::tempdir().expect("tempdir");
    let ownership = OwnershipStore::at(holder.path().join("state")).expect("store");
    let adapter = ClaudeAdapter::new(
        Box::new(FailingAddBoundary {
            inner: FakeClaudeBoundary::new(),
            failures: failures.clone(),
        }),
        ownership.clone(),
    );
    (
        Harness {
            _holder: holder,
            ownership,
            adapter,
        },
        failures,
    )
}

#[test]
fn a_failed_replacement_restores_the_original_entry() {
    let (mut harness, failures) = failing_add_harness();
    let first = spec("/synthetic/one");
    let second = spec("/synthetic/two");
    assert_eq!(
        harness
            .adapter
            .install(&first, &mut NoFaults)
            .expect("install"),
        AdapterResult::Installed
    );
    failures.store(1, Ordering::SeqCst);
    assert_eq!(
        harness
            .adapter
            .install(&second, &mut NoFaults)
            .expect_err("the replacement add fails"),
        AdapterError::CommandFailed
    );
    assert!(
        matches!(
            harness.adapter.inspect(&first).expect("inspect"),
            EntryState::Current { .. }
        ),
        "the original entry is restored, not deleted"
    );
}

#[test]
fn an_unrestorable_failed_replacement_is_reported_as_restore_failed() {
    let (mut harness, failures) = failing_add_harness();
    let first = spec("/synthetic/one");
    let second = spec("/synthetic/two");
    assert_eq!(
        harness
            .adapter
            .install(&first, &mut NoFaults)
            .expect("install"),
        AdapterResult::Installed
    );
    failures.store(2, Ordering::SeqCst);
    assert_eq!(
        harness
            .adapter
            .install(&second, &mut NoFaults)
            .expect_err("both the replacement and the restore fail"),
        AdapterError::RestoreFailed
    );
    assert_eq!(
        harness.adapter.inspect(&first).expect("inspect"),
        EntryState::EntryAbsent
    );
}

#[test]
fn a_get_failure_after_removal_restores_the_original_entry() {
    /// Fails the first `get` after a successful conditional removal.
    struct GetFailsAfterRemoval {
        inner: FakeClaudeBoundary,
        fail_next_get: bool,
    }

    impl ClaudeBoundary for GetFailsAfterRemoval {
        fn get(&mut self, name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
            if self.fail_next_get {
                self.fail_next_get = false;
                return Err(ClaudeBoundaryError::Failed);
            }
            self.inner.get(name)
        }

        fn add_json(&mut self, name: &str, json: &str) -> Result<(), ClaudeBoundaryError> {
            self.inner.add_json(name, json)
        }

        fn remove(&mut self, name: &str) -> Result<(), ClaudeBoundaryError> {
            self.inner.remove(name)
        }

        fn remove_if(
            &mut self,
            name: &str,
            expected: &Value,
        ) -> Result<Removal, ClaudeBoundaryError> {
            let result = self.inner.remove_if(name, expected);
            if result == Ok(Removal::Removed) {
                self.fail_next_get = true;
            }
            result
        }
    }

    let mut harness = harness(GetFailsAfterRemoval {
        inner: FakeClaudeBoundary::new(),
        fail_next_get: false,
    });
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
            .expect_err("the confirmation get fails"),
        AdapterError::CommandFailed
    );
    assert!(
        matches!(
            harness.adapter.inspect(&first).expect("inspect"),
            EntryState::Current { .. }
        ),
        "the original entry is restored, not lost"
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

        fn remove_if(
            &mut self,
            _name: &str,
            _expected: &Value,
        ) -> Result<Removal, ClaudeBoundaryError> {
            Ok(Removal::Unavailable)
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
fn a_lost_readback_after_add_completes_ownership_on_replay() {
    struct LostReadback {
        inner: FakeClaudeBoundary,
        gets: usize,
    }

    impl ClaudeBoundary for LostReadback {
        fn get(&mut self, name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
            self.gets += 1;
            if self.gets == 2 {
                return Err(ClaudeBoundaryError::Failed);
            }
            self.inner.get(name)
        }

        fn add_json(&mut self, name: &str, json: &str) -> Result<(), ClaudeBoundaryError> {
            self.inner.add_json(name, json)
        }

        fn remove(&mut self, name: &str) -> Result<(), ClaudeBoundaryError> {
            self.inner.remove(name)
        }

        fn remove_if(
            &mut self,
            name: &str,
            expected: &Value,
        ) -> Result<Removal, ClaudeBoundaryError> {
            self.inner.remove_if(name, expected)
        }
    }

    let mut lost = harness(LostReadback {
        inner: FakeClaudeBoundary::new(),
        gets: 0,
    });
    let first = spec("/synthetic/one");
    let desired = ObservedHash::of_value(&desired_entry(ClientId::ClaudeCode, &first));
    assert_eq!(
        lost.adapter.install(&first, &mut NoFaults),
        Err(AdapterError::CommandFailed)
    );
    let ledger = lost.ownership.load().expect("ledger");
    assert!(!ledger.owns(ClientId::ClaudeCode, &desired));
    assert!(
        ledger.pending_owns(ClientId::ClaudeCode, &desired),
        "the pending write is durable before the entry is written"
    );
    assert_eq!(
        lost.adapter.inspect(&first).expect("inspect"),
        EntryState::Current {
            observed: desired.clone()
        }
    );

    // A different spec directly after the lost readback is classified from
    // the pending record too, so it replaces the entry instead of refusing it
    // as same-name unrelated.
    let second = spec("/synthetic/two");
    let second_hash = ObservedHash::of_value(&desired_entry(ClientId::ClaudeCode, &second));
    assert_eq!(
        lost.adapter
            .install(&second, &mut NoFaults)
            .expect("update after a lost readback"),
        AdapterResult::Updated
    );
    let ledger = lost.ownership.load().expect("ledger");
    assert!(ledger.owns(ClientId::ClaudeCode, &second_hash));
    assert!(!ledger.pending_owns(ClientId::ClaudeCode, &second_hash));

    // The same-spec replay still adopts the pending record it matches.
    let mut replay = harness(LostReadback {
        inner: FakeClaudeBoundary::new(),
        gets: 0,
    });
    let third = spec("/synthetic/three");
    let third_hash = ObservedHash::of_value(&desired_entry(ClientId::ClaudeCode, &third));
    assert_eq!(
        replay.adapter.install(&third, &mut NoFaults),
        Err(AdapterError::CommandFailed)
    );
    assert!(
        replay
            .ownership
            .load()
            .expect("ledger")
            .pending_owns(ClientId::ClaudeCode, &third_hash)
    );
    assert_eq!(
        replay.adapter.inspect(&third).expect("inspect"),
        EntryState::Current {
            observed: third_hash.clone()
        }
    );
    assert_eq!(
        replay
            .adapter
            .install(&third, &mut NoFaults)
            .expect("replay"),
        AdapterResult::AlreadyCurrent
    );
    let ledger = replay.ownership.load().expect("ledger");
    assert!(ledger.owns(ClientId::ClaudeCode, &third_hash));
    assert!(!ledger.pending_owns(ClientId::ClaudeCode, &third_hash));
}

#[test]
fn a_failed_replacement_keeps_the_restored_original_owned() {
    struct LostReadbackAndFailedSecondAdd {
        inner: FakeClaudeBoundary,
        gets: usize,
        adds: usize,
    }

    impl ClaudeBoundary for LostReadbackAndFailedSecondAdd {
        fn get(&mut self, name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
            self.gets += 1;
            if self.gets == 2 {
                return Err(ClaudeBoundaryError::Failed);
            }
            self.inner.get(name)
        }

        fn add_json(&mut self, name: &str, json: &str) -> Result<(), ClaudeBoundaryError> {
            self.adds += 1;
            if self.adds == 2 {
                return Err(ClaudeBoundaryError::Failed);
            }
            self.inner.add_json(name, json)
        }

        fn remove(&mut self, name: &str) -> Result<(), ClaudeBoundaryError> {
            self.inner.remove(name)
        }

        fn remove_if(
            &mut self,
            name: &str,
            expected: &Value,
        ) -> Result<Removal, ClaudeBoundaryError> {
            self.inner.remove_if(name, expected)
        }
    }

    let mut harness = harness(LostReadbackAndFailedSecondAdd {
        inner: FakeClaudeBoundary::new(),
        gets: 0,
        adds: 0,
    });
    let first = spec("/synthetic/one");
    let second = spec("/synthetic/two");
    let third = spec("/synthetic/three");
    // The first write lands, but its readback fails and leaves only the
    // pending record.
    assert_eq!(
        harness.adapter.install(&first, &mut NoFaults),
        Err(AdapterError::CommandFailed)
    );
    // The replacement removes the first entry, fails to add the second and
    // restores the first through the no-overwrite add boundary.
    assert_eq!(
        harness.adapter.install(&second, &mut NoFaults),
        Err(AdapterError::CommandFailed)
    );
    assert_eq!(
        harness
            .adapter
            .install(&third, &mut NoFaults)
            .expect("update after a failed replacement"),
        AdapterResult::Updated
    );
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
