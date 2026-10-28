//! Accepted-loader integration with disposable, neutral selected folders.
//! These are preview tests, not live-folder or authenticated-write acceptance.

#[allow(dead_code)]
#[path = "support/config.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::Path;

use kontor_core::id::{ContentHash, ExternalId, RoleSlotId, parse_utc_timestamp};
use kontor_core::naming::{
    NameSeparator, NativeNameSegment, NativeNameTemplate, NativeNameToken, NativeNameValues,
};
use kontor_daemon::composition_snapshot::{
    CompositionSubject, SelectedCompositionInput, preview_selected_composition,
};
use kontor_daemon::config_folder::{ConfigFolder, ENTRY};
use kontor_profiles::pack::PackCategoryKey;
use serde_json::{Value, json};

fn selected_folder(root: &Path) -> std::path::PathBuf {
    let folder = support::write_config_folder(root);
    // The neutral fleet and profile examples demonstrate independent schemas.
    // Join the synthetic builder slot explicitly; production must never alias it.
    let path = folder.join("orchestration/fleet.yml");
    let fleet = std::fs::read_to_string(&path)
        .unwrap()
        .replace("/implement", "/builder");
    std::fs::write(path, fleet).unwrap();
    folder
}

fn input() -> SelectedCompositionInput {
    SelectedCompositionInput {
        subject: CompositionSubject {
            project: ExternalId::parse("synthetic-native-project").unwrap(),
            workspace: ExternalId::parse("synthetic-native-workspace").unwrap(),
            parent: Some(ExternalId::parse("synthetic-native-parent").unwrap()),
            slot: RoleSlotId::parse("builder").unwrap(),
        },
        category: PackCategoryKey::parse("code").unwrap(),
        template: NativeNameTemplate::from_segments(vec![
            NativeNameSegment::Token(NativeNameToken::RoleCode),
            NativeNameSegment::Token(NativeNameToken::Topic),
        ])
        .unwrap(),
        separator: NameSeparator::parse(" · ").unwrap(),
        values: NativeNameValues::new()
            .with_role_code("CALLER-ROLE")
            .with_topic("Synthetic topic"),
        eligibility: Default::default(),
        resolved_at: parse_utc_timestamp("2026-01-01T00:00:00Z").unwrap(),
    }
}

fn preview(folder: &Path, request: &SelectedCompositionInput) -> Value {
    preview_selected_composition(Some(folder), None, request)
        .unwrap()
        .document()
        .deserialize()
        .unwrap()
}

fn edit_yaml(path: &Path, edit: impl FnOnce(&mut serde_yaml_ng::Value)) {
    let mut value = serde_yaml_ng::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    edit(&mut value);
    std::fs::write(path, serde_yaml_ng::to_string(&value).unwrap()).unwrap();
}

fn files(root: &Path) -> BTreeMap<std::path::PathBuf, ContentHash> {
    fn visit(root: &Path, path: &Path, hashes: &mut BTreeMap<std::path::PathBuf, ContentHash>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, hashes);
            } else {
                hashes.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    ContentHash::of(&std::fs::read(&path).unwrap()),
                );
            }
        }
    }
    let mut hashes = BTreeMap::new();
    visit(root, root, &mut hashes);
    hashes
}

#[test]
fn accepted_loader_resolves_slot_role_fleet_and_exact_pack_pins() {
    let root = tempfile::tempdir().unwrap();
    let folder = selected_folder(root.path());
    let before = files(root.path());
    let request = input();
    let first = preview_selected_composition(Some(&folder), None, &request).unwrap();
    let second = preview_selected_composition(Some(&folder), None, &request).unwrap();
    assert_eq!(first, second);
    let value: Value = first.document().deserialize().unwrap();
    assert_eq!(value["naming"]["title"], "SWE · Synthetic topic");
    assert_eq!(value["slot"]["id"], "builder");
    assert_eq!(value["slot"]["role"]["role"], "builder");
    assert_eq!(
        value["fleet_binding"],
        "team/01936f5a-0000-7000-8000-000000000001/builder"
    );
    assert!(!value["fleet"]["selected"].is_null());
    assert_eq!(
        value["attachments"]["skills"]["references"],
        value["slot"]["skills"]
    );
    assert_eq!(
        value["attachments"]["persona"],
        json!({"state":"not_configured", "role":"SWE"})
    );
    let pins = value["capture"]["pack_sources"].as_object().unwrap();
    assert_eq!(pins.len(), 4);
    for (path, hash) in pins {
        assert_eq!(
            *hash,
            json!(ContentHash::of(&std::fs::read(folder.join(path)).unwrap()))
        );
    }
    assert_eq!(
        files(root.path()),
        before,
        "preview must not alter or create files"
    );
}

