//! Publication identity decisions (ASMA-8101): the contract behind the ASMA
//! CLI's push and pull-request steps and behind the forge check.

use kontor_core::branch::{BranchName, TrackerKey};
use kontor_core::id::ExternalName;
use kontor_core::publication::{
    CommitSha, POLICY_REVISION, PublicationBinding, PublicationDecision, PublicationIdentity,
    evaluate, title_key,
};

fn key(text: &str) -> TrackerKey {
    TrackerKey::parse(text).expect("a canonical key")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a name")
}

fn binding() -> PublicationBinding {
    PublicationBinding {
        epic_key: key("ASMA-8101"),
        task_key: None,
        child_keys: vec![key("ASMA-8102"), key("ASMA-8104")],
        default_branch: name("master"),
    }
}

fn identity(head_branch: &str, base: &str, title: Option<&str>) -> PublicationIdentity {
    PublicationIdentity {
        repository: name("Carasent-ASMA/asma-modules"),
        base_branch: name(base),
        head_branch: BranchName::parse(head_branch).expect("a canonical branch"),
        head_sha: CommitSha::parse("82c56f5043b436ba666962cf82a89e93e330a271").expect("a sha"),
        pull_request: Some(2985),
        title: title.map(name),
    }
}

#[test]
fn an_epic_branch_with_a_child_title_is_accepted() {
    let decision = evaluate(
        &identity(
            "feat/ASMA-8101-publication-identity-enforcement",
            "master",
            Some("ASMA-8104 Enforce the publication branch grammar"),
        ),
        &binding(),
    );
    assert_eq!(decision, PublicationDecision::accepted());
    assert_eq!(decision.policy_revision, POLICY_REVISION);
}

#[test]
fn a_task_branch_binds_through_its_own_key() {
    let mut task_binding = binding();
    task_binding.task_key = Some(key("ASMA-8102"));
    let decision = evaluate(
        &identity("feat/ASMA-8102-kontor-attestation", "master", None),
        &task_binding,
    );
    assert!(decision.accepted, "{decision:?}");
    // Without the task key in the binding, the same branch is somebody else's.
    let decision = evaluate(
        &identity("feat/ASMA-8102-kontor-attestation", "master", None),
        &binding(),
    );
    assert!(
        decision.refused_for("branch_binding_mismatch"),
        "{decision:?}"
    );
}

#[test]
fn every_failing_rule_is_reported_at_once() {
    let decision = evaluate(
        &identity("feat/ASMA-1-somebody-elses", "develop", Some("cat 11")),
        &binding(),
    );
    assert!(!decision.accepted);
    assert_eq!(
        decision.reasons,
        vec![
            "branch_binding_mismatch".to_owned(),
            "base_branch_not_default".to_owned(),
            "pr_title_key_missing".to_owned(),
        ]
    );
}

#[test]
fn a_title_naming_a_foreign_key_is_a_mismatch() {
    let decision = evaluate(
        &identity(
            "feat/ASMA-8101-x",
            "master",
            Some("ASMA-7869 Something else"),
        ),
        &binding(),
    );
    assert_eq!(decision.reasons, vec!["pr_title_key_mismatch".to_owned()]);
}

#[test]
fn a_title_leads_with_exactly_one_key_and_one_space() {
    assert_eq!(
        title_key("ASMA-8101 Close the case"),
        Some(key("ASMA-8101"))
    );
    for title in [
        "ASMA-8101",
        "ASMA-8101: Close",
        "asma-8101 close",
        "Close ASMA-8101",
        "ASMA-8101 ",
    ] {
        assert_eq!(title_key(title), None, "{title}");
    }
}

#[test]
fn a_keyless_or_unbound_branch_is_refused_before_any_rule() {
    let refused =
        PublicationDecision::refused_branch(kontor_core::branch::BranchRefusal::ShapeInvalid);
    assert_eq!(refused.reasons, vec!["branch_shape_invalid".to_owned()]);
    assert!(!refused.accepted);
    assert_eq!(
        PublicationDecision::unconfirmed().reasons,
        vec!["binding_unconfirmed".to_owned()]
    );
}

#[test]
fn a_commit_sha_is_forty_lowercase_hex_digits() {
    assert!(CommitSha::parse("82c56f5043b436ba666962cf82a89e93e330a271").is_ok());
    for text in [
        "82c56f50",
        "82C56F5043B436BA666962CF82A89E93E330A271",
        "82c56f5043b436ba666962cf82a89e93e330a27g",
        "",
    ] {
        assert!(CommitSha::parse(text).is_err(), "{text}");
    }
}
