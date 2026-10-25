//! The read-only seam's own TEST-003 half: pagination, retries, bounds,
//! redirects and credential absence. These tests use a controlled HTTP fixture
//! and never a production Jira.

use std::sync::Arc;
use std::time::Duration;

use kontor_accounts::{KeychainBackend, KeychainFailure, KeychainTarget};
use kontor_core::id::RealmId;
use kontor_jira::{JiraReadConfig, JiraReadFailure, JiraReadLimits, JiraReadOnly, JiraRetryPolicy};
use secrecy::SecretString;
use serde_json::{Value, json};
use tempfile::TempDir;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct FixtureKeychain {
    secret: Result<SecretString, KeychainFailure>,
}

impl KeychainBackend for FixtureKeychain {
    fn secret(&self, _target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
        self.secret.clone()
    }
}

fn credential() -> SecretString {
    SecretString::from(r#"{"email":"operator@example.test","api_token":"synthetic-canary"}"#)
}

fn compose_reader(
    server: &MockServer,
    limits: JiraReadLimits,
    retry: JiraRetryPolicy,
    keychain: Arc<dyn KeychainBackend>,
) -> (JiraReadOnly, TempDir) {
    let root = TempDir::new().expect("a credential root");
    let config = JiraReadConfig {
        endpoint: server.uri(),
        project_key: "ASMA".to_owned(),
        scope_jql: "project = ASMA AND issuetype = Epic".to_owned(),
        credential_alias: "work".to_owned(),
        limits,
        retry,
    };
    let reader =
        JiraReadOnly::new_with_keychain(root.path(), RealmId::generate(), config, keychain)
            .expect("the read-only seam composes");
    (reader, root)
}

fn fast() -> (JiraReadLimits, JiraRetryPolicy) {
    (
        JiraReadLimits {
            max_response_bytes: 1024 * 1024,
            connect_timeout: Duration::from_millis(200),
            request_timeout: Duration::from_millis(500),
            max_issues: 100,
            max_pages: 10,
            page_size: 2,
        },
        JiraRetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            max_retry_after: Duration::from_secs(1),
        },
    )
}

fn issue(id: &str, key: &str, level: i64, labels: &[&str]) -> Value {
    json!({
        "id": id,
        "key": key,
        "fields": {
            "issuetype": {"name": if level == 1 { "Epic" } else { "Story" }, "hierarchyLevel": level},
            "summary": format!("Synthetic {key}"),
            "labels": labels,
            "status": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}},
            "updated": "2026-10-25T10:00:00.000+0100"
        }
    })
}

#[tokio::test]
async fn pagination_walks_every_enhanced_search_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param_is_missing("nextPageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issues": [issue("101", "ASMA-101", 1, &[])],
            "nextPageToken": "p2",
            "isLast": false
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("nextPageToken", "p2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issues": [issue("102", "ASMA-102", 1, &["ws:kontor"])],
            "isLast": true
        })))
        .mount(&server)
        .await;
    let (limits, retry) = fast();
    let (reader, _root) = compose_reader(
        &server,
        limits,
        retry,
        Arc::new(FixtureKeychain {
            secret: Ok(credential()),
        }),
    );
    let search = reader
        .search_all("project = ASMA AND issuetype = Epic")
        .await
        .expect("the search completes");
    assert_eq!(search.pages, 2);
    assert_eq!(search.stats.pages, 2);
    assert_eq!(search.stats.requests, 2);
    assert_eq!(
        search
            .issues
            .iter()
            .map(|issue| issue.key.as_str())
            .collect::<Vec<_>>(),
        vec!["ASMA-101", "ASMA-102"]
    );
    assert_eq!(search.issues[0].updated, "2026-10-25T09:00:00Z");
}

#[tokio::test]
async fn retry_after_is_honoured_and_exhaustion_exposes_the_backoff() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_json(json!({"errorMessages": ["rate limited"]})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"accountId": "synthetic"})))
        .mount(&server)
        .await;
    let (limits, mut retry) = fast();
    retry.max_attempts = 3;
    let (reader, _root) = compose_reader(
        &server,
        limits,
        retry,
        Arc::new(FixtureKeychain {
            secret: Ok(credential()),
        }),
    );
    let preflight = reader.preflight().await.expect("the retry succeeds");
    assert_eq!(preflight.stats.requests, 2);
    assert_eq!(preflight.stats.retries, 1);
    assert_eq!(preflight.stats.rate_limited, 1);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_json(json!({"errorMessages": ["rate limited"]})),
        )
        .mount(&server)
        .await;
    let (limits, mut retry) = fast();
    retry.max_attempts = 2;
    let (reader, _root) = compose_reader(
        &server,
        limits,
        retry,
        Arc::new(FixtureKeychain {
            secret: Ok(credential()),
        }),
    );
    let error = reader
        .preflight()
        .await
        .expect_err("the retry bound is spent");
    assert_eq!(error.failure, JiraReadFailure::RateLimited);
    assert_eq!(error.stats.requests, 2);
    assert_eq!(error.stats.retries, 1);
    assert!(error.backoff_until.is_some());
}

