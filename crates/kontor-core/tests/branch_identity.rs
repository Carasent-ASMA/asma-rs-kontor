//! Shared publication-branch grammar (ASMA-8101): the ASMA CLI derivation,
//! ported so Kontor can neither create nor accept a branch without a confirmed
//! tracker identity.

use kontor_core::DomainError;
use kontor_core::branch::{
    BranchName, BranchRefusal, BranchType, MANAGED_WORKTREES_DIR, SLUG_MAX_LEN, TrackerKey,
    managed_branch_text, managed_worktree_path, slugify,
};
use kontor_core::id::ExternalId;

fn key(text: &str) -> TrackerKey {
    TrackerKey::parse(text).expect("a canonical tracker key")
}

#[test]
fn derivation_matches_the_asma_cli_examples() {
    for (branch_type, tracker, title, expected) in [
        (
            BranchType::Feat,
            "ASMA-1234",
            "Add user profile page",
            "feat/ASMA-1234-add-user-profile-page",
        ),
        (
            BranchType::Fix,
            "ASMA-99",
            "Fix login crash",
            "fix/ASMA-99-fix-login-crash",
        ),
        (
            BranchType::Fix,
            "ASMA-7",
            "Fix: login (v2)",
            "fix/ASMA-7-fix-login-v2",
        ),
        (
            BranchType::Docs,
            "ASMA-8101",
            "a   b---c",
            "docs/ASMA-8101-a-b-c",
        ),
        (
            BranchType::Chore,
            "ASMA-8101",
            "!!!",
            "chore/ASMA-8101-work-item",
        ),
    ] {
        let branch = BranchName::derive(branch_type, &key(tracker), title);
        assert_eq!(branch.as_str(), expected);
        assert_eq!(
            BranchName::parse(expected).expect("a derived branch parses back"),
            branch,
            "derivation and parsing agree on {expected}"
        );
    }
}

#[test]
fn a_long_title_is_cut_without_a_dangling_hyphen() {
    let long = format!("{} {}", "a".repeat(SLUG_MAX_LEN - 1), "b".repeat(50));
    let slug = slugify(&long);
    assert_eq!(
        slug.len(),
        SLUG_MAX_LEN - 1,
        "the cut lands on the separator, which is dropped"
    );
    assert!(!slug.ends_with('-'));
    assert!(slugify(&"x".repeat(300)).len() <= SLUG_MAX_LEN);
    assert_eq!(
        slugify("  Émigré café "),
        "migr-caf",
        "non-ASCII letters are separators, as in the CLI"
    );
}

#[test]
fn the_release_line_is_the_only_keyless_form() {
    let release = BranchName::parse("releases/v1.42.0").expect("a hotpatch line");
    assert_eq!(release.branch_type(), BranchType::Releases);
    assert!(release.key().is_none());
    assert_eq!(
        release.ensure_bound_to([&key("ASMA-1")]),
        Err(BranchRefusal::BindingMismatch),
        "a keyless branch never satisfies a task binding"
    );
    let keyed = BranchName::parse("releases/ASMA-1-hotfix").expect("a keyed release branch");
    assert_eq!(keyed.key(), Some(&key("ASMA-1")));
    assert_eq!(
        BranchName::parse("releases/v1.2"),
        Err(BranchRefusal::KeyMissing)
    );
    assert_eq!(
        BranchName::parse("releases/1.2.3"),
        Err(BranchRefusal::KeyMissing)
    );
}

#[test]
fn every_audited_deviation_is_refused_with_its_code() {
    for (branch, refusal) in [
        ("cat-11", BranchRefusal::ShapeInvalid),
        ("kon-op-22", BranchRefusal::ShapeInvalid),
        ("master", BranchRefusal::ShapeInvalid),
        (
            "consultation-01a07302-88e4-7192-95f3-f26c06aaa642",
            BranchRefusal::ShapeInvalid,
        ),
        ("land/qnr-plan-amendment", BranchRefusal::TypeUnknown),
        ("bot/pointer/ASMA-1-x", BranchRefusal::TypeUnknown),
        ("Feat/ASMA-1-x", BranchRefusal::TypeUnknown),
        ("fix/restore-ignore-suffix-rule", BranchRefusal::KeyMissing),
        (
            "fix/og030-mcp-v2-seat-credential",
            BranchRefusal::KeyMissing,
        ),
        (
            "chore/task-045-stranded-work-sweep",
            BranchRefusal::KeyMissing,
        ),
        ("fix/", BranchRefusal::KeyMissing),
        ("docs/asma-8050-closeout", BranchRefusal::KeyNotCanonical),
        (
            "fix/asma-8001-consultation-seat-profile",
            BranchRefusal::KeyNotCanonical,
        ),
        (
            "fix/KON-OP-22-current-master-reconstruction",
            BranchRefusal::KeyNotCanonical,
        ),
        ("feat/ASMA-0123-x", BranchRefusal::KeyNotCanonical),
        ("feat/ASMA-x", BranchRefusal::KeyNotCanonical),
        ("feat/ASMA8101-x", BranchRefusal::KeyNotCanonical),
        ("feat/ASMA-8101", BranchRefusal::SlugInvalid),
        ("feat/ASMA-8101-", BranchRefusal::SlugInvalid),
        ("feat/ASMA-8101-Foo", BranchRefusal::SlugInvalid),
        ("feat/ASMA-8101-a--b", BranchRefusal::SlugInvalid),
        ("feat/ASMA-8101-a_b", BranchRefusal::SlugInvalid),
        ("feat/ASMA-8101-a/b", BranchRefusal::SlugInvalid),
        ("feat/ASMA-8101-x/", BranchRefusal::SlugInvalid),
    ] {
        assert_eq!(BranchName::parse(branch), Err(refusal), "{branch}");
    }
}

