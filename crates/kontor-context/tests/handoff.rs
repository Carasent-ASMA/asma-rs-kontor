//! Same-engine continuation, portable capsules and receiver acknowledgement.
//!
//! The mutants this suite exists to kill:
//!
//! * resuming a native session across a different engine, a different runtime
//!   generation or a different original Context Pack;
//! * letting native session metadata — transcript, session id, host, generation,
//!   correlation — ride along inside the portable capsule;
//! * dropping a required handoff category, or silently defaulting it to empty;
//! * accepting an ambiguous capsule with exact duplicate entries;
//! * acknowledging "a handoff" instead of one exact capsule digest;
//! * accepting an unknown field on import — a native session locator, a
//!   transcript, a runtime generation — instead of refusing it as the schema's
//!   `additionalProperties: false` promises, at every nesting level;
//! * letting a run other than the target a capsule names acknowledge it;
//! * accepting a source, capsule or acknowledgement from another realm.

use kontor_context::{ContextLayer, ContextSource, ReferenceInputs, ResolutionRequest, preview};
use kontor_context::{
    ContinuationMode, HandoffAcknowledgement, HandoffCapsule, SameEngineContinuation, TestAttempt,
    TestResult, WorkspaceRef, acknowledge,
};
use kontor_core::DomainError;
use kontor_core::id::{
    AgentRunId, BoundedText, CanonicalDocument, CommandReceiptId, ContentHash, ContextPackId,
    ExternalId, ExternalName, HandoffId, RealmId, RuntimeKindKey, SCHEMA_VERSION, SpecVersion,
    Timestamp, parse_utc_timestamp,
};
use kontor_core::state::NativeRuntimeIdentity;
use serde_json::json;

const CAPSULE: &str = include_str!("fixtures/handoff/capsule.json");
const ACKNOWLEDGEMENT: &str = include_str!("fixtures/handoff/acknowledgement.json");

const REALM: &str = "0192f0a1-0000-7000-8000-00000000f001";
const FOREIGN_REALM: &str = "0192f0a1-0000-7000-8000-00000000f002";
const SOURCE_RUN: &str = "0192f0a1-0000-7000-8000-0000000000a1";
const RECEIVER_RUN: &str = "0192f0a1-0000-7000-8000-0000000000a2";
const THIRD_RUN: &str = "0192f0a1-0000-7000-8000-0000000000a3";

/// One way of smuggling an extra key into an otherwise valid capsule.
type Smuggler = fn(&mut serde_json::Value);
const HANDOFF: &str = "0192f0a1-0000-7000-8000-0000000000d1";
const RECEIPT: &str = "0192f0a1-0000-7000-8000-0000000000e1";
const PACK_ID: &str = "0192f0a1-0000-7000-8000-00000000c001";

/// Native session strings that must never appear in a portable capsule.
const NATIVE_ONLY: &[&str] = &[
    "codex.exec",
    "workstation-1",
    "native-session-4f1a",
    "correlation-9c2e",
];

fn realm() -> RealmId {
    RealmId::parse(REALM).expect("fixture realm id is canonical")
}

fn foreign_realm() -> RealmId {
    RealmId::parse(FOREIGN_REALM).expect("fixture realm id is canonical")
}

fn run(text: &str) -> AgentRunId {
    AgentRunId::parse(text).expect("fixture run id is canonical")
}

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("fixture timestamp is canonical UTC")
}

fn text(value: &str) -> BoundedText {
    BoundedText::parse(value).expect("fixture text is bounded and non-sensitive")
}

fn external(value: &str) -> ExternalId {
    ExternalId::parse(value).expect("fixture id is bounded and non-sensitive")
}

fn pack_hash() -> ContentHash {
    ContentHash::of(b"context pack canonical bytes")
}

fn identity(generation: u64) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("codex.exec").expect("valid runtime key"),
        host: ExternalName::parse("workstation-1").expect("valid host"),
        generation,
        native_id: external("native-session-4f1a"),
    }
}

