//! TEST-002: identity, pins, operator sources, divergent worktrees, ambiguous
//! renames, malformed front matter and registry honesty.
//!
//! Every case asserts what stays visible. None of these cases may delete a fact,
//! create a registry entry or turn a declaration into a verification.

mod support;

use std::path::PathBuf;

use kontor_catalog::config::OperatorSource;
use kontor_catalog::contract::ConflictKind;
use kontor_catalog::{CatalogSettings, EntryFilter, SourceKind, SourceRevisionKind};
use support::{Fixture, Repo, plan, registry, sweep};
use tempfile::TempDir;

fn operator_source(
    id: &str,
    path: PathBuf,
    logical_plan_id: Option<&str>,
    pin: Option<String>,
) -> OperatorSource {
    OperatorSource {
        id: id.to_owned(),
        title: None,
        kind: SourceKind::Plan,
        path,
        workstream: Some("kontor".to_owned()),
        epic: Some("ASMA-9100".to_owned()),
        project_id: None,
        revision_kind: if pin.is_some() {
            SourceRevisionKind::ApprovedRevision
        } else {
            SourceRevisionKind::WorkingCopy
        },
        expected_sha256: pin,
        logical_plan_id: logical_plan_id.map(ToOwned::to_owned),
    }
}

#[tokio::test]
async fn explicit_source_identity_pins_and_conflicts_are_preserved() {
    let repo = Repo::new();
    let content = plan(
        "Pinned plan",
        Some("ASMA-9100"),
        Some("kontor"),
        "catalog_id: stable-id\n",
    );
    let path = repo.write("_docs/plans/pinned.md", &content);
    repo.commit("pinned");
    let pin = kontor_catalog::config::sha256(content.as_bytes());
    let settings = CatalogSettings {
        schema_version: 1,
        repositories: vec![],
        sources: vec![operator_source(
            "source-alpha",
            path.clone(),
            Some("logical-alpha"),
            Some(pin.clone()),
        )],
        registry_path: None,
        cadence_seconds: 60,
        scan_budget_seconds: 30,
        ..CatalogSettings::default()
    };
    let fixture = Fixture::open_settings(settings);

    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    let entry = entries
        .entries
        .iter()
        .find(|entry| entry.logical_plan_id == "logical-alpha")
        .expect("the operator logical id is preserved");
    let occurrence = &entry.occurrences[0];
    assert_eq!(
        occurrence.content_kind,
        kontor_catalog::ContentKind::ExternalSource
    );
    assert_eq!(occurrence.source_ids, vec!["source-alpha".to_owned()]);
    assert_eq!(occurrence.expected_sha256.as_deref(), Some(pin.as_str()));
    assert_eq!(occurrence.pin_matches, Some(true));
    assert!(
        entry.associations.iter().any(|association| association.kind
            == kontor_catalog::contract::AssociationKind::ExplicitOperator
            && association.relation == kontor_catalog::contract::AssociationRelation::Workstream
            && association.value == "kontor"
            && !association.verified),
        "the explicit association is retained and not upgraded to verified"
    );
    assert!(
        entry
            .aliases
            .iter()
            .any(|alias| alias.kind == kontor_catalog::contract::AliasKind::ExplicitId)
    );

    // A changed file under a declared pin is a visible conflict, not a removal
    // and not a silent acceptance.
    repo.write(
        "_docs/plans/pinned.md",
        &plan(
            "Pinned plan changed",
            Some("ASMA-9100"),
            Some("kontor"),
            "catalog_id: stable-id\n",
        ),
    );
    repo.commit("changed after approval");
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    let entry = entries
        .entries
        .iter()
        .find(|entry| entry.logical_plan_id == "logical-alpha")
        .expect("the pinned plan remains visible");
    assert_eq!(entry.occurrences[0].pin_matches, Some(false));
    assert!(
        entry
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == ConflictKind::PinMismatch)
    );
}

