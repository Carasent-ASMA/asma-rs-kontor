//! TASK-002's durable Jira collection: declared verification, Jira-only epic
//! discovery, actual type/parent preservation, the compatibility projection and
//! the unchanged base association authority.

mod jira_support;
mod support;

use std::collections::BTreeSet;

use jira_support::{JiraFixture, epic, issue, story};
use kontor_catalog::contract::AssociationRelation;
use kontor_catalog::jira_contract::verified_workstream_join;
use kontor_catalog::{Catalog, CatalogSettings, EntryFilter, RepositoryConfig};
use serde_json::Value;
use sha2::{Digest, Sha256};
use support::{Repo, plan};

#[tokio::test]
async fn jira_only_epics_and_declared_relationships_form_the_compatible_export() {
    let fixture = JiraFixture::new(&["ASMA-101", "ASMA-140", "ASMA-404", "EX-1"]).await;
    fixture
        .mount_scope_page(
            false,
            0,
            None,
            vec![
                epic("100", "ASMA-100", &["ws:kontor"]),
                epic("200", "ASMA-200", &[]),
            ],
            true,
        )
        .await;
    fixture
        .mount_issue(story("101", "ASMA-101", "ASMA-100", &["ws:kontor"]))
        .await;
    // The declared key ASMA-140 has moved to ASMA-141.
    fixture
        .mount_issue_at("ASMA-140", story("141", "ASMA-141", "ASMA-100", &[]))
        .await;
    fixture.mount_missing("ASMA-404").await;
    fixture.mount_catch_all_missing().await;

    let report = fixture.collect(fixture.now(0)).await;
    assert!(report.complete, "{report:?}");
    assert_eq!(report.state, kontor_catalog::JiraCollectionState::Complete);
    assert_eq!(report.scope_issues, 2);
    assert_eq!(report.observed, 4, "{report:?}");
    assert_eq!(report.coverage.declared, 3);
    assert_eq!(report.coverage.found, 2);
    assert_eq!(report.coverage.missing, vec!["ASMA-404"]);
    assert_eq!(report.coverage.out_of_scope, vec!["EX-1"]);
    assert!(report.coverage.unresolved.is_empty());
    assert_eq!(report.coverage.removed, 0);

    let evidence = fixture.evidence();
    assert_eq!(evidence.generation, 1);
    assert!(evidence.order > 0);
    assert_eq!(evidence.source.realm, "127.0.0.1");
    assert_eq!(evidence.scope.project, "ASMA");
    assert_eq!(
        evidence.scope.declared,
        vec!["ASMA-101", "ASMA-140", "ASMA-404"]
    );
    let jira_only = evidence
        .issues
        .iter()
        .find(|issue| issue.key == "ASMA-200")
        .expect("the Jira-only epic is discovered without catalog membership");
    assert!(jira_only.is_actual_epic());
    assert!(jira_only.labels.is_empty());
    let story = evidence
        .issues
        .iter()
        .find(|issue| issue.key == "ASMA-101")
        .expect("the declared story is directly verified");
    assert_eq!(story.issue_type, "User Story");
    assert_eq!(story.level, 0);
    assert_eq!(story.parent.as_deref(), Some("ASMA-100"));
    assert_eq!(story.via, kontor_catalog::JiraObservationVia::Declared);
    assert!(
        evidence
            .aliases
            .iter()
            .any(|alias| { alias.old_key == "ASMA-140" && alias.current_key == "ASMA-141" })
    );

    let export = fixture.export();
    assert_eq!(export.schema, "kontor/epic-identity@1");
    assert_eq!(
        export.scope.declared,
        vec!["ASMA-101", "ASMA-140", "ASMA-404"]
    );
    assert_eq!(
        export
            .epic("ASMA-101")
            .unwrap()
            .map(|epic| epic.key.as_str()),
        Some("ASMA-100")
    );
    assert_eq!(
        export
            .epic("ASMA-140")
            .unwrap()
            .map(|epic| epic.key.as_str()),
        Some("ASMA-100")
    );
    let conflict = export
        .conflicts
        .iter()
        .find(|conflict| conflict.key == "ASMA-140")
        .expect("the moved key is a conflict");
    assert_eq!(conflict.current, "ASMA-141");
    assert!(export.issues.iter().any(|issue| issue.key == "ASMA-200"));
    assert_schema_compatible(&serde_json::to_value(&export).expect("the export serializes"));
}