fn continuation() -> SameEngineContinuation {
    SameEngineContinuation {
        schema_version: SCHEMA_VERSION,
        realm_id: realm(),
        parent_run_id: run(SOURCE_RUN),
        native_identity: identity(7),
        correlation: external("correlation-9c2e"),
        evidence_hash: ContentHash::of(b"runtime binding evidence"),
        context_pack_hash: pack_hash(),
    }
}

fn workspace() -> WorkspaceRef {
    WorkspaceRef {
        root: text("/work/kontor"),
        branch: ExternalName::parse("feat/context-packs").expect("valid branch"),
        baseline_commit: external("45126a6"),
    }
}

fn capsule() -> HandoffCapsule {
    HandoffCapsule {
        schema_version: SCHEMA_VERSION,
        realm_id: realm(),
        handoff_id: HandoffId::parse(HANDOFF).expect("canonical handoff id"),
        continuation_mode: ContinuationMode::CrossEngineHandoff,
        source_run_id: run(SOURCE_RUN),
        target_run_id: Some(run(RECEIVER_RUN)),
        context_pack_id: ContextPackId::parse(PACK_ID).expect("canonical pack id"),
        context_pack_hash: pack_hash(),
        workspace: workspace(),
        attempted_work: vec![
            text("resolved the pack from all six layers"),
            text("wrote the golden fixtures"),
        ],
        touched_files: vec![
            text("crates/kontor-context/src/resolve.rs"),
            text("crates/kontor-context/tests/resolution.rs"),
        ],
        commits: vec![external("9f2c1ab"), external("c40de11")],
        tests: vec![
            TestAttempt {
                command: text("cargo test -p kontor-context"),
                result: TestResult::Failed,
            },
            TestAttempt {
                command: text("cargo test -p kontor-context"),
                result: TestResult::Passed,
            },
            TestAttempt {
                command: text("cargo deny check"),
                result: TestResult::Skipped,
            },
        ],
        decisions: vec![
            text("redaction runs before the core scan, never instead of it"),
            text("array values replace rather than concatenate"),
        ],
        evidence: vec![
            external("evidence.kontor.0001"),
            external("evidence.kontor.0002"),
        ],
        remaining_work: vec![text("wire the resolver into the scheduler")],
        risks: vec![text("the workspace lock refresh belongs to another ticket")],
        recommended_next_action: text("review the golden pack, then land the store binding"),
    }
}

fn acknowledgement(sealed: &CanonicalDocument) -> HandoffAcknowledgement {
    acknowledge(
        realm(),
        sealed,
        run(RECEIVER_RUN),
        CommandReceiptId::parse(RECEIPT).expect("canonical receipt id"),
        ContentHash::of(b"acknowledgement evidence"),
        at("2026-08-10T09:30:00Z"),
    )
    .expect("the receiver may acknowledge this capsule")
}

#[test]
fn same_engine_continuation_requires_matching_engine_and_original_pack_hash() {
    let metadata = continuation();
    metadata
        .ensure_resumable(realm(), run(SOURCE_RUN), &identity(7), &pack_hash())
        .expect("the same session in the same generation resumes");

    // A different runtime engine wearing the same provider shape.
    let mut other_engine = identity(7);
    other_engine.runtime_kind = RuntimeKindKey::parse("paseo.acp").expect("valid runtime key");
    assert!(matches!(
        metadata
            .ensure_resumable(realm(), run(SOURCE_RUN), &other_engine, &pack_hash())
            .expect_err("a different engine is not resumable"),
        DomainError::MissingAuthority { .. }
    ));

    // The same runtime, restarted: a new generation is not the same session.
    assert!(matches!(
        metadata
            .ensure_resumable(realm(), run(SOURCE_RUN), &identity(8), &pack_hash())
            .expect_err("a different generation is not resumable"),
        DomainError::MissingAuthority { .. }
    ));

    // Same engine and generation, different native session.
    let mut other_session = identity(7);
    other_session.native_id = external("native-session-0000");
    assert!(matches!(
        metadata
            .ensure_resumable(realm(), run(SOURCE_RUN), &other_session, &pack_hash())
            .expect_err("a different native session is not resumable"),
        DomainError::MissingAuthority { .. }
    ));

    // Right session, wrong original pack.
    assert!(matches!(
        metadata
            .ensure_resumable(
                realm(),
                run(SOURCE_RUN),
                &identity(7),
                &ContentHash::of(b"a different context pack")
            )
            .expect_err("a different original pack is not resumable"),
        DomainError::Invalid { .. }
    ));

    // Right session, wrong parent run.
    assert!(matches!(
        metadata
            .ensure_resumable(realm(), run(RECEIVER_RUN), &identity(7), &pack_hash())
            .expect_err("a different parent run is not resumable"),
        DomainError::Invalid { .. }
    ));
}

