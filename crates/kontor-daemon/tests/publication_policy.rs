//! Project publication authority from selected folders and activated artifacts only.

#[path = "support/config.rs"]
mod support;

use std::path::PathBuf;

use kontor_api::applications::{PublicationDecisionDto, PublicationIdentityRequest};
use kontor_api::error::ApiErrorCode;
use kontor_core::id::{
    ConnectorKey, ContentHash, ExternalId, ExternalName, IdempotencyKey, MiniProjectId, ModuleKey,
    ProjectId, TaskId, TicketLinkId,
};
use kontor_core::repository::{
    NewMiniProject, NewProject, NewTask, NewTicketLink, ProjectRepository, TicketRepository,
};
use kontor_core::state::TaskState;
use kontor_daemon::config_folder::{ConfigFolder, ENTRY};
use kontor_daemon::fleet::{ActivationFence, FleetSource};
use kontor_daemon::orchestration::{BundleSources, resolve_bundle};
use kontor_daemon::{Daemon, DaemonConfig};
use kontor_store::EpicExecutionScopeDeclaration;

struct Realm {
    directory: tempfile::TempDir,
    folder: PathBuf,
    daemon: Daemon,
    project: ProjectId,
    key: String,
    root_repository: String,
    base: String,
}

impl Realm {
    fn new(key: &str, owner: &str, base: &str, modules: bool) -> Self {
        Self::with_id(key, owner, base, modules, ProjectId::generate(), true)
    }

    fn with_id(
        key: &str,
        owner: &str,
        base: &str,
        modules: bool,
        project: ProjectId,
        configured: bool,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let folder = support::write_config_folder(directory.path());
        let path = folder.join(ENTRY);
        let mut entry: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let root_repository = format!("{owner}/root");
        entry["project"]["key"] = key.into();
        entry["project"]["tracker"]["project_key"] = key.into();
        entry["project"]["resource_roots"]["project"] = root_repository.clone().into();
        entry["project"]["forge"]["repositories"] = serde_yaml_ng::to_value(vec![
            root_repository.clone(),
            format!("{owner}/worker"),
            format!("{owner}/sibling"),
            "listed-only/unowned".to_owned(),
        ])
        .unwrap();
        entry["project"]["forge"]["base_branch"] = base.into();
        let fields = entry["project"].as_mapping_mut().unwrap();
        fields.remove("project_id");
        fields.remove("publication");
        if configured {
            fields.insert(
                "project_id".into(),
                serde_yaml_ng::to_value(project).unwrap(),
            );
            fields.insert(
                "publication".into(),
                serde_yaml_ng::to_value(serde_json::json!({
                    "root_repository": root_repository,
                    "module_repositories": if modules { serde_json::json!({
                        "components/worker": format!("{owner}/worker"),
                        "components/sibling": format!("{owner}/sibling"),
                    }) } else { serde_json::json!({}) },
                }))
                .unwrap(),
            );
        }
        std::fs::write(&path, serde_yaml_ng::to_string(&entry).unwrap()).unwrap();
        support::write_runtime_planes(
            &folder,
            &serde_json::json!({"schema_version":5,"runtimes":[]}),
        );
        let catalog = kontor_profiles::bundled_operational_domain()
            .unwrap()
            .role_catalogs
            .remove(0);
        std::fs::write(
            folder.join("orchestration/teams/core-team.yml"),
            kontor_daemon::orchestration::propose_core_team(&catalog).unwrap(),
        )
        .unwrap();
        let selected = ConfigFolder::select(Some(&folder), None).unwrap();
        assert_eq!(selected.project.publication_policy.is_some(), configured);
        let daemon = Daemon::start_configured(
            DaemonConfig::at(directory.path().join("state"))
                .with_config_dir(&folder)
                .with_port(0),
        )
        .unwrap();
        assert!(
            daemon
                .state()
                .with_store(|store| store.list_projects())
                .unwrap()
                .is_empty()
        );
        let realm = Self {
            directory,
            folder,
            daemon,
            project,
            key: key.to_owned(),
            root_repository,
            base: base.to_owned(),
        };
        realm.seed_graph(project, key, "own");
        realm
    }