#[tokio::test]
async fn an_absent_jira_section_starts_no_collector() {
    let state_root = tempfile::TempDir::new().expect("a state root");
    let catalog = std::sync::Arc::new(Catalog::open(state_root.path()).expect("the catalog opens"));
    let jira = kontor_catalog::JiraCatalog::open(
        catalog,
        kontor_core::id::RealmId::generate(),
        state_root.path(),
    )
    .expect("an absent section is not a failure");
    assert!(jira.is_none());
}

#[tokio::test]
async fn the_base_association_authority_never_becomes_verified() {
    let fixture = JiraFixture::new(&[]).await;
    let repo = Repo::new();
    repo.write(
        "plans/observed.md",
        &plan("Observed", Some("ASMA-100"), Some("kontor"), ""),
    );
    repo.write(
        "plans/missing.md",
        &plan("Missing", Some("ASMA-9999"), Some("kontor"), ""),
    );
    repo.commit("plans");
    let settings = CatalogSettings {
        schema_version: 1,
        repositories: vec![RepositoryConfig {
            root: repo.root().to_path_buf(),
            include_github_plans: false,
        }],
        ..CatalogSettings::default()
    };
    std::fs::write(
        Catalog::settings_path(fixture.state_root.path()),
        serde_json::to_vec(&settings).expect("settings"),
    )
    .expect("settings written");
    fixture
        .catalog
        .collect_once()
        .await
        .expect("the local sweep completes");

    fixture
        .mount_scope_page(
            false,
            0,
            None,
            vec![epic("100", "ASMA-100", &["ws:kontor"])],
            true,
        )
        .await;
    fixture.mount_catch_all_missing().await;
    fixture.collect(fixture.now(0)).await;

    let entries = fixture
        .catalog
        .entries(&EntryFilter::default())
        .expect("the entries read");
    let associations: Vec<_> = entries
        .entries
        .iter()
        .flat_map(|entry| entry.associations.iter())
        .collect();
    assert!(
        associations.iter().any(
            |association| association.relation == AssociationRelation::Epic
                && association.value == "ASMA-100"
        ),
        "the plan declaration is present: {associations:?}"
    );
    assert!(
        associations.iter().all(|association| !association.verified),
        "the frozen base contract never flips verified"
    );

    let evidence = fixture.evidence();
    assert!(
        evidence
            .dangling_associations
            .iter()
            .any(|association| association.epic.as_deref() == Some("ASMA-9999")),
        "an explicit association to an unobserved epic stays visible"
    );
    let epic_observation = evidence
        .issues
        .iter()
        .find(|issue| issue.key == "ASMA-100")
        .expect("the epic is observed");
    let join = verified_workstream_join(epic_observation, &["kontor".to_owned()])
        .expect("an actual Epic with one declared ws: label verifies");
    assert!(join.verified);
    assert_eq!(join.workstream, "kontor");
    assert_ne!(
        join.verified,
        associations
            .iter()
            .find(|association| association.value == "ASMA-100")
            .map(|association| association.verified)
            .unwrap_or(true),
        "the separate evidence type does not rewrite the base association"
    );
}

