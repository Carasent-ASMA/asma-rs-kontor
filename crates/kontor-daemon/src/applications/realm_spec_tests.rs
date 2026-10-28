//! Selected authoring, explicit installation and realm-local governing bytes.
use super::*;
use kontor_core::repository::NewProject;
use std::path::{Path, PathBuf};

use super::config_fixture as config;

fn write(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, serde_yaml_ng::to_string(value).unwrap()).unwrap();
}

fn sources(root: &Path, project: &str) -> PathBuf {
    let folder = config::write_config_folder(root);
    let fields = serde_json::json!({
        "schema_version":1,"connector":"connector.jira","project":project,
        "issue_type":"epic","version":1,"mappings":[{
            "key":"summary","owner":"kontor","direction":"outbound",
            "external":{"field_id":"summary","field_type":"text","encoding":"plain_text","options":[]},
            "required":false
        }]
    });
    let mut workflow: serde_json::Value = serde_json::from_str(include_str!(
        "../../../kontor-jira/tests/fixtures/external-workflow-asma-epic-v2.json"
    ))
    .unwrap();
    workflow["project"] = project.into();
    write(&folder.join("tracker/fields.yml"), &fields);
    write(&folder.join("tracker/workflow.yml"), &workflow);
    write(
        &folder.join("tracker/jira.yml"),
        &serde_json::json!({
            "schema_version":1,"kind":"jira","project_key":"EXM","specs":[
                {"kind":"ticket_fields","path":"tracker/fields.yml"},
                {"kind":"external_workflow","path":"tracker/workflow.yml"}
            ]
        }),
    );
    folder
}

fn start(root: &Path, folder: &Path) -> crate::Daemon {
    crate::Daemon::start(
        crate::DaemonConfig::at(root)
            .with_config_dir(folder)
            .with_port(0),
        kontor_api::state::RuntimeRegistry::new(),
    )
    .unwrap()
}

fn project(daemon: &crate::Daemon) -> ProjectId {
    let id = ProjectId::generate();
    daemon
        .state()
        .with_store(|store| {
            store.create_project(&NewProject {
                id,
                name: ExternalName::parse(&format!("Example Project {id}")).unwrap(),
                root_path: ExternalName::parse(&format!("/example/project/{id}")).unwrap(),
                created_at: kontor_api::now(),
            })
        })
        .unwrap();
    id
}

fn install(
    daemon: &crate::Daemon,
    id: ProjectId,
    external: &str,
    key: &str,
) -> Result<kontor_api::applications::InstalledWorkflowSpecDto, ApiError> {
    let revision = daemon
        .state()
        .with_store(|store| store.get_project(id))
        .unwrap()
        .map_or(AggregateRevision::INITIAL, |project| project.revision);
    daemon
        .state()
        .applications()
        .install_connector_workflow_spec(
            &IdempotencyKey::parse(key).unwrap(),
            id,
            "jira",
            &kontor_api::applications::InstallWorkflowSpecRequest {
                external_project: external.to_owned(),
                issue_type: "epic".to_owned(),
                version: SpecVersion::parse(2).unwrap(),
                expected_revision: revision,
            },
        )
}

#[test]
fn two_realms_offer_only_their_selected_specs_and_install_both_with_readback() {
    for external in ["example-alpha", "example-beta"] {
        let root = tempfile::tempdir().unwrap();
        let folder = sources(root.path(), external);
        let daemon = start(&root.path().join("state"), &folder);
        assert!(
            daemon
                .state()
                .with_store(|store| store.list_projects())
                .unwrap()
                .is_empty()
        );
        let id = project(&daemon);
        let offered = daemon
            .state()
            .applications()
            .connector_workflow_specs(id, "jira")
            .unwrap();
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].external_project, external);
        assert!(!offered[0].installed);
        assert!(
            daemon
                .jira_reconciler()
                .jira_specs_for(id, "epic", None)
                .is_err()
        );
        assert!(install(&daemon, id, "asma", "foreign-project").is_err());
        assert!(install(&daemon, ProjectId::generate(), external, "unknown-project").is_err());
        let receipt = install(&daemon, id, external, "own-project").unwrap();
        assert_eq!(receipt.receipt.applied, AppliedDto::Created);
        let fields = daemon
            .state()
            .applications()
            .connector_field_specs(id, "connector.jira")
            .unwrap();
        assert_eq!(fields.len(), 1);
        assert!(fields[0].installed);
        let (fields, workflow) = daemon
            .jira_reconciler()
            .jira_specs_for(id, "epic", None)
            .unwrap();
        assert_eq!(fields.spec().project.as_str(), external);
        assert_eq!(workflow.spec().project.as_str(), external);
        let other = project(&daemon);
        assert!(
            daemon
                .jira_reconciler()
                .jira_specs_for(other, "epic", None)
                .is_err()
        );
    }
}