    /// Synthetic externally-held graph facts through the same repository ports.
    fn seed_graph(&self, project: ProjectId, key: &str, root: &str) {
        self.daemon.state().with_store(|store| {
            let now = kontor_api::now();
            if store.get_project(project).unwrap().is_none() {
                store
                    .create_project(&NewProject {
                        id: project,
                        name: ExternalName::parse(&format!("{key} project")).unwrap(),
                        root_path: ExternalName::parse(
                            &self.directory.path().join(root).to_string_lossy(),
                        )
                        .unwrap(),
                        created_at: now,
                    })
                    .unwrap();
            }
            let epic = MiniProjectId::generate();
            store
                .create_mini_project(&NewMiniProject {
                    id: epic,
                    project_id: project,
                    name: ExternalName::parse(&format!("{key} publication fixture")).unwrap(),
                    created_at: now,
                })
                .unwrap();
            store
                .declare_epic_execution_scope(
                    project,
                    epic,
                    &EpicExecutionScopeDeclaration {
                        external_epic_key: ExternalId::parse(&format!("{key}-1")).unwrap(),
                        short_title: ExternalName::parse("Publication fixture").unwrap(),
                        kontor_backlog_code: None,
                        ai_short_name: None,
                    },
                    now,
                )
                .unwrap();
            for (index, module) in [
                Some("components/worker"),
                Some("components/sibling"),
                Some("components/worker-extra"),
                None,
            ]
            .into_iter()
            .enumerate()
            {
                let task = TaskId::generate();
                store
                    .create_task(&NewTask {
                        id: task,
                        project_id: project,
                        mini_project_id: Some(epic),
                        title: ExternalName::parse(&format!("Work {index}")).unwrap(),
                        module: module.map(|value| ModuleKey::parse(value).unwrap()),
                        state: TaskState::Ready,
                        created_at: now,
                    })
                    .unwrap();
                store
                    .create_ticket_link(&NewTicketLink {
                        id: TicketLinkId::generate(),
                        project_id: project,
                        task_id: task,
                        connector: ConnectorKey::parse("connector.jira").unwrap(),
                        external_issue_key: ExternalId::parse(&format!("{key}-{}", index + 2))
                            .unwrap(),
                        created_at: now,
                    })
                    .unwrap();
            }
        });
    }

    fn publish(&self, activate: bool) -> ContentHash {
        let orchestration = std::fs::read_to_string(self.folder.join(ENTRY)).unwrap();
        let fleet = std::fs::read_to_string(self.folder.join("orchestration/fleet.yml")).unwrap();
        let team =
            std::fs::read_to_string(self.folder.join("orchestration/teams/core-team.yml")).unwrap();
        let catalog = kontor_profiles::bundled_operational_domain()
            .unwrap()
            .role_catalogs
            .remove(0);
        let bundle = resolve_bundle(
            &BundleSources {
                orchestration: &orchestration,
                fleet: &fleet,
                core_team: &team,
            },
            &catalog,
        )
        .unwrap();
        let source = FleetSource::at(&self.daemon.config().state_root);
        let published = source.publish_bundle(&bundle).unwrap();
        if activate {
            let standing =
                kontor_fleet_activation::read_activation(&self.daemon.config().state_root).ok();
            source
                .activate_bundle(
                    &published.bundle_hash,
                    &ActivationFence {
                        policy_hash: standing.as_ref().map(|record| record.policy_hash.clone()),
                        source_bundle_hash: standing.and_then(|record| record.source_bundle_hash),
                    },
                )
                .unwrap();
        }
        published.bundle_hash
    }

    fn request(&self, key: &str, repository: &str, base: &str) -> PublicationIdentityRequest {
        PublicationIdentityRequest {
            repository: ExternalName::parse(repository).unwrap(),
            base_branch: ExternalName::parse(base).unwrap(),
            head_branch: ExternalName::parse(&format!("feat/{key}-work")).unwrap(),
            head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            pull_request: Some(1),
            title: Some(ExternalName::parse(&format!("{key} Work")).unwrap()),
        }
    }

    async fn preview(&self, key: &str, repository: &str, base: &str) -> PublicationDecisionDto {
        self.daemon
            .state()
            .applications()
            .preview_publication(self.project, &self.request(key, repository, base))
            .await
            .unwrap()
    }

    fn restart(self) -> Self {
        let Self {
            directory,
            folder,
            daemon,
            project,
            key,
            root_repository,
            base,
        } = self;
        daemon.shutdown();
        let daemon = Daemon::start_configured(
            DaemonConfig::at(directory.path().join("state"))
                .with_config_dir(&folder)
                .with_port(0),
        )
        .unwrap();
        Self {
            directory,
            folder,
            daemon,
            project,
            key,
            root_repository,
            base,
        }
    }
}

