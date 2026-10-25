//! TEST-003 recovery: incomplete sweeps, 404s, permission loss, search lag,
//! removed labels, stale answers, overlap and periodic full sweeps.

mod jira_support;

use jira_support::{JiraFixture, epic, issue, story};
use kontor_catalog::{JiraCollectionState, JiraSweepMode};
use kontor_core::id::Timestamp;
use serde_json::{Value, json};

async fn full(
    fixture: &JiraFixture,
    at: Timestamp,
    issues: Vec<Value>,
) -> kontor_catalog::JiraSweepReport {
    fixture.mount_scope_page(false, 0, None, issues, true).await;
    fixture.mount_catch_all_missing().await;
    fixture.collect(at).await
}

fn epic_at(id: &str, key: &str, labels: &[&str], updated: &str, summary: &str) -> Value {
    issue(id, key, "Epic", 1, None, labels, updated, summary)
}

#[tokio::test]
async fn a_404_and_a_permission_loss_never_delete() {
    let fixture = JiraFixture::new(&[]).await;
    let report = full(
        &fixture,
        fixture.now(0),
        vec![
            epic("100", "ASMA-100", &["ws:kontor"]),
            epic("200", "ASMA-200", &[]),
        ],
    )
    .await;
    assert!(report.complete);
    assert_eq!(fixture.evidence().generation, 1);

    fixture.reset().await;
    fixture
        .mount_scope_page(
            false,
            0,
            None,
            vec![epic("100", "ASMA-100", &["ws:kontor"])],
            true,
        )
        .await;
    fixture.mount_missing("ASMA-200").await;
    fixture.mount_catch_all_missing().await;
    let report = fixture.collect(fixture.now(901)).await;
    assert!(report.complete, "{report:?}");
    assert_eq!(report.coverage.retained_unknown, vec!["ASMA-200"]);
    let evidence = fixture.evidence();
    assert_eq!(evidence.generation, 2);
    assert!(
        evidence.issues.iter().any(|issue| issue.key == "ASMA-200"),
        "a 404 retains the prior fact"
    );

    fixture.reset().await;
    fixture.mount_scope_failure(false, 403).await;
    let report = fixture.collect(fixture.now(1802)).await;
    assert!(!report.complete);
    assert_eq!(report.state, JiraCollectionState::Failed);
    assert!(report.coverage.permission_loss);
    let evidence = fixture.evidence();
    assert_eq!(
        evidence.generation, 2,
        "a failed sweep does not advance the generation"
    );
    assert_eq!(evidence.issues.len(), 2, "permission loss is not deletion");
    assert_eq!(evidence.collection_state, JiraCollectionState::Failed);
    assert!(evidence.latest_error.is_some());
}

#[tokio::test]
async fn an_incomplete_sweep_retains_prior_facts_and_cannot_tombstone() {
    let fixture = JiraFixture::new(&[]).await;
    let report = full(
        &fixture,
        fixture.now(0),
        vec![
            epic("100", "ASMA-100", &["ws:kontor"]),
            epic("200", "ASMA-200", &[]),
        ],
    )
    .await;
    assert!(report.complete);

    fixture.reset().await;
    fixture.mount_scope_failure(false, 500).await;
    let report = fixture.collect(fixture.now(60)).await;
    assert!(!report.complete);
    assert_eq!(report.state, JiraCollectionState::Incomplete);
    assert!(!report.coverage.census);
    assert_eq!(report.coverage.removed, 0);
    let evidence = fixture.evidence();
    assert_eq!(evidence.generation, 1);
    assert_eq!(evidence.issues.len(), 2);
    assert_eq!(evidence.collection_state, JiraCollectionState::Incomplete);
    assert!(evidence.latest_error.is_some());
}