#[test]
fn uninstalled_edits_and_restart_keep_installed_bytes_and_conflicting_revision_refuses() {
    let root = tempfile::tempdir().unwrap();
    let folder = sources(root.path(), "example");
    let state = root.path().join("state");
    let daemon = start(&state, &folder);
    let id = project(&daemon);
    install(&daemon, id, "example", "initial-install").unwrap();
    let original = daemon
        .jira_reconciler()
        .jira_specs_for(id, "epic", None)
        .unwrap();
    let path = folder.join("tracker/fields.yml");
    let mut fields: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    fields["mappings"][0]["external"]["field_id"] = "changed_summary".into();
    write(&path, &fields);
    // This process has no raw-YAML reader.
    assert_eq!(
        daemon
            .jira_reconciler()
            .jira_specs_for(id, "epic", None)
            .unwrap(),
        original
    );
    drop(daemon);
    let daemon = start(&state, &folder);
    // Startup advertises the edit, but never activates it.
    assert_eq!(
        daemon
            .jira_reconciler()
            .jira_specs_for(id, "epic", None)
            .unwrap(),
        original
    );
    assert!(install(&daemon, id, "example", "conflicting-install").is_err());
    assert_eq!(
        daemon
            .jira_reconciler()
            .jira_specs_for(id, "epic", None)
            .unwrap(),
        original
    );
    drop(daemon);
    write(
        &folder.join("tracker/jira.yml"),
        &serde_json::json!({"schema_version":1,"kind":"jira","project_key":"EXM","specs":[]}),
    );
    let daemon = start(&state, &folder);
    assert!(
        daemon
            .state()
            .applications()
            .connector_workflow_specs(id, "jira")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        daemon
            .jira_reconciler()
            .jira_specs_for(id, "epic", None)
            .unwrap(),
        original
    );
}

#[test]
fn undeclared_specs_never_fall_back_to_a_compiled_project() {
    let root = tempfile::tempdir().unwrap();
    let folder = config::write_config_folder(root.path());
    let daemon = start(&root.path().join("state"), &folder);
    let id = project(&daemon);
    assert!(
        daemon
            .state()
            .applications()
            .connector_field_specs(id, "connector.jira")
            .unwrap()
            .is_empty()
    );
    assert!(
        daemon
            .state()
            .applications()
            .connector_workflow_specs(id, "jira")
            .unwrap()
            .is_empty()
    );
    assert!(install(&daemon, id, "asma", "no-fallback").is_err());
    assert!(
        daemon
            .jira_reconciler()
            .jira_specs_for(id, "epic", None)
            .is_err()
    );
}

#[test]
fn a_field_conflict_rolls_back_the_new_workflow_and_its_authority_receipt() {
    let root = tempfile::tempdir().unwrap();
    let folder = sources(root.path(), "example");
    let state = root.path().join("state");
    let daemon = start(&state, &folder);
    let id = project(&daemon);
    install(&daemon, id, "example", "first-install").unwrap();
    let revision = daemon
        .state()
        .with_store(|store| store.get_project(id))
        .unwrap()
        .unwrap()
        .revision;
    drop(daemon);
    let path = folder.join("tracker/fields.yml");
    let mut fields: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    fields["mappings"][0]["external"]["field_id"] = "changed_summary".into();
    write(&path, &fields);
    let path = folder.join("tracker/workflow.yml");
    let mut workflow: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    workflow["version"] = 3.into();
    write(&path, &workflow);
    let daemon = start(&state, &folder);
    let key = IdempotencyKey::parse("rejected-pair").unwrap();
    let error = daemon
        .state()
        .applications()
        .install_connector_workflow_spec(
            &key,
            id,
            "jira",
            &kontor_api::applications::InstallWorkflowSpecRequest {
                external_project: "example".to_owned(),
                issue_type: "epic".to_owned(),
                version: SpecVersion::parse(3).unwrap(),
                expected_revision: revision,
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ApiErrorCode::RevisionConflict);
    daemon.state().with_store(|store| {
        assert_eq!(store.get_project(id).unwrap().unwrap().revision, revision);
        let workflows = store.installed_external_workflow_specs(id).unwrap();
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].version, SpecVersion::parse(2).unwrap());
        assert!(store.get_receipt_by_key(&key).unwrap().is_none());
    });
}

