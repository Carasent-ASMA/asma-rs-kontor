#![allow(dead_code)]
#![allow(unreachable_pub)]

//! A controlled Jira fixture for the catalog collector suites.
//!
//! Every request is a real GET against a loopback mock. The keychain is an
//! in-memory synthetic document, so the tests exercise the same credential
//! resolution and transport path as production without a production Jira.

use std::sync::Arc;
use std::time::Duration;

use kontor_accounts::{KeychainBackend, KeychainFailure, KeychainTarget};
use kontor_catalog::config::JiraSettings;
use kontor_catalog::jira_contract::EpicIdentitySource;
use kontor_catalog::{Catalog, JiraCatalog, JiraSweepReport};
use kontor_core::id::{RealmId, Timestamp};
use kontor_jira::{JiraReadConfig, JiraReadLimits, JiraReadOnly, JiraRetryPolicy};
use secrecy::SecretString;
use serde_json::{Value, json};
use tempfile::TempDir;
use wiremock::matchers::{method, path, path_regex, query_param, query_param_is_missing};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

pub struct FixtureKeychain;

impl KeychainBackend for FixtureKeychain {
    fn secret(&self, _target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
        Ok(SecretString::from(
            r#"{"email":"operator@example.test","api_token":"synthetic-canary"}"#,
        ))
    }
}

pub struct JiraFixture {
    pub server: MockServer,
    pub catalog: Arc<Catalog>,
    pub jira: JiraCatalog,
    pub settings: JiraSettings,
    pub state_root: TempDir,
    _credential_root: TempDir,
}

impl JiraFixture {
    pub async fn new(declared: &[&str]) -> Self {
        Self::with_bounds(declared, 1, 500).await
    }

    pub async fn with_bounds(declared: &[&str], max_attempts: u32, max_recheck: usize) -> Self {
        let server = MockServer::start().await;
        let state_root = TempDir::new().expect("a catalog state root");
        let catalog = Arc::new(Catalog::open(state_root.path()).expect("the catalog opens"));
        let settings = jira_settings(declared, max_attempts, max_recheck);
        let credential_root = TempDir::new().expect("a credential root");
        let reader = JiraReadOnly::new_with_keychain(
            credential_root.path(),
            RealmId::generate(),
            JiraReadConfig {
                endpoint: server.uri(),
                project_key: settings.project.clone(),
                scope_jql: settings.scope_jql.clone(),
                credential_alias: "work".to_owned(),
                limits: JiraReadLimits {
                    max_response_bytes: 1024 * 1024,
                    connect_timeout: Duration::from_millis(200),
                    request_timeout: Duration::from_secs(2),
                    max_issues: 100,
                    max_pages: 10,
                    page_size: 2,
                },
                retry: JiraRetryPolicy {
                    max_attempts,
                    base_delay: Duration::from_millis(1),
                    max_delay: Duration::from_millis(5),
                    max_retry_after: Duration::from_secs(1),
                },
            },
            Arc::new(FixtureKeychain),
        )
        .expect("the read-only seam composes");
        let jira = JiraCatalog::from_parts(catalog.clone(), reader, settings.clone())
            .expect("the Jira adapter composes");
        Self {
            server,
            catalog,
            jira,
            settings,
            state_root,
            _credential_root: credential_root,
        }
    }

    pub fn now(&self, offset_seconds: i64) -> Timestamp {
        let base: Timestamp = "2026-10-25T00:00:00Z".parse().expect("a fixed instant");
        base + kontor_core::id::SignedDuration::from_secs(offset_seconds)
    }

    pub async fn collect(&self, now: Timestamp) -> JiraSweepReport {
        self.jira
            .collect_once_at(now)
            .await
            .expect("the Jira sweep runs")
    }

    pub fn evidence(&self) -> kontor_catalog::JiraEvidenceDocument {
        self.jira.evidence().expect("the evidence view")
    }

    pub fn export(&self) -> EpicIdentitySource {
        self.jira
            .export_epic_identity()
            .expect("the compatible export")
    }

    pub async fn reset(&self) {
        self.server.reset().await;
    }