#[tokio::test]
async fn an_activated_policy_cannot_override_a_selected_tracker_consumer() {
    let realm = Realm::new("EXM", "example-org", "main", false);
    let tracker = realm.folder.join("tracker/jira.yml");
    let mut document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&tracker).unwrap()).unwrap();
    document["project_id"] = serde_yaml_ng::to_value(realm.project).unwrap();
    std::fs::write(tracker, serde_yaml_ng::to_string(&document).unwrap()).unwrap();
    let realm = realm.restart();
    let other = ProjectId::generate();
    realm.seed_graph(other, "ALT", "other-consumer");
    let path = realm.folder.join(ENTRY);
    let mut entry: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    entry["project"]["project_id"] = serde_yaml_ng::to_value(other).unwrap();
    std::fs::write(path, serde_yaml_ng::to_string(&entry).unwrap()).unwrap();
    // A verified prior artifact can exist independently of current consumer
    // bindings. The normal activation writer does not grant integration authority.
    let hash = realm.publish(true);
    let result = realm
        .daemon
        .state()
        .applications()
        .preview_publication(other, &realm.request("ALT-2", "example-org/root", "main"))
        .await
        .unwrap();
    assert_eq!(result.reasons, ["publication_consumer_mismatch"]);
    assert_eq!(result.source_bundle_hash, Some(hash));
}

#[tokio::test]
async fn two_selected_folders_authorize_only_their_own_stored_project_and_module_scope() {
    let a = Realm::new("EXM", "example-org", "main", true);
    let b = Realm::new("ALT", "alternate-org", "develop", false);
    assert_ne!(a.daemon.realm_id(), b.daemon.realm_id());
    let hash_a = a.publish(true);
    let hash_b = b.publish(true);
    for (key, repository, allowed) in [
        ("EXM-2", "example-org/root", true),
        ("EXM-2", "example-org/worker", true),
        ("EXM-2", "example-org/sibling", false),
        ("EXM-4", "example-org/worker", false),
        ("EXM-5", "example-org/worker", false),
        ("EXM-1", "example-org/sibling", true),
        ("EXM-1", "listed-only/unowned", false),
        ("EXM-1", "alternate-org/root", false),
    ] {
        let result = a.preview(key, repository, "main").await;
        assert_eq!(result.accepted, allowed, "{key}, {repository}: {result:?}");
        assert_eq!(result.source_bundle_hash.as_ref(), Some(&hash_a));
        if !allowed {
            assert_eq!(result.reasons, ["repository_mismatch"]);
        }
    }
    assert_eq!(
        a.preview("EXM-2", "example-org/root", "master")
            .await
            .reasons,
        ["base_branch_not_default"]
    );
    assert_eq!(
        a.preview("EXM-999", "example-org/root", "main")
            .await
            .reasons,
        ["binding_unconfirmed"]
    );
    assert_eq!(
        b.preview("ALT-2", "alternate-org/worker", "develop")
            .await
            .reasons,
        ["repository_mismatch"]
    );
    let own_b = b.preview("ALT-2", "alternate-org/root", "develop").await;
    assert!(own_b.accepted);
    assert_eq!(own_b.source_bundle_hash.as_ref(), Some(&hash_b));
    assert_eq!(
        b.preview("ALT-2", "example-org/root", "develop")
            .await
            .reasons,
        ["repository_mismatch"]
    );
    let foreign = a
        .daemon
        .state()
        .applications()
        .preview_publication(
            b.project,
            &b.request("ALT-2", "alternate-org/root", "develop"),
        )
        .await
        .unwrap_err();
    assert_eq!(foreign.code, ApiErrorCode::NotFound);
    a.seed_graph(b.project, "ALT", "foreign-row");
    let wrong_id = a
        .daemon
        .state()
        .applications()
        .preview_publication(b.project, &a.request("ALT-2", "example-org/root", "main"))
        .await
        .unwrap();
    assert_eq!(wrong_id.reasons, ["publication_project_mismatch"]);
}

