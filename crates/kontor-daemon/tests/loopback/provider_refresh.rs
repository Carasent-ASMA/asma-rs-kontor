use super::*;

#[tokio::test]
async fn usage_endpoint_throttle_is_public_persisted_and_never_refreshes_quota_evidence() {
    let reporter = Arc::new(ScriptedUsageReporter::new(
        [
            Ok(UsageReading {
                provider: "claude".to_owned(),
                limit_reached: false,
                windows: Vec::new(),
                credits_exhausted: false,
            }),
            Err(ProviderUsageProbeFailure::Throttled {
                retry_after_seconds: 600,
            }),
        ],
        Duration::from_millis(25),
    ));
    let world = World::open_empty_with_usage_reporter(Arc::clone(&reporter) as Arc<_>).await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "usage-throttle",
        "Usage throttle",
        "/tmp/usage-throttle",
    )
    .await;
    let project = created.json()["project_id"].as_str().unwrap().to_owned();
    let account = Call::post(format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({ "label":"Claude usage", "harness":"fake.runtime",
            "credential_alias":"claude-work", "selectable_providers":["claude-work"], "enabled":true }))
        .signed_as(&world,"admin").with_key("throttle-account").send(&world).await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account = account.json()["account_profile_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let uri = format!("/v1/projects/{project}/provider-account-profiles/{account}/quota:probe");
    let body = serde_json::json!({"provider":"claude-work"});
    let positive = Call::post(&uri, &body)
        .signed_as(&world, "operator")
        .with_key("throttle-prior-success")
        .send(&world)
        .await;
    assert_eq!(positive.status, 200, "{}", positive.body);
    let project_id = ProjectId::parse(&project).unwrap();
    let account_id = AccountProfileId::parse(&account).unwrap();
    let prior = world.daemon.state().with_store(|store| {
        (
            store.list_provider_quota_states(project_id).unwrap(),
            store
                .latest_provider_usage_observation(project_id, account_id, "claude-work")
                .unwrap()
                .unwrap(),
        )
    });
    let first = Call::post(&uri, &body)
        .signed_as(&world, "operator")
        .with_key("throttle-first");
    let concurrent = Call::post(&uri, &body)
        .signed_as(&world, "operator")
        .with_key("throttle-concurrent");
    let (first, concurrent) = tokio::join!(first.send(&world), concurrent.send(&world));
    for answer in [&first, &concurrent] {
        assert_eq!(answer.status, 429, "{}", answer.body);
        assert_eq!(answer.code(), "provider_usage_throttled");
        assert!((1..=600).contains(&answer.json()["retry_after_seconds"].as_u64().unwrap()));
    }
    assert_eq!(
        reporter.calls(),
        2,
        "the concurrent independent key honors the account retry deadline"
    );
    let operator = secret(&world, "operator");
    let World {
        directory,
        daemon,
        router,
        fake,
        ..
    } = world;
    daemon.state().signals().stop();
    drop(router);
    drop(daemon);
    let poller = kontor_daemon::usage::UsagePoller::with_exact_reporter(
        directory.path(),
        Arc::clone(&reporter) as Arc<_>,
    );
    let restarted = Daemon::start_with_usage_poller(
        DaemonConfig::at(directory.path()).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
        poller,
    )
    .unwrap();
    restarted.reconcile().await;
    let after = Call::post(&uri, &body)
        .with_token(&operator)
        .with_key("throttle-after-restart")
        .send_to(&restarted.router())
        .await;
    assert_eq!(after.status, 429, "{}", after.body);
    assert_eq!(after.code(), "provider_usage_throttled");
    assert_eq!(
        reporter.calls(),
        2,
        "restart cannot reset the actual provider's retry deadline"
    );
    restarted.state().with_store(|store| {
        assert_eq!(
            store.list_provider_quota_states(project_id).unwrap(),
            prior.0
        );
        assert_eq!(
            store
                .latest_provider_usage_observation(project_id, account_id, "claude-work")
                .unwrap()
                .unwrap(),
            prior.1,
            "throttling is neither fresh quota evidence nor an exhausted model allowance"
        );
    });
}