#[tokio::test]
async fn a_story_with_a_ws_label_never_verifies_a_workstream_join() {
    let fixture = JiraFixture::new(&["ASMA-101"]).await;
    fixture
        .mount_scope_page(false, 0, None, Vec::new(), true)
        .await;
    fixture
        .mount_issue(issue(
            "101",
            "ASMA-101",
            "Story",
            0,
            Some("ASMA-100"),
            &["ws:kontor"],
            "2026-10-25T10:00:00.000+0100",
            "Synthetic story",
        ))
        .await;
    fixture.mount_issue(epic("100", "ASMA-100", &[])).await;
    fixture.mount_catch_all_missing().await;
    let report = fixture.collect(fixture.now(0)).await;
    assert!(report.complete, "{report:?}");
    let evidence = fixture.evidence();
    let story = evidence
        .issues
        .iter()
        .find(|issue| issue.key == "ASMA-101")
        .expect("the story");
    assert_eq!(story.issue_type, "Story");
    assert!(matches!(
        verified_workstream_join(story, &["kontor".to_owned()]),
        Err(kontor_catalog::JiraJoinDiagnostic::NotEpic { .. })
    ));
}

fn assert_schema_compatible(value: &Value) {
    let bytes = include_bytes!("fixtures/epic-identity-v1.schema.json");
    assert_eq!(
        hex::encode(Sha256::digest(bytes)),
        "913055c2343608afed3e88a014434bf2237daa46d3c94f67d00af6b51741f9c7",
        "the frozen schema fixture is exact"
    );
    let schema: Value = serde_json::from_slice(bytes).expect("the schema parses");
    assert_eq!(
        keys(value),
        keys(&schema["properties"]),
        "the top-level shape matches the frozen schema"
    );
    for block in ["source", "scope", "provenance", "coverage"] {
        assert_eq!(
            keys(&value[block]),
            keys(&schema["properties"][block]["properties"]),
            "the {block} shape matches"
        );
    }
    assert_eq!(
        keys(&value["provenance"]["sessions"]),
        keys(&schema["properties"]["provenance"]["properties"]["sessions"]["properties"]),
        "the sessions shape matches"
    );
    assert_eq!(
        keys(&value["issues"][0]),
        keys(&schema["properties"]["issues"]["items"]["properties"]),
        "the issue shape matches"
    );
    assert_eq!(
        keys(&value["conflicts"][0]),
        keys(&schema["properties"]["conflicts"]["items"]["properties"]),
        "the conflict shape matches"
    );
    assert_eq!(value["schema"], "kontor/epic-identity@1");
    assert_eq!(value["source"]["kind"], "jira-rest");
    assert!(value["order"].as_u64().is_some_and(|order| order > 0));
    assert_eq!(
        value["scope"]["project"],
        schema["properties"]["scope"]["properties"]["project"]["pattern"]
            .as_str()
            .map(|_| "ASMA")
            .unwrap_or("ASMA")
    );
    for issue in value["issues"].as_array().expect("issues") {
        assert!(valid_key(issue["key"].as_str().expect("a key")));
        assert!(
            issue["level"]
                .as_i64()
                .is_some_and(|level| (-1..=9).contains(&level))
        );
        assert!(matches!(
            issue["cat"].as_str(),
            Some("new" | "indeterminate" | "done")
        ));
        assert!(matches!(
            issue["via"].as_str(),
            Some("scope" | "declared" | "parent")
        ));
        assert!(
            issue["labels"]
                .as_array()
                .is_some_and(|labels| labels.len() <= 100)
        );
    }
    for conflict in value["conflicts"].as_array().expect("conflicts") {
        assert_eq!(conflict["kind"], "moved_key");
        assert!(valid_key(
            conflict["current"].as_str().expect("a current key")
        ));
    }
    for key in value["scope"]["declared"].as_array().expect("declared") {
        assert!(valid_key(key.as_str().expect("a key")));
    }
}

fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect()
}

fn valid_key(key: &str) -> bool {
    let Some((project, number)) = key.rsplit_once('-') else {
        return false;
    };
    !project.is_empty()
        && project.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_uppercase()
            } else {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
            }
        })
        && !number.is_empty()
        && !number.starts_with('0')
        && number.bytes().all(|byte| byte.is_ascii_digit())
}
