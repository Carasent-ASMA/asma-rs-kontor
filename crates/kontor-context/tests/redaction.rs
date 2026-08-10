//! Secret rejection, explicit redaction and restricted-reference admission.
//!
//! The mutants this suite exists to kill:
//!
//! * skipping the core sensitive scan, or running it before redaction so an
//!   explicitly redacted secret still rejects the whole pack;
//! * running redaction *after* canonicalization, so the value is already in the
//!   hashed bytes;
//! * removing only the redacted node and leaving its descendants or its losing
//!   provenance behind;
//! * accepting a redaction path that no longer resolves, so a policy typo
//!   silently retains data;
//! * treating a missing grant, a denied grant or a foreign-realm grant as
//!   permission to merge;
//! * checking a grant against the reference's own declared realm instead of the
//!   realm the pack is being resolved for, so a source can declare realm B,
//!   present a matching realm-B grant and carry the value into a realm-A pack;
//! * treating authorization as permission to persist a secret;
//! * echoing a rejected value into an error, its `Debug` rendering or any
//!   serialized output.
//!
//! Every canary in `tests/fixtures/redaction` is fake.

use std::collections::BTreeMap;

use kontor_context::{
    ContextLayer, ContextSource, RedactionReason, RedactionRule, ReferenceInputs,
    ResolutionRequest, ResolvedContextPack, ResolvedReference, RestrictedReference, preview,
};
use kontor_core::DomainError;
use kontor_core::id::{RealmId, SchemaVersion, SpecVersion};
use kontor_core::spec::JsonPointer;
use serde_json::{Value, json};

const CANARIES: &str = include_str!("fixtures/redaction/secret_canaries.json");
const REDACTED_SOURCE: &str = include_str!("fixtures/redaction/redacted_source.json");
const RESTRICTED_SOURCE: &str = include_str!("fixtures/redaction/restricted_reference_source.json");

const REALM: &str = "0192f0a1-0000-7000-8000-00000000f001";
const FOREIGN_REALM: &str = "0192f0a1-0000-7000-8000-00000000f002";

/// Where the resolver must say each canary was found: a path *inside the pack*,
/// not inside the snapshot envelope. Pinning this proves the explicit scan on
/// the resolved pack still runs; the canonical document's own scan would report
/// the same nodes one level down, under `pack.`.
const CANARY_PATHS: &[(&str, &str)] = &[
    ("assignment_value", "dsn"),
    ("forbidden_key", "api_key"),
    ("marker_value", "notes"),
    ("nested_marker_value", "runtime.headers[1]"),
    ("url_userinfo_value", "endpoint"),
];

/// Every fake secret string that must never survive into output.
const CANARY_STRINGS: &[&str] = &[
    "canary-forbidden-key-placeholder",
    "ghp_EXAMPLEONLYNOTAREALTOKEN0000",
    "canary-not-a-real-password",
    "canary-userinfo-secret",
    "canary-not-a-real-bearer",
    "canary-redacted-not-a-real-password",
    "canary-granted-not-a-real-token",
];

fn realm() -> RealmId {
    RealmId::parse(REALM).expect("fixture realm id is canonical")
}

fn foreign_realm() -> RealmId {
    RealmId::parse(FOREIGN_REALM).expect("fixture realm id is canonical")
}

fn canaries() -> BTreeMap<String, Value> {
    serde_json::from_str(CANARIES).expect("canary fixture deserializes")
}

fn source(layer: ContextLayer, source_id: &str, content: Value) -> ContextSource {
    ContextSource {
        schema_version: SchemaVersion::parse(1).expect("schema version 1"),
        realm_id: realm(),
        layer,
        source_id: source_id.to_owned(),
        revision: SpecVersion::parse(1).expect("positive revision"),
        restricted_references: Vec::new(),
        redactions: Vec::new(),
        content,
    }
}

fn resolve(sources: &[ContextSource], references: &ReferenceInputs) -> ResolvedContextPack {
    preview(&ResolutionRequest {
        realm_id: realm(),
        sources,
        references,
    })
    .expect("fixture resolves")
}

fn reject(sources: &[ContextSource], references: &ReferenceInputs) -> DomainError {
    preview(&ResolutionRequest {
        realm_id: realm(),
        sources,
        references,
    })
    .expect_err("fixture must reject")
}

