//! Native Jira transport and configuration contract.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use kontor_accounts::{KeychainBackend, KeychainFailure, KeychainTarget};
use kontor_core::id::{ExternalId, IdempotencyKey, ProjectId, SCHEMA_VERSION};
use kontor_core::ticket::OwnershipAction;
use kontor_jira::jira::{JiraExchange, JiraOperation, JiraOutcome, JiraRequest};
use kontor_jira::{JiraConnectors, JiraIssueKind, JiraIssuePlan};
use secrecy::SecretString;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

#[derive(Default)]
struct FixtureKeychain {
    reads: AtomicUsize,
}

impl KeychainBackend for FixtureKeychain {
    fn secret(&self, target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
        assert_eq!(target.service(), "kontor-jira");
        assert_eq!(target.account(), "work");
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(SecretString::from(
            r#"{"email":"operator@example.test","api_token":"secret"}"#.to_owned(),
        ))
    }
}

struct MarkerSearch(Arc<AtomicUsize>);

impl Respond for MarkerSearch {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let issues = if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            serde_json::json!([])
        } else {
            serde_json::json!([{"key": "ASMA-1"}])
        };
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"issues": issues}))
    }
}

struct ExistingLinkedTask(Arc<AtomicUsize>);

impl Respond for ExistingLinkedTask {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let parent = if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            "ASMA-8049"
        } else {
            "ASMA-9999"
        };
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "key": "ASMA-8050",
            "fields": {
                "project": {"key": "ASMA"},
                "issuetype": {"name": "Task", "hierarchyLevel": 0},
                "parent": {"key": parent},
                "summary": "Existing operator-owned Jira summary",
                "description": {"type":"doc","version":1,"content":[{
                    "type":"paragraph","content":[{"type":"text","text":"Existing Jira description"}]
                }]},
                "labels": []
            }
        }))
    }
}

#[tokio::test]
async fn create_is_marker_idempotent_and_credentials_are_resolved_per_request() {
    let server = MockServer::start().await;
    let searches = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(MarkerSearch(Arc::clone(&searches)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/createmeta/ASMA/issuetypes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issueTypes": [{"id": "10000", "hierarchyLevel": 1}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(serde_json::json!({"key": "ASMA-1"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "key": "ASMA-1",
            "fields": {
                "project": {"key": "ASMA"},
                "issuetype": {"name": "Epic", "hierarchyLevel": 1},
                "parent": null,
                "summary": "Operational MVP",
                "description": {"type":"doc","version":1,"content":[{
                    "type":"paragraph","content":[{"type":"text","text":"Server derived"}]
                }]},
                "labels": ["kontor-epic-fixture"]
            }
        })))
        .mount(&server)
        .await;

    let root = tempfile::tempdir().expect("a state root");
    let project_id = ProjectId::generate();
    std::fs::write(
        root.path().join("jira.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "projects": [{
                "project_id": project_id.to_string(),
                "endpoint": server.uri(),
                "project_key": "ASMA",
                "credential_alias": "work"
            }]
        }))
        .expect("configuration serializes"),
    )
    .expect("configuration is written");
    let keychain = Arc::new(FixtureKeychain::default());
    let connectors = JiraConnectors::read_with_keychain(
        root.path(),
        Arc::clone(&keychain) as Arc<dyn KeychainBackend>,
    )
    .expect("strict configuration loads");
    let connector = connectors
        .for_project(project_id)
        .expect("project is configured");
    let plan = JiraIssuePlan {
        kind: JiraIssueKind::Epic,
        requested_key: None,
        marker: ExternalId::parse("kontor-epic-fixture").expect("marker"),
        summary: "Operational MVP".to_owned(),
        description: "Server derived".to_owned(),
        parent_key: None,
    };

    let first = connector
        .materialize(&plan)
        .await
        .expect("created and confirmed");
    let replay = connector
        .materialize(&plan)
        .await
        .expect("found and confirmed");
    assert_eq!(first, replay);
    assert_eq!(searches.load(Ordering::SeqCst), 2);
    assert_eq!(keychain.reads.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn explicit_link_confirms_existing_identity_without_claiming_summary_or_description() {
    let server = MockServer::start().await;
    let readbacks = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-8050"))
        .respond_with(ExistingLinkedTask(Arc::clone(&readbacks)))
        .expect(2)
        .mount(&server)
        .await;

    let root = tempfile::tempdir().expect("a state root");
    let project_id = ProjectId::generate();
    std::fs::write(
        root.path().join("jira.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "projects": [{
                "project_id": project_id.to_string(),
                "endpoint": server.uri(),
                "project_key": "ASMA",
                "credential_alias": "work"
            }]
        }))
        .expect("configuration serializes"),
    )
    .expect("configuration is written");
    let connector =
        JiraConnectors::read_with_keychain(root.path(), Arc::new(FixtureKeychain::default()))
            .expect("configuration loads");
    let plan = JiraIssuePlan {
        kind: JiraIssueKind::Task,
        requested_key: Some(ExternalId::parse("ASMA-8050").expect("issue key")),
        marker: ExternalId::parse("kontor-task-link-fixture").expect("marker"),
        summary: "Kontor-derived summary is not linked-field authority".to_owned(),
        description: "Kontor-derived description is not linked-field authority".to_owned(),
        parent_key: Some(ExternalId::parse("ASMA-8049").expect("parent key")),
    };

    let confirmed = connector
        .for_project(project_id)
        .expect("project is configured")
        .materialize(&plan)
        .await
        .expect("the exact existing Jira identity is confirmed");
    assert_eq!(confirmed.issue_key.as_str(), "ASMA-8050");
    assert!(
        connector
            .for_project(project_id)
            .expect("project remains configured")
            .materialize(&plan)
            .await
            .is_err(),
        "an explicit link still refuses a task attached to another epic"
    );
}