#[test]
fn portable_capsule_omits_native_session_metadata() {
    let sealed = capsule().canonical(realm()).expect("the capsule seals");
    let rendered = sealed.json();
    for native in NATIVE_ONLY {
        assert!(
            !rendered.contains(native),
            "the portable capsule leaked native session metadata: {native}"
        );
    }
    // Nor does it claim to carry hidden model state under another name.
    for forbidden in [
        "native",
        "session",
        "transcript",
        "token_cache",
        "generation",
        "correlation",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "the portable capsule carries a `{forbidden}` field"
        );
    }
    // The same-engine locator does carry them, and stays a separate document.
    let locator = serde_json::to_string(&continuation()).expect("locator serializes");
    for native in NATIVE_ONLY {
        assert!(locator.contains(native));
    }
    assert_eq!(
        capsule().continuation_mode,
        ContinuationMode::CrossEngineHandoff
    );
}

#[test]
fn cross_engine_handoff_contains_workspace_files_commits_tests_decisions_evidence_and_remaining_work()
 {
    let capsule = capsule();
    let sealed = capsule.canonical(realm()).expect("the capsule seals");
    let reread: HandoffCapsule = sealed.deserialize().expect("the capsule round-trips");
    assert_eq!(reread, capsule);

    assert_eq!(reread.workspace.root.as_str(), "/work/kontor");
    assert_eq!(reread.workspace.branch.as_str(), "feat/context-packs");
    assert_eq!(reread.workspace.baseline_commit.as_str(), "45126a6");
    assert_eq!(reread.attempted_work.len(), 2);
    assert_eq!(reread.touched_files.len(), 2);
    assert_eq!(reread.commits.len(), 2);
    assert_eq!(reread.decisions.len(), 2);
    assert_eq!(reread.evidence.len(), 2);
    assert_eq!(reread.remaining_work.len(), 1);
    assert_eq!(reread.risks.len(), 1);
    assert!(!reread.recommended_next_action.as_str().is_empty());

    // Test commands and their results are both present, in order, and the same
    // command re-run after a fix is two distinct attempts rather than a duplicate.
    assert_eq!(
        reread.tests,
        vec![
            TestAttempt {
                command: text("cargo test -p kontor-context"),
                result: TestResult::Failed,
            },
            TestAttempt {
                command: text("cargo test -p kontor-context"),
                result: TestResult::Passed,
            },
            TestAttempt {
                command: text("cargo deny check"),
                result: TestResult::Skipped,
            },
        ]
    );

    // Every required category is present in the serialized contract.
    let value: serde_json::Value = serde_json::from_str(sealed.json()).expect("valid JSON");
    for category in [
        "workspace",
        "attempted_work",
        "touched_files",
        "commits",
        "tests",
        "decisions",
        "evidence",
        "remaining_work",
        "risks",
        "recommended_next_action",
    ] {
        assert!(
            value.get(category).is_some(),
            "the capsule must carry `{category}`"
        );
    }
}