#[tokio::test]
async fn search_lag_and_a_removed_label_are_visible_not_deleted() {
    let fixture = JiraFixture::new(&[]).await;
    let report = full(
        &fixture,
        fixture.now(0),
        vec![
            epic_at(
                "100",
                "ASMA-100",
                &["ws:kontor"],
                "2026-10-25T10:00:00.000+0000",
                "Before",
            ),
            epic_at(
                "200",
                "ASMA-200",
                &[],
                "2026-10-25T10:00:00.000+0000",
                "Jira only",
            ),
        ],
    )
    .await;
    assert!(report.complete);

    fixture.reset().await;
    fixture
        .mount_scope_page(
            false,
            0,
            None,
            vec![epic_at(
                "100",
                "ASMA-100",
                &[],
                "2026-10-25T11:00:00.000+0000",
                "After",
            )],
            true,
        )
        .await;
    fixture.mount_issue(epic("200", "ASMA-200", &[])).await;
    fixture.mount_catch_all_missing().await;
    let report = fixture.collect(fixture.now(901)).await;
    assert!(report.complete, "{report:?}");
    assert!(report.coverage.search_lag_suspected);
    assert_eq!(report.coverage.removed, 0);
    assert_eq!(report.scope_issues, 1);
    assert_eq!(report.observed, 2);
    assert!(
        report
            .coverage
            .diagnostics
            .iter()
            .any(|line| line.contains("ws:kontor") && line.contains("removed or changed")),
        "{:?}",
        report.coverage.diagnostics
    );
    let evidence = fixture.evidence();
    let one_hundred = evidence
        .issues
        .iter()
        .find(|issue| issue.key == "ASMA-100")
        .expect("the epic");
    assert!(
        one_hundred.labels.is_empty(),
        "the removed label is observed"
    );
    assert_eq!(one_hundred.summary, "After");
    assert!(
        evidence.issues.iter().any(|issue| issue.key == "ASMA-200"),
        "search lag keeps the omitted issue active"
    );
}

#[tokio::test]
async fn a_stale_answer_cannot_overwrite_a_newer_observation() {
    let fixture = JiraFixture::new(&[]).await;
    let report = full(
        &fixture,
        fixture.now(0),
        vec![epic_at(
            "100",
            "ASMA-100",
            &["ws:kontor"],
            "2026-10-25T10:00:00.000+0000",
            "new",
        )],
    )
    .await;
    assert!(report.complete);

    fixture.reset().await;
    let report = full(
        &fixture,
        fixture.now(901),
        vec![epic_at(
            "100",
            "ASMA-100",
            &[],
            "2026-10-25T09:00:00.000+0000",
            "old",
        )],
    )
    .await;
    assert!(report.complete);
    let evidence = fixture.evidence();
    let issue = evidence
        .issues
        .iter()
        .find(|issue| issue.key == "ASMA-100")
        .expect("the epic");
    assert_eq!(issue.summary, "new", "a late older answer may not win");
    assert_eq!(issue.updated, "2026-10-25T10:00:00Z");
    assert_eq!(issue.labels, vec!["ws:kontor"]);
}

#[tokio::test]
async fn a_declared_key_moved_out_of_project_is_refused_not_rewritten() {
    let fixture = JiraFixture::new(&["ASMA-140"]).await;
    fixture
        .mount_scope_page(false, 0, None, Vec::new(), true)
        .await;
    fixture
        .mount_issue_at(
            "ASMA-140",
            issue(
                "141",
                "EX-1",
                "Story",
                0,
                Some("EX-2"),
                &[],
                "2026-10-25T10:00:00.000+0000",
                "Outside the configured project",
            ),
        )
        .await;
    fixture.mount_missing("EX-2").await;
    fixture.mount_catch_all_missing().await;
    let report = fixture.collect(fixture.now(0)).await;
    assert!(report.complete, "{report:?}");
    assert_eq!(report.coverage.declared, 1);
    assert_eq!(report.coverage.found, 0);
    assert_eq!(report.coverage.out_of_scope, vec!["ASMA-140"]);
    let evidence = fixture.evidence();
    assert_eq!(evidence.unanswered, vec!["ASMA-140"]);
    assert!(
        evidence.aliases.is_empty(),
        "an alias must reference an observed immutable issue row"
    );
    assert!(matches!(
        fixture.jira.export_epic_identity(),
        Err(kontor_catalog::JiraCatalogError::Projection(
            kontor_catalog::JiraProjectionError::CollectionUnanswered(_)
        ))
    ));
}

