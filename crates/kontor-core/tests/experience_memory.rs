//! Strict typed experience acceptance and redacted refusals.
use kontor_core::id::CanonicalDocument;
use kontor_core::memory::*;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/experience-v1.json")).unwrap()
}
fn admit(value: &Value) -> Result<ExperienceMemoryV1, kontor_core::DomainError> {
    ExperienceMemoryV1::from_document(&CanonicalDocument::from_value(value)?)
}

#[test]
fn strict_experience_roundtrip_and_non_memory_eligibility() {
    let value = fixture();
    let doc = CanonicalDocument::from_value(&value).unwrap();
    let experience = admit(&value).unwrap();
    assert_eq!(experience.projection_policy, ProjectionPolicy::LocalOnly);
    assert_eq!(experience.kind, ExperienceKind::Experience);
    assert!(is_recall_eligible(&doc));
    assert!(is_recall_eligible(&experience.canonical().unwrap()));
    for kind in [
        "operational_gap",
        "progress",
        "handoff",
        "open_question",
        "verification_report",
        "source",
        "documentation",
    ] {
        let mut wrong = value.clone();
        wrong["kind"] = json!(kind);
        let doc = CanonicalDocument::from_value(&wrong).unwrap();
        assert!(
            !is_recall_eligible(&doc),
            "operational evidence must never become recall eligible"
        );
    }
    for kind in ExperienceKind::ALL {
        let mut v = value.clone();
        v["kind"] = json!(kind);
        assert!(admit(&v).is_ok());
    }
}
#[test]
fn every_field_enum_bound_and_unknown_field_is_checked() {
    let value = fixture();
    for key in value
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| *key != "projection_policy")
    {
        let mut v = value.clone();
        v.as_object_mut().unwrap().remove(key);
        assert!(admit(&v).is_err(), "missing field {key}");
    }
    for (key, bad) in [
        ("schema_version", json!(2)),
        ("document_type", json!("operational_gap")),
        ("confidence", json!("certain")),
        ("projection_policy", json!("any_provider")),
        ("occurred_at", json!("yesterday")),
        (
            "outcome",
            json!({"kind":"pending","summary":"still working"}),
        ),
    ] {
        let mut v = value.clone();
        v[key] = bad;
        assert!(admit(&v).is_err(), "invalid field {key}");
    }
    for key in ["situation", "intent", "lesson"] {
        for bad in ["", " ", &"x".repeat(4097)] {
            let mut v = value.clone();
            v[key] = json!(bad);
            assert!(admit(&v).is_err());
        }
        let mut v = value.clone();
        v[key] = json!("x".repeat(4096));
        assert!(admit(&v).is_ok());
        v[key] = json!("ø".repeat(2049));
        assert!(admit(&v).is_err());
    }
    for key in [
        "actions",
        "went_well",
        "went_wrong",
        "future_cues",
        "avoid",
        "domains",
    ] {
        let mut v = value.clone();
        v[key] = json!(vec!["x"; 17]);
        assert!(admit(&v).is_err());
        v[key] = json!(["x".repeat(2049)]);
        assert!(admit(&v).is_err());
        v[key] = json!([" "]);
        assert!(admit(&v).is_err());
        v[key] = json!(vec!["x"; 16]);
        assert!(admit(&v).is_ok());
    }
    for key in ["actions", "future_cues", "domains", "evidence_refs"] {
        let mut v = value.clone();
        v[key] = json!([]);
        assert!(admit(&v).is_err());
    }
    for location in ["root", "outcome", "evidence"] {
        let mut v = value.clone();
        match location {
            "root" => v["unknown"] = json!(1),
            "outcome" => v["outcome"]["unknown"] = json!(1),
            _ => v["evidence_refs"][0]["unknown"] = json!(1),
        };
        assert!(admit(&v).is_err());
    }
    for locator in [
        "../escape",
        "/absolute",
        "https://example.invalid/evidence",
        "evidence//file",
        "evidence/./file",
    ] {
        let mut v = value.clone();
        v["evidence_refs"][0]["locator"] = json!(locator);
        assert!(admit(&v).is_err());
    }
    let mut v = value.clone();
    v["evidence_refs"] =
        json!([{"type":"receipt","receipt_id":"not-a-uuid","content_hash":"a".repeat(64)}]);
    assert!(admit(&v).is_err());
    v["evidence_refs"] =
        json!([{"type":"artifact","locator":"synthetic/x","content_hash":"invalid"}]);
    assert!(admit(&v).is_err());
}
#[test]
fn secrets_in_every_text_list_locator_and_key_are_redacted() {
    let value = fixture();
    let secrets = [
        "ghp_abcdefghijklmnopqrstuvwxyz",
        "password=CANARY_SECRET_VALUE",
        "-----BEGIN PRIVATE KEY-----",
        "https://user:CANARY_SECRET_VALUE@example.invalid",
        "https://CANARY_SECRET_VALUE@example.invalid",
    ];
    for secret in secrets {
        for key in ["situation", "intent", "lesson"] {
            let mut v = value.clone();
            v[key] = json!(secret);
            let err = admit(&v).unwrap_err();
            assert!(!format!("{err:?} {err}").contains(secret));
        }
        for key in [
            "actions",
            "went_well",
            "went_wrong",
            "future_cues",
            "avoid",
            "domains",
        ] {
            let mut v = value.clone();
            v[key] = json!([secret]);
            let err = admit(&v).unwrap_err();
            assert!(!format!("{err:?} {err}").contains(secret));
        }
        let mut v = value.clone();
        v["evidence_refs"][0]["locator"] = json!(secret);
        let err = admit(&v).unwrap_err();
        assert!(!format!("{err:?} {err}").contains(secret));
        let mut v = value.clone();
        v[secret] = json!("safe");
        let err = CanonicalDocument::from_value(&v).unwrap_err();
        assert!(!format!("{err:?} {err}").contains(secret));
    }
    let mut generic = json!({"schema_version":1,"text":"generic ledger document"});
    assert!(CanonicalDocument::from_value(&generic).is_ok());
    generic["nested"] = json!({"password":"canary"});
    assert!(CanonicalDocument::from_value(&generic).is_err());
}

#[test]
fn typed_byte_and_evidence_bounds_preserve_the_generic_document_ceiling() {
    let value = fixture();
    let mut oversized = value.clone();
    oversized["actions"] = json!(vec!["x".repeat(2048); 16]);
    oversized["went_well"] = json!(vec!["x".repeat(2048); 16]);
    assert!(CanonicalDocument::from_value(&oversized).is_ok());
    assert!(admit(&oversized).is_err());
    let mut references = value.clone();
    references["evidence_refs"] = json!(vec![value["evidence_refs"][0].clone(); 17]);
    assert!(admit(&references).is_err());
}