#[tokio::test]
async fn divergent_worktrees_are_one_identity_with_two_occurrences_and_a_conflict() {
    let repo = Repo::new();
    let content = plan("Shared plan", Some("ASMA-9200"), Some("kontor"), "");
    repo.write("_docs/plans/shared.md", &content);
    repo.commit("shared");

    let worktree_root = TempDir::new().expect("a worktree parent");
    let worktree = repo.worktree(&worktree_root.path().join("wt"), "divergent");
    std::fs::write(
        worktree.join("_docs/plans/shared.md"),
        plan("Shared plan edited", Some("ASMA-9200"), Some("kontor"), ""),
    )
    .expect("the worktree edit");
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["add", "-A"])
        .output()
        .expect("git add");
    assert!(output.status.success());
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["commit", "-q", "-m", "divergent"])
        .output()
        .expect("git commit");
    assert!(output.status.success());

    let fixture = Fixture::open(
        &[(repo.root().to_path_buf(), false), (worktree, false)],
        None,
        30,
    );
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(entries.entries.len(), 1, "the logical plan is one identity");
    let entry = &entries.entries[0];
    assert_eq!(entry.occurrences.len(), 2);
    assert_ne!(
        entry.occurrences[0].content.revision_id,
        entry.occurrences[1].content.revision_id
    );
    assert!(
        entry
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == ConflictKind::DivergentWorktree)
    );
}

#[tokio::test]
async fn ambiguous_rename_is_exposed_and_never_merged() {
    let repo = Repo::new();
    let content = plan("Ambiguous plan", Some("ASMA-9300"), Some("kontor"), "");
    repo.write("plans/a.md", &content);
    repo.write("plans/b.md", &content);
    repo.commit("two identical plans");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;
    let before = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(before.entries.len(), 2);
    let old_ids: Vec<String> = before
        .entries
        .iter()
        .map(|entry| entry.logical_plan_id.clone())
        .collect();

    std::fs::remove_file(repo.root().join("plans/a.md")).expect("remove a");
    std::fs::remove_file(repo.root().join("plans/b.md")).expect("remove b");
    repo.write("plans/c.md", &content);
    repo.commit("one identical successor");
    sweep(&fixture.catalog).await;
    let after = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(after.entries.len(), 1);
    assert!(!old_ids.contains(&after.entries[0].logical_plan_id));
    assert!(
        after.entries[0]
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == ConflictKind::AmbiguousRename),
        "two predecessors are a visible ambiguity, not a coin toss"
    );
    assert!(
        after.entries[0].aliases.is_empty(),
        "no rename alias is invented under ambiguity"
    );
}

#[tokio::test]
async fn malformed_yaml_unknown_and_non_epic_declarations_stay_visible() {
    let repo = Repo::new();
    repo.write(
        "plans/malformed.md",
        "---\nepic: [unterminated\n---\n# Malformed\n",
    );
    repo.write(
        "plans/unknown.md",
        &plan("Unknown workstream", Some("ASMA-9400"), Some("nope"), ""),
    );
    repo.write(
        "plans/nonepic.md",
        &plan("Not an epic", Some("PROJ-1"), Some("kontor"), ""),
    );
    repo.write(
        "plans/bad-id.md",
        &plan(
            "Bad id",
            Some("ASMA-9401"),
            Some("kontor"),
            "catalog_id: BAD ID\n",
        ),
    );
    repo.write(
        "config/orchestration/workstreams.json",
        &registry(&["kontor"]),
    );
    repo.commit("edge documents");
    let fixture = Fixture::open(
        &[(repo.root().to_path_buf(), false)],
        Some(repo.root().join("config/orchestration/workstreams.json")),
        30,
    );
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(entries.entries.len(), 4, "every document stays visible");

    let malformed = entries
        .entries
        .iter()
        .find(|entry| entry.title == "Malformed")
        .expect("malformed front matter does not hide the plan");
    assert!(
        malformed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "frontmatter_invalid")
    );
    assert!(malformed.associations.iter().all(|association| {
        association.relation != kontor_catalog::contract::AssociationRelation::Epic
    }));

    let unknown = entries
        .entries
        .iter()
        .find(|entry| entry.title == "Unknown workstream")
        .expect("unknown workstream stays visible");
    assert!(unknown.associations.iter().any(|association| {
        association.relation == kontor_catalog::contract::AssociationRelation::Workstream
            && association.value == "nope"
            && association.kind == kontor_catalog::contract::AssociationKind::PlanDeclaration
            && !association.verified
    }));
    let status = fixture.catalog.status().unwrap();
    assert_eq!(
        status.registry.workstreams, 1,
        "an unknown key never creates a registry entry"
    );
    assert!(
        status
            .collection
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("unknown workstream `nope`"))
    );

    let nonepic = entries
        .entries
        .iter()
        .find(|entry| entry.title == "Not an epic")
        .expect("a non-ASMA declaration stays visible");
    assert!(nonepic.associations.iter().all(|association| {
        association.relation != kontor_catalog::contract::AssociationRelation::Epic
    }));
    assert!(
        nonepic
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "epic_invalid")
    );

    let bad_id = entries
        .entries
        .iter()
        .find(|entry| entry.title == "Bad id")
        .expect("an invalid explicit id does not hide the plan");
    assert!(
        bad_id
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "explicit_id_invalid")
    );
}

