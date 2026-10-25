//! Read-only live Jira qualification.
//!
//! This test reads the host's configured Kontor credential and the configured
//! ASMA scope. It performs GET requests only, writes a disposable catalog under
//! a temporary state root, and never touches the live daemon, its catalog, or
//! any upstream fact.
//!
//! Run explicitly:
//!   cargo test -p kontor-catalog --test live_jira_read -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontor_catalog::config::JiraSettings;
use kontor_catalog::jira_contract::{EpicIdentitySource, JiraCollectionState};
use kontor_catalog::{Catalog, JiraCatalog};
use kontor_core::id::{RealmId, Timestamp};
use kontor_jira::{JiraConfig, JiraReadConfig, JiraReadLimits, JiraReadOnly, JiraRetryPolicy};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const DECLARED: [&str; 7] = [
    "ASMA-8633",
    "ASMA-8639",
    "ASMA-8637",
    "ASMA-8640",
    "ASMA-8641",
    "ASMA-8645",
    "ASMA-8501",
];
const SCOPE_JQL: &str = "project = ASMA AND issuetype = Epic";

#[tokio::test]
#[ignore = "read-only live Jira qualification; reads the host keychain and configured ASMA scope"]
async fn live_read_only_collection_and_compatibility_export() {
    let state_root = std::env::var("KONTOR_LIVE_STATE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").expect("HOME")).join(".local/state/kontor/asma")
        });
    let realm = live_realm(&state_root).expect("the live realm metadata is readable");
    let (endpoint, credential_alias) =
        live_connection(&state_root).expect("the live jira.json names the ASMA project");
    let catalog_root = TempDir::new().expect("a disposable catalog root");
    let catalog = Arc::new(Catalog::open(catalog_root.path()).expect("the catalog opens"));
    let settings = JiraSettings {
        enabled: true,
        project: "ASMA".to_owned(),
        scope_jql: SCOPE_JQL.to_owned(),
        declared_keys: DECLARED.iter().map(|key| (*key).to_owned()).collect(),
        endpoint: None,
        credential_alias: None,
        cadence_seconds: 120,
        full_sweep_seconds: 900,
        overlap_seconds: 180,
        max_pages: 20,
        max_issues: 5000,
        max_recheck: 500,
        page_size: 100,
        max_attempts: 3,
        scope_is_epics: true,
        parent_rounds: 5,
    };
    let reader = JiraReadOnly::new(
        &state_root,
        realm,
        JiraReadConfig {
            endpoint: endpoint.clone(),
            project_key: "ASMA".to_owned(),
            scope_jql: SCOPE_JQL.to_owned(),
            credential_alias: credential_alias.clone(),
            limits: JiraReadLimits::default(),
            retry: JiraRetryPolicy::default(),
        },
    )
    .expect("the live read-only seam composes");
    let preflight = reader
        .preflight()
        .await
        .expect("the Kontor credential alias resolves and Jira accepts an authenticated read");
    println!(
        "PREFLIGHT base={endpoint} scope={SCOPE_JQL:?} credential_reference={credential_alias} requests={} retries={} status={:?}",
        preflight.stats.requests, preflight.stats.retries, preflight.stats.last_status
    );
    let jira = JiraCatalog::from_parts(Arc::clone(&catalog), reader, settings)
        .expect("the live Jira adapter composes");
    let report = jira
        .collect_once_at(Timestamp::now())
        .await
        .expect("the read-only live sweep runs");
    println!(
        "SWEEP generation={} mode={:?} state={:?} complete={} scope_issues={} observed={} search_pages={} observation_pages={} requests={} retries={} duration_ms={} declared={} found={} missing={:?} out_of_scope={:?} unresolved={:?} retained_unknown={:?} removed={} permission_loss={} search_lag={}",
        report.generation,
        report.mode,
        report.state,
        report.complete,
        report.scope_issues,
        report.observed,
        report.pages,
        report.observation_pages,
        report.requests,
        report.retries,
        report.duration_ms,
        report.coverage.declared,
        report.coverage.found,
        report.coverage.missing,
        report.coverage.out_of_scope,
        report.coverage.unresolved,
        report.coverage.retained_unknown,
        report.coverage.removed,
        report.coverage.permission_loss,
        report.coverage.search_lag_suspected,
    );
    assert_eq!(report.state, JiraCollectionState::Complete, "{report:?}");
    assert_eq!(
        report.coverage.declared,
        DECLARED.len(),
        "the configured declarations are all in scope"
    );
    assert_eq!(
        report.coverage.found,
        DECLARED.len(),
        "every declared key is answered"
    );
    assert!(
        report.coverage.missing.is_empty(),
        "the seven live declarations exist"
    );
    let evidence = jira.evidence().expect("the durable evidence view");
    println!(
        "EVIDENCE as_of={} order={} generation={} realm={} base_url={} scope_issues={} issues={} conflicts={} dangling={} collection_state={:?} latest_error={:?}",
        evidence.as_of,
        evidence.order,
        evidence.generation,
        evidence.source.realm,
        evidence.source.base_url,
        evidence.coverage.scope_issues,
        evidence.issues.len(),
        evidence.conflicts.len(),
        evidence.dangling_associations.len(),
        evidence.collection_state,
        evidence.latest_error,
    );
    let export = jira
        .export_epic_identity()
        .expect("the frozen compatibility view");
    let bytes = serde_json::to_vec(&export).expect("the export serializes");
    let digest = hex::encode(Sha256::digest(&bytes));
    println!(
        "EXPORT schema={} order={} as_of={} declared={} issues={} conflicts={} sha256={} keys={:?}",
        export.schema,
        export.order,
        export.as_of,
        export.scope.declared.len(),
        export.issues.len(),
        export.conflicts.len(),
        digest,
        DECLARED,
    );
    for key in DECLARED {
        let found = export
            .issues
            .iter()
            .find(|issue| issue.key == key)
            .map(|issue| {
                (
                    issue.key.as_str(),
                    issue.issue_type.as_str(),
                    issue.level,
                    issue.parent.as_deref(),
                )
            })
            .or_else(|| {
                export
                    .conflicts
                    .iter()
                    .find(|conflict| conflict.key == key)
                    .map(|conflict| (conflict.current.as_str(), "moved", 0, None))
            });
        println!("DECLARED {key} -> {found:?}");
    }
    assert_eq!(export.schema, "kontor/epic-identity@1");
    assert!(export.issues.len() >= DECLARED.len());
    assert!(
        export
            .issues
            .iter()
            .any(|issue| issue.key == "ASMA-8501" && issue.level == 1),
        "the live scope includes the known epic"
    );
}

fn live_realm(state_root: &Path) -> Option<RealmId> {
    let connection = open_read_only(&state_root.join("kontor.db"))?;
    let realm: String = connection
        .query_row("SELECT realm_id FROM realm_metadata", [], |row| row.get(0))
        .ok()?;
    RealmId::parse(&realm).ok()
}

fn live_connection(state_root: &Path) -> Option<(String, String)> {
    let bytes = std::fs::read(state_root.join("jira.json")).ok()?;
    let config: JiraConfig = serde_json::from_slice(&bytes).ok()?;
    let project = config
        .projects
        .iter()
        .find(|project| project.project_key.as_str() == "ASMA")?;
    Some((project.endpoint.clone(), project.credential_alias.clone()))
}

fn open_read_only(path: &Path) -> Option<Connection> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .ok()?;
    Some(connection)
}

#[allow(dead_code)]
fn _schema_is_reexported(source: &EpicIdentitySource) -> &'static str {
    let _ = source;
    kontor_catalog::EPIC_IDENTITY_SCHEMA
}
