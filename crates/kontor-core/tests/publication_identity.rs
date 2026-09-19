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

/// The governed roots the daemon derives for this epic: the superproject and
/// the one module repository its tasks own.
fn binding() -> PublicationBinding {
    PublicationBinding {
        repositories: vec![
            name("Carasent-ASMA/asma-modules"),
            name("Carasent-ASMA/asma-rs-kontor"),
        ],
        epic_key: key("ASMA-8101"),
        task_key: None,
        child_keys: vec![key("ASMA-8102"), key("ASMA-8104")],
        default_branch: name("master"),
    }
}

#[test]
fn a_publication_for_an_unbound_repository_is_refused() {
    let mut wrong = identity(
        "feat/ASMA-8101-x",
        "master",
        Some("ASMA-8101 Enforce the grammar"),
    );
    wrong.repository = name("Carasent-ASMA/not-this-repository");
    assert_eq!(
        evaluate(&wrong, &binding()).reasons,
        vec!["repository_mismatch".to_owned()]
    );
}

fn identity_in(
    repository: &str,
    head_branch: &str,
    base: &str,
    pull_request: Option<u64>,
    title: Option<&str>,
) -> PublicationIdentity {
    PublicationIdentity {
        repository: name(repository),
        base_branch: name(base),
        head_branch: BranchName::parse(head_branch).expect("a canonical branch"),
        head_sha: CommitSha::parse("82c56f5043b436ba666962cf82a89e93e330a271").expect("a sha"),
        pull_request,
        title: title.map(name),
    }
}

/// A pull-request publication from the superproject root.
fn identity(head_branch: &str, base: &str, title: Option<&str>) -> PublicationIdentity {
    identity_in(
        "Carasent-ASMA/asma-modules",
        head_branch,
        base,
        Some(2985),
        title,
    )
}

/// A push: a branch with no pull request and therefore no title to judge.
fn push(head_branch: &str) -> PublicationIdentity {
    identity_in(
        "Carasent-ASMA/asma-modules",
        head_branch,
        "master",
        None,
        None,
    )
}

#[test]
fn an_epic_branch_accepts_only_its_own_title_key() {
    let accepted = evaluate(
        &identity(
            "feat/ASMA-8101-publication-identity-enforcement",
            "master",
            Some("ASMA-8101 Enforce the publication branch grammar"),
        ),
        &binding(),
    );
    assert_eq!(accepted, PublicationDecision::accepted());
    assert_eq!(accepted.policy_revision, POLICY_REVISION);

    let child = evaluate(
        &identity(
            "feat/ASMA-8101-publication-identity-enforcement",
            "master",
            Some("ASMA-8104 Borrow the epic branch"),
        ),
        &binding(),
    );
    assert_eq!(child.reasons, vec!["pr_title_key_mismatch".to_owned()]);
}

#[test]
fn a_task_branch_binds_through_its_own_key() {
    let mut task_binding = binding();
    task_binding.task_key = Some(key("ASMA-8102"));
    let decision = evaluate(&push("feat/ASMA-8102-kontor-attestation"), &task_binding);
    assert!(decision.accepted, "{decision:?}");

    let same_task_title = evaluate(
        &identity(
            "feat/ASMA-8102-kontor-attestation",
            "master",
            Some("ASMA-8102 Attest the publication"),
        ),
        &task_binding,
    );
    assert!(same_task_title.accepted, "{same_task_title:?}");

    let sibling_title = evaluate(
        &identity(
            "feat/ASMA-8102-kontor-attestation",
            "master",
            Some("ASMA-8104 Reuse the sibling branch"),
        ),
        &task_binding,
    );
    assert_eq!(
        sibling_title.reasons,
        vec!["pr_title_key_mismatch".to_owned()],
        "a task branch must never publish a sibling task"
    );

    // Without the task key in the binding, the same branch is somebody else's.
    let decision = evaluate(&push("feat/ASMA-8102-kontor-attestation"), &binding());
    assert!(
        decision.refused_for("branch_binding_mismatch"),
        "{decision:?}"
    );
}