#[tokio::test]
async fn absent_registry_keeps_declarations_and_creates_nothing() {
    let repo = Repo::new();
    repo.write(
        "plans/no-registry.md",
        &plan("No registry", Some("ASMA-9500"), Some("kontor"), ""),
    );
    repo.commit("plan");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    let entry = &entries.entries[0];
    assert!(entry.associations.iter().any(|association| {
        association.relation == kontor_catalog::contract::AssociationRelation::Workstream
            && association.value == "kontor"
            && association.kind == kontor_catalog::contract::AssociationKind::PlanDeclaration
            && !association.verified
    }));
    let status = fixture.catalog.status().unwrap();
    assert_eq!(
        status.registry.state,
        kontor_catalog::contract::RegistryState::Absent
    );
    assert_eq!(status.registry.workstreams, 0);
}

#[tokio::test]
async fn a_removed_explicit_source_leaves_its_association_dangling() {
    let repo = Repo::new();
    let path = repo.write(
        "plans/pinned.md",
        &plan("Pinned source", Some("ASMA-9600"), Some("kontor"), ""),
    );
    repo.commit("pinned");
    let settings = CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root: repo.root().to_path_buf(),
            include_github_plans: false,
        }],
        sources: vec![operator_source("source-dangling", path, None, None)],
        registry_path: None,
        cadence_seconds: 60,
        scan_budget_seconds: 30,
        ..CatalogSettings::default()
    };
    let fixture = Fixture::open_settings(settings);
    sweep(&fixture.catalog).await;
    let before = fixture.catalog.status().unwrap();
    assert_eq!(before.dangling_associations.len(), 0);

    repo.remove("plans/pinned.md");
    repo.commit("removed");
    sweep(&fixture.catalog).await;
    let after = fixture.catalog.status().unwrap();
    assert!(
        after
            .dangling_associations
            .iter()
            .any(|association| association.source_id.as_deref() == Some("source-dangling")),
        "the explicit association survives its source and its plan"
    );
    assert!(after.counts.dangling_associations >= 1);
}

#[tokio::test]
async fn conflicting_explicit_and_declared_workstreams_keep_the_operator_choice() {
    let repo = Repo::new();
    let path = repo.write(
        "plans/conflict.md",
        &plan("Conflicting", Some("ASMA-9100"), Some("kontor"), ""),
    );
    repo.commit("conflict");
    let mut source = operator_source("source-conflict", path, None, None);
    source.workstream = Some("mobile-app".to_owned());
    let settings = CatalogSettings {
        schema_version: 1,
        repositories: vec![],
        sources: vec![source],
        registry_path: None,
        cadence_seconds: 60,
        scan_budget_seconds: 30,
        ..CatalogSettings::default()
    };
    let fixture = Fixture::open_settings(settings);
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    let entry = &entries.entries[0];
    assert!(entry.associations.iter().any(|association| {
        association.kind == kontor_catalog::contract::AssociationKind::ExplicitOperator
            && association.value == "mobile-app"
    }));
    assert!(entry.associations.iter().any(|association| {
        association.kind == kontor_catalog::contract::AssociationKind::PlanDeclaration
            && association.value == "kontor"
    }));
    assert!(
        entry
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == ConflictKind::AssociationConflict)
    );
}