#[test]
fn handoff_golden_is_canonical_and_hash_stable() {
    let golden = CAPSULE.trim_end();
    let capsule: HandoffCapsule = serde_json::from_str(golden).expect("the golden deserializes");
    let sealed = capsule.canonical(realm()).expect("the golden seals");
    assert_eq!(
        sealed.json(),
        golden,
        "the reviewed golden is exactly the canonical form"
    );
    assert_eq!(
        sealed.hash(),
        &ContentHash::of(golden.as_bytes()),
        "the digest is the digest of those bytes and nothing else"
    );
    // The golden is the capsule this suite builds in code.
    assert_eq!(capsule, self::capsule());
    // Re-sealing is stable.
    assert_eq!(
        capsule.canonical(realm()).expect("re-seals").hash(),
        sealed.hash()
    );

    let golden_ack = ACKNOWLEDGEMENT.trim_end();
    let ack: HandoffAcknowledgement =
        serde_json::from_str(golden_ack).expect("the golden acknowledgement deserializes");
    assert_eq!(
        ack.canonical()
            .expect("acknowledgement canonicalizes")
            .json(),
        golden_ack
    );
    ack.ensure_acknowledges(realm(), &sealed)
        .expect("the golden acknowledgement is bound to the golden capsule");
}

#[test]
fn acknowledgement_binds_receiver_and_exact_capsule_hash() {
    let sealed = capsule().canonical(realm()).expect("the capsule seals");
    let ack = acknowledgement(&sealed);
    assert_eq!(&ack.capsule_hash, sealed.hash());
    assert_eq!(ack.receiver_run_id, run(RECEIVER_RUN));
    assert_eq!(ack.schema_version, SCHEMA_VERSION);
    ack.ensure_acknowledges(realm(), &sealed)
        .expect("bound to this capsule");

    // One byte of difference is a different capsule, and the same
    // acknowledgement no longer applies.
    let mut altered = capsule();
    altered
        .remaining_work
        .push(text("re-run the redaction suite"));
    let other = altered.canonical(realm()).expect("the variant seals");
    assert_ne!(other.hash(), sealed.hash());
    assert!(matches!(
        ack.ensure_acknowledges(realm(), &other)
            .expect_err("an acknowledgement is bound to one exact capsule"),
        DomainError::Invalid { .. }
    ));

    // The producer cannot acknowledge its own handoff.
    assert!(matches!(
        acknowledge(
            realm(),
            &sealed,
            run(SOURCE_RUN),
            CommandReceiptId::parse(RECEIPT).expect("canonical receipt id"),
            ContentHash::of(b"acknowledgement evidence"),
            at("2026-08-10T09:30:00Z"),
        )
        .expect_err("the source run cannot acknowledge itself"),
        DomainError::Invalid { .. }
    ));
}

#[test]
fn foreign_realm_source_handoff_or_acknowledgement_is_rejected() {
    // A source from another realm never merges.
    let foreign_source = ContextSource {
        schema_version: SCHEMA_VERSION,
        realm_id: foreign_realm(),
        layer: ContextLayer::GlobalProfile,
        source_id: "global.profile".to_owned(),
        revision: SpecVersion::FIRST,
        restricted_references: Vec::new(),
        redactions: Vec::new(),
        content: json!({ "goal": "from elsewhere" }),
    };
    assert!(matches!(
        preview(&ResolutionRequest {
            realm_id: realm(),
            sources: std::slice::from_ref(&foreign_source),
            references: &ReferenceInputs::new(),
        })
        .expect_err("a foreign source rejects"),
        DomainError::RealmMismatch { .. }
    ));

    // A capsule from another realm never seals here.
    let mut foreign_capsule = capsule();
    foreign_capsule.realm_id = foreign_realm();
    assert!(matches!(
        foreign_capsule
            .canonical(realm())
            .expect_err("a foreign capsule rejects"),
        DomainError::RealmMismatch { .. }
    ));

    // An acknowledgement from another realm never binds here.
    let sealed = capsule().canonical(realm()).expect("the capsule seals");
    let mut foreign_ack = acknowledgement(&sealed);
    foreign_ack.realm_id = foreign_realm();
    assert!(matches!(
        foreign_ack
            .ensure_acknowledges(realm(), &sealed)
            .expect_err("a foreign acknowledgement rejects"),
        DomainError::RealmMismatch { .. }
    ));

    // And a same-engine locator from another realm never resumes here.
    let mut foreign_locator = continuation();
    foreign_locator.realm_id = foreign_realm();
    assert!(matches!(
        foreign_locator
            .ensure_resumable(realm(), run(SOURCE_RUN), &identity(7), &pack_hash())
            .expect_err("a foreign locator rejects"),
        DomainError::RealmMismatch { .. }
    ));
}