#[test]
fn every_failing_rule_is_reported_at_once() {
    let mut wrong = identity("feat/ASMA-1-somebody-elses", "develop", Some("cat 11"));
    wrong.repository = name("Carasent-ASMA/not-this-repository");
    let decision = evaluate(&wrong, &binding());
    assert!(!decision.accepted);
    assert_eq!(
        decision.reasons,
        vec![
            "repository_mismatch".to_owned(),
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

#[test]
fn a_repository_outside_the_binding_is_refused() {
    let title = Some("ASMA-8101 Enforce the grammar");
    let head = "feat/ASMA-8101-publication-identity-enforcement";

    // Both governed roots are authorized for this epic.
    for governed in ["Carasent-ASMA/asma-modules", "Carasent-ASMA/asma-rs-kontor"] {
        let accepted = evaluate(
            &identity_in(governed, head, "master", Some(2985), title),
            &binding(),
        );
        assert!(accepted.accepted, "{governed}: {accepted:?}");
    }

    // A different owner is a different forge account, however familiar the name.
    for foreign in [
        "WrongOrg/asma-modules",
        "WrongOrg/asma-rs-kontor",
        "Carasent-ASMA/asma-unrelated",
        "carasent-asma/asma-modules",
    ] {
        let decision = evaluate(
            &identity_in(foreign, head, "master", Some(2985), title),
            &binding(),
        );
        assert_eq!(
            decision.reasons,
            vec!["repository_mismatch".to_owned()],
            "{foreign} must not publish under this binding"
        );
    }
}

#[test]
fn a_task_branch_is_narrower_than_its_epic() {
    // The daemon derives only the root for a task owning no module repository.
    let mut task_binding = binding();
    task_binding.task_key = Some(key("ASMA-8102"));
    task_binding.repositories = vec![name("Carasent-ASMA/asma-modules")];

    let accepted = evaluate(&push("feat/ASMA-8102-kontor-attestation"), &task_binding);
    assert!(accepted.accepted, "{accepted:?}");

    let sibling = evaluate(
        &identity_in(
            "Carasent-ASMA/asma-rs-kontor",
            "feat/ASMA-8102-kontor-attestation",
            "master",
            None,
            None,
        ),
        &task_binding,
    );
    assert_eq!(
        sibling.reasons,
        vec!["repository_mismatch".to_owned()],
        "a task must not publish into a module repository it does not own"
    );
}

#[test]
fn an_empty_authorization_set_authorizes_nothing() {
    let mut closed = binding();
    closed.repositories = Vec::new();
    let decision = evaluate(
        &push("feat/ASMA-8101-publication-identity-enforcement"),
        &closed,
    );
    assert_eq!(decision.reasons, vec!["repository_mismatch".to_owned()]);
}

#[test]
fn a_pull_request_must_carry_a_title() {
    let head = "feat/ASMA-8101-publication-identity-enforcement";

    // The defect: a pull request with no title skipped title validation and was
    // accepted, so a pull request could publish under no confirmed key at all.
    let untitled = evaluate(
        &identity_in(
            "Carasent-ASMA/asma-modules",
            head,
            "master",
            Some(2985),
            None,
        ),
        &binding(),
    );
    assert_eq!(
        untitled.reasons,
        vec!["pr_title_key_missing".to_owned()],
        "a pull request without a title names no key"
    );

    // A push carries no pull request, so there is no title to require.
    let pushed = evaluate(&push(head), &binding());
    assert!(pushed.accepted, "{pushed:?}");

    // A titled pull request still passes, and a mis-keyed one still fails.
    let titled = evaluate(
        &identity(head, "master", Some("ASMA-8101 Enforce the grammar")),
        &binding(),
    );
    assert!(titled.accepted, "{titled:?}");
    let mis_keyed = evaluate(
        &identity(head, "master", Some("ASMA-7869 Something else")),
        &binding(),
    );
    assert_eq!(mis_keyed.reasons, vec!["pr_title_key_mismatch".to_owned()]);
}

#[test]
fn an_unauthorized_repository_is_reported_beside_every_other_failure() {
    let decision = evaluate(
        &identity_in(
            "WrongOrg/asma-modules",
            "feat/ASMA-1-somebody-elses",
            "develop",
            Some(2985),
            None,
        ),
        &binding(),
    );
    assert_eq!(
        decision.reasons,
        vec![
            "repository_mismatch".to_owned(),
            "branch_binding_mismatch".to_owned(),
            "base_branch_not_default".to_owned(),
            "pr_title_key_missing".to_owned(),
        ],
        "a producer learns everything wrong at once"
    );
}
