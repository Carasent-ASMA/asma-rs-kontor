//! The Claude Code adapter matrix over its injected command boundary.
//!
//! Nothing here spawns `claude`: the boundary is a fake, and the exact argv a
//! production boundary would receive is asserted directly.

use serde_json::{Value, json};

use kontor_bootstrap::client::claude::{
    ClaudeBoundary, ClaudeBoundaryError, ClaudeCall, ENTRY_NAME, add_json_argv, get_argv,
    remove_argv,
};
use kontor_bootstrap::{
    AdapterResult, ClaudeAdapter, ClientAdapter, ClientId, ConflictReason, EntryState,
    FakeClaudeBoundary, NoFaults, ObservedHash, ServerSpec,
};
use std::path::PathBuf;

fn spec(program: &str) -> ServerSpec {
    ServerSpec {
        program: PathBuf::from(program),
        args: vec![
            "--state-root".to_owned(),
            "/synthetic/realm".to_owned(),
            "--credential-tier".to_owned(),
            "admin".to_owned(),
        ],
    }
}

fn managed() -> ServerSpec {
    spec("/opt/kontor tools/kontor-mcp")
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
fn a_clean_install_writes_then_reads_back() {
    let mut adapter = ClaudeAdapter::new(Box::new(FakeClaudeBoundary::new()));
    assert_eq!(
        adapter.inspect(&managed()).expect("inspect"),
        EntryState::EntryAbsent
    );
    assert_eq!(
        adapter.install(&managed(), &mut NoFaults).expect("install"),
        AdapterResult::Installed
    );
    assert_eq!(
        adapter.inspect(&managed()).expect("inspect"),
        EntryState::Current {
            observed: ObservedHash::of_entry("/opt/kontor tools/kontor-mcp", &managed().args)
        }
    );

    let boundary = FakeClaudeBoundary::new();
    let mut adapter = ClaudeAdapter::new(Box::new(boundary));
    assert_eq!(
        adapter.install(&managed(), &mut NoFaults).expect("install"),
        AdapterResult::Installed
    );
    assert_eq!(
        adapter.install(&managed(), &mut NoFaults).expect("replay"),
        AdapterResult::AlreadyCurrent
    );
}

#[test]
fn an_update_removes_then_adds_and_never_assumes_overwrite() {
    let mut boundary = FakeClaudeBoundary::new();
    boundary.seed(
        ENTRY_NAME,
        json!({
            "type": "stdio",
            "command": "/old/install/kontor-mcp",
            "args": ["--state-root", "/old"],
        }),
    );
    let mut adapter = ClaudeAdapter::new(Box::new(boundary));
    assert_eq!(
        adapter.install(&managed(), &mut NoFaults).expect("install"),
        AdapterResult::Updated
    );
    assert_eq!(
        adapter.inspect(&managed()).expect("inspect"),
        EntryState::Current {
            observed: ObservedHash::of_entry("/opt/kontor tools/kontor-mcp", &managed().args)
        }
    );
}

#[test]
fn the_recorded_call_sequence_for_an_update_is_get_add_remove_add_get() {
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Shared(Rc<RefCell<FakeClaudeBoundary>>);

    impl ClaudeBoundary for Shared {
        fn get(&mut self, name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
            self.0.borrow_mut().get(name)
        }

        fn add_json(&mut self, name: &str, json: &str) -> Result<(), ClaudeBoundaryError> {
            self.0.borrow_mut().add_json(name, json)
        }

        fn remove(&mut self, name: &str) -> Result<(), ClaudeBoundaryError> {
            self.0.borrow_mut().remove(name)
        }
    }

    let fake = Rc::new(RefCell::new(FakeClaudeBoundary::new()));
    fake.borrow_mut().seed(
        ENTRY_NAME,
        json!({"type": "stdio", "command": "/old/kontor-mcp", "args": []}),
    );
    let mut adapter = ClaudeAdapter::new(Box::new(Shared(Rc::clone(&fake))));
    assert_eq!(
        adapter.install(&managed(), &mut NoFaults).expect("install"),
        AdapterResult::Updated
    );
    assert_eq!(
        fake.borrow().calls(),
        &[
            ClaudeCall::Get {
                name: "kontor".to_owned()
            },
            ClaudeCall::AddJson {
                name: "kontor".to_owned(),
                json: serde_json::json!({
                    "type": "stdio",
                    "command": "/opt/kontor tools/kontor-mcp",
                    "args": [
                        "--state-root", "/synthetic/realm",
                        "--credential-tier", "admin"
                    ],
                })
                .to_string(),
            },
            ClaudeCall::Remove {
                name: "kontor".to_owned()
            },
            ClaudeCall::AddJson {
                name: "kontor".to_owned(),
                json: serde_json::json!({
                    "type": "stdio",
                    "command": "/opt/kontor tools/kontor-mcp",
                    "args": [
                        "--state-root", "/synthetic/realm",
                        "--credential-tier", "admin"
                    ],
                })
                .to_string(),
            },
            ClaudeCall::Get {
                name: "kontor".to_owned()
            },
        ]
    );
}

#[test]
fn a_same_name_unrelated_entry_is_conflict_and_repairs_only_with_its_hash() {
    let mut boundary = FakeClaudeBoundary::new();
    boundary.seed(
        ENTRY_NAME,
        json!({"type": "stdio", "command": "/usr/bin/other-mcp", "args": ["--own"]}),
    );
    let mut adapter = ClaudeAdapter::new(Box::new(boundary));
    let observed = match adapter.inspect(&managed()).expect("inspect") {
        EntryState::Unrelated { observed } => observed,
        other => panic!("expected unrelated, got {other:?}"),
    };
    assert_eq!(
        adapter.install(&managed(), &mut NoFaults).expect("install"),
        AdapterResult::Conflict {
            reason: ConflictReason::SameNameUnrelated,
            observed: Some(observed.clone()),
        }
    );
    assert_eq!(
        adapter
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
        adapter
            .repair(&managed(), &observed, &mut NoFaults)
            .expect("repair"),
        AdapterResult::Repaired
    );
    assert!(matches!(
        adapter.inspect(&managed()).expect("inspect"),
        EntryState::Current { .. }
    ));
}

#[test]
fn an_unavailable_client_is_absent_and_never_a_silent_success() {
    let mut adapter = ClaudeAdapter::new(Box::new(FakeClaudeBoundary::unavailable()));
    assert_eq!(
        adapter.inspect(&managed()).expect("inspect"),
        EntryState::ClientAbsent
    );
    assert_eq!(
        adapter.install(&managed(), &mut NoFaults).expect("install"),
        AdapterResult::Absent
    );
    assert_eq!(
        adapter
            .repair(
                &managed(),
                &ObservedHash::parse(&"b".repeat(64)).expect("hash"),
                &mut NoFaults
            )
            .expect("repair"),
        AdapterResult::Absent
    );
}

#[test]
fn a_write_that_does_not_read_back_is_failed_readback() {
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

    let mut adapter = ClaudeAdapter::new(Box::new(Vanish));
    assert_eq!(
        adapter.install(&managed(), &mut NoFaults).expect("install"),
        AdapterResult::FailedReadback
    );
}

#[test]
fn a_spec_that_does_not_point_at_kontor_mcp_is_refused() {
    let mut adapter = ClaudeAdapter::new(Box::new(FakeClaudeBoundary::new()));
    let invalid = spec("/usr/bin/other-mcp");
    assert_eq!(
        adapter.inspect(&invalid).expect_err("invalid spec"),
        kontor_bootstrap::AdapterError::InvalidSpec
    );
    assert_eq!(
        adapter
            .install(&invalid, &mut NoFaults)
            .expect_err("invalid"),
        kontor_bootstrap::AdapterError::InvalidSpec
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
