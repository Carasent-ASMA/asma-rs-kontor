//! A renamed command reuses frozen receipt authority without rewriting history.
use super::*;
use kontor_core::repository::{NewMiniProject, NewProject};

#[test]
fn neutral_activation_replay_preserves_history_and_refuses_argument_substitution() {
    let root = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(root.path());
    let daemon = crate::Daemon::start(
        crate::DaemonConfig::at(root.path().join("state"))
            .with_config_dir(folder)
            .with_port(0),
        kontor_api::state::RuntimeRegistry::new(),
    )
    .unwrap();
    let services = daemon.jira_reconciler();
    let project = ProjectId::generate();
    let epic = MiniProjectId::generate();
    let at = kontor_api::now();
    daemon.state().with_store(|store| {
        store
            .create_project(&NewProject {
                id: project,
                name: ExternalName::parse("Example").unwrap(),
                root_path: ExternalName::parse("/example").unwrap(),
                created_at: at,
            })
            .unwrap();
        store
            .create_mini_project(&NewMiniProject {
                id: epic,
                project_id: project,
                name: ExternalName::parse("Example epic").unwrap(),
                created_at: at,
            })
            .unwrap();
    });
    let target = AggregateRef::MiniProject {
        mini_project_id: epic,
    };
    let key = IdempotencyKey::parse("renamed-activation").unwrap();
    let mut value = serde_json::json!({"schema_version":1,"operation":"activate_epic","project_id":project,"epic_id":epic,"materialization_receipt_id":CommandReceiptId::generate()});
    let expected = services.intent(&value).unwrap();
    assert_eq!(
        services
            .epic_activation_intent(&key, &target, expected.clone())
            .unwrap(),
        expected
    );
    value["operation"] = "previous_activation_label".into();
    let frozen = services.intent(&value).unwrap();
    let receipt = services
        .record(
            &key,
            project,
            CommandKind::ActivateEpic,
            target,
            AggregateRevision::INITIAL,
            &frozen,
        )
        .unwrap();
    let replay = services
        .epic_activation_intent(&key, &target, expected.clone())
        .unwrap();
    assert_eq!(replay, frozen);
    assert_ne!(replay.hash(), expected.hash());
    assert_eq!(
        services
            .record(
                &key,
                project,
                CommandKind::ActivateEpic,
                target,
                AggregateRevision::INITIAL,
                &replay
            )
            .unwrap(),
        receipt
    );
    for field in ["project_id", "epic_id", "materialization_receipt_id"] {
        let mut changed: serde_json::Value = expected.deserialize().unwrap();
        changed[field] = serde_json::json!(CommandReceiptId::generate());
        assert_eq!(
            services
                .epic_activation_intent(&key, &target, services.intent(&changed).unwrap())
                .unwrap_err()
                .body()
                .code,
            ApiErrorCode::IdempotencyConflict
        );
    }
    let foreign = AggregateRef::MiniProject {
        mini_project_id: MiniProjectId::generate(),
    };
    assert!(
        services
            .epic_activation_intent(&key, &foreign, expected)
            .is_err()
    );
    let saved = daemon
        .state()
        .with_store(|store| store.get_receipt_by_key(&key))
        .unwrap()
        .unwrap();
    assert_eq!(saved.intent, frozen);
    assert_eq!(saved.kind, CommandKind::ActivateEpic);
}