#[test]
fn missing_or_duplicated_handoff_categories_are_rejected() {
    // An empty category serializes explicitly and is accepted…
    let mut empty = capsule();
    empty.risks = Vec::new();
    empty.commits = Vec::new();
    let sealed = empty.canonical(realm()).expect("explicit empties are fine");
    assert!(sealed.json().contains("\"risks\":[]"));
    assert!(sealed.json().contains("\"commits\":[]"));

    // …but an omitted category is a rejected document, not an empty one.
    for category in [
        "commits",
        "tests",
        "decisions",
        "evidence",
        "remaining_work",
        "risks",
        "touched_files",
        "attempted_work",
        "workspace",
        "recommended_next_action",
    ] {
        let mut value: serde_json::Value =
            serde_json::from_str(CAPSULE.trim_end()).expect("golden is valid JSON");
        value
            .as_object_mut()
            .expect("capsule is an object")
            .remove(category);
        assert!(
            serde_json::from_value::<HandoffCapsule>(value).is_err(),
            "a capsule missing `{category}` must not deserialize"
        );
    }

    // Exact duplicates make the capsule ambiguous.
    let mut duplicated = capsule();
    duplicated.commits.push(external("9f2c1ab"));
    assert!(matches!(
        duplicated
            .canonical(realm())
            .expect_err("an exact duplicate commit rejects"),
        DomainError::Invalid {
            subject: "HandoffCapsule.commits",
            ..
        }
    ));

    let mut duplicated = capsule();
    duplicated.tests.push(TestAttempt {
        command: text("cargo deny check"),
        result: TestResult::Skipped,
    });
    assert!(matches!(
        duplicated
            .canonical(realm())
            .expect_err("an exact duplicate attempt rejects"),
        DomainError::Invalid {
            subject: "HandoffCapsule.tests",
            ..
        }
    ));
}

#[test]
fn a_capsule_claiming_same_engine_continuation_is_rejected() {
    let mut mislabelled = capsule();
    mislabelled.continuation_mode = ContinuationMode::SameEngine;
    assert!(matches!(
        mislabelled
            .canonical(realm())
            .expect_err("portable context may not claim same-engine continuity"),
        DomainError::Invalid {
            subject: "HandoffCapsule",
            ..
        }
    ));

    let mut self_handoff = capsule();
    self_handoff.target_run_id = Some(run(SOURCE_RUN));
    assert!(matches!(
        self_handoff
            .canonical(realm())
            .expect_err("a run cannot hand over to itself"),
        DomainError::Invalid {
            subject: "HandoffCapsule",
            ..
        }
    ));

    let mut no_action = capsule();
    no_action.recommended_next_action = text("   ");
    assert!(matches!(
        no_action
            .canonical(realm())
            .expect_err("a capsule must recommend a next action"),
        DomainError::Invalid {
            subject: "HandoffCapsule",
            ..
        }
    ));
}

