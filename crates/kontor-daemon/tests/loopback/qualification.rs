//! Integrated ASMA-8454 rehearsal. All writes and fixtures are disposable.
use super::super::durable_desks::desk_call;
use super::*;
use kontor_daemon::DATABASE_FILE;
use kontor_store::backup::{create_snapshot, export_realm};

/// Create the old-pin control before any desk vocabulary is selected.
async fn legacy_realm() -> Review {
    let world = World::open_empty_with_a_plane().await;
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let root = world.directory.path().join("qualification-repository");
    let project = ensure_governed_project(&world, root.to_str().expect("path"));
    let category = first_category(&world).await;
    let name = "Qualification legacy";
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            current_project_revision(&world, &project).await,
            name,
            &category,
            serde_json::json!([{"title": "Legacy fixture", "module": "_tools/asma-rs-kontor"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("qualification-legacy")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("epic").to_owned();
    confirm_test_epic_identity(&world, &project, &epic);
    Review {
        project_id: ProjectId::parse(&project).expect("project"),
        world,
        project,
        epic,
        epic_key: test_jira_key(&format!("{name}-epic")),
        task_key: test_jira_key(&format!("{name}-task-0")),
    }
}

fn pin(realm: &Review) -> kontor_core::spec::TeamDefinitionSnapshot {
    realm.world.daemon.state().with_store(|store| {
        store
            .get_mini_project_team_definition(
                realm.project_id,
                MiniProjectId::parse(&realm.epic).expect("epic"),
            )
            .expect("pin reads")
            .expect("pin exists")
            .definition
    })
}

async fn select(realm: &Review, definition: &TeamDefinitionSpec, key: &str) {
    let _domain_selection =
        harness::DomainSelectionFixture::activate(&realm.world, realm.project_id);
    let preview = Call::post(
        format!(
            "/v1/projects/{}/team-definition-selection:preview",
            realm.project
        ),
        &serde_json::json!({"target_definition": {
            "id": definition.definition_id, "version": definition.version}}),
    )
    .signed_as(&realm.world, "admin")
    .send(&realm.world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let applied = Call::post(
        format!(
            "/v1/projects/{}/team-definition-selection:apply",
            realm.project
        ),
        &serde_json::json!({"preview_hash": preview.json()["preview_hash"],
            "expected_revision": current_project_revision(&realm.world, &realm.project).await}),
    )
    .signed_as(&realm.world, "admin")
    .with_key(key)
    .send(&realm.world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
}

/// Durable, secret-free logical/native evidence, read from the actual store.
fn census(realm: &Review) -> serde_json::Value {
    let old_pin = pin(realm);
    let reviews = subjects(realm);
    realm.world.daemon.state().with_store(|store| {
        let nodes = store.list_project_topology_nodes(realm.project_id).expect("nodes");
        let bindings: Vec<_> = nodes.iter().map(|node| {
            let container = store.get_topology_node_container(realm.project_id, node.id).expect("container")
                .map(|binding| serde_json::json!({"id": binding.container_binding_id, "identity": binding.identity,
                    "kind": binding.observed_kind, "cwd": binding.canonical_cwd,
                    "title": binding.readback.as_ref().map(|readback| &readback.visible_title),
                    "parent": binding.readback.as_ref().and_then(|readback| readback.native_parent.as_ref())}));
            let seats = store.list_seat_bindings(realm.project_id, node.id).expect("seats");
            let hosted: Vec<_> = seats.iter().map(|seat|
                store.get_hosted_topology_seat(realm.project_id, seat.id).expect("hosted")).collect();
            serde_json::json!({"node": node,
                "container": container, "seats": seats, "hosted": hosted})
        }).collect();
        serde_json::json!({"bindings": bindings,
            "desks": store.list_desks(realm.project_id).expect("desks"),
            "old_pin": old_pin, "reviews": reviews})
    })
}

#[tokio::test]
async fn a_new_desk_under_a_parent_that_already_knows_an_older_desk_kind_refuses_without_effects() {
    let realm = review_realm("qualification-known-kind", Some(PR_REVIEW)).await;
    let first = desk_call(
        &realm.world,
        &realm.project,
        "materialize",
        "adam",
        "old-desk-kind",
    )
    .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let supported = realm.world.daemon.state().with_store(|store| {
        let selected = store
            .get_project_team_definition_default(realm.project_id)
            .expect("selection")
            .expect("default");
        store
            .get_team_definition(
                realm.project_id,
                selected.definition.definition_id,
                selected.definition.version,
            )
            .expect("definition")
            .expect("supported definition")
    });
    select_qualification_definition(&realm.world, &realm.project, Some(PR_REVIEW), true).await;
    let before = census(&realm);
    let now = at("2026-10-11T12:00:00Z");
    let records = realm.world.daemon.state().with_store(|store| {
        export_realm(store, now)
            .expect("before export")
            .records_hash
    });
    let natives = realm.world.fake.minted_natives();
    let calls = realm.world.fake.calls();
    let refused = desk_call(
        &realm.world,
        &realm.project,
        "materialize",
        "pr-review",
        "known-kind-refusal",
    )
    .await;
    assert_eq!(
        refused.json()["code"],
        "revision_conflict",
        "{}",
        refused.body
    );
    assert_eq!(census(&realm), before);
    assert_eq!(realm.world.fake.minted_natives(), natives);
    assert_eq!(
        realm.world.fake.calls(),
        calls,
        "no runtime call on refusal"
    );
    realm.world.daemon.state().with_store(|store| {
        assert_eq!(
            export_realm(store, now).expect("after export").records_hash,
            records,
            "every exported durable record is unchanged, including pins, bindings and receipts"
        );
        assert!(
            store
                .get_desk(
                    realm.project_id,
                    &kontor_core::id::DeskKey::parse("pr-review").expect("desk key")
                )
                .expect("desk")
                .is_none(),
            "no successor desk reservation"
        );
        assert!(
            store
                .get_receipt_by_key(&IdempotencyKey::parse("known-kind-refusal").expect("key"))
                .expect("receipt")
                .is_none()
        );
    });
    // A refusal must not poison the key: return to the supported revision,
    // then use the same command key and recover the original behavior.
    select(&realm, &supported, "known-kind-supported-selection").await;
    let supported_ensure = desk_call(
        &realm.world,
        &realm.project,
        "materialize",
        "pr-review",
        "known-kind-refusal",
    )
    .await;
    assert_eq!(supported_ensure.status, 200, "{}", supported_ensure.body);
    let after = census(&realm);
    for binding in before["bindings"].as_array().expect("bindings") {
        assert!(
            after["bindings"]
                .as_array()
                .expect("bindings")
                .contains(binding),
            "existing rows and native identities survive the supported ensure"
        );
    }
    let replay = desk_call(
        &realm.world,
        &realm.project,
        "materialize",
        "pr-review",
        "known-kind-refusal",
    )
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(
        census(&realm),
        after,
        "supported ensure replay is unchanged"
    );
}

#[tokio::test]
async fn four_structures_restore_replay_and_selection_rollback_keep_old_pins_and_natives() {
    // Start desk-less: an existing legacy epic is a positive old-pin control.
    let realm = legacy_realm().await;
    let old_pin = pin(&realm);
    let original = realm.world.daemon.state().with_store(|store| {
        store
            .get_team_definition(realm.project_id, old_pin.definition_id, old_pin.version)
            .expect("definition")
            .expect("old definition")
    });
    let original_bytes = original.canonicalize().expect("canonical");
    // Remove desks too for the rollback control; the bundled old revision is v1.
    let deskless = realm.world.daemon.state().with_store(|store| {
        store
            .get_team_definition(
                realm.project_id,
                old_pin.definition_id,
                kontor_core::id::SpecVersion::FIRST,
            )
            .expect("definition")
            .expect("v1")
    });
    select_qualification_definition(&realm.world, &realm.project, Some(PR_REVIEW), true).await;
    assert_eq!(
        pin(&realm),
        old_pin,
        "selection must never migrate an existing epic"
    );
    let successor = realm.world.daemon.state().with_store(|store| {
        let selected = store
            .get_project_team_definition_default(realm.project_id)
            .expect("selection")
            .expect("default");
        store
            .get_team_definition(
                realm.project_id,
                selected.definition.definition_id,
                selected.definition.version,
            )
            .expect("definition")
            .expect("successor")
    });
    let topology = Call::get(format!(
        "/v1/projects/{}/topology-specs/{}/{}",
        realm.project,
        successor.topology.spec_id,
        successor.topology.version.get()
    ))
    .signed_as(&realm.world, "admin")
    .send(&realm.world)
    .await;
    assert_eq!(topology.status, 200, "{}", topology.body);
    for desk in ["adam", "pr-review"] {
        let answer = desk_call(
            &realm.world,
            &realm.project,
            "materialize",
            desk,
            &format!("qualification-desk-{desk}"),
        )
        .await;
        assert_eq!(answer.status, 200, "{}", answer.body);
    }
    // The ASMA-8419 seats are declarations only; one unrelated daemon fixture launches.
    for (key, label, owner) in [
        ("general", "GENERAL", "paseo_direct"),
        ("checker", "PR DRAFTS STATUS CHECKER", "paseo_direct"),
        ("fixture", "QUALIFICATION FIXTURE", "daemon"),
    ] {
        let mut body = serde_json::json!({"expected_revision":
            current_project_revision(&realm.world, &realm.project).await,
            "seat_key": key, "role_code": "LSA", "seat_label": label,
            "recovery_owner": owner, "decided_by": "Test Owner", "decided_on": "2026-10-08"});
        if owner == "daemon" {
            body["model_route"] = codex_route();
        } else {
            body["procedure"] = serde_json::json!(if key == "general" {
                ".github/skills/asma-pr-review/SKILL.md"
            } else {
                ".github/skills/pr-draft-status-checker/SKILL.md"
            });
        }
        let answer = Call::post(
            format!(
                "/v1/projects/{}/desks/pr-review/seats:declare",
                realm.project
            ),
            &body,
        )
        .signed_as(&realm.world, "operator")
        .with_key(format!("qualification-declare-{key}"))
        .send(&realm.world)
        .await;
        assert_eq!(answer.status, 200, "{}", answer.body);
    }
    let reconcile_body = serde_json::json!({"expected_revision":
        current_project_revision(&realm.world, &realm.project).await,
        "seat_keys": ["fixture"], "restart_missing": true});
    let reconcile_path = format!(
        "/v1/projects/{}/desks/pr-review/seats:reconcile",
        realm.project
    );
    let answer = Call::post(&reconcile_path, &reconcile_body)
        .signed_as(&realm.world, "operator")
        .with_key("qualification-reconcile")
        .send(&realm.world)
        .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let durable_report = Call::get(format!(
        "/v1/projects/{}/desks/pr-review/seats",
        realm.project
    ))
    .signed_as(&realm.world, "observer")
    .send(&realm.world)
    .await
    .json();
    for seat in durable_report["seats"].as_array().expect("seats") {
        if seat["recovery_owner"] == "paseo_direct" {
            assert_eq!(seat["status"], "unchecked");
            assert!(seat["native_id"].is_null());
        } else {
            assert_eq!(seat["status"], "healthy");
        }
    }
    adopt_session_base(
        &realm.world,
        &realm.project,
        current_project_revision(&realm.world, &realm.project).await,
    )
    .await;
    publish_core_team(
        &realm.world,
        &realm.project,
        serde_json::json!([seat("SA", "on_demand", true)]),
    )
    .await;
    prepare_fake_provider_headroom(&realm.world, &realm.project).await;
    let category = first_category(&realm.world).await;
    for topic in ["Qualification Alpha", "Qualification Beta"] {
        let applied = Call::post(
            format!("/v1/projects/{}/epics:apply", realm.project),
            &epic_body(
                current_project_revision(&realm.world, &realm.project).await,
                topic,
                &category,
                serde_json::json!([{"title": "Fixture work"}]),
            ),
        )
        .signed_as(&realm.world, "admin")
        .with_key(format!("qualification-epic-{topic}"))
        .send(&realm.world)
        .await;
        assert_eq!(applied.status, 200, "{}", applied.body);
        let epic = applied.json()["epic_id"].as_str().expect("epic").to_owned();
        confirm_test_epic_identity(&realm.world, &realm.project, &epic);
        let mini = realm.world.daemon.state().with_store(|store| {
            store
                .get_mini_project(realm.project_id, MiniProjectId::parse(&epic).expect("epic"))
                .expect("epic")
                .expect("epic")
        });
        let launched = Call::post(
            format!(
                "/v1/projects/{}/epics/{epic}/core-team/seats:materialize",
                realm.project
            ),
            &serde_json::json!({"expected_revision": mini.revision.get(),
                "routes": [{"role_code": "LSA", "model_route": claude_route()}]}),
        )
        .signed_as(&realm.world, "admin")
        .with_key(format!("qualification-lsa-{topic}"))
        .send(&realm.world)
        .await;
        assert_eq!(launched.status, 200, "{}", launched.body);
        let nodes = realm.world.daemon.state().with_store(|store| {
            store
                .list_topology_nodes(realm.project_id, Some(mini.id))
                .expect("nodes")
        });
        assert!(nodes.iter().all(|node| node.kind.as_str() != "ECP"));
        assert!(nodes.iter().any(|node| node.kind.as_str() == "ADE"));
    }
    let ad_hoc = Call::post(
        format!("/v1/projects/{}/desk-adams/ad-hoc:open", realm.project),
        &serde_json::json!({"topic": "Qualification triage", "model_route": claude_route()}),
    )
    .signed_as(&realm.world, "admin")
    .with_key("qualification-ad-hoc")
    .send(&realm.world)
    .await;
    assert_eq!(ad_hoc.status, 200, "{}", ad_hoc.body);
    // Legacy leadership retains its original routing and pins. The publication
    // journey below explicitly activates its own repository authority.
    activate_publication_policy(&realm.world, &realm.project);
    let bodies = [
        open_body(&realm.epic_key, MODULE_REPOSITORY, 101, Some(routes())),
        open_body(&realm.epic_key, MODULE_REPOSITORY, 102, Some(routes())),
    ];
    let first = opened(&realm, &bodies[0], "qualification-pr-101").await;
    let second = opened(&realm, &bodies[1], "qualification-pr-102").await;
    assert_ne!(
        first["adam"]["topology_node_id"],
        second["adam"]["topology_node_id"]
    );
    assert_ne!(
        first["room"]["topology_node_id"],
        second["room"]["topology_node_id"]
    );
    let before = census(&realm);
    let report = list(&realm).await;
    let minted = realm.world.fake.minted_natives();
    let now = at("2026-10-11T12:00:00Z");
    let exported = realm
        .world
        .daemon
        .state()
        .with_store(|store| export_realm(store, now))
        .expect("export");
    let snapshot = create_snapshot(
        &realm.world.directory.path().join(DATABASE_FILE),
        &realm.world.directory.path().join("qualification-backups"),
        now,
    )
    .expect("snapshot");
    // Offline restore into this same isolated root; credentials stay beside it.
    let Review {
        world,
        project,
        project_id,
        epic,
        epic_key,
        task_key,
    } = realm;
    let World {
        directory,
        daemon,
        router,
        fake,
        project: world_project,
        task,
        team_run,
    } = world;
    drop(router);
    daemon.shutdown();
    // A clean stop must release the store connection, not just the state-root
    // lock: a retained handle leaves committed frames in the WAL, and restore
    // refuses that rather than discarding them.
    let wal = directory.path().join(format!("{DATABASE_FILE}-wal"));
    assert!(
        !wal.exists() || std::fs::metadata(&wal).expect("wal metadata").len() == 0,
        "the stopped fixture leaves no WAL frames behind"
    );
    let restored = kontor_daemon::recovery::restore(directory.path(), &snapshot.snapshot, now)
        .expect("verified same-realm restore");
    assert!(restored.reconciliation_required);
    let store = kontor_store::SqliteStore::open(&directory.path().join(DATABASE_FILE))
        .expect("restored store");
    assert_eq!(
        export_realm(&store, now)
            .expect("offline restored export")
            .records_hash,
        exported.records_hash,
        "restore preserves exact exported records before startup reconciliation"
    );
    drop(store);
    fake.rebuild_adapter_state();
    let daemon = Daemon::start(
        DaemonConfig::at(directory.path()).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("restored daemon");
    assert_eq!(daemon.reconcile().await, BarrierState::Open);
    let router = daemon.router();
    let realm = Review {
        world: World {
            directory,
            daemon,
            router,
            fake,
            project: world_project,
            task,
            team_run,
        },
        project,
        project_id,
        epic,
        epic_key,
        task_key,
    };
    assert_eq!(
        census(&realm),
        before,
        "restore preserves all logical/native identities"
    );
    assert_eq!(list(&realm).await, report);
    for (body, key) in bodies
        .iter()
        .zip(["qualification-pr-101", "qualification-pr-102"])
    {
        let replay = open(&realm, body, key).await;
        assert_eq!(replay.status, 200, "{}", replay.body);
        assert_eq!(replay.json()["receipt"]["applied"], "unchanged");
    }
    let replay = Call::post(&reconcile_path, &reconcile_body)
        .signed_as(&realm.world, "operator")
        .with_key("qualification-reconcile")
        .send(&realm.world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(realm.world.fake.minted_natives(), minted);
    select(&realm, &deskless, "qualification-rollback").await;
    for desk in ["adam", "pr-review"] {
        let redriven = desk_call(
            &realm.world,
            &realm.project,
            "materialize",
            desk,
            &format!("qualification-rollback-{desk}"),
        )
        .await;
        assert_eq!(redriven.status, 200, "{}", redriven.body);
    }
    assert_eq!(
        census(&realm),
        before,
        "desk-less rollback keeps existing desk pins and identities"
    );
    assert_eq!(pin(&realm), old_pin);
    assert_eq!(realm.world.fake.minted_natives(), minted);
    select(&realm, &successor, "qualification-reselect").await;
    assert_eq!(census(&realm), before);
    let after = realm.world.daemon.state().with_store(|store| {
        store
            .get_team_definition(realm.project_id, old_pin.definition_id, old_pin.version)
            .expect("definition")
            .expect("old definition")
            .canonicalize()
            .expect("canonical")
    });
    assert_eq!(
        after, original_bytes,
        "old canonical bytes and hash unchanged"
    );
    let closed = close(&realm, MODULE_REPOSITORY, 101, "qualification-close").await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    assert_eq!(
        review_in(&list(&realm).await, MODULE_REPOSITORY, 102),
        second
    );
    let seats = Call::get(format!(
        "/v1/projects/{}/desks/pr-review/seats",
        realm.project
    ))
    .signed_as(&realm.world, "observer")
    .send(&realm.world)
    .await;
    assert_eq!(seats.json(), durable_report);
    assert_eq!(pin(&realm), old_pin);
    println!(
        "ASMA-8454 integrated evidence: {}",
        serde_json::json!({
        "runtime": "scripted_fake", "cleanup_owner": "ASMA-8454 TempDir drop",
        "snapshot_manifest": snapshot.manifest, "export_hash": exported.records_hash,
        "old_pin": old_pin, "old_definition_hash": original_bytes.hash(),
        "successor_hash": successor.canonicalize().expect("canonical").hash(),
        "qualification_definition": successor, "qualification_topology": topology.json()["document"],
        "durable_seats_readback": durable_report,
        "restored_census": before, "pr_readback": report})
    );
}