#[tokio::test]
async fn a_changed_ws_label_is_diagnostic_and_not_deletion() {
    let fixture = JiraFixture::new(&[]).await;
    full(
        &fixture,
        fixture.now(0),
        vec![epic("100", "ASMA-100", &["ws:before"])],
    )
    .await;
    fixture.reset().await;
    let report = full(
        &fixture,
        fixture.now(901),
        vec![epic("100", "ASMA-100", &["ws:after"])],
    )
    .await;
    assert!(report.complete, "{report:?}");
    let evidence = fixture.evidence();
    let issue = evidence
        .issues
        .iter()
        .find(|issue| issue.key == "ASMA-100")
        .expect("the epic");
    assert_eq!(issue.labels, vec!["ws:after"]);
    assert!(
        report
            .coverage
            .diagnostics
            .iter()
            .any(|line| line.contains("ws:before") && line.contains("removed or changed")),
        "{:?}",
        report.coverage.diagnostics
    );
}

#[tokio::test]
async fn an_incremental_sweep_uses_an_overlap_and_never_tombstones() {
    let fixture = JiraFixture::new(&[]).await;
    let report = full(
        &fixture,
        fixture.now(0),
        vec![
            epic("100", "ASMA-100", &["ws:kontor"]),
            epic("200", "ASMA-200", &[]),
        ],
    )
    .await;
    assert!(report.complete);
    assert_eq!(report.mode, JiraSweepMode::Full);

    fixture.reset().await;
    fixture
        .mount_scope_page(
            true,
            0,
            None,
            vec![epic("100", "ASMA-100", &["ws:kontor"])],
            true,
        )
        .await;
    fixture
        .mount_issue(story("200", "ASMA-200", "ASMA-100", &[]))
        .await;
    fixture.mount_catch_all_missing().await;
    let report = fixture.collect(fixture.now(60)).await;
    assert!(report.complete, "{report:?}");
    assert_eq!(report.mode, JiraSweepMode::Incremental);
    assert!(!report.coverage.census);
    assert_eq!(report.coverage.removed, 0);
    assert_eq!(
        report.coverage.window_start.as_deref(),
        Some("2026-10-24T23:57:00Z")
    );
    let evidence = fixture.evidence();
    assert_eq!(evidence.generation, 2);
    assert!(
        evidence.issues.iter().any(|issue| issue.key == "ASMA-200"),
        "an incremental window never removes a prior fact"
    );
    let requests = fixture.server.received_requests().await.unwrap_or_default();
    let jql = requests
        .iter()
        .find(|request| request.url.path() == "/rest/api/3/search/jql")
        .and_then(|request| {
            request
                .url
                .query_pairs()
                .find(|(key, _)| key == "jql")
                .map(|(_, value)| value.into_owned())
        })
        .expect("the incremental search JQL");
    assert!(jql.contains("updated >="), "{jql}");
    assert!(jql.contains("2026-10-24 23:57"), "{jql}");

    fixture.reset().await;
    let report = full(
        &fixture,
        fixture.now(961),
        vec![
            epic("100", "ASMA-100", &["ws:kontor"]),
            epic("200", "ASMA-200", &[]),
        ],
    )
    .await;
    assert_eq!(report.mode, JiraSweepMode::Full);
    assert!(report.coverage.census);
    assert!(
        report.coverage.overdue,
        "a full sweep past its deadline reports the overdue schedule"
    );
}

#[tokio::test]
async fn a_failed_full_sweep_does_not_postpone_the_full_census() {
    let fixture = JiraFixture::new(&[]).await;
    fixture.mount_scope_failure(false, 500).await;
    let first = fixture.collect(fixture.now(0)).await;
    assert_eq!(first.mode, JiraSweepMode::Full);
    assert_eq!(first.state, JiraCollectionState::Incomplete);

    fixture.reset().await;
    let second = full(
        &fixture,
        fixture.now(60),
        vec![epic("100", "ASMA-100", &["ws:kontor"])],
    )
    .await;
    assert_eq!(
        second.mode,
        JiraSweepMode::Full,
        "a failed full sweep must retry the full census, not wait a full interval"
    );
    assert!(second.complete);
}

