//! TASK-003a: the writer-set completeness test.
//!
//! The attention channel's task-state and receipt triggers are only as complete
//! as the set of SQL writers that can settle those rows. A future eighth writer
//! that nobody classifies would silently stop alerting — exactly the defect
//! TASK-003 corrected. This test enumerates every `INSERT INTO command_receipts`,
//! `UPDATE command_receipts` and `UPDATE tasks SET state` string literal under
//! `crates/kontor-store/src` and fails when that set changes.
//!
//! Each entry must be classified. The classification is documentation tied to the
//! literal, so adding or editing a writer forces a human to say whether it emits
//! and why. Line numbers are deliberately not part of the identity: a move is not
//! a change, a different statement is.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const VERBS: &[&str] = &[
    "INSERT INTO command_receipts",
    "UPDATE command_receipts",
    "UPDATE tasks SET state",
];

/// What the attention channel does with one writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Observation {
    /// No alert: a recorded intention, a dispatch claim or an ordinary command
    /// confirmation (REQ-006, DEP-008 fences the receipt-transition class).
    None,
    /// Emits a task-state alert for the four human-attention states.
    TaskState,
    /// Emits one confirmed-command alert (the abandon path).
    AbandonConfirmed,
}

struct Writer {
    file: &'static str,
    sql: &'static str,
    observation: Observation,
    why: &'static str,
}

/// The classified set at this revision.
///
/// TASK-003's prose named seven sites at an older revision. The real set is
/// larger because ordinary local commands gained their own confirmed writers;
/// all of them are enumerated here rather than pretending the older count still
/// holds.
const EXPECTED: &[Writer] = &[
    Writer {
        file: "commands/intent.rs",
        sql: "INSERT INTO command_receipts (id, project_id, idempotency_key, kind, target, target_revision, intent, intent_hash, state, attempts, created_at, updated_at, execution_mode) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'intent_persisted', 0, ?9, ?9, 'local')",
        observation: Observation::None,
        why: "an intention that has not happened (REQ-006)",
    },
    Writer {
        file: "commands/intent.rs",
        sql: "INSERT INTO command_receipts (id, project_id, idempotency_key, kind, target, target_revision, intent, intent_hash, state, attempts, created_at, updated_at, execution_mode) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'intent_persisted', 0, ?9, ?9, 'dispatch')",
        observation: Observation::None,
        why: "an intention that has not happened (REQ-006)",
    },
    Writer {
        file: "commands/intent.rs",
        sql: "UPDATE command_receipts SET state = 'dispatch_pending', correlation = ?1, updated_at = ?2 WHERE project_id = ?3 AND id = ?4 AND state = 'intent_persisted' AND execution_mode = 'dispatch'",
        observation: Observation::None,
        why: "a dispatch claim, not a settled command (REQ-006)",
    },
    Writer {
        file: "commands/intent.rs",
        sql: "UPDATE command_receipts SET state = 'dispatch_pending', correlation = ?1, updated_at = ?2 WHERE project_id = ?3 AND id = ?4 AND state = 'intent_persisted'",
        observation: Observation::None,
        why: "a dispatch claim, not a settled command (REQ-006)",
    },
    Writer {
        file: "commands/receipts.rs",
        sql: "UPDATE command_receipts SET state = ?1, correlation = ?2, native_identity = ?3, result_ref = ?4, attempts = ?5, updated_at = ?6 WHERE project_id = ?7 AND id = ?8 AND state = ?9",
        observation: Observation::None,
        why: "the receipt-transition class is deferred while advance_receipt has zero production callers (DEP-008)",
    },
    Writer {
        file: "policy.rs",
        sql: "INSERT INTO command_receipts (id, project_id, idempotency_key, kind, target, target_revision, intent, intent_hash, state, attempts, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'confirmed', 0, ?9, ?9)",
        observation: Observation::TaskState,
        why: "the gate-rejection park writes this closure receipt and parks the task in the same transaction; the task-state alert is the primary trigger and names it",
    },
    Writer {
        file: "policy.rs",
        sql: "UPDATE tasks SET state = ?1, imported_state = NULL, revision = ?2, updated_at = ?3 WHERE project_id = ?4 AND id = ?5 AND revision = ?6",
        observation: Observation::TaskState,
        why: "park_task: the second task-state writer (TASK-004)",
    },
    Writer {
        file: "repository.rs",
        sql: "UPDATE command_receipts SET state = 'confirmed', result_ref = ?3, updated_at = ?4 WHERE project_id = ?1 AND id = ?2 AND execution_mode = 'local' AND state = 'intent_persisted'",
        observation: Observation::None,
        why: "ordinary atomic local command confirmations (record_atomic_local_result and team-run admission adoption) are not the receipt-transition class",
    },
    Writer {
        file: "repository.rs",
        sql: "UPDATE tasks SET state = ?1, imported_state = NULL, revision = ?2, updated_at = ?3 WHERE project_id = ?4 AND id = ?5 AND revision = ?6",
        observation: Observation::TaskState,
        why: "transition_task_in_transaction: the first task-state writer (TASK-004)",
    },
    Writer {
        file: "repository.rs",
        sql: "UPDATE command_receipts SET state = 'confirmed', result_ref = ?3, updated_at = ?4 WHERE project_id = ?1 AND id = ?2 AND execution_mode = 'local' AND state = 'intent_persisted'",
        observation: Observation::None,
        why: "ordinary atomic local command confirmations (record_atomic_local_result and team-run admission adoption) are not the receipt-transition class",
    },
    Writer {
        file: "repository.rs",
        sql: "INSERT INTO command_receipts (id, project_id, idempotency_key, kind, target, target_revision, intent, intent_hash, state, attempts, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'confirmed', 0, ?9, ?9)",
        observation: Observation::AbandonConfirmed,
        why: "record_abandon_receipt: born confirmed and executes in production",
    },
];