#[test]
fn source_bytes_and_fleet_are_pinned_without_claiming_atomic_capture() {
    let root = tempfile::tempdir().unwrap();
    let folder = selected_folder(root.path());
    let original = preview_selected_composition(Some(&folder), None, &input()).unwrap();
    let saved = original.document().clone();
    let domain = folder.join("packs/operational-domain.yml");
    let text = std::fs::read_to_string(&domain).unwrap();
    std::fs::write(
        &domain,
        format!("# Changed source bytes, identical semantics\n{text}"),
    )
    .unwrap();
    let changed = preview_selected_composition(Some(&folder), None, &input()).unwrap();
    assert_ne!(original.hash(), changed.hash());
    let fleet_path = folder.join("orchestration/fleet.yml");
    let fleet = format!(
        "# Changed fleet bytes\n{}",
        std::fs::read_to_string(&fleet_path).unwrap()
    );
    std::fs::write(&fleet_path, &fleet).unwrap();
    let newer = preview_selected_composition(Some(&folder), None, &input()).unwrap();
    assert_ne!(changed.hash(), newer.hash());
    assert_eq!(original.document(), &saved);
    let value: Value = newer.document().deserialize().unwrap();
    assert_eq!(
        value["capture"]["fleet_bytes"],
        json!(ContentHash::of(fleet.as_bytes()))
    );
    assert_eq!(value["capture"]["atomic_folder_generation"], false);
    assert!(
        value["unverified"]
            .as_array()
            .unwrap()
            .contains(&json!("atomic_selected_folder_generation"))
    );
}

#[test]
fn persona_comes_from_selected_role_and_never_exposes_prompt_text() {
    let root = tempfile::tempdir().unwrap();
    let folder = selected_folder(root.path());
    let original = preview_selected_composition(Some(&folder), None, &input()).unwrap();
    let prompt = "Synthetic selected builder persona.";
    edit_yaml(&folder.join("packs/operational-domain.yml"), |value| {
        value["role_prompts"] =
            serde_yaml_ng::to_value(json!([{"role_code":"SWE", "prompt":prompt}])).unwrap();
    });
    let selected = preview_selected_composition(Some(&folder), None, &input()).unwrap();
    assert_ne!(original.hash(), selected.hash());
    let value: Value = selected.document().deserialize().unwrap();
    assert_eq!(
        value["attachments"]["persona"],
        json!({
            "state":"planned", "role":"SWE", "prompt_hash":ContentHash::of(prompt.as_bytes())
        })
    );
    assert!(!serde_json::to_string(&value).unwrap().contains(prompt));
    assert_eq!(value["effective_loading"], "not_observed");
}

#[test]
fn project_metadata_and_native_ids_never_substitute_for_write_authority() {
    let root = tempfile::tempdir().unwrap();
    let folder = selected_folder(root.path());
    let bound = preview(&folder, &input());
    assert_eq!(
        bound["project"]["configured_project_id"],
        "00000000-0000-7000-8000-000000000001"
    );
    edit_yaml(&folder.join(ENTRY), |value| {
        let project = value["project"].as_mapping_mut().unwrap();
        project.remove("project_id");
        project.remove("publication");
    });
    let before = files(root.path());
    let unbound = preview(&folder, &input());
    assert!(unbound["project"]["configured_project_id"].is_null());
    assert_eq!(unbound["project"]["tracker_key"], "EXM");
    assert_eq!(
        unbound["project"]["resource_roots"]["project"],
        "example-org/example-project"
    );
    for value in [&bound, &unbound] {
        assert_eq!(value["project"]["registration"], "not_observed");
        assert_eq!(value["native_actuation_authorized"], false);
        for reason in [
            "seat_bound_issuer_not_qualified",
            "project_workspace_authority_not_observed",
            "tenant_binding_not_supplied",
        ] {
            assert!(
                value["write_refusals"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(reason))
            );
        }
    }
    assert!(
        unbound["write_refusals"]
            .as_array()
            .unwrap()
            .contains(&json!("internal_project_id_not_configured"))
    );
    assert_eq!(files(root.path()), before);
    let mut changed = input();
    changed.subject.parent = None;
    assert_ne!(preview(&folder, &changed), unbound);
}