/// Fail if any fake secret shows up in a rendering that leaves the process.
fn assert_no_canary(subject: &str, rendered: &str) {
    for canary in CANARY_STRINGS {
        assert!(
            !rendered.contains(canary),
            "{subject} leaked the canary {canary}"
        );
    }
}

#[test]
fn secret_like_keys_and_values_are_rejected_without_echo() {
    let canaries = canaries();
    assert_eq!(canaries.len(), 5, "every canary shape is exercised");
    for (label, content) in canaries {
        let error = reject(
            &[source(
                ContextLayer::GlobalProfile,
                "global.profile",
                content,
            )],
            &ReferenceInputs::new(),
        );
        let DomainError::SensitiveMaterial { path } = &error else {
            panic!("{label} must be rejected as sensitive material, got {error:?}");
        };
        let expected = CANARY_PATHS
            .iter()
            .find(|(candidate, _)| *candidate == label)
            .map(|(_, path)| *path)
            .unwrap_or_else(|| panic!("{label} has a declared expected path"));
        assert_eq!(
            path, expected,
            "{label} must be reported at its path inside the pack"
        );
        assert_no_canary(&format!("{label} error"), &format!("{error}"));
        assert_no_canary(&format!("{label} debug"), &format!("{error:?}"));
    }
}

#[test]
fn secret_canaries_never_reach_error_debug_or_serialized_output() {
    // Every canary, alone and mixed with a benign source, in both renderings.
    for (label, content) in canaries() {
        let sources = vec![
            source(
                ContextLayer::GlobalProfile,
                "global.profile",
                json!({ "goal": "benign context that must survive" }),
            ),
            source(ContextLayer::RunOverride, "run.override", content),
        ];
        let error = reject(&sources, &ReferenceInputs::new());
        assert_no_canary(&format!("{label} display"), &format!("{error}"));
        assert_no_canary(&format!("{label} debug"), &format!("{error:?}"));
    }

    // And the resolved, redacted pack — the only serialized output this crate
    // produces — carries none of them either.
    let redacted: ContextSource =
        serde_json::from_str(REDACTED_SOURCE).expect("redacted fixture deserializes");
    let pack = resolve(&[redacted], &ReferenceInputs::new());
    assert_no_canary("resolved pack json", pack.document().json());
    assert_no_canary("resolved pack debug", &format!("{pack:?}"));
}

#[test]
fn redacted_value_is_absent_from_snapshot_json_hash_input_and_provenance() {
    let redacted: ContextSource =
        serde_json::from_str(REDACTED_SOURCE).expect("redacted fixture deserializes");
    let pack = resolve(std::slice::from_ref(&redacted), &ReferenceInputs::new());

    // The whole subtree is gone: node, siblings and nested members alike.
    assert!(pack.resolved().pointer("/connection").is_none());
    assert!(pack.resolved().pointer("/connection/host").is_none());
    assert!(pack.resolved().pointer("/connection/pool/max").is_none());
    // Useful context is retained; redaction is not a blanket drop.
    assert_eq!(
        pack.resolved().pointer("/goal"),
        Some(&json!("ship the context pack"))
    );
    assert_eq!(
        pack.resolved().pointer("/allowed_tools"),
        Some(&json!(["read", "write"]))
    );

    // Absent from the canonical bytes, and therefore from the hash input. The
    // path survives once, as report metadata; the node and its values do not.
    let json = pack.document().json();
    assert!(!json.contains("\"connection\":"), "the node is gone");
    assert!(!json.contains("db.example"), "its values are gone");
    assert!(!json.contains("\"max\""), "its descendants are gone");
    assert_eq!(
        json.matches("/connection").count(),
        1,
        "path is metadata only"
    );
    assert_no_canary("redacted pack json", json);

    // Absent from provenance, node and descendants alike.
    assert!(
        !pack
            .provenance()
            .iter()
            .any(|entry| entry.path.as_str().starts_with("/connection")),
        "a redacted subtree keeps no provenance"
    );

    // The report is metadata only: path, declaring source, reason code.
    assert_eq!(pack.redactions().len(), 1);
    let record = &pack.redactions()[0];
    assert_eq!(record.path.as_str(), "/connection");
    assert_eq!(record.source_id, "project.profile");
    assert_eq!(record.reason, RedactionReason::CredentialLike);

    // Control: without the declared rule the same source is refused outright, so
    // redaction demonstrably runs before the core scan rather than instead of it.
    let mut undeclared = redacted;
    undeclared.redactions.clear();
    let error = reject(&[undeclared], &ReferenceInputs::new());
    assert!(matches!(error, DomainError::SensitiveMaterial { .. }));
    assert_no_canary("undeclared error", &format!("{error:?}"));
}

