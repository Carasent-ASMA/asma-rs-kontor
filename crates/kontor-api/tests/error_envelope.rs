//! The refusal envelope: every domain rejection is classified deliberately, and
//! none of them is echoed back.
//!
//! The defect this suite exists for: a session-message refusal came back as
//! `"the request was refused by a domain rule"` and nothing else. The code was
//! right, the envelope was honest, and an operator holding it could not tell a
//! malformed field from a state machine saying no — five different domain
//! variants collapsed into one sentence.

use kontor_api::error::{ApiError, ApiErrorCode};
use kontor_core::DomainError;
use kontor_core::id::RealmId;

/// The sentence that must never be the answer for a variant this build knows.
const UNCLASSIFIED: &str = "a domain rule refused the request and this build cannot classify it";

/// One instance of every `DomainError` variant this build declares.
///
/// Spelled out rather than derived. The point of the list is to be a second,
/// independent statement of the variant set: adding a variant to `kontor-core`
/// without deciding how it is reported should make this fail.
fn every_variant() -> Vec<(&'static str, DomainError)> {
    vec![
        (
            "Invalid",
            DomainError::invalid("ExternalName", "must not be empty"),
        ),
        (
            "InvalidAt",
            DomainError::invalid_at("TeamTemplate", "slots[2].role", "names an unknown role"),
        ),
        (
            "IllegalTransition",
            DomainError::IllegalTransition {
                subject: "TaskState",
                from: "blocked",
                to: "succeeded",
            },
        ),
        (
            "Terminal",
            DomainError::Terminal {
                subject: "AgentRun",
            },
        ),
        (
            "RevisionConflict",
            DomainError::RevisionConflict {
                subject: "Project",
                expected: 3,
                found: 5,
            },
        ),
        (
            "MissingAuthority",
            DomainError::MissingAuthority {
                subject: "epics:apply",
                rule: "requires the admin tier",
            },
        ),
        (
            "MissingEvidence",
            DomainError::MissingEvidence {
                subject: "gate.review",
                rule: "no approval receipt has been recorded",
            },
        ),
        (
            "RealmMismatch",
            DomainError::RealmMismatch {
                expected: RealmId::generate(),
                found: RealmId::generate(),
            },
        ),
        (
            "SensitiveMaterial",
            DomainError::SensitiveMaterial {
                path: "steps[0].instruction".to_owned(),
            },
        ),
    ]
}

/// No variant this build knows may answer with the unclassified sentence.
#[test]
fn every_current_domain_variant_is_classified_rather_than_collapsed() {
    let realm_id = RealmId::generate();
    let mut rules = Vec::new();

    for (name, error) in every_variant() {
        let refusal = ApiError::from_domain(realm_id, &error);
        assert_ne!(
            refusal.rule, UNCLASSIFIED,
            "`{name}` fell through to the unclassified catch-all"
        );
        assert!(
            !refusal.action.is_empty(),
            "`{name}` gives an operator nothing to try"
        );
        assert_ne!(
            refusal.rule, refusal.action,
            "`{name}` restates itself instead of advising"
        );
        rules.push((name, refusal.rule));
    }

    // And they are distinguishable from each other, which is the whole
    // complaint: five variants sharing one sentence is what made the original
    // refusal unactionable.
    for (index, (name, rule)) in rules.iter().enumerate() {
        for (other_name, other_rule) in rules.iter().skip(index + 1) {
            assert_ne!(
                rule, other_rule,
                "`{name}` and `{other_name}` are reported identically"
            );
        }
    }
}

/// The codes stay exactly where they were. A client branches on these.
#[test]
fn the_stable_codes_are_unchanged_by_the_richer_envelope() {
    let realm_id = RealmId::generate();
    let expected = [
        ("Invalid", ApiErrorCode::InvalidRequest),
        ("InvalidAt", ApiErrorCode::InvalidRequest),
        ("IllegalTransition", ApiErrorCode::InvalidRequest),
        ("MissingEvidence", ApiErrorCode::InvalidRequest),
        ("SensitiveMaterial", ApiErrorCode::InvalidRequest),
        ("Terminal", ApiErrorCode::RevisionConflict),
        ("RevisionConflict", ApiErrorCode::RevisionConflict),
        ("MissingAuthority", ApiErrorCode::Forbidden),
        ("RealmMismatch", ApiErrorCode::RealmMismatch),
    ];
    for (name, code) in expected {
        let error = every_variant()
            .into_iter()
            .find(|(variant, _)| *variant == name)
            .map(|(_, error)| error)
            .expect("the variant is in the table");
        assert_eq!(
            ApiError::from_domain(realm_id, &error).code,
            code,
            "`{name}` changed the code a client branches on"
        );
    }
}

/// The new fields say *where*, and never *what*.
#[test]
fn the_envelope_names_the_subject_and_the_path_but_never_the_value() {
    let realm_id = RealmId::generate();

    let refusal = ApiError::from_domain(
        realm_id,
        &DomainError::invalid_at("TeamTemplate", "slots[2].role", "names an unknown role"),
    );
    assert_eq!(refusal.subject(), Some("TeamTemplate"));
    assert_eq!(refusal.at(), Some("slots[2].role"));

    // A refused credential is located exactly and described only by category.
    let refusal = ApiError::from_domain(
        realm_id,
        &DomainError::SensitiveMaterial {
            path: "steps[0].instruction".to_owned(),
        },
    );
    assert_eq!(refusal.at(), Some("steps[0].instruction"));
    let rendered = format!("{:?}", refusal.body());
    for forbidden in ["ghp_", "sk-", "Bearer ", "password"] {
        assert!(
            !rendered.contains(forbidden),
            "the envelope quoted credential-shaped material: {rendered}"
        );
    }
}

/// A transition to the state the aggregate is already in says so.
#[test]
fn asking_for_the_state_an_aggregate_already_holds_is_advised_differently() {
    let realm_id = RealmId::generate();
    let same = ApiError::from_domain(
        realm_id,
        &DomainError::IllegalTransition {
            subject: "TaskState",
            from: "ready",
            to: "ready",
        },
    );
    let different = ApiError::from_domain(
        realm_id,
        &DomainError::IllegalTransition {
            subject: "TaskState",
            from: "blocked",
            to: "succeeded",
        },
    );
    assert_ne!(
        same.action, different.action,
        "\"it is already there\" and \"it cannot go there\" need different advice"
    );
    assert!(same.action.contains("already"));
}

/// Every code carries an action, including the ones no domain error produces.
#[test]
fn every_code_tells_an_operator_something_to_try() {
    for code in ApiErrorCode::ALL {
        let action = code.default_action();
        assert!(
            !action.is_empty(),
            "`{code}` leaves an operator with nothing to do"
        );
        assert!(
            action.len() > 20,
            "`{code}` advises too little to act on: {action}"
        );
    }
}