#[test]
fn explicit_selection_has_no_fallback_and_does_not_bind_identity_to_a_path() {
    let root = tempfile::tempdir().unwrap();
    let folder = selected_folder(root.path());
    let missing = root.path().join("absent selection");
    let request = input();
    assert_eq!(
        preview_selected_composition(None, None, &request)
            .unwrap_err()
            .problem,
        "no config folder selected"
    );
    assert!(preview_selected_composition(Some(&missing), Some(&folder), &request).is_err());
    let flag = preview_selected_composition(Some(&folder), Some(&missing), &request).unwrap();
    assert_eq!(
        flag,
        preview_selected_composition(None, Some(&folder), &request).unwrap()
    );
    let elsewhere = root.path().join("unrelated location");
    std::fs::rename(&folder, &elsewhere).unwrap();
    assert_eq!(
        flag,
        preview_selected_composition(Some(&elsewhere), None, &request).unwrap()
    );
}

#[test]
fn missing_or_invalid_pack_and_legacy_conflicts_use_loader_refusals() {
    let root = tempfile::tempdir().unwrap();
    let folder = selected_folder(root.path());
    let path = folder.join("packs/profiles.yml");
    std::fs::write(&path, "invalid: [").unwrap();
    let before = files(root.path());
    let error = preview_selected_composition(Some(&folder), None, &input()).unwrap_err();
    assert_eq!(error.path, path.canonicalize().unwrap());
    assert_eq!(
        error.problem,
        "invalid YAML configuration or unsupported document shape"
    );
    assert_eq!(files(root.path()), before);
    std::fs::remove_file(&path).unwrap();
    assert!(preview_selected_composition(Some(&folder), None, &input()).is_err());
    let folder = selected_folder(root.path());
    std::fs::write(folder.join("packs/profiles.json"), "{}").unwrap();
    assert!(preview_selected_composition(Some(&folder), None, &input()).is_err());
}

#[test]
fn unknown_profile_slot_or_fleet_binding_refuses_instead_of_guessing() {
    let root = tempfile::tempdir().unwrap();
    let folder = selected_folder(root.path());
    let mut request = input();
    request.category = PackCategoryKey::parse("unconfigured").unwrap();
    assert_eq!(
        preview_selected_composition(Some(&folder), None, &request)
            .unwrap_err()
            .problem,
        "composition profile cannot be resolved"
    );
    request = input();
    request.subject.slot = RoleSlotId::parse("unconfigured").unwrap();
    assert_eq!(
        preview_selected_composition(Some(&folder), None, &request)
            .unwrap_err()
            .problem,
        "composition slot is not in the selected profile team"
    );
    request = input();
    request.subject.slot = RoleSlotId::parse("architect").unwrap();
    assert_eq!(
        preview_selected_composition(Some(&folder), None, &request)
            .unwrap_err()
            .problem,
        "composition slot has no selected fleet binding"
    );
}

#[test]
fn unavailable_routes_stay_blocked_without_a_native_fallback() {
    let root = tempfile::tempdir().unwrap();
    let folder = selected_folder(root.path());
    let mut request = input();
    let fleet = ConfigFolder::select(Some(&folder), None)
        .unwrap()
        .fleet_snapshot()
        .unwrap();
    request.eligibility.unavailable_accounts = fleet
        .resolve("team/01936f5a-0000-7000-8000-000000000001/builder")
        .unwrap()
        .routes
        .iter()
        .map(|route| route.rung.provider.0.clone())
        .collect();
    let before = files(root.path());
    let blocked = preview(&folder, &request);
    assert!(blocked["fleet"]["selected"].is_null());
    assert_eq!(blocked["native_actuation_authorized"], false);
    assert_eq!(files(root.path()), before);
}