#[test]
fn unknown_fields_in_a_portable_capsule_are_rejected_on_import() {
    // Control: the untouched golden imports and acknowledges.
    let golden: serde_json::Value =
        serde_json::from_str(CAPSULE.trim_end()).expect("golden is valid JSON");
    let sealed = CanonicalDocument::from_value(&golden).expect("the golden canonicalizes");
    acknowledge(
        realm(),
        &sealed,
        run(RECEIVER_RUN),
        CommandReceiptId::parse(RECEIPT).expect("canonical receipt id"),
        ContentHash::of(b"acknowledgement evidence"),
        at("2026-08-10T09:30:00Z"),
    )
    .expect("the golden is importable");

    // The schema says additionalProperties:false; the type must agree, at every
    // level. A capsule is portable *because* it carries nothing else — an
    // unrecognized key is refused at import rather than silently dropped, so a
    // native session locator cannot ride in on a valid-looking capsule.
    let smuggled: &[(&str, Smuggler)] = &[
        ("top-level native_session_id", |value| {
            value["native_session_id"] = json!("native-session-4f1a");
        }),
        ("top-level transcript", |value| {
            value["transcript"] = json!(["turn one", "turn two"]);
        }),
        ("top-level runtime generation", |value| {
            value["native_generation"] = json!(7);
        }),
        ("nested inside workspace", |value| {
            value["workspace"]["native_session_id"] = json!("native-session-4f1a");
        }),
        ("nested inside a test attempt", |value| {
            value["tests"][0]["native_generation"] = json!(7);
        }),
    ];

    for (label, smuggle) in smuggled {
        let mut value = golden.clone();
        smuggle(&mut value);

        assert!(
            serde_json::from_value::<HandoffCapsule>(value.clone()).is_err(),
            "{label} must not deserialize into a capsule"
        );

        // And the same bytes must not survive `acknowledge`, which is the real
        // import boundary: canonicalization alone does not type-check.
        let document = CanonicalDocument::from_value(&value)
            .expect("the smuggled document still canonicalizes");
        let error = acknowledge(
            realm(),
            &document,
            run(RECEIVER_RUN),
            CommandReceiptId::parse(RECEIPT).expect("canonical receipt id"),
            ContentHash::of(b"acknowledgement evidence"),
            at("2026-08-10T09:30:00Z"),
        )
        .expect_err("an unknown field must not be acknowledged");
        assert!(
            matches!(
                error,
                DomainError::Invalid {
                    subject: "CanonicalDocument",
                    ..
                }
            ),
            "{label} rejected with an unexpected error: {error:?}"
        );
    }

    // The acknowledgement contract is closed the same way.
    let mut ack: serde_json::Value =
        serde_json::from_str(ACKNOWLEDGEMENT.trim_end()).expect("golden ack is valid JSON");
    ack["native_session_id"] = json!("native-session-4f1a");
    assert!(
        serde_json::from_value::<HandoffAcknowledgement>(ack).is_err(),
        "an acknowledgement carrying an unknown field must not deserialize"
    );

    // So is the resolution input contract.
    let mut source: serde_json::Value = json!({
        "schema_version": 1,
        "realm_id": REALM,
        "layer": "global_profile",
        "source_id": "global.profile",
        "revision": 1,
        "restricted_references": [],
        "redactions": [],
        "content": { "goal": "benign" }
    });
    serde_json::from_value::<ContextSource>(source.clone())
        .expect("the well-formed source imports");
    source["native_session_id"] = json!("native-session-4f1a");
    assert!(
        serde_json::from_value::<ContextSource>(source).is_err(),
        "a source carrying an unknown field must not deserialize"
    );
}

#[test]
fn acknowledgement_rejects_a_receiver_that_is_not_the_named_target() {
    // `target_run_id` is optional because a capsule is usually written before its
    // successor exists — but once it names a target, only that run may take over.
    let sealed = capsule().canonical(realm()).expect("the capsule seals");
    let error = acknowledge(
        realm(),
        &sealed,
        run(THIRD_RUN),
        CommandReceiptId::parse(RECEIPT).expect("canonical receipt id"),
        ContentHash::of(b"acknowledgement evidence"),
        at("2026-08-10T09:30:00Z"),
    )
    .expect_err("a run the capsule does not name cannot acknowledge it");
    assert!(matches!(
        error,
        DomainError::Invalid {
            subject: "HandoffAcknowledgement",
            ..
        }
    ));

    // With no target named, any run other than the producer may acknowledge.
    let mut open = capsule();
    open.target_run_id = None;
    let sealed = open.canonical(realm()).expect("an open capsule seals");
    for receiver in [RECEIVER_RUN, THIRD_RUN] {
        let ack = acknowledge(
            realm(),
            &sealed,
            run(receiver),
            CommandReceiptId::parse(RECEIPT).expect("canonical receipt id"),
            ContentHash::of(b"acknowledgement evidence"),
            at("2026-08-10T09:30:00Z"),
        )
        .expect("an untargeted capsule may be taken by any successor");
        assert_eq!(ack.receiver_run_id, run(receiver));
        assert_eq!(&ack.capsule_hash, sealed.hash());
    }
}