#[test]
fn canonical_branches_parse_with_their_key() {
    for (branch, tracker) in [
        (
            "feat/ASMA-8101-publication-identity-enforcement",
            "ASMA-8101",
        ),
        (
            "fix/ASMA-8030-kontor-documentation-audit-release",
            "ASMA-8030",
        ),
        ("docs/ASMA-7628-phase0-session-decisions", "ASMA-7628"),
        (
            "chore/ASMA-8101-consultation-01a07302-88e4-7192-95f3-f26c06aaa642",
            "ASMA-8101",
        ),
        ("ci/K2-1-x", "K2-1"),
    ] {
        let parsed = BranchName::parse(branch).expect(branch);
        assert_eq!(parsed.as_str(), branch);
        assert_eq!(parsed.key(), Some(&key(tracker)));
    }
}

#[test]
fn the_binding_check_accepts_only_a_confirmed_key() {
    let branch = BranchName::parse("feat/ASMA-8101-x").expect("canonical");
    let epic = key("ASMA-8101");
    let task = key("ASMA-8102");
    assert_eq!(branch.ensure_bound_to([&task, &epic]), Ok(()));
    assert_eq!(
        branch.ensure_bound_to([&task]),
        Err(BranchRefusal::BindingMismatch)
    );
    assert_eq!(
        branch.ensure_bound_to([]),
        Err(BranchRefusal::BindingUnconfirmed)
    );
}

#[test]
fn a_tracker_key_is_never_repaired() {
    assert_eq!(
        TrackerKey::parse("asma-8050"),
        Err(BranchRefusal::KeyNotCanonical)
    );
    assert_eq!(
        TrackerKey::parse("ASMA-8050-x"),
        Err(BranchRefusal::KeyNotCanonical)
    );
    assert_eq!(
        TrackerKey::parse("KON-OP-22"),
        Err(BranchRefusal::KeyNotCanonical)
    );
    assert_eq!(TrackerKey::parse(""), Err(BranchRefusal::KeyMissing));
    assert_eq!(TrackerKey::parse("8050"), Err(BranchRefusal::KeyMissing));
    let internal = ExternalId::parse("01a0721b-ea30-7fe3-88a5-4d33ca613414").expect("an id");
    assert_eq!(
        TrackerKey::from_external(&internal),
        Err(BranchRefusal::KeyMissing),
        "an internal id standing in for a key is not a key"
    );
    assert_eq!(key("ASMA-8101").to_string(), "ASMA-8101");
}

#[test]
fn a_managed_worktree_encodes_exactly_its_branch() {
    let branch = BranchName::parse("feat/ASMA-8101-x").expect("canonical");
    let path = managed_worktree_path("/repo/", &branch);
    assert_eq!(
        path,
        format!("/repo/{MANAGED_WORKTREES_DIR}/feat/ASMA-8101-x")
    );
    assert_eq!(
        managed_branch_text("/repo", &path),
        Some("feat/ASMA-8101-x")
    );
    assert_eq!(
        managed_branch_text("/repo", "/repo/.worktrees/cat-11/"),
        Some("cat-11")
    );
    assert_eq!(managed_branch_text("/repo", "/repo/.worktrees/"), None);
    assert_eq!(
        managed_branch_text("/repo", "/repo/other/feat/ASMA-1-x"),
        None
    );
    assert_eq!(
        managed_branch_text("/repo", "/elsewhere/.worktrees/feat/ASMA-1-x"),
        None
    );
    assert_eq!(
        managed_branch_text("/repo", "/repository/.worktrees/x"),
        None
    );
}

#[test]
fn every_refusal_reports_its_stable_code_first() {
    for refusal in BranchRefusal::ALL {
        assert!(
            refusal.rule().starts_with(refusal.as_str()),
            "{} must lead with its code",
            refusal.as_str()
        );
    }
    assert_eq!(
        DomainError::from(BranchRefusal::KeyMissing),
        DomainError::invalid("BranchName", BranchRefusal::KeyMissing.rule())
    );
}