#[tokio::test]
async fn unactivated_edits_publications_and_restart_preserve_authority_and_receipt_basis() {
    let realm = Realm::new("EXM", "example-org", "main", true);
    let first = realm.publish(true);
    let key = IdempotencyKey::parse("frozen-publication").unwrap();
    let request = realm.request("EXM-2", "example-org/worker", "main");
    let receipt = realm
        .daemon
        .state()
        .applications()
        .attest_publication(&key, realm.project, &request)
        .await
        .unwrap();
    assert!(receipt.accepted);
    assert_eq!(receipt.source_bundle_hash.as_ref(), Some(&first));
    let path = realm.folder.join(ENTRY);
    let mut entry: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    entry["project"]["publication"]["module_repositories"]["components/worker"] =
        "example-org/sibling".into();
    std::fs::write(&path, serde_yaml_ng::to_string(&entry).unwrap()).unwrap();
    let candidate = realm.publish(false);
    assert_ne!(candidate, first);
    let realm = realm.restart();
    assert!(
        realm
            .preview("EXM-2", "example-org/worker", "main")
            .await
            .accepted
    );
    assert!(
        !realm
            .preview("EXM-2", "example-org/sibling", "main")
            .await
            .accepted
    );
    let next = realm.publish(true);
    assert_eq!(next, candidate);
    assert!(
        !realm
            .preview("EXM-2", "example-org/worker", "main")
            .await
            .accepted
    );
    assert!(
        realm
            .preview("EXM-2", "example-org/sibling", "main")
            .await
            .accepted
    );
    let moved = realm
        .daemon
        .state()
        .applications()
        .attest_publication(&key, realm.project, &request)
        .await
        .unwrap_err();
    assert_eq!(moved.code, ApiErrorCode::RevisionConflict);
    let old = realm
        .daemon
        .state()
        .applications()
        .publication_attestation(realm.project, receipt.attestation_id.unwrap())
        .unwrap();
    assert!(old.accepted);
    assert_eq!(old.source_bundle_hash.as_ref(), Some(&first));
    std::fs::write(
        kontor_fleet_activation::manifest_path(&realm.daemon.config().state_root, &next),
        "corrupt",
    )
    .unwrap();
    assert_eq!(
        realm
            .preview("EXM-2", "example-org/sibling", "main")
            .await
            .reasons,
        ["publication_policy_invalid"]
    );
}

#[tokio::test]
async fn selected_branch_narrowing_changes_only_with_the_activated_policy() {
    let realm = Realm::new("EXM", "example-org", "trunk", true);
    let first = realm.publish(true);
    assert!(
        realm
            .preview("EXM-2", "example-org/worker", "trunk")
            .await
            .accepted
    );
    let path = realm.folder.join(ENTRY);
    let mut entry: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    entry["project"]["branch"]["types"] = serde_yaml_ng::to_value(vec!["fix"]).unwrap();
    entry["project"]["branch"]["pattern"] = "^(fix)/<KEY>-<number>-<slug>$".into();
    std::fs::write(&path, serde_yaml_ng::to_string(&entry).unwrap()).unwrap();
    let candidate = realm.publish(false);
    assert_ne!(candidate, first);
    let realm = realm.restart();
    assert!(
        realm
            .preview("EXM-2", "example-org/worker", "trunk")
            .await
            .accepted
    );
    assert_eq!(realm.publish(true), candidate);
    let refused = realm.preview("EXM-2", "example-org/worker", "trunk").await;
    assert_eq!(refused.reasons, ["branch_type_not_allowed"]);
    assert_eq!(refused.source_bundle_hash.as_ref(), Some(&candidate));
    let mut request = realm.request("EXM-2", "example-org/worker", "trunk");
    request.head_branch = ExternalName::parse("fix/EXM-2-work").unwrap();
    assert!(
        realm
            .daemon
            .state()
            .applications()
            .preview_publication(realm.project, &request)
            .await
            .unwrap()
            .accepted
    );
    // Even a confirmed foreign key stored in this project cannot widen its grammar.
    realm.seed_graph(realm.project, "OTHER", "own");
    request.head_branch = ExternalName::parse("fix/OTHER-2-work").unwrap();
    request.title = Some(ExternalName::parse("OTHER-2 Work").unwrap());
    assert_eq!(
        realm
            .daemon
            .state()
            .applications()
            .preview_publication(realm.project, &request)
            .await
            .unwrap()
            .reasons,
        ["branch_project_mismatch"]
    );
}

#[tokio::test]
async fn no_activated_policy_means_no_authority_even_for_the_historic_project_mapping() {
    // The historic public regression fixture, never a runtime identity default.
    let historic = ProjectId::parse("01a0064a-e056-7603-9968-ef64fdaacb75").unwrap();
    let realm = Realm::with_id("ASMA", "Carasent-ASMA", "master", false, historic, false);
    let request = realm.request("ASMA-2", "Carasent-ASMA/asma-modules", "master");
    let result = realm
        .daemon
        .state()
        .applications()
        .preview_publication(historic, &request)
        .await
        .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.reasons, ["publication_policy_unconfigured"]);
    realm.publish(true);
    let result = realm
        .daemon
        .state()
        .applications()
        .preview_publication(historic, &request)
        .await
        .unwrap();
    assert_eq!(result.reasons, ["publication_policy_unconfigured"]);
}