#[test]
fn stale_redaction_declaration_is_rejected() {
    let mut stale = source(
        ContextLayer::ProjectProfile,
        "project.profile",
        json!({ "goal": "keep the policy honest" }),
    );
    stale.redactions.push(RedactionRule {
        path: JsonPointer::parse("/notes/draft").expect("valid pointer"),
        reason: RedactionReason::PolicyExcluded,
    });
    let error = reject(&[stale], &ReferenceInputs::new());
    assert!(
        matches!(
            error,
            DomainError::InvalidAt {
                subject: "RedactionRule",
                ..
            }
        ),
        "a rule whose path no longer resolves is a stale policy, got {error:?}"
    );

    // Redacting an array element renumbers everything after it, so it is refused
    // rather than silently applied.
    let mut array_rule = source(
        ContextLayer::ProjectProfile,
        "project.profile",
        json!({ "steps": ["one", "two"] }),
    );
    array_rule.redactions.push(RedactionRule {
        path: JsonPointer::parse("/steps/0").expect("valid pointer"),
        reason: RedactionReason::PolicyExcluded,
    });
    assert!(matches!(
        reject(&[array_rule], &ReferenceInputs::new()),
        DomainError::InvalidAt {
            subject: "RedactionRule",
            ..
        }
    ));
}

#[test]
fn unresolved_restricted_reference_is_rejected() {
    let restricted: ContextSource =
        serde_json::from_str(RESTRICTED_SOURCE).expect("restricted fixture deserializes");
    let error = reject(std::slice::from_ref(&restricted), &ReferenceInputs::new());
    assert!(
        matches!(
            error,
            DomainError::InvalidAt {
                subject: "RestrictedReference",
                ..
            }
        ),
        "an unresolved grant rejects, got {error:?}"
    );

    // A declared reference whose path does not exist in the source is equally a
    // stale declaration.
    let mut dangling = restricted;
    dangling.restricted_references = vec![RestrictedReference {
        path: JsonPointer::parse("/decisions/absent").expect("valid pointer"),
        reference_key: "decision.restricted".to_owned(),
        realm_id: realm(),
    }];
    let mut grants = ReferenceInputs::new();
    grants.insert(
        "decision.restricted".to_owned(),
        ResolvedReference::Allowed {
            realm_id: realm(),
            value: json!("allowed"),
        },
    );
    assert!(matches!(
        reject(&[dangling], &grants),
        DomainError::InvalidAt {
            subject: "RestrictedReference",
            ..
        }
    ));
}