#[tokio::test]
async fn a_401_and_a_403_are_permission_not_absence() {
    for status in [401, 403] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/ASMA-1"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"errorMessages": []})))
            .mount(&server)
            .await;
        let (limits, retry) = fast();
        let (reader, _root) = compose_reader(
            &server,
            limits,
            retry,
            Arc::new(FixtureKeychain {
                secret: Ok(credential()),
            }),
        );
        let error = reader
            .issue("ASMA-1")
            .await
            .expect_err("the read is refused");
        assert_eq!(error.failure, JiraReadFailure::Permission);
        assert_ne!(error.failure, JiraReadFailure::NotFound);
        assert!(error.is_uncertain());
    }
}

#[tokio::test]
async fn a_404_is_an_answer_that_is_not_a_deletion() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-404"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"errorMessages": []})))
        .mount(&server)
        .await;
    let (limits, retry) = fast();
    let (reader, _root) = compose_reader(
        &server,
        limits,
        retry,
        Arc::new(FixtureKeychain {
            secret: Ok(credential()),
        }),
    );
    let answer = reader
        .issue("ASMA-404")
        .await
        .expect("the answer is an answer");
    assert!(answer.issue.is_none());
    assert_eq!(answer.stats.not_found, 1);
}

#[tokio::test]
async fn oversized_timeout_and_malformed_answers_are_bounded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 4096]))
        .mount(&server)
        .await;
    let (mut limits, retry) = fast();
    limits.max_response_bytes = 1024;
    let (reader, _root) = compose_reader(
        &server,
        limits,
        retry,
        Arc::new(FixtureKeychain {
            secret: Ok(credential()),
        }),
    );
    assert_eq!(
        reader
            .preflight()
            .await
            .expect_err("the body is bounded")
            .failure,
        JiraReadFailure::Oversized
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(300))
                .set_body_json(json!({"accountId": "synthetic"})),
        )
        .mount(&server)
        .await;
    let (mut limits, mut retry) = fast();
    limits.request_timeout = Duration::from_millis(50);
    retry.max_attempts = 1;
    let (reader, _root) = compose_reader(
        &server,
        limits,
        retry,
        Arc::new(FixtureKeychain {
            secret: Ok(credential()),
        }),
    );
    assert_eq!(
        reader
            .preflight()
            .await
            .expect_err("the request is bounded")
            .failure,
        JiraReadFailure::Timeout
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"unexpected": true})))
        .mount(&server)
        .await;
    let (limits, retry) = fast();
    let (reader, _root) = compose_reader(
        &server,
        limits,
        retry,
        Arc::new(FixtureKeychain {
            secret: Ok(credential()),
        }),
    );
    assert_eq!(
        reader
            .search_all("project = ASMA")
            .await
            .expect_err("the shape is required")
            .failure,
        JiraReadFailure::Malformed
    );
}

#[tokio::test]
async fn a_redirect_is_refused_before_another_origin_sees_the_credential() {
    let elsewhere = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"stolen": true})))
        .mount(&elsewhere)
        .await;
    let server = MockServer::start().await;
    let location = format!("{}/rest/api/3/myself", elsewhere.uri());
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", location.as_str())
                .set_body_json(json!({"errorMessages": ["moved"]})),
        )
        .mount(&server)
        .await;
    let (limits, retry) = fast();
    let (reader, _root) = compose_reader(
        &server,
        limits,
        retry,
        Arc::new(FixtureKeychain {
            secret: Ok(credential()),
        }),
    );
    let error = reader
        .preflight()
        .await
        .expect_err("a redirect leaves the origin");
    assert_eq!(error.failure, JiraReadFailure::Rejected);
    assert!(
        elsewhere
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty(),
        "the other origin never receives the request or the credential"
    );
}

#[tokio::test]
async fn a_missing_credential_never_reaches_the_network() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"accountId": "synthetic"})))
        .mount(&server)
        .await;
    let (limits, retry) = fast();
    let (reader, _root) = compose_reader(
        &server,
        limits,
        retry,
        Arc::new(FixtureKeychain {
            secret: Err(KeychainFailure::NotFound),
        }),
    );
    let error = reader
        .preflight()
        .await
        .expect_err("the credential is absent");
    assert_eq!(error.failure, JiraReadFailure::Credential);
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty(),
        "no request is made without a credential"
    );
    let rendered = format!("{error} {error:?}");
    assert!(!rendered.contains("synthetic-canary"));
}