#[test]
fn the_receipt_and_task_writer_set_is_exactly_the_classified_set() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let actual = scan(&root);

    let mut expected = BTreeMap::new();
    for writer in EXPECTED {
        *expected
            .entry((writer.file.to_owned(), writer.sql.to_owned()))
            .or_insert(0_u32) += 1;
    }
    let actual: BTreeMap<_, _> = actual.into_iter().collect();

    assert_eq!(
        actual, expected,
        "the command_receipts/task-state writer set changed.\n\
         Add or remove the writer in EXPECTED with an explicit Observation and reason.\n\
         actual: {actual:#?}\n\
         expected: {expected:#?}"
    );
}

#[test]
fn no_writer_is_left_unclassified() {
    for writer in EXPECTED {
        assert!(
            !writer.why.is_empty(),
            "{} carries no classification reason",
            writer.file
        );
        match writer.observation {
            Observation::None | Observation::TaskState | Observation::AbandonConfirmed => {}
        }
    }
}

/// Every SQL string literal under `root` whose first words are one of `VERBS`.
fn scan(root: &Path) -> BTreeMap<(String, String), u32> {
    let mut found = BTreeMap::new();
    let mut files = Vec::new();
    collect(root, &mut files);
    for path in files {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(root)
            .expect("a file under the scanned root")
            .to_string_lossy()
            .into_owned();
        for sql in literals(&source) {
            *found.entry((relative.clone(), sql)).or_insert(0_u32) += 1;
        }
    }
    found
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn literals(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut cursor = 0;
    while let Some(open) = source[cursor..].find('"').map(|offset| cursor + offset) {
        let start = open + 1;
        if !VERBS.iter().any(|verb| source[start..].starts_with(verb)) {
            cursor = start;
            continue;
        }
        let mut end = start;
        let mut escaped = false;
        for (offset, character) in source[start..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == '"' {
                end = start + offset;
                break;
            }
        }
        if end > start {
            found.push(
                source[start..end]
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            cursor = end + 1;
        } else {
            cursor = start;
        }
    }
    found
}