#[test]
fn malformed_missing_duplicate_and_cross_project_specs_refuse_by_selected_source() {
    let root = tempfile::tempdir().unwrap();
    let folder = sources(root.path(), "example");
    let manifest = folder.join("tracker/jira.yml");
    let original: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    for (reference, reason) in [
        (
            serde_json::json!({"kind":"ticket_fields","path":"tracker/missing.yml"}),
            "missing",
        ),
        (
            serde_json::json!({"kind":"ticket_fields","path":"../escape.yml"}),
            "traversal",
        ),
        (
            serde_json::json!({"kind":"ticket_fields","path":"tracker/fields.json"}),
            "YAML",
        ),
        (
            serde_json::json!({"kind":"unknown","path":"tracker/fields.yml"}),
            "invalid",
        ),
        (
            serde_json::json!({"kind":"ticket_fields","path":"tracker/fields.yml"}),
            "duplicate",
        ),
    ] {
        let mut value = original.clone();
        value["specs"].as_array_mut().unwrap().push(reference);
        write(&manifest, &value);
        let error = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap_err();
        assert!(error.path.starts_with(folder.canonicalize().unwrap()));
        assert!(error.to_string().contains(reason), "{error}");
    }
    write(&manifest, &original);
    let workflow = folder.join("tracker/workflow.yml");
    let mut value: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&workflow).unwrap()).unwrap();
    value["project"] = "foreign-project".into();
    write(&workflow, &value);
    let error = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap_err();
    assert_eq!(error.path, workflow.canonicalize().unwrap());
    assert!(error.problem.contains("conflicting external projects"));
    std::fs::write(&workflow, "private-canary: [broken").unwrap();
    let error = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap_err();
    assert!(!error.to_string().contains("private-canary"));
}

#[test]
fn all_six_portable_tracker_conversions_keep_canonical_bytes_and_hashes() {
    let root = tempfile::tempdir().unwrap();
    let folder = config::write_config_folder(root.path());
    let path = folder.join(crate::config_folder::ENTRY);
    let mut entry: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    entry["project"]["key"] = "ASMA".into();
    entry["project"]["tracker"]["project_key"] = "ASMA".into();
    write(&path, &entry);
    let sources = [
        (
            "ticket_fields",
            "ticket-fields-asma",
            include_str!("../../../kontor-jira/tests/fixtures/ticket-fields-asma.json"),
        ),
        (
            "ticket_fields",
            "ticket-fields-asma-epic",
            include_str!("../../../kontor-jira/tests/fixtures/ticket-fields-asma-epic.json"),
        ),
        (
            "external_workflow",
            "external-workflow-asma",
            include_str!("../../../kontor-jira/tests/fixtures/external-workflow-asma.json"),
        ),
        (
            "external_workflow",
            "external-workflow-asma-high-stakes",
            include_str!(
                "../../../kontor-jira/tests/fixtures/external-workflow-asma-high-stakes.json"
            ),
        ),
        (
            "external_workflow",
            "external-workflow-asma-docs",
            include_str!("../../../kontor-jira/tests/fixtures/external-workflow-asma-docs.json"),
        ),
        (
            "external_workflow",
            "external-workflow-asma-epic-v2",
            include_str!("../../../kontor-jira/tests/fixtures/external-workflow-asma-epic-v2.json"),
        ),
    ];
    // The same assertion can read the separately owned authored overlay, rather
    // than a generated test copy, without baking a workstation path into tests.
    let overlay = std::env::var_os("KONTOR_SPEC_EQUIVALENCE_FOLDER").map(PathBuf::from);
    let mut expected = SpecCatalog::empty();
    let mut references = Vec::new();
    for (kind, name, json) in sources {
        let relative = format!("tracker/{name}.yml");
        if let Some(overlay) = &overlay {
            std::fs::copy(overlay.join(&relative), folder.join(&relative)).unwrap();
        } else {
            write(
                &folder.join(&relative),
                &serde_json::from_str(json).unwrap(),
            );
        }
        references.push(serde_json::json!({"kind":kind,"path":relative}));
        if kind == "ticket_fields" {
            expected.load_field_spec(json).unwrap();
        } else {
            expected.load_workflow_spec(json).unwrap();
        }
    }
    let manifest = serde_json::json!({"schema_version":1,"kind":"jira","project_key":"ASMA","specs":references});
    if let Some(overlay) = overlay {
        std::fs::copy(
            overlay.join("tracker/jira.yml"),
            folder.join("tracker/jira.yml"),
        )
        .unwrap();
    } else {
        write(&folder.join("tracker/jira.yml"), &manifest);
    }
    let selected = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap();
    assert_eq!(selected.tracker_specs, expected);
    assert_eq!(selected.tracker_specs.field_specs().len(), 2);
    assert_eq!(selected.tracker_specs.workflow_specs().len(), 4);
}