#[test]
fn denied_or_foreign_restricted_reference_is_rejected_without_value() {
    let restricted: ContextSource =
        serde_json::from_str(RESTRICTED_SOURCE).expect("restricted fixture deserializes");

    let mut denied = ReferenceInputs::new();
    denied.insert("decision.restricted".to_owned(), ResolvedReference::Denied);
    let error = reject(std::slice::from_ref(&restricted), &denied);
    assert!(
        matches!(
            error,
            DomainError::InvalidAt {
                subject: "RestrictedReference",
                ..
            }
        ),
        "a denied grant rejects, got {error:?}"
    );
    assert_no_canary("denied error", &format!("{error:?}"));

    let mut foreign = ReferenceInputs::new();
    foreign.insert(
        "decision.restricted".to_owned(),
        ResolvedReference::Allowed {
            realm_id: foreign_realm(),
            value: json!({ "adr": "ADR-0007" }),
        },
    );
    let error = reject(std::slice::from_ref(&restricted), &foreign);
    assert!(
        matches!(error, DomainError::RealmMismatch { .. }),
        "a grant issued in another realm rejects, got {error:?}"
    );
    // The mismatch names the two realms and nothing from the payload.
    let rendered = format!("{error:?}");
    assert!(!rendered.contains("ADR-0007"));
    assert!(rendered.contains(FOREIGN_REALM));

    // The full realm matrix. The pack's realm is the only realm that authorizes
    // anything: a source in realm A cannot declare a reference in realm B and
    // then satisfy it with a matching realm-B grant. Both halves of the check are
    // exercised, so dropping either one fails here.
    let mut foreign_declaration = restricted.clone();
    foreign_declaration.restricted_references[0].realm_id = foreign_realm();
    for (label, declared_in, granted_in) in [
        ("declared foreign, granted foreign", true, true),
        ("declared foreign, granted local", true, false),
        ("declared local, granted foreign", false, true),
    ] {
        let source = if declared_in {
            &foreign_declaration
        } else {
            &restricted
        };
        let mut grants = ReferenceInputs::new();
        grants.insert(
            "decision.restricted".to_owned(),
            ResolvedReference::Allowed {
                realm_id: if granted_in { foreign_realm() } else { realm() },
                value: json!({ "adr": "ADR-0007", "body": "restricted decision text" }),
            },
        );
        let error = reject(std::slice::from_ref(source), &grants);
        assert!(
            matches!(error, DomainError::RealmMismatch { .. }),
            "{label} must reject, got {error:?}"
        );
        let rendered = format!("{error:?}");
        assert!(
            !rendered.contains("ADR-0007") && !rendered.contains("restricted decision text"),
            "{label} leaked the referenced value"
        );
    }

    // Same-realm declaration plus same-realm grant is the only accepted corner.
    let mut local = ReferenceInputs::new();
    local.insert(
        "decision.restricted".to_owned(),
        ResolvedReference::Allowed {
            realm_id: realm(),
            value: json!({ "adr": "ADR-0007" }),
        },
    );
    assert_eq!(
        resolve(std::slice::from_ref(&restricted), &local)
            .resolved()
            .pointer("/decisions/restricted/adr"),
        Some(&json!("ADR-0007"))
    );

    // The happy path still works, so the guard is not simply refusing everything.
    let mut allowed = ReferenceInputs::new();
    allowed.insert(
        "decision.restricted".to_owned(),
        ResolvedReference::Allowed {
            realm_id: realm(),
            value: json!({ "adr": "ADR-0007" }),
        },
    );
    let pack = resolve(std::slice::from_ref(&restricted), &allowed);
    assert_eq!(
        pack.resolved().pointer("/decisions/restricted/adr"),
        Some(&json!("ADR-0007"))
    );
}

#[test]
fn an_authorized_reference_carrying_secret_material_still_fails_the_scan() {
    let restricted: ContextSource =
        serde_json::from_str(RESTRICTED_SOURCE).expect("restricted fixture deserializes");
    let mut granted = ReferenceInputs::new();
    granted.insert(
        "decision.restricted".to_owned(),
        ResolvedReference::Allowed {
            realm_id: realm(),
            value: json!({ "handle": "ghp_EXAMPLEONLYNOTAREALTOKEN0000" }),
        },
    );
    let error = reject(&[restricted], &granted);
    assert!(
        matches!(error, DomainError::SensitiveMaterial { .. }),
        "authorization is not permission to persist a secret, got {error:?}"
    );
    assert_no_canary("granted error", &format!("{error:?}"));
}

#[test]
fn source_metadata_cannot_become_a_secret_side_channel() {
    // A source key is an open key and goes through the same scanner.
    let mut key_channel = source(
        ContextLayer::GlobalProfile,
        "global.profile",
        json!({ "goal": "benign" }),
    );
    key_channel.source_id = "sk-abcdefghijklmnopqrstuvwxyz012345".to_owned();
    assert!(matches!(
        reject(&[key_channel], &ReferenceInputs::new()),
        DomainError::SensitiveMaterial { .. }
    ));

    // So does a declared redaction path…
    let mut path_channel = source(
        ContextLayer::GlobalProfile,
        "global.profile",
        json!({ "goal": "benign" }),
    );
    path_channel.redactions.push(RedactionRule {
        path: JsonPointer::parse("/x?password=canary-not-a-real-password").expect("valid pointer"),
        reason: RedactionReason::PolicyExcluded,
    });
    assert!(matches!(
        reject(&[path_channel], &ReferenceInputs::new()),
        DomainError::SensitiveMaterial { .. }
    ));

    // …and a declared grant key.
    let mut grant_channel = source(
        ContextLayer::GlobalProfile,
        "global.profile",
        json!({ "goal": "benign" }),
    );
    grant_channel
        .restricted_references
        .push(RestrictedReference {
            path: JsonPointer::parse("/goal").expect("valid pointer"),
            reference_key: "Decision Key".to_owned(),
            realm_id: realm(),
        });
    assert!(matches!(
        reject(&[grant_channel], &ReferenceInputs::new()),
        DomainError::Invalid {
            subject: "RestrictedReference.reference_key",
            ..
        }
    ));
}