#[tokio::test]
async fn native_observe_reads_issue_transitions_and_principal() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "fields": {
                "project": {"key": "ASMA"},
                "status": {"id": "3", "name": "In Progress", "statusCategory": {"name": "In Progress"}},
                "issuetype": {"name": "Task"},
                "assignee": {"accountId": "acct-1", "displayName": "Operator"},
                "updated": "2026-08-23T10:00:00.000+0000"
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-9/transitions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "transitions": [{"id":"31","to":{"id":"10000","name":"Done","statusCategory":{"name":"Done"}}}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "accountId": "acct-1"
        })))
        .mount(&server)
        .await;

    let root = tempfile::tempdir().expect("a state root");
    let project_id = ProjectId::generate();
    std::fs::write(
        root.path().join("jira.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "projects": [{
                "project_id": project_id.to_string(),
                "endpoint": server.uri(),
                "project_key": "ASMA",
                "credential_alias": "work"
            }]
        }))
        .expect("configuration serializes"),
    )
    .expect("configuration is written");
    let connector =
        JiraConnectors::read_with_keychain(root.path(), Arc::new(FixtureKeychain::default()))
            .expect("configuration loads");
    let response = connector
        .for_project(project_id)
        .expect("project is configured")
        .execute(
            "observe",
            &JiraRequest {
                schema_version: SCHEMA_VERSION,
                operation: JiraOperation::Observe,
                issue_key: ExternalId::parse("ASMA-9").expect("issue key"),
                idempotency_key: IdempotencyKey::parse("native-observe").expect("key"),
                intent_hash: None,
                field_spec_hash: None,
                workflow_spec_hash: None,
                expected: None,
                field_writes: Vec::new(),
                destination: None,
                ownership_action: OwnershipAction::Preserve,
                transition: None,
                authorized_apply: false,
            },
        )
        .await
        .expect("native observation succeeds");
    assert_eq!(response.outcome, JiraOutcome::Observed);
    assert_eq!(
        response
            .observation
            .expect("observation")
            .status_id
            .as_str(),
        "3"
    );
    assert_eq!(response.live_transitions[0].transition_id.as_str(), "31");
    assert_eq!(
        response
            .principal_account_id
            .as_ref()
            .map(ExternalId::as_str),
        Some("acct-1")
    );
}

#[tokio::test]
async fn native_requests_time_out_without_holding_the_daemon_open() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-9"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(31))
                .set_body_json(serde_json::json!({"fields": {}})),
        )
        .mount(&server)
        .await;

    let root = tempfile::tempdir().expect("a state root");
    let project_id = ProjectId::generate();
    std::fs::write(
        root.path().join("jira.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "projects": [{
                "project_id": project_id.to_string(),
                "endpoint": server.uri(),
                "project_key": "ASMA",
                "credential_alias": "work"
            }]
        }))
        .expect("configuration serializes"),
    )
    .expect("configuration is written");
    let connector =
        JiraConnectors::read_with_keychain(root.path(), Arc::new(FixtureKeychain::default()))
            .expect("configuration loads");
    let request = JiraRequest {
        schema_version: SCHEMA_VERSION,
        operation: JiraOperation::Observe,
        issue_key: ExternalId::parse("ASMA-9").expect("issue key"),
        idempotency_key: IdempotencyKey::parse("timeout-observe").expect("key"),
        intent_hash: None,
        field_spec_hash: None,
        workflow_spec_hash: None,
        expected: None,
        field_writes: Vec::new(),
        destination: None,
        ownership_action: OwnershipAction::Preserve,
        transition: None,
        authorized_apply: false,
    };

    let result = tokio::time::timeout(
        Duration::from_secs(31),
        connector
            .for_project(project_id)
            .expect("project is configured")
            .execute("observe", &request),
    )
    .await
    .expect("the connector owns the request timeout");
    assert!(result.is_err());
}

#[test]
fn strict_configuration_rejects_inline_credentials() {
    let root = tempfile::tempdir().expect("a state root");
    std::fs::write(
        root.path().join("jira.json"),
        r#"{"schema_version":1,"projects":[{"project_id":"0198fb22-056d-7de0-a8b2-777e719c83fd","endpoint":"https://user:secret@example.test","project_key":"ASMA","credential_alias":"work"}]}"#,
    )
    .expect("configuration is written");
    let error =
        JiraConnectors::read_with_keychain(root.path(), Arc::new(FixtureKeychain::default()))
            .expect_err("inline credentials are refused");
    assert!(!error.to_string().contains("secret"));
}