    pub async fn mount_scope_page(
        &self,
        incremental: bool,
        page_index: usize,
        next_token: Option<&str>,
        issues: Vec<Value>,
        is_last: bool,
    ) {
        let mut mock = Mock::given(method("GET"))
            .and(path("/rest/api/3/search/jql"))
            .and(JqlMode { incremental });
        mock = match next_token {
            Some(token) => mock.and(query_param("nextPageToken", token)),
            None => mock.and(query_param_is_missing("nextPageToken")),
        };
        let page = if let Some(token) = next_token {
            json!({"issues": issues, "nextPageToken": token, "isLast": is_last})
        } else {
            json!({"issues": issues, "isLast": is_last})
        };
        mock.respond_with(ResponseTemplate::new(200).set_body_json(page))
            .with_priority(2)
            .mount(&self.server)
            .await;
        let _ = page_index;
    }

    pub async fn mount_scope_failure(&self, incremental: bool, status: u16) {
        Mock::given(method("GET"))
            .and(path("/rest/api/3/search/jql"))
            .and(JqlMode { incremental })
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                "errorMessages": ["synthetic"]
            })))
            .with_priority(2)
            .mount(&self.server)
            .await;
    }

    pub async fn mount_issue(&self, issue: Value) {
        let key = issue["key"].as_str().expect("a key").to_owned();
        self.mount_issue_at(&key, issue).await;
    }

    pub async fn mount_issue_at(&self, requested_key: &str, issue: Value) {
        Mock::given(method("GET"))
            .and(path(format!("/rest/api/3/issue/{requested_key}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(issue))
            .with_priority(1)
            .mount(&self.server)
            .await;
    }

    pub async fn mount_missing(&self, key: &str) {
        Mock::given(method("GET"))
            .and(path(format!("/rest/api/3/issue/{key}")))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"errorMessages": []})))
            .with_priority(1)
            .mount(&self.server)
            .await;
    }

    pub async fn mount_issue_failure(&self, key: &str, status: u16) {
        Mock::given(method("GET"))
            .and(path(format!("/rest/api/3/issue/{key}")))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                "errorMessages": ["synthetic"]
            })))
            .with_priority(1)
            .mount(&self.server)
            .await;
    }

    pub async fn mount_catch_all_missing(&self) {
        Mock::given(method("GET"))
            .and(path_regex(r"^/rest/api/3/issue/"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"errorMessages": []})))
            .with_priority(20)
            .mount(&self.server)
            .await;
    }
}

fn jira_settings(declared: &[&str], max_attempts: u32, max_recheck: usize) -> JiraSettings {
    JiraSettings {
        enabled: true,
        project: "ASMA".to_owned(),
        scope_jql: "project = ASMA AND issuetype = Epic".to_owned(),
        declared_keys: declared.iter().map(|key| (*key).to_owned()).collect(),
        endpoint: None,
        credential_alias: None,
        cadence_seconds: 120,
        full_sweep_seconds: 900,
        overlap_seconds: 180,
        max_pages: 10,
        max_issues: 100,
        max_recheck,
        page_size: 2,
        max_attempts,
        scope_is_epics: true,
        parent_rounds: 5,
    }
}

struct JqlMode {
    incremental: bool,
}

impl Match for JqlMode {
    fn matches(&self, request: &Request) -> bool {
        let jql_text = request
            .url
            .query_pairs()
            .find(|(key, _)| key == "jql")
            .map(|(_, value)| value.into_owned())
            .unwrap_or_default();
        jql_text.contains("updated >=") == self.incremental
    }
}

#[allow(clippy::too_many_arguments)]
pub fn issue(
    id: &str,
    key: &str,
    kind: &str,
    level: i64,
    parent: Option<&str>,
    labels: &[&str],
    updated: &str,
    summary: &str,
) -> Value {
    let mut fields = json!({
        "issuetype": {"name": kind, "hierarchyLevel": level},
        "summary": summary,
        "labels": labels,
        "status": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}},
        "updated": updated
    });
    if let Some(parent) = parent {
        fields["parent"] =
            json!({"key": parent, "id": parent.rsplit_once('-').map(|(_, n)| n).unwrap_or("1")});
    }
    json!({"id": id, "key": key, "fields": fields})
}

pub fn epic(id: &str, key: &str, labels: &[&str]) -> Value {
    issue(
        id,
        key,
        "Epic",
        1,
        None,
        labels,
        "2026-10-25T10:00:00.000+0100",
        &format!("Synthetic epic {key}"),
    )
}

pub fn story(id: &str, key: &str, parent: &str, labels: &[&str]) -> Value {
    issue(
        id,
        key,
        "User Story",
        0,
        Some(parent),
        labels,
        "2026-10-25T10:01:00.000+0100",
        &format!("Synthetic story {key}"),
    )
}