#[tokio::test]
async fn repeated_observations_advance_the_order_without_duplicates() {
    let fixture = JiraFixture::new(&[]).await;
    full(
        &fixture,
        fixture.now(0),
        vec![
            epic("100", "ASMA-100", &["ws:kontor"]),
            epic("200", "ASMA-200", &[]),
        ],
    )
    .await;
    let first = fixture.evidence();
    let first_order = first.order;
    let first_observed = first
        .issues
        .iter()
        .find(|issue| issue.key == "ASMA-100")
        .expect("the epic")
        .observed_at
        .clone();
    fixture.reset().await;
    full(
        &fixture,
        fixture.now(901),
        vec![
            epic("100", "ASMA-100", &["ws:kontor"]),
            epic("200", "ASMA-200", &[]),
        ],
    )
    .await;
    let second = fixture.evidence();
    assert!(
        second.order > first_order,
        "the projection order is monotonic"
    );
    assert_eq!(
        second.issues.len(),
        2,
        "repeated observation adds no duplicate"
    );
    let mut keys: Vec<&str> = second
        .issues
        .iter()
        .map(|issue| issue.key.as_str())
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["ASMA-100", "ASMA-200"]);
    let mut ids: Vec<&str> = second
        .issues
        .iter()
        .map(|issue| issue.id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 2);
    let second_observed = second
        .issues
        .iter()
        .find(|issue| issue.key == "ASMA-100")
        .expect("the epic")
        .observed_at
        .clone();
    assert!(second_observed >= first_observed);
}

#[tokio::test]
async fn an_unread_parent_is_unresolved_and_never_an_inferred_epic() {
    let fixture = JiraFixture::new(&["ASMA-101"]).await;
    fixture
        .mount_scope_page(false, 0, None, Vec::new(), true)
        .await;
    fixture
        .mount_issue(story("101", "ASMA-101", "ASMA-999", &[]))
        .await;
    fixture.mount_missing("ASMA-999").await;
    fixture.mount_catch_all_missing().await;
    let report = fixture.collect(fixture.now(0)).await;
    assert!(report.complete, "{report:?}");
    assert_eq!(report.coverage.unresolved, vec!["ASMA-101"]);
    let export = fixture.export();
    assert!(export.epic("ASMA-101").unwrap().is_none());
    assert_eq!(export.coverage.unresolved, vec!["ASMA-101"]);
}

#[tokio::test]
async fn a_complete_sweep_promotes_an_issue_first_seen_by_an_incomplete_sweep() {
    let fixture = JiraFixture::new(&["ASMA-999"]).await;
    fixture
        .mount_scope_page(
            false,
            0,
            None,
            vec![epic("300", "ASMA-300", &["ws:kontor"])],
            true,
        )
        .await;
    fixture.mount_issue_failure("ASMA-999", 500).await;
    fixture.mount_catch_all_missing().await;
    let incomplete = fixture.collect(fixture.now(0)).await;
    assert!(!incomplete.complete, "{incomplete:?}");
    assert_eq!(incomplete.state, JiraCollectionState::Incomplete);

    fixture.reset().await;
    fixture
        .mount_scope_page(
            false,
            0,
            None,
            vec![epic("300", "ASMA-300", &["ws:kontor"])],
            true,
        )
        .await;
    fixture.mount_missing("ASMA-999").await;
    fixture.mount_catch_all_missing().await;
    let complete = fixture.collect(fixture.now(60)).await;
    assert!(complete.complete, "{complete:?}");
    let evidence = fixture.evidence();
    assert!(
        evidence.issues.iter().any(|issue| issue.key == "ASMA-300"),
        "the hidden first observation becomes visible at promotion"
    );
}

#[tokio::test]
async fn rate_limit_exhaustion_is_incomplete_and_keeps_prior_facts() {
    let fixture = JiraFixture::with_bounds(&[], 2, 500).await;
    let report = full(
        &fixture,
        fixture.now(0),
        vec![epic("100", "ASMA-100", &["ws:kontor"])],
    )
    .await;
    assert!(report.complete);

    fixture.reset().await;
    fixture
        .server
        .register(
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .and(wiremock::matchers::path("/rest/api/3/search/jql"))
                .respond_with(
                    wiremock::ResponseTemplate::new(429)
                        .insert_header("Retry-After", "0")
                        .set_body_json(json!({"errorMessages": ["slow down"]})),
                )
                .with_priority(2),
        )
        .await;
    let report = fixture.collect(fixture.now(60)).await;
    assert!(!report.complete);
    assert_eq!(report.state, JiraCollectionState::Incomplete);
    assert_eq!(report.coverage.rate_limited, 2);
    assert_eq!(report.coverage.retries, 1);
    assert!(report.coverage.backoff_until.is_some());
    let evidence = fixture.evidence();
    assert_eq!(evidence.generation, 1);
    assert_eq!(evidence.issues.len(), 1);
}
