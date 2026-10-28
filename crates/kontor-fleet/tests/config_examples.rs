//! The neutral config folder's schema, exercised as data.
//!
//! WP-02 defines the folder before the consumers read it. These tests bind the
//! neutral example and ASMA's concrete values to the same `project:` block, so
//! a later reader cannot quietly require a different shape. The fleet example
//! is additionally parsed by the production policy reader.

use std::path::{Path, PathBuf};

use kontor_core::id::ContentHash;
use kontor_fleet::FleetSnapshot;
use serde_yaml_ng::{Mapping, Value};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root")
}

fn read(relative: &str) -> Value {
    let path = repo_root().join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{} is readable: {error}", path.display());
    });
    serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!("{} is valid YAML: {error}", path.display());
    })
}

fn text(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{} is readable: {error}", path.display());
    })
}

fn map<'a>(value: &'a Value, path: &str) -> &'a Mapping {
    value
        .as_mapping()
        .unwrap_or_else(|| panic!("{path} must be a mapping"))
}

fn string(value: &Value, path: &str) -> String {
    value
        .as_str()
        .unwrap_or_else(|| panic!("{path} must be a string"))
        .to_owned()
}

fn sequence<'a>(value: &'a Value, path: &str) -> &'a [Value] {
    value
        .as_sequence()
        .unwrap_or_else(|| panic!("{path} must be a sequence"))
}

fn required<'a>(value: &'a Value, key: &str, path: &str) -> &'a Value {
    match value.get(key) {
        Some(found) => found,
        None => panic!("{path}.{key} is required"),
    }
}

fn required_string(value: &Value, key: &str, path: &str) -> String {
    string(required(value, key, path), &format!("{path}.{key}"))
}

fn required_sequence(value: &Value, key: &str, path: &str) -> Vec<Value> {
    sequence(required(value, key, path), &format!("{path}.{key}")).to_vec()
}

fn required_map<'a>(value: &'a Value, key: &str, path: &str) -> &'a Mapping {
    map(required(value, key, path), &format!("{path}.{key}"))
}

fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !Path::new(path)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
}

fn resource_roots<'a>(project: &'a Value, path: &str) -> &'a Mapping {
    required_map(project, "resource_roots", path)
}

fn assert_project_resource_paths(project: &Value, path: &str) {
    let roots = resource_roots(project, path);
    assert!(!roots.is_empty(), "{path}.resource_roots is non-empty");
    let repositories = schema_keys(
        required(project.get("forge").expect("forge"), "repositories", path),
        &format!("{path}.forge.repositories"),
    );
    for (root, repository) in roots {
        let root = root.as_str().expect("a resource root name");
        let repository = string(repository, &format!("{path}.resource_roots.{root}"));
        assert!(
            repositories
                .iter()
                .any(|candidate| candidate == &repository),
            "{path}.resource_roots.{root} names a repository in project.forge.repositories"
        );
    }

    let mut paths = vec![
        required_string(project, "gap_ledger", path),
        required_string(project, "glossary", path),
        required_string(
            project.get("decision_authorities").expect("authorities"),
            "decision_records_root",
            &format!("{path}.decision_authorities"),
        ),
    ];
    paths.extend(
        required_sequence(project, "documentation_roots", path)
            .iter()
            .map(|value| string(value, &format!("{path}.documentation_roots[]"))),
    );
    for resource in paths {
        assert!(
            is_safe_relative_path(&resource),
            "{path}: project resource {resource} is a safe relative path"
        );
        let (root, rest) = resource
            .split_once('/')
            .unwrap_or_else(|| panic!("{path}: {resource} starts with a resource root"));
        assert!(
            roots.contains_key(root),
            "{path}: {resource} starts with a declared resource root"
        );
        assert!(
            !rest.is_empty(),
            "{path}: {resource} names a path inside resource root {root}"
        );
    }
}

fn runtime_pairs(project: &Value) -> Vec<(String, String)> {
    required_sequence(
        project.get("runtime").expect("runtime"),
        "matrix",
        "project.runtime",
    )
    .iter()
    .map(|entry| {
        (
            required_string(entry, "harness", "project.runtime.matrix[]"),
            required_string(entry, "provider", "project.runtime.matrix[]"),
        )
    })
    .collect()
}

fn route_pair_allowed(project: &Value, account: &Value) -> Result<(), &'static str> {
    let pair = (
        account["harness"].as_str().ok_or("route_harness")?,
        account["provider"].as_str().ok_or("route_provider")?,
    );
    if runtime_pairs(project)
        .iter()
        .any(|allowed| allowed.0 == pair.0 && allowed.1 == pair.1)
    {
        Ok(())
    } else {
        Err("runtime_pair_not_allowed")
    }
}

fn validate_pr_desk(project: &Value, path: &str) -> Result<(), String> {
    let desk = project
        .get("pr_desk")
        .and_then(Value::as_mapping)
        .ok_or_else(|| format!("{path}.pr_desk is required"))?;
    for key in ["command", "settings", "history_root"] {
        let field = desk
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{path}.pr_desk.{key} is required"))?;
        if field.is_empty() {
            return Err(format!("{path}.pr_desk.{key} is non-empty"));
        }
    }
    let settings = desk["settings"]
        .as_str()
        .expect("settings was checked as a string");
    if !is_safe_relative_path(settings) {
        return Err(format!(
            "{path}.pr_desk.settings is a safe folder-relative reference"
        ));
    }
    if settings != "orchestration/pr-desk.yml" {
        return Err(format!(
            "{path}.pr_desk.settings names orchestration/pr-desk.yml"
        ));
    }
    let history_root = desk["history_root"]
        .as_str()
        .expect("history_root was checked as a string");
    if !is_safe_relative_path(history_root) {
        return Err(format!(
            "{path}.pr_desk.history_root is a safe data-root-relative path"
        ));
    }
    Ok(())
}

fn validate_pr_desk_policy(policy: &Value, path: &str) -> Result<(), String> {
    let policy = policy
        .as_mapping()
        .ok_or_else(|| format!("{path} is a mapping"))?;
    if policy.get("schema_version").and_then(Value::as_i64) != Some(1) {
        return Err(format!("{path}.schema_version is 1"));
    }
    for key in [
        "poll_start_budget",
        "ready_silence_days",
        "draft_silence_days",
        "fix_round_limit",
        "revise_round_limit",
    ] {
        if !policy
            .get(key)
            .and_then(Value::as_i64)
            .is_some_and(|value| value > 0)
        {
            return Err(format!("{path}.{key} is a positive integer"));
        }
    }
    match policy.get("active_pr_policy").and_then(Value::as_str) {
        Some("unlimited") | Some("one_active") => Ok(()),
        _ => Err(format!(
            "{path}.active_pr_policy is unlimited or one_active"
        )),
    }
}

fn learning_log(host: &Value, project: &Value) -> Result<PathBuf, String> {
    let paths = host
        .get("paths")
        .and_then(Value::as_mapping)
        .ok_or_else(|| "host.paths is required".to_owned())?;
    if let Some(overridden) = paths.get("pr_desk") {
        let overridden = overridden
            .as_str()
            .ok_or_else(|| "host.paths.pr_desk is a string".to_owned())?;
        let overridden = Path::new(overridden);
        if !overridden.is_absolute() {
            return Err("host.paths.pr_desk is absolute".to_owned());
        }
        return Ok(overridden.join("learning.log"));
    }
    let data_root = paths
        .get("data_root")
        .and_then(Value::as_str)
        .ok_or_else(|| "host.paths.data_root is required".to_owned())?;
    let data_root = Path::new(data_root);
    if !data_root.is_absolute() {
        return Err("host.paths.data_root is absolute".to_owned());
    }
    let history_root = project
        .get("pr_desk")
        .and_then(Value::as_mapping)
        .and_then(|desk| desk.get("history_root"))
        .and_then(Value::as_str)
        .ok_or_else(|| "project.pr_desk.history_root is required".to_owned())?;
    if !is_safe_relative_path(history_root) {
        return Err("project.pr_desk.history_root is a safe data-root-relative path".to_owned());
    }
    Ok(data_root.join(history_root).join("learning.log"))
}

fn validate_project(value: &Value, path: &str) -> Mapping {
    let project = map(value, path);
    for key in [
        "key",
        "name",
        "owner",
        "resource_roots",
        "decision_authorities",
        "tracker",
        "branch",
        "forge",
        "runtime",
        "cli",
        "team_pack",
        "fleet",
        "supervision",
        "pr_desk",
        "merge_authority",
        "documentation_roots",
        "gap_ledger",
        "attention_channel",
        "glossary",
        "worktree_layout",
        "check_runs",
        "capacity",
        "time_zone",
        "holiday_countries",
        "currency",
    ] {
        required(value, key, path);
        if !project.contains_key(key) {
            panic!("{path}.{key} is required");
        }
    }

    for key in ["key", "name", "owner", "team_pack", "fleet", "supervision"] {
        assert!(
            !required_string(value, key, path).is_empty(),
            "{path}.{key} is non-empty"
        );
    }

    validate_pr_desk(value, path).unwrap_or_else(|error| panic!("{error}"));

    let authorities = required_map(value, "decision_authorities", path);
    for key in ["platform", "architecture", "decision_records_root"] {
        assert!(
            authorities.contains_key(key),
            "{path}.decision_authorities.{key} is required"
        );
        assert!(
            !required_string(
                value.get("decision_authorities").expect("authorities"),
                key,
                &format!("{path}.decision_authorities")
            )
            .is_empty(),
            "{path}.decision_authorities.{key} is non-empty"
        );
    }

    let tracker = required_map(value, "tracker", path);
    for key in [
        "kind",
        "site",
        "project_key",
        "key_pattern",
        "credential_alias",
        "spec",
    ] {
        assert!(
            tracker.contains_key(key),
            "{path}.tracker.{key} is required"
        );
        assert!(
            !required_string(
                value.get("tracker").expect("tracker"),
                key,
                &format!("{path}.tracker")
            )
            .is_empty(),
            "{path}.tracker.{key} is non-empty"
        );
    }
    let key_pattern = required_string(
        value.get("tracker").expect("tracker"),
        "key_pattern",
        &format!("{path}.tracker"),
    );
    assert!(key_pattern.starts_with('^') && key_pattern.ends_with('$'));

    let branch = required_map(value, "branch", path);
    assert!(
        !required_sequence(
            value.get("branch").expect("branch"),
            "types",
            &format!("{path}.branch")
        )
        .is_empty()
    );
    for key in ["pattern", "commit_pattern"] {
        assert!(branch.contains_key(key), "{path}.branch.{key} is required");
        assert!(
            !required_string(
                value.get("branch").expect("branch"),
                key,
                &format!("{path}.branch")
            )
            .is_empty()
        );
    }

    let forge = required_map(value, "forge", path);
    for key in ["kind", "organization", "repositories", "base_branch"] {
        assert!(forge.contains_key(key), "{path}.forge.{key} is required");
    }
    assert!(
        !required_sequence(
            value.get("forge").expect("forge"),
            "repositories",
            &format!("{path}.forge")
        )
        .is_empty()
    );
    for repository in required_sequence(
        value.get("forge").expect("forge"),
        "repositories",
        &format!("{path}.forge"),
    ) {
        let repository = string(&repository, &format!("{path}.forge.repositories[]"));
        assert!(
            repository.split_once('/').is_some_and(|(owner, name)| {
                !owner.is_empty() && !name.is_empty() && !name.contains('/')
            }),
            "{path}.forge.repositories[] is owner/name"
        );
    }
    assert_project_resource_paths(value, path);

    let matrix = required_sequence(
        value.get("runtime").expect("runtime"),
        "matrix",
        &format!("{path}.runtime"),
    );
    assert!(!matrix.is_empty(), "{path}.runtime.matrix is non-empty");
    let mut pairs = std::collections::BTreeSet::new();
    for (index, entry) in matrix.iter().enumerate() {
        let entry_path = format!("{path}.runtime.matrix[{index}]");
        for key in ["name", "harness", "provider"] {
            assert!(
                !required_string(entry, key, &entry_path).is_empty(),
                "{entry_path}.{key} is non-empty"
            );
        }
        assert!(
            pairs.insert((
                required_string(entry, "harness", &entry_path),
                required_string(entry, "provider", &entry_path)
            )),
            "{entry_path} is a unique provider/harness pair"
        );
    }

    assert!(
        !required_string(
            value.get("cli").expect("cli"),
            "command",
            &format!("{path}.cli")
        )
        .is_empty(),
        "{path}.cli.command is non-empty"
    );
    assert!(
        !required_sequence(
            value.get("cli").expect("cli"),
            "verbs",
            &format!("{path}.cli")
        )
        .is_empty()
    );

    for key in ["role", "policy"] {
        assert!(
            !required_string(
                value.get("merge_authority").expect("merge"),
                key,
                &format!("{path}.merge_authority")
            )
            .is_empty()
        );
    }
    for key in ["documentation_roots", "check_runs", "holiday_countries"] {
        assert!(
            !required_sequence(value, key, path).is_empty(),
            "{path}.{key} is non-empty"
        );
    }
    let capacity = required_map(value, "capacity", path);
    for key in [
        "max_delivery_seats",
        "max_parallel_tasks",
        "start_admission",
    ] {
        assert!(
            capacity
                .get(key)
                .and_then(Value::as_i64)
                .is_some_and(|value| value > 0),
            "{path}.capacity.{key} is a positive integer"
        );
    }
    let ceiling = capacity["max_parallel_tasks"]
        .as_i64()
        .expect("max_parallel_tasks is an integer");
    let start = capacity["start_admission"]
        .as_i64()
        .expect("start_admission is an integer");
    assert!(
        start <= ceiling,
        "{path}.capacity.start_admission ({start}) must not exceed max_parallel_tasks ({ceiling})"
    );
    for key in [
        "gap_ledger",
        "attention_channel",
        "glossary",
        "worktree_layout",
    ] {
        assert!(
            !required_string(value, key, path).is_empty(),
            "{path}.{key} is non-empty"
        );
    }
    for key in ["time_zone", "currency"] {
        assert!(
            !required_string(value, key, path).is_empty(),
            "{path}.{key} is non-empty"
        );
    }

    project.clone()
}

fn validate_orchestration(relative: &str) {
    let document = read(relative);
    assert_eq!(
        document
            .get("schema_version")
            .and_then(Value::as_i64)
            .expect("schema_version"),
        1
    );
    let project = validate_project(&document["project"], "project");
    assert!(
        document.get("fleet").and_then(Value::as_str)
            == project.get("fleet").and_then(Value::as_str),
        "{relative}: the bundle selector and the project block agree on fleet"
    );
    for key in ["fleet", "core_team"] {
        assert!(
            document.get(key).and_then(Value::as_str).is_some(),
            "{relative}: {key} is a selector"
        );
    }
    let fleet = document
        .get("fleet")
        .and_then(Value::as_str)
        .expect("fleet selector");
    assert!(
        fleet.starts_with("orchestration/"),
        "{relative}: fleet resolves from the selected-folder root under orchestration/"
    );
    for reference in [
        fleet,
        document
            .get("core_team")
            .and_then(Value::as_str)
            .expect("core team selector"),
        project
            .get("team_pack")
            .and_then(Value::as_str)
            .expect("team pack"),
        project.get("fleet").and_then(Value::as_str).expect("fleet"),
        project
            .get("supervision")
            .and_then(Value::as_str)
            .expect("supervision"),
        project
            .get("tracker")
            .and_then(Value::as_mapping)
            .and_then(|tracker| tracker.get("spec"))
            .and_then(Value::as_str)
            .expect("tracker spec"),
        project
            .get("pr_desk")
            .and_then(Value::as_mapping)
            .and_then(|desk| desk.get("settings"))
            .and_then(Value::as_str)
            .expect("pr desk settings"),
    ] {
        assert!(
            is_safe_relative_path(reference),
            "{relative}: config reference {reference} is safe and relative"
        );
    }
}

#[test]
fn the_neutral_orchestration_matches_the_project_schema() {
    validate_orchestration("config/examples/orchestration/orchestration.yml");
}

#[test]
fn publication_is_an_optional_explicit_project_policy_in_the_same_schema() {
    use kontor_core::id::{ExternalName, ProjectId};
    use kontor_core::publication::PublicationPolicy;
    use kontor_core::pull_request::ForgeRepository;

    let schema = read("config/examples/orchestration/orchestration.schema.yml");
    assert_eq!(
        schema_keys(&schema["project"]["optional"], "project.optional"),
        ["project_id", "publication", "consultation_layout"]
    );
    assert_eq!(
        schema_keys(
            &schema["publication_contract"]["required"],
            "publication.required"
        ),
        ["root_repository", "module_repositories"]
    );
    let document = read("config/examples/orchestration/orchestration.yml");
    let project = &document["project"];
    let policy = PublicationPolicy {
        project_id: ProjectId::parse(project["project_id"].as_str().unwrap()).unwrap(),
        repositories: sequence(&project["forge"]["repositories"], "repositories")
            .iter()
            .map(|value| ForgeRepository::parse(value.as_str().unwrap()).unwrap())
            .collect(),
        root_repository: ForgeRepository::parse(
            project["publication"]["root_repository"].as_str().unwrap(),
        )
        .unwrap(),
        module_repositories: serde_yaml_ng::from_value(
            project["publication"]["module_repositories"].clone(),
        )
        .unwrap(),
        base_branch: ExternalName::parse(project["forge"]["base_branch"].as_str().unwrap())
            .unwrap(),
        branch_rules: None,
    };
    policy.validate().unwrap();
    assert!(
        policy.module_repositories.is_empty(),
        "the neutral example is root-only"
    );
    let direct = read("tests/fixtures/config/asma-orchestration.yml");
    assert!(direct["project"].get("project_id").is_none());
    assert!(direct["project"].get("publication").is_none());
}

fn schema_keys(value: &Value, path: &str) -> Vec<String> {
    sequence(value, path)
        .iter()
        .map(|key| string(key, path))
        .collect()
}

#[test]
fn the_schema_file_and_both_examples_agree() {
    let schema = read("config/examples/orchestration/orchestration.schema.yml");
    assert_eq!(
        schema_keys(&schema["required"], "required"),
        ["schema_version", "fleet", "core_team", "project"]
    );
    let project_schema = &schema["project"];
    let required_keys = schema_keys(&project_schema["required"], "project.required");
    assert_eq!(
        required_keys,
        [
            "key",
            "name",
            "owner",
            "resource_roots",
            "decision_authorities",
            "tracker",
            "branch",
            "forge",
            "runtime",
            "cli",
            "team_pack",
            "fleet",
            "supervision",
            "pr_desk",
            "merge_authority",
            "documentation_roots",
            "gap_ledger",
            "attention_channel",
            "glossary",
            "worktree_layout",
            "check_runs",
            "capacity",
            "time_zone",
            "holiday_countries",
            "currency",
        ]
    );

    let paths = &schema["paths"];
    assert_eq!(
        schema_keys(&paths["config_file_references"]["keys"], "file refs"),
        [
            "fleet",
            "core_team",
            "project.team_pack",
            "project.fleet",
            "project.supervision",
            "project.tracker.spec",
            "project.pr_desk.settings",
        ]
    );
    assert_eq!(
        schema_keys(&paths["project_resource_paths"]["keys"], "project paths"),
        [
            "project.documentation_roots",
            "project.gap_ledger",
            "project.glossary",
            "project.decision_authorities.decision_records_root",
        ]
    );
    assert_eq!(
        schema_keys(
            &paths["host_data_relative_paths"]["keys"],
            "host data paths"
        ),
        ["project.pr_desk.history_root"]
    );
    assert_eq!(
        paths["host_data_relative_paths"]["base"].as_str(),
        Some("host.paths.data_root"),
        "the desk history root resolves under the host data root"
    );
    assert_eq!(
        schema["route_binding"]["join_key"].as_str(),
        Some("slot"),
        "the only fleet/host join key is the logical slot"
    );
    for field in [
        "route_model_id",
        "canonical_model",
        "canonical_effort",
        "canonical_vendor",
    ] {
        assert!(
            schema_keys(&schema["route_snapshot"]["fields"], "snapshot fields")
                .contains(&field.to_owned()),
            "the snapshot preserves {field}"
        );
    }
    for field in ["serves", "efforts", "qualification"] {
        assert!(
            schema_keys(
                &schema["route_binding"]["host_route"]["fields"],
                "host route fields"
            )
            .contains(&field.to_owned()),
            "the host route declares {field}"
        );
    }
    assert!(
        schema["route_binding"]["compatibility"]["fleet_edit"]
            .as_str()
            .is_some_and(|text| text.contains("never requires a fleet policy edit"))
    );
    assert!(
        schema["route_binding"]["migration"]
            .as_str()
            .is_some_and(|text| text.contains("no daemon change before KR-14"))
    );
    assert!(
        schema["route_binding"]["qualification"]["source"]
            .as_str()
            .is_some_and(|text| text == "orchestration/route-qualifications.yml"),
        "qualification records live in the activated source, not host.yml"
    );
    assert!(
        schema["route_binding"]["runtime_matrix"]["rule"]
            .as_str()
            .is_some_and(|text| text.contains("allowlist")),
        "the project runtime matrix is an allowlist"
    );

    for relative in [
        "config/examples/orchestration/orchestration.yml",
        "tests/fixtures/config/asma-orchestration.yml",
    ] {
        let document = read(relative);
        let project = &document["project"];
        for key in &required_keys {
            assert!(
                project.get(key.as_str()).is_some(),
                "{relative}: project.{key} is required by the schema"
            );
        }
        let nested = map(&project_schema["nested"], "project.nested");
        for (name, spec) in nested {
            let name = name.as_str().expect("a nested schema name");
            let nested_required = schema_keys(
                required(spec, "required", &format!("project.nested.{name}")),
                &format!("project.nested.{name}.required"),
            );
            for key in nested_required {
                assert!(
                    project[name].get(key.as_str()).is_some(),
                    "{relative}: project.{name}.{key} is required by the schema"
                );
            }
        }
    }
}

#[test]
fn asmas_values_match_the_same_project_schema() {
    let document = read("tests/fixtures/config/asma-orchestration.yml");
    let project = validate_project(&document["project"], "project");
    assert_eq!(
        project.get("key").and_then(Value::as_str),
        Some("ASMA"),
        "the test case is ASMA's concrete project key"
    );
    let forge = project
        .get("forge")
        .and_then(Value::as_mapping)
        .expect("forge");
    assert_eq!(
        forge.get("organization").and_then(Value::as_str),
        Some("Carasent-ASMA")
    );
    assert_eq!(
        forge.get("base_branch").and_then(Value::as_str),
        Some("master")
    );
    assert!(
        forge
            .get("repositories")
            .and_then(Value::as_sequence)
            .is_some_and(|repositories| repositories.len() >= 2)
    );
}

#[test]
fn every_folder_reference_resolves_inside_the_example() {
    let root = repo_root().join("config/examples");
    for relative in [
        "orchestration/orchestration.yml",
        "orchestration/orchestration.schema.yml",
        "orchestration/fleet.yml",
        "orchestration/route-qualifications.yml",
        "orchestration/teams/core-team.yml",
        "orchestration/tpm-supervision.yml",
        "orchestration/pr-desk.yml",
        "paseo-supervision.yml",
        "packs/team.yml",
        "tracker/jira.yml",
        "forge/github-app.example.yml",
    ] {
        assert!(
            root.join(relative).is_file(),
            "config/examples/{relative} exists"
        );
    }

    let document = read("config/examples/orchestration/orchestration.yml");
    for reference in [
        document["fleet"].as_str().expect("fleet"),
        document["core_team"].as_str().expect("core_team"),
        document["project"]["team_pack"]
            .as_str()
            .expect("team_pack"),
        document["project"]["supervision"]
            .as_str()
            .expect("supervision"),
        document["project"]["tracker"]["spec"]
            .as_str()
            .expect("tracker spec"),
        document["project"]["pr_desk"]["settings"]
            .as_str()
            .expect("pr desk settings"),
    ] {
        assert!(
            !Path::new(reference).is_absolute(),
            "{reference} is folder-relative"
        );
        assert!(
            !Path::new(reference)
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir)),
            "{reference} stays inside the config folder"
        );
        assert!(
            root.join(reference).is_file(),
            "config/examples/{reference} exists"
        );
    }
}

#[test]
fn both_examples_define_the_pr_desk_contract_and_its_settings_file() {
    for (relative, folder, command) in [
        (
            "config/examples/orchestration/orchestration.yml",
            "config/examples",
            "example-pr-desk",
        ),
        (
            "tests/fixtures/config/asma-orchestration.yml",
            "tests/fixtures/config",
            "python3 _tools/ai-orchestration/pr_desk.py",
        ),
    ] {
        let document = read(relative);
        let project = &document["project"];
        validate_pr_desk(project, &format!("{relative}: project"))
            .unwrap_or_else(|error| panic!("{error}"));

        let desk = &project["pr_desk"];
        assert_eq!(
            desk["command"].as_str(),
            Some(command),
            "{relative}: project.pr_desk.command"
        );
        assert_eq!(
            desk["settings"].as_str(),
            Some("orchestration/pr-desk.yml"),
            "{relative}: project.pr_desk.settings"
        );
        assert_eq!(
            desk["history_root"].as_str(),
            Some("pr-review"),
            "{relative}: project.pr_desk.history_root"
        );

        let root = repo_root().join(folder);
        let settings_reference = desk["settings"]
            .as_str()
            .expect("the settings reference is a string");
        let settings = root.join(settings_reference);
        assert!(
            settings.is_file(),
            "{relative}: the settings file exists in the config folder"
        );
        assert!(
            settings
                .canonicalize()
                .expect("canonical settings path")
                .starts_with(root.canonicalize().expect("canonical folder root")),
            "{relative}: the settings file stays inside the config folder"
        );
    }
}

#[test]
fn a_missing_pr_desk_key_or_an_escaping_path_is_refused() {
    let document = read("config/examples/orchestration/orchestration.yml");
    let project = &document["project"];
    assert!(validate_pr_desk(project, "project").is_ok());

    let mut without_desk = project.clone();
    without_desk
        .as_mapping_mut()
        .expect("a project mapping")
        .remove("pr_desk");
    assert!(
        validate_pr_desk(&without_desk, "project").is_err(),
        "project.pr_desk is required"
    );

    for key in ["command", "settings", "history_root"] {
        let mut candidate = project.clone();
        candidate["pr_desk"]
            .as_mapping_mut()
            .expect("a pr_desk mapping")
            .remove(key);
        assert!(
            validate_pr_desk(&candidate, "project").is_err(),
            "project.pr_desk.{key} is required"
        );
    }

    for escaped in ["../orchestration/pr-desk.yml", "/orchestration/pr-desk.yml"] {
        let mut candidate = project.clone();
        candidate["pr_desk"]
            .as_mapping_mut()
            .expect("a pr_desk mapping")
            .insert(
                Value::String("settings".to_owned()),
                Value::String(escaped.to_owned()),
            );
        assert!(
            validate_pr_desk(&candidate, "project").is_err(),
            "an escaping settings path {escaped} is refused"
        );
    }

    let mut wrong_settings = project.clone();
    wrong_settings["pr_desk"]
        .as_mapping_mut()
        .expect("a pr_desk mapping")
        .insert(
            Value::String("settings".to_owned()),
            Value::String("orchestration/other.yml".to_owned()),
        );
    assert!(
        validate_pr_desk(&wrong_settings, "project").is_err(),
        "the contained but wrong settings name is refused"
    );

    for escaped in ["../x", "/x"] {
        let mut escaped_history = project.clone();
        escaped_history["pr_desk"]
            .as_mapping_mut()
            .expect("a pr_desk mapping")
            .insert(
                Value::String("history_root".to_owned()),
                Value::String(escaped.to_owned()),
            );
        assert!(
            validate_pr_desk(&escaped_history, "project").is_err(),
            "a history root that escapes the data root is refused: {escaped}"
        );
    }
}

#[test]
fn the_pr_desk_learning_log_resolves_under_data_root_or_the_host_override() {
    let document = read("config/examples/orchestration/orchestration.yml");
    let project = &document["project"];

    let host = read("config/examples/host.example.yml");
    let overridden = learning_log(&host, project).expect("the host override resolves");
    let override_root = Path::new(
        host["paths"]["pr_desk"]
            .as_str()
            .expect("host.paths.pr_desk"),
    );
    assert!(override_root.is_absolute());
    assert_eq!(overridden, override_root.join("learning.log"));
    assert!(overridden.starts_with(override_root));

    let alternate = read("tests/fixtures/config/host-alternate.yml");
    assert!(
        alternate["paths"].get("pr_desk").is_none(),
        "the alternate host has no explicit desk root"
    );
    let resolved = learning_log(&alternate, project).expect("the data-root resolution");
    let data_root = Path::new(
        alternate["paths"]["data_root"]
            .as_str()
            .expect("host.paths.data_root"),
    );
    let expected = data_root.join("pr-review").join("learning.log");
    assert_eq!(resolved, expected);
    assert!(resolved.starts_with(data_root));

    let mut relative_override = host.clone();
    relative_override["paths"]
        .as_mapping_mut()
        .expect("host paths")
        .insert(
            Value::String("pr_desk".to_owned()),
            Value::String("relative/desk".to_owned()),
        );
    assert!(
        learning_log(&relative_override, project).is_err(),
        "host.paths.pr_desk is absolute"
    );
}

#[test]
fn the_pr_desk_policy_files_carry_the_required_values() {
    let neutral = read("config/examples/orchestration/pr-desk.yml");
    validate_pr_desk_policy(&neutral, "config/examples/orchestration/pr-desk.yml")
        .expect("the neutral policy is valid");
    assert_eq!(neutral["schema_version"].as_i64(), Some(1));
    assert_eq!(neutral["active_pr_policy"].as_str(), Some("one_active"));

    let mut with_project_keys = neutral.clone();
    let project_keys = with_project_keys
        .as_mapping_mut()
        .expect("the neutral policy mapping");
    project_keys.insert(
        Value::String("desk".to_owned()),
        Value::String("project-owned".to_owned()),
    );
    project_keys.insert(
        Value::String("skip_repositories".to_owned()),
        Value::Sequence(Vec::new()),
    );
    validate_pr_desk_policy(
        &with_project_keys,
        "config/examples/orchestration/pr-desk.yml",
    )
    .expect("project-owned keys are allowed beside the required seven");

    let asma = read("tests/fixtures/config/orchestration/pr-desk.yml");
    validate_pr_desk_policy(&asma, "tests/fixtures/config/orchestration/pr-desk.yml")
        .expect("ASMA's policy is valid");
    assert_eq!(asma["schema_version"].as_i64(), Some(1));
    assert_eq!(asma["poll_start_budget"].as_i64(), Some(3));
    assert_eq!(asma["active_pr_policy"].as_str(), Some("unlimited"));
    assert_eq!(asma["ready_silence_days"].as_i64(), Some(3));
    assert_eq!(asma["draft_silence_days"].as_i64(), Some(15));
    assert_eq!(asma["fix_round_limit"].as_i64(), Some(2));
    assert_eq!(asma["revise_round_limit"].as_i64(), Some(2));

    let schema = read("config/examples/orchestration/orchestration.schema.yml");
    let contract = &schema["pr_desk_contract"];
    assert_eq!(
        schema_keys(
            &schema["project"]["nested"]["pr_desk"]["required"],
            "pr_desk required"
        ),
        ["command", "settings", "history_root"],
        "the schema pins the pr_desk block's required key list"
    );
    assert_eq!(
        contract["project.pr_desk"]["settings"]
            .as_str()
            .map(|text| text.contains("orchestration/pr-desk.yml")),
        Some(true),
        "the schema names the pinned settings file"
    );
    assert!(
        contract["settings_file"]["extra_keys"]
            .as_str()
            .is_some_and(|text| text.contains("allowed")),
        "the settings contract allows project-owned keys beside the required seven"
    );
    let precedence = schema_keys(
        &contract["learning_log_root"]["precedence"],
        "learning log precedence",
    );
    assert_eq!(
        precedence.len(),
        3,
        "the learning log root has one precedence chain"
    );
    for (position, expected) in ["PR_DESK_DIR", "host.paths.pr_desk", "host.paths.data_root"]
        .iter()
        .enumerate()
    {
        assert!(
            precedence[position].contains(expected),
            "precedence {position} names {expected}"
        );
    }
    assert!(
        contract["learning_log_root"]["resolved_log"]
            .as_str()
            .is_some_and(|text| text.contains("no second candidate")),
        "the schema states the single resolved learning log"
    );
    assert_eq!(
        schema_keys(
            &contract["settings_file"]["required"],
            "pr desk settings required"
        ),
        [
            "schema_version",
            "poll_start_budget",
            "active_pr_policy",
            "ready_silence_days",
            "draft_silence_days",
            "fix_round_limit",
            "revise_round_limit",
        ]
    );
    assert_eq!(
        schema_keys(
            &contract["settings_file"]["active_pr_policy_values"],
            "active PR policy values"
        ),
        ["unlimited", "one_active"]
    );
}

#[test]
fn the_neutral_fleet_example_is_schema_2_and_names_no_real_account() {
    let document = text("config/examples/orchestration/fleet.yml");
    let snapshot = FleetSnapshot::parse_policy(&document).expect("the neutral fleet example");
    assert_eq!(snapshot.schema_version(), 2);
    for refused in [
        concat!("claude", "-work"),
        concat!("claude", "-personal"),
        concat!("codex", "-work"),
        concat!("codex", "-personal"),
    ] {
        assert!(!document.contains(refused), "no owner alias {refused}");
    }
    for refused in ["/Users/", "/home/", "token", "secret"] {
        assert!(
            !document.contains(refused),
            "no {refused} in the fleet example"
        );
    }

    let parsed = read("config/examples/orchestration/fleet.yml");
    let allowed = [
        "claude-primary",
        "claude-secondary",
        "codex-primary",
        "cursor",
        "opencode",
    ];
    let domains = map(required(&parsed, "domains", "fleet"), "fleet.domains");
    for (name, domain) in domains {
        let name = name.as_str().expect("a domain name");
        for account in sequence(required(domain, "accounts", name), "accounts") {
            let account = account.as_str().expect("an account alias");
            assert!(
                allowed.contains(&account),
                "fleet domain {name} names only a placeholder account, found {account}"
            );
        }
    }
}

#[test]
fn the_host_example_declares_every_bootstrap_section_without_a_secret() {
    let document = read("config/examples/host.example.yml");
    for key in [
        "schema_version",
        "time_zone",
        "paths",
        "binaries",
        "runtime",
        "provider_accounts",
        "routes",
        "keychain",
        "launchd",
    ] {
        assert!(document.get(key).is_some(), "host.example.yml has {key}");
    }
    for key in [
        "state_root",
        "data_root",
        "worktrees_root",
        "paseo_home",
        "logs",
    ] {
        assert!(
            !required_string(&document["paths"], key, "paths").is_empty(),
            "paths.{key} is non-empty"
        );
    }
    let desk_root = required_string(&document["paths"], "pr_desk", "paths");
    assert!(
        Path::new(&desk_root).is_absolute(),
        "the optional paths.pr_desk override is an absolute history root"
    );
    for key in ["paseo", "git", "gh", "python", "cargo"] {
        assert!(
            !required_string(&document["binaries"], key, "binaries").is_empty(),
            "binaries.{key} is non-empty"
        );
    }
    assert!(
        document["binaries"].get("asma").is_none(),
        "the neutral host does not require an asma-named executable"
    );
    let project_command = required_string(&document["project_cli"], "example", "project_cli");
    assert!(
        !project_command.is_empty(),
        "project_cli binds project.cli.command to an executable"
    );
    let orchestration = read("config/examples/orchestration/orchestration.yml");
    assert_eq!(
        orchestration["project"]["cli"]["command"].as_str(),
        Some("example"),
        "the neutral host binds the neutral project command"
    );
    for runtime in ["kontord", "paseo"] {
        assert!(
            !required_string(&document["runtime"][runtime], "endpoint", runtime).is_empty(),
            "runtime.{runtime}.endpoint is non-empty"
        );
        assert!(
            document["runtime"][runtime]
                .get("port")
                .and_then(Value::as_i64)
                .is_some_and(|port| (1..=65535).contains(&port)),
            "runtime.{runtime}.port is a port"
        );
    }

    let accounts = map(&document["provider_accounts"], "provider_accounts");
    for (name, account) in accounts {
        let name = name.as_str().expect("an account name");
        for key in ["provider", "native_alias", "harness", "credential"] {
            assert!(
                !required_string(account, key, name).is_empty(),
                "provider_accounts.{name}.{key} is non-empty"
            );
        }
        assert!(
            required_string(account, "credential", name).starts_with("keychain:"),
            "provider_accounts.{name}.credential is a reference, never a value"
        );
        assert!(
            !required_string(account, "native_alias", name).starts_with('/'),
            "provider_accounts.{name}.native_alias is a runtime account id, not a path"
        );
        assert!(
            !map(required(account, "serves", name), name).is_empty(),
            "provider_accounts.{name}.serves maps at least one canonical model"
        );
    }
    for (domain, route) in map(&document["routes"], "routes") {
        let domain = domain.as_str().expect("a route domain");
        for account in sequence(route, domain) {
            let account = account.as_str().expect("an account alias");
            assert!(
                accounts.keys().any(|key| key.as_str() == Some(account)),
                "routes.{domain} names a declared account: {account}"
            );
        }
    }

    let fleet = read("config/examples/orchestration/fleet.yml");
    for (slot, account) in accounts {
        let slot = slot.as_str().expect("a logical slot");
        let route_provider = required_string(account, "provider", slot);
        let route_harness = required_string(account, "harness", slot);
        for (model, launch_id) in map(required(account, "serves", slot), slot) {
            let model = model.as_str().expect("a canonical model name");
            let launch_id = string(launch_id, &format!("{slot}.serves.{model}"));
            let model_spec = &fleet["models"][model];
            assert!(
                !model_spec.is_null(),
                "{slot} serves a fleet model: {model}"
            );
            assert!(
                !launch_id.is_empty(),
                "{slot}.serves.{model} names the route-native model id"
            );
            let domain = model_spec["domain"].as_str().expect("a model domain");
            let fleet_provider = fleet["domains"][domain]["provider"]
                .as_str()
                .expect("a fleet domain provider");
            if route_provider != fleet_provider {
                assert!(
                    account
                        .get("qualification")
                        .and_then(Value::as_str)
                        .is_some(),
                    "{slot} serves {model} through a cross-provider route and states a qualification"
                );
            }
            if let Some(route_efforts) = account.get("efforts") {
                let fleet_efforts = schema_keys(&model_spec["efforts"], "fleet efforts");
                for effort in schema_keys(route_efforts, "route efforts") {
                    assert!(
                        fleet_efforts.iter().any(|candidate| candidate == &effort),
                        "{slot} effort {effort} is a subset of {model}'s fleet efforts"
                    );
                }
            }
            let project = read("config/examples/orchestration/orchestration.yml");
            assert!(
                runtime_pairs(&project["project"])
                    .iter()
                    .any(|pair| pair == &(route_harness.clone(), route_provider.clone())),
                "{slot} route {route_harness}/{route_provider} is allowed by the project matrix"
            );
        }
    }

    for forbidden in ["token", "password", "secret", "private_key", "api_key"] {
        assert!(
            !document_contains_key(&document, forbidden),
            "host.example.yml has no {forbidden} field"
        );
    }
    for forbidden in ["/Users/", "/home/"] {
        assert!(
            !text("config/examples/host.example.yml").contains(forbidden),
            "host.example.yml has no home path"
        );
    }
}

fn document_contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Mapping(mapping) => mapping.iter().any(|(candidate, nested)| {
            candidate.as_str() == Some(key) || document_contains_key(nested, key)
        }),
        Value::Sequence(sequence) => sequence
            .iter()
            .any(|nested| document_contains_key(nested, key)),
        _ => false,
    }
}

#[test]
fn the_remaining_neutral_files_are_well_formed_yaml() {
    for relative in [
        "config/examples/paseo-supervision.yml",
        "config/examples/attention.yml",
        "config/examples/quota-signals.yml",
        "config/examples/tracker/jira.yml",
        "config/examples/packs/team.yml",
        "config/examples/packs/profiles.yml",
        "config/examples/packs/operational-domain.yml",
        "config/examples/packs/committee-presets.yml",
        "config/examples/orchestration/teams/core-team.yml",
        "config/examples/orchestration/route-qualifications.yml",
        "config/examples/orchestration/tpm-supervision.yml",
        "config/examples/orchestration/pr-desk.yml",
        "config/examples/forge/github-app.example.yml",
    ] {
        let value = read(relative);
        assert!(
            value.get("schema_version").is_some(),
            "{relative} declares schema_version"
        );
    }

    let tracker = read("config/examples/tracker/jira.yml");
    assert_eq!(
        tracker.get("schema_version").and_then(Value::as_i64),
        Some(1)
    );
    for key in ["kind", "site", "project_key", "credential_alias"] {
        assert!(
            !required_string(&tracker, key, "tracker/jira.yml").is_empty(),
            "tracker/jira.yml {key} is non-empty"
        );
    }
    assert!(
        tracker.get("specs").and_then(Value::as_sequence).is_some(),
        "tracker/jira.yml declares its specs list"
    );

    let pack = read("config/examples/packs/team.yml");
    for key in ["profile_pack", "operational_domain", "committee_presets"] {
        let reference = required_string(&pack, key, "packs/team.yml");
        assert!(is_safe_relative_path(&reference));
        assert!(read(&format!("config/examples/{reference}")).is_mapping());
    }
}

fn route_slots(host: &Value, domain: &str) -> Vec<String> {
    host.get("routes")
        .and_then(|routes| routes.get(domain))
        .map(|slots| schema_keys(slots, &format!("routes.{domain}")))
        .unwrap_or_default()
}

fn parse_qualification(reference: &str) -> Result<(&str, &str), &'static str> {
    let (route, revision) = reference
        .split_once('@')
        .ok_or("route_qualification_malformed")?;
    if route.is_empty()
        || revision.is_empty()
        || revision.contains('@')
        || route.chars().any(char::is_whitespace)
        || revision.chars().any(char::is_whitespace)
    {
        return Err("route_qualification_malformed");
    }
    Ok((route, revision))
}

fn qualification_hash(record: &Value) -> String {
    let efforts = schema_keys(&record["efforts"], "qualification efforts").join(",");
    let canonical = format!(
        "route={}\nharness={}\nmodel={}\ncanonical_model_id={}\ncanonical_vendor={}\nroute_model_id={}\nefforts={}\nrevision={}",
        record["route"].as_str().unwrap_or_default(),
        record["harness"].as_str().unwrap_or_default(),
        record["model"].as_str().unwrap_or_default(),
        record["canonical_model_id"].as_str().unwrap_or_default(),
        record["canonical_vendor"].as_str().unwrap_or_default(),
        record["route_model_id"].as_str().unwrap_or_default(),
        efforts,
        record["revision"].as_str().unwrap_or_default(),
    );
    ContentHash::of(canonical.as_bytes()).as_str().to_owned()
}

struct QualificationScope<'a> {
    route: &'a str,
    harness: &'a str,
    model: &'a str,
    canonical_model_id: &'a str,
    canonical_vendor: &'a str,
    route_model_id: &'a str,
    effort: &'a str,
    revision: &'a str,
}

#[derive(Clone)]
struct QualificationFixture {
    records: Value,
    activation: Value,
}

fn qualification_activation() -> Value {
    let source_hash = ContentHash::of(text(QUALIFICATIONS).as_bytes())
        .as_str()
        .to_owned();
    serde_yaml_ng::from_str(&format!(
        "schema_version: 1\nsource: {QUALIFICATIONS}\nsource_hash: '{source_hash}'\nactivated_at: '2026-10-20T00:00:00Z'\n"
    ))
    .expect("the activation fixture is valid YAML")
}

fn qualifications() -> QualificationFixture {
    QualificationFixture {
        records: read(QUALIFICATIONS),
        activation: qualification_activation(),
    }
}

fn proven_qualification<'a>(
    qualifications: &'a QualificationFixture,
    scope: &QualificationScope<'_>,
) -> Result<&'a Value, &'static str> {
    let source_hash = ContentHash::of(text(QUALIFICATIONS).as_bytes());
    let activation = &qualifications.activation;
    if qualifications.records["schema_version"].as_i64() != Some(1)
        || activation["schema_version"].as_i64() != Some(1)
        || activation["source"].as_str() != Some(QUALIFICATIONS)
        || activation["source_hash"].as_str() != Some(source_hash.as_str())
        || activation["activated_at"]
            .as_str()
            .is_none_or(str::is_empty)
    {
        return Err("route_qualification_unproven");
    }
    let record = qualifications.records["records"]
        .as_sequence()
        .ok_or("route_qualification_unproven")?
        .iter()
        .find(|record| {
            record["route"].as_str() == Some(scope.route)
                && record["revision"].as_str() == Some(scope.revision)
        })
        .ok_or("route_qualification_unproven")?;
    if record["harness"].as_str() != Some(scope.harness)
        || record["model"].as_str() != Some(scope.model)
        || record["canonical_model_id"].as_str() != Some(scope.canonical_model_id)
        || record["canonical_vendor"].as_str() != Some(scope.canonical_vendor)
        || record["route_model_id"].as_str() != Some(scope.route_model_id)
        || !schema_keys(&record["efforts"], "qualification efforts")
            .iter()
            .any(|candidate| candidate == scope.effort)
    {
        return Err("route_qualification_unproven");
    }
    let expected = record["record_hash"]
        .as_str()
        .ok_or("route_qualification_unproven")?;
    if expected != qualification_hash(record) {
        return Err("route_qualification_hash_mismatch");
    }
    Ok(record)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedRoute {
    slot: String,
    route_provider: String,
    route_harness: String,
    native_alias: String,
    qualification: Option<String>,
    route_model_id: String,
    canonical_model: String,
    canonical_effort: String,
    canonical_vendor: String,
}

/// The successor route-compatibility rule: the fleet supplies the canonical
/// model, effort and vendor; the host route maps the canonical model to the
/// model id it actually launches, and a cross-provider route needs an activated,
/// hash-bound qualification record covering the pinned model and effort.
fn eligible_route(
    fleet: &Value,
    host: &Value,
    qualifications: &QualificationFixture,
    slot: &str,
    model: &str,
    effort: &str,
) -> Result<ResolvedRoute, &'static str> {
    let account = host
        .get("provider_accounts")
        .and_then(|accounts| accounts.get(slot))
        .ok_or("route_missing")?;
    let fleet_model = &fleet["models"][model];
    let canonical_model_id = fleet_model["id"].as_str().ok_or("canonical_model")?;
    let fleet_efforts = schema_keys(&fleet_model["efforts"], "fleet efforts");
    if !fleet_efforts.iter().any(|candidate| candidate == effort) {
        return Err("fleet_effort_unsupported");
    }

    let route_model_id = account
        .get("serves")
        .and_then(Value::as_mapping)
        .ok_or("route_not_served")?
        .get(model)
        .and_then(Value::as_str)
        .ok_or("route_not_served")?;
    if route_model_id.is_empty() {
        return Err("route_not_served");
    }

    if let Some(route_efforts) = account.get("efforts") {
        let route_efforts = schema_keys(route_efforts, "route efforts");
        if !route_efforts.iter().any(|candidate| candidate == effort) {
            return Err("route_effort_unsupported");
        }
    }

    let domain = fleet_model["domain"].as_str().ok_or("fleet_model_domain")?;
    let fleet_provider = fleet["domains"][domain]["provider"]
        .as_str()
        .ok_or("fleet_provider")?;
    let route_provider = account["provider"].as_str().ok_or("route_provider")?;
    let route_harness = account["harness"].as_str().ok_or("route_harness")?;
    let qualification = account
        .get("qualification")
        .and_then(Value::as_str)
        .map(str::to_owned);

    if let Some(reference) = qualification.as_deref() {
        let (qualification_route, revision) = parse_qualification(reference)?;
        if qualification_route != route_provider {
            return Err("route_unqualified");
        }
        proven_qualification(
            qualifications,
            &QualificationScope {
                route: route_provider,
                harness: route_harness,
                model,
                canonical_model_id,
                canonical_vendor: fleet_model["vendor"].as_str().ok_or("canonical_vendor")?,
                route_model_id,
                effort,
                revision,
            },
        )?;
    } else {
        if route_provider != fleet_provider {
            return Err("route_unqualified");
        }
        if route_model_id != canonical_model_id {
            return Err("route_model_id_unproven");
        }
    }

    Ok(ResolvedRoute {
        slot: slot.to_owned(),
        route_provider: route_provider.to_owned(),
        route_harness: route_harness.to_owned(),
        native_alias: account["native_alias"]
            .as_str()
            .ok_or("route_native_alias")?
            .to_owned(),
        qualification,
        route_model_id: route_model_id.to_owned(),
        canonical_model: canonical_model_id.to_owned(),
        canonical_effort: effort.to_owned(),
        canonical_vendor: fleet_model["vendor"]
            .as_str()
            .ok_or("canonical_vendor")?
            .to_owned(),
    })
}

fn resolve_binding_avoiding(
    fleet: &Value,
    host: &Value,
    qualifications: &QualificationFixture,
    project: &Value,
    binding: &str,
    excluded_vendors: &[&str],
) -> Result<ResolvedRoute, &'static str> {
    let chain_name = fleet["bindings"][binding]
        .as_str()
        .ok_or("binding_missing")?;
    let steps = fleet["chains"][chain_name]
        .as_sequence()
        .ok_or("chain_missing")?;
    let mut last = "no_compatible_route";
    let mut blocked_by_independence = false;
    for step in steps {
        let entries = step.as_sequence().ok_or("step_missing")?;
        for entry in entries {
            let entry = entry.as_str().ok_or("entry_missing")?;
            let (model, effort) = entry.split_once('@').ok_or("entry_malformed")?;
            let domain = fleet["models"][model]["domain"]
                .as_str()
                .ok_or("model_domain")?;
            for slot in route_slots(host, domain) {
                let account = match host
                    .get("provider_accounts")
                    .and_then(|accounts| accounts.get(slot.as_str()))
                {
                    Some(account) => account,
                    None => continue,
                };
                if route_pair_allowed(project, account).is_err() {
                    last = "runtime_pair_not_allowed";
                    continue;
                }
                match eligible_route(fleet, host, qualifications, &slot, model, effort) {
                    Ok(route) => {
                        if excluded_vendors.contains(&route.canonical_vendor.as_str()) {
                            blocked_by_independence = true;
                            last = "vendor_independence_unpreserved";
                            continue;
                        }
                        return Ok(route);
                    }
                    Err(reason) => last = reason,
                }
            }
        }
    }
    if blocked_by_independence {
        Err("vendor_independence_unpreserved")
    } else {
        Err(last)
    }
}

fn resolve_binding(
    fleet: &Value,
    host: &Value,
    qualifications: &QualificationFixture,
    project: &Value,
    binding: &str,
) -> Result<ResolvedRoute, &'static str> {
    resolve_binding_avoiding(fleet, host, qualifications, project, binding, &[])
}

/// Resolve one fleet model through a host's logical slot order. This is the
/// join WP-08 implements: the fleet supplies the canonical model id, effort and
/// vendor; the host supplies the route-native model id, native alias, provider
/// and harness.
fn resolve_on_host(
    fleet: &Value,
    host: &Value,
    domain: &str,
    model: &str,
    effort: &str,
) -> ResolvedRoute {
    let no_qualifications = QualificationFixture {
        records: Value::Mapping(Mapping::new()),
        activation: Value::Null,
    };
    for slot in route_slots(host, domain) {
        if let Ok(route) = eligible_route(fleet, host, &no_qualifications, &slot, model, effort) {
            return route;
        }
    }
    panic!("{domain} has an available route for {model}@{effort}");
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouteSnapshot {
    policy_hash: String,
    host_route_hash: String,
    slot: String,
    route_provider: String,
    route_harness: String,
    native_alias: String,
    qualification: Option<String>,
    route_model_id: String,
    canonical_model: String,
    canonical_effort: String,
    canonical_vendor: String,
    resolved_at: String,
}

fn route_snapshot(
    route: &ResolvedRoute,
    fleet_text: &str,
    host_text: &str,
    resolved_at: &str,
) -> RouteSnapshot {
    RouteSnapshot {
        policy_hash: ContentHash::of(fleet_text.as_bytes()).as_str().to_owned(),
        host_route_hash: ContentHash::of(host_text.as_bytes()).as_str().to_owned(),
        slot: route.slot.clone(),
        route_provider: route.route_provider.clone(),
        route_harness: route.route_harness.clone(),
        native_alias: route.native_alias.clone(),
        qualification: route.qualification.clone(),
        route_model_id: route.route_model_id.clone(),
        canonical_model: route.canonical_model.clone(),
        canonical_effort: route.canonical_effort.clone(),
        canonical_vendor: route.canonical_vendor.clone(),
        resolved_at: resolved_at.to_owned(),
    }
}

#[test]
fn the_same_fleet_resolves_under_two_different_hosts() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let first = read("config/examples/host.example.yml");
    let second = read("tests/fixtures/config/host-alternate.yml");

    let first_route = resolve_on_host(&fleet, &first, "claude", "opus-example", "xhigh");
    let second_route = resolve_on_host(&fleet, &second, "claude", "opus-example", "xhigh");

    assert_eq!(first_route.slot, "claude-primary");
    assert_eq!(second_route.slot, "claude-secondary");
    assert_ne!(
        first_route.native_alias, second_route.native_alias,
        "the two hosts use different native aliases"
    );
    assert_eq!(
        first_route.route_provider, second_route.route_provider,
        "both are Claude routes"
    );
    assert_eq!(
        first_route.canonical_model, second_route.canonical_model,
        "the canonical model id is unchanged"
    );
    assert_eq!(
        first_route.route_model_id, second_route.route_model_id,
        "the route-native model id is unchanged for the same provider"
    );
    assert_eq!(
        first_route.canonical_effort, second_route.canonical_effort,
        "the effort is unchanged"
    );
    assert_eq!(
        first_route.canonical_vendor, second_route.canonical_vendor,
        "the vendor is unchanged"
    );

    // A host that loses its preferred slot falls through to the next logical
    // slot with the same canonical model, effort and vendor.
    let mut fallback_host = first.clone();
    fallback_host["routes"]["claude"] =
        Value::Sequence(vec![Value::String("claude-secondary".to_owned())]);
    let fallback = resolve_on_host(&fleet, &fallback_host, "claude", "opus-example", "xhigh");
    assert_eq!(fallback.slot, "claude-secondary");
    assert_eq!(fallback.canonical_model, "claude-opus-example");
    assert_eq!(fallback.canonical_effort, "xhigh");
    assert_eq!(fallback.canonical_vendor, "anthropic");

    // PIN-001: the snapshot is bound to the placement. A later host edit
    // changes the new placement's host-route hash, never the frozen canonical
    // fields or the old snapshot.
    let fleet_text = text("config/examples/orchestration/fleet.yml");
    let first_text = text("config/examples/host.example.yml");
    let frozen = route_snapshot(
        &first_route,
        &fleet_text,
        &first_text,
        "2026-10-20T00:00:00Z",
    );
    let mut edited_host = first.clone();
    edited_host["provider_accounts"]["claude-primary"]["native_alias"] =
        Value::String("laptop-claude-renamed".to_owned());
    let edited_text = serde_yaml_ng::to_string(&edited_host).expect("the edited host renders");
    let new_placement = resolve_on_host(&fleet, &edited_host, "claude", "opus-example", "xhigh");
    let new_snapshot = route_snapshot(
        &new_placement,
        &fleet_text,
        &edited_text,
        "2026-10-20T00:01:00Z",
    );

    assert_ne!(
        frozen.host_route_hash, new_snapshot.host_route_hash,
        "the host edit changes the new placement's host-route hash"
    );
    assert_eq!(frozen.native_alias, "laptop-claude-1");
    assert_eq!(new_snapshot.native_alias, "laptop-claude-renamed");
    assert_eq!(frozen.canonical_model, new_snapshot.canonical_model);
    assert_eq!(frozen.route_model_id, new_snapshot.route_model_id);
    assert_eq!(frozen.canonical_effort, new_snapshot.canonical_effort);
    assert_eq!(frozen.canonical_vendor, new_snapshot.canonical_vendor);
    assert_eq!(frozen.resolved_at, "2026-10-20T00:00:00Z");
}

const PROJECT: &str = "config/examples/orchestration/orchestration.yml";
const QUALIFICATIONS: &str = "config/examples/orchestration/route-qualifications.yml";

fn project() -> Value {
    read(PROJECT)["project"].clone()
}
const OPEN_BINDING: &str = "team/01936f5a-0000-7000-8000-000000000001/open";
const FALLBACK_BINDING: &str = "team/01936f5a-0000-7000-8000-000000000001/fallback";
const IMPLEMENT_BINDING: &str = "team/01936f5a-0000-7000-8000-000000000001/implement";
const VERIFY_BINDING: &str = "team/01936f5a-0000-7000-8000-000000000001/verify";

#[test]
fn the_same_fleet_resolves_through_a_different_provider_route() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let opencode_route = resolve_binding(
        &fleet,
        &read("config/examples/host.example.yml"),
        &qualifications,
        &project(),
        OPEN_BINDING,
    )
    .expect("the OpenCode route serves the canonical model");
    let acp_route = resolve_binding(
        &fleet,
        &read("tests/fixtures/config/host-cross-provider.yml"),
        &qualifications,
        &project(),
        OPEN_BINDING,
    )
    .expect("the qualified ACP route serves the same canonical model");

    assert_eq!(opencode_route.route_provider, "opencode");
    assert_eq!(opencode_route.route_harness, "opencode");
    assert_eq!(acp_route.route_provider, "glm-acp-agent");
    assert_eq!(acp_route.route_harness, "acp-agent");
    assert_eq!(acp_route.qualification.as_deref(), Some("glm-acp-agent@1"));

    assert_eq!(opencode_route.canonical_model, acp_route.canonical_model);
    assert_eq!(opencode_route.canonical_effort, acp_route.canonical_effort);
    assert_eq!(opencode_route.canonical_vendor, acp_route.canonical_vendor);
    assert_eq!(opencode_route.canonical_model, "example/model");
    assert_eq!(opencode_route.canonical_effort, "max");
    assert_eq!(opencode_route.canonical_vendor, "example");
    assert_eq!(opencode_route.route_model_id, "example/model");
    assert_eq!(acp_route.route_model_id, "glm-5.3");
    assert_ne!(
        opencode_route.route_model_id, acp_route.route_model_id,
        "the route launches its own model id while the canonical model is equal"
    );
    assert_ne!(opencode_route.native_alias, acp_route.native_alias);
}

#[test]
fn an_unqualified_cross_provider_route_is_refused() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let mut host = read("tests/fixtures/config/host-cross-provider.yml");
    host["provider_accounts"]["opencode"]
        .as_mapping_mut()
        .expect("the opencode route")
        .remove("qualification");
    assert_eq!(
        resolve_binding(&fleet, &host, &qualifications, &project(), OPEN_BINDING),
        Err("route_unqualified"),
        "a cross-provider substitution without a qualification is refused"
    );

    for reference in [
        "glm-acp-agent-anything@1",
        "glm-acp-agent@1@extra",
        "glm-acp-agent@NOT-A-REVISION",
        "glm-acp-agent@1 ",
    ] {
        let mut wrong = read("tests/fixtures/config/host-cross-provider.yml");
        wrong["provider_accounts"]["opencode"]["qualification"] =
            Value::String(reference.to_owned());
        assert!(
            matches!(
                resolve_binding(&fleet, &wrong, &qualifications, &project(), OPEN_BINDING),
                Err("route_unqualified")
                    | Err("route_qualification_malformed")
                    | Err("route_qualification_unproven")
            ),
            "{reference} is refused, not accepted as proof"
        );
    }
}

#[test]
fn qualification_activation_and_records_are_hash_bound() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let cross = read("tests/fixtures/config/host-cross-provider.yml");

    let base = qualifications();

    let mut missing_record = base.clone();
    missing_record.records["records"] = Value::Sequence(vec![]);
    assert_eq!(
        resolve_binding(&fleet, &cross, &missing_record, &project(), OPEN_BINDING),
        Err("route_qualification_unproven"),
        "a reference with no matching record is refused"
    );

    let mut missing_activation = base.clone();
    missing_activation.activation = Value::Null;
    assert_eq!(
        resolve_binding(
            &fleet,
            &cross,
            &missing_activation,
            &project(),
            OPEN_BINDING
        ),
        Err("route_qualification_unproven"),
        "a record source with no activation record is refused"
    );

    let mut wrong_source_hash = base.clone();
    wrong_source_hash.activation["source_hash"] = Value::String("0".repeat(64));
    assert_eq!(
        resolve_binding(&fleet, &cross, &wrong_source_hash, &project(), OPEN_BINDING),
        Err("route_qualification_unproven"),
        "an activation record that binds different file bytes is refused"
    );

    let mut missing_time = base.clone();
    missing_time.activation["activated_at"] = Value::Null;
    assert_eq!(
        resolve_binding(&fleet, &cross, &missing_time, &project(), OPEN_BINDING),
        Err("route_qualification_unproven"),
        "an activation record without a timestamp is refused"
    );

    let mut changed_time = base.clone();
    changed_time.activation["activated_at"] = Value::String("2026-10-21T00:00:00Z".to_owned());
    assert!(
        resolve_binding(&fleet, &cross, &changed_time, &project(), OPEN_BINDING).is_ok(),
        "activation time is carried by the activation record, not the record hash"
    );

    let mut tampered = base.clone();
    tampered.records["records"][0]["efforts"] =
        serde_yaml_ng::from_str("[high, max, low]").expect("an effort superset");
    assert_eq!(
        resolve_binding(&fleet, &cross, &tampered, &project(), OPEN_BINDING),
        Err("route_qualification_hash_mismatch"),
        "a record edited after approval no longer hash-matches"
    );

    let mut wrong_model = base.clone();
    wrong_model.records["records"][0]["model"] = Value::String("sol-example".to_owned());
    let rehashed = qualification_hash(&wrong_model.records["records"][0]);
    wrong_model.records["records"][0]["record_hash"] = Value::String(rehashed);
    assert_eq!(
        resolve_binding(&fleet, &cross, &wrong_model, &project(), OPEN_BINDING),
        Err("route_qualification_unproven"),
        "a validly hashed record that does not cover the pinned model is refused"
    );

    let mut wrong_effort = base.clone();
    wrong_effort.records["records"][0]["efforts"] =
        serde_yaml_ng::from_str("[high]").expect("an effort subset");
    let rehashed = qualification_hash(&wrong_effort.records["records"][0]);
    wrong_effort.records["records"][0]["record_hash"] = Value::String(rehashed);
    assert_eq!(
        resolve_binding(&fleet, &cross, &wrong_effort, &project(), OPEN_BINDING),
        Err("route_qualification_unproven"),
        "a validly hashed record that omits the pinned effort is refused"
    );
}

#[test]
fn qualification_binds_canonical_identity_and_route_model_id() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let cross = read("tests/fixtures/config/host-cross-provider.yml");
    assert!(
        resolve_binding(&fleet, &cross, &qualifications, &project(), OPEN_BINDING).is_ok(),
        "the baseline qualified route is accepted"
    );

    let mut changed_launch_id = cross.clone();
    changed_launch_id["provider_accounts"]["opencode"]["serves"]["open-example"] =
        Value::String("unqualified-other-vendor-model".to_owned());
    assert_eq!(
        resolve_binding(
            &fleet,
            &changed_launch_id,
            &qualifications,
            &project(),
            OPEN_BINDING
        ),
        Err("route_qualification_unproven"),
        "a host mapping that changes the launch id no longer matches the qualified record"
    );

    let mut changed_canonical_id = fleet.clone();
    changed_canonical_id["models"]["open-example"]["id"] =
        Value::String("other-canonical-id".to_owned());
    assert_eq!(
        resolve_binding(
            &changed_canonical_id,
            &cross,
            &qualifications,
            &project(),
            OPEN_BINDING
        ),
        Err("route_qualification_unproven"),
        "a changed canonical model id no longer matches the qualified record"
    );

    let mut changed_vendor = fleet.clone();
    changed_vendor["models"]["open-example"]["vendor"] = Value::String("other-vendor".to_owned());
    assert_eq!(
        resolve_binding(
            &changed_vendor,
            &cross,
            &qualifications,
            &project(),
            OPEN_BINDING
        ),
        Err("route_qualification_unproven"),
        "a changed canonical vendor no longer matches the qualified record"
    );
}

#[test]
fn a_route_that_does_not_serve_the_model_is_refused() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let mut host = read("tests/fixtures/config/host-cross-provider.yml");
    let mut serves = Mapping::new();
    serves.insert(
        Value::String("grok-example".to_owned()),
        Value::String("grok-example".to_owned()),
    );
    host["provider_accounts"]["opencode"]["serves"] = Value::Mapping(serves);
    assert_eq!(
        resolve_binding(&fleet, &host, &qualifications, &project(), OPEN_BINDING),
        Err("route_not_served"),
        "a route that does not prove the model is not eligible"
    );
}

#[test]
fn a_same_provider_remap_without_qualification_is_refused() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let mut host = read("config/examples/host.example.yml");
    host["provider_accounts"]["opencode"]["serves"]["open-example"] =
        Value::String("unproven-route-model".to_owned());
    assert_eq!(
        resolve_binding(&fleet, &host, &qualifications, &project(), OPEN_BINDING),
        Err("route_model_id_unproven"),
        "a same-provider route cannot change the launch id without a qualification record"
    );
}

#[test]
fn an_incompatible_route_falls_through_the_configured_chain() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let cross = read("tests/fixtures/config/host-cross-provider.yml");

    // The ACP route states high/max only, so the pinned low effort is refused
    // on step one and the configured chain descends to the next policy step.
    let fallback = resolve_binding(
        &fleet,
        &cross,
        &qualifications,
        &project(),
        FALLBACK_BINDING,
    )
    .expect("the next policy step is compatible");
    assert_eq!(fallback.slot, "codex-primary");
    assert_eq!(fallback.route_provider, "codex");
    assert_eq!(fallback.canonical_model, "gpt-example");
    assert_eq!(fallback.route_model_id, "gpt-example");
    assert_eq!(fallback.canonical_effort, "xhigh");
    assert_eq!(fallback.canonical_vendor, "openai");

    // The OpenCode host can serve the pinned low effort directly; it does not
    // silently take the fallback.
    let direct = resolve_binding(
        &fleet,
        &read("config/examples/host.example.yml"),
        &qualifications,
        &project(),
        FALLBACK_BINDING,
    )
    .expect("the OpenCode route serves the low effort");
    assert_eq!(direct.slot, "opencode");
    assert_eq!(direct.canonical_model, "example/model");
    assert_eq!(direct.route_model_id, "example/model");
    assert_eq!(direct.canonical_effort, "low");
}

#[test]
fn a_chain_with_no_compatible_route_is_refused_without_substitution() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let mut host = read("tests/fixtures/config/host-cross-provider.yml");
    host["routes"]["codex"] = Value::Sequence(vec![]);
    let outcome = resolve_binding(&fleet, &host, &qualifications, &project(), FALLBACK_BINDING);
    assert_eq!(
        outcome,
        Err("route_effort_unsupported"),
        "the typed reason names the last refusal; no model or effort is substituted"
    );
}

fn independent_vendor(
    fleet: &Value,
    host: &Value,
    qualifications: &QualificationFixture,
    project: &Value,
    binding: &str,
) -> String {
    let partner = fleet["rules"]["independent_of"][binding]
        .as_str()
        .expect("the binding has an independent_of partner");
    resolve_binding(fleet, host, qualifications, project, partner)
        .expect("the partner route resolves")
        .canonical_vendor
}

#[test]
fn vendor_independence_skips_the_partners_vendor_on_a_cross_provider_route() {
    let mut fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let cross = read("tests/fixtures/config/host-cross-provider.yml");

    // Route the policy's implement binding through the qualified open route,
    // so the vendor this reviewer must avoid is read from independent_of.
    fleet["bindings"][IMPLEMENT_BINDING] = Value::String("open-route".to_owned());
    let implement = resolve_binding(
        &fleet,
        &cross,
        &qualifications,
        &project(),
        IMPLEMENT_BINDING,
    )
    .expect("the ACP route places the first seat");
    assert_eq!(implement.canonical_vendor, "example");
    assert_eq!(implement.route_provider, "glm-acp-agent");

    let excluded = independent_vendor(&fleet, &cross, &qualifications, &project(), VERIFY_BINDING);
    assert_eq!(excluded, implement.canonical_vendor);
    let verify = resolve_binding_avoiding(
        &fleet,
        &cross,
        &qualifications,
        &project(),
        VERIFY_BINDING,
        &[excluded.as_str()],
    )
    .expect("an independent route remains");
    assert_ne!(verify.canonical_vendor, implement.canonical_vendor);
    assert_eq!(verify.canonical_vendor, "openai");
    assert_eq!(verify.canonical_model, "gpt-example");
}

#[test]
fn vendor_independence_falls_through_the_chain_without_relaxing_the_vendor() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let mut host = read("config/examples/host.example.yml");
    host["routes"]["claude"] = Value::Sequence(vec![]);

    let implement = resolve_binding(
        &fleet,
        &host,
        &qualifications,
        &project(),
        IMPLEMENT_BINDING,
    )
    .expect("the codex route implements");
    assert_eq!(implement.canonical_vendor, "openai");

    let excluded = independent_vendor(&fleet, &host, &qualifications, &project(), VERIFY_BINDING);
    assert_eq!(excluded, implement.canonical_vendor);
    let verify = resolve_binding_avoiding(
        &fleet,
        &host,
        &qualifications,
        &project(),
        VERIFY_BINDING,
        &[excluded.as_str()],
    )
    .expect("the next independent chain step is compatible");
    assert_eq!(verify.canonical_vendor, "xai");
    assert_eq!(verify.canonical_model, "grok-example");
    assert_eq!(verify.canonical_effort, "high");
}

#[test]
fn vendor_independence_is_refused_when_no_route_remains() {
    let mut fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let mut host = read("tests/fixtures/config/host-cross-provider.yml");
    for domain in ["claude", "codex", "cursor"] {
        host["routes"][domain] = Value::Sequence(vec![]);
    }
    fleet["bindings"][IMPLEMENT_BINDING] = Value::String("open-route".to_owned());

    let implement = resolve_binding(
        &fleet,
        &host,
        &qualifications,
        &project(),
        IMPLEMENT_BINDING,
    )
    .expect("the only remaining route implements");
    assert_eq!(implement.canonical_vendor, "example");

    let excluded = independent_vendor(&fleet, &host, &qualifications, &project(), VERIFY_BINDING);
    assert_eq!(excluded, implement.canonical_vendor);
    assert_eq!(
        resolve_binding_avoiding(
            &fleet,
            &host,
            &qualifications,
            &project(),
            VERIFY_BINDING,
            &[excluded.as_str()],
        ),
        Err("vendor_independence_unpreserved"),
        "placement is refused rather than reusing the partner's vendor"
    );
}

#[test]
fn the_project_runtime_matrix_allows_every_host_route() {
    let project = read("config/examples/orchestration/orchestration.yml");
    let project = &project["project"];
    let pairs = runtime_pairs(project);
    for relative in [
        "config/examples/host.example.yml",
        "tests/fixtures/config/host-alternate.yml",
        "tests/fixtures/config/host-cross-provider.yml",
    ] {
        let host = read(relative);
        for (slot, account) in map(&host["provider_accounts"], "provider_accounts") {
            let slot = slot.as_str().expect("a logical slot");
            let pair = (
                required_string(account, "harness", slot),
                required_string(account, "provider", slot),
            );
            assert!(
                pairs.iter().any(|allowed| allowed == &pair),
                "{relative}: {slot} route {pair:?} is allowed by project.runtime.matrix"
            );
            assert_eq!(
                route_pair_allowed(project, account),
                Ok(()),
                "{relative}: {slot} passes the project allowlist"
            );
        }
    }
}

#[test]
fn placement_refuses_a_route_outside_the_project_matrix() {
    let fleet = read("config/examples/orchestration/fleet.yml");
    let qualifications = qualifications();
    let mut host = read("config/examples/host.example.yml");
    host["provider_accounts"]["opencode"]["harness"] = Value::String("unlisted-harness".to_owned());
    assert_eq!(
        resolve_binding(&fleet, &host, &qualifications, &project(), OPEN_BINDING),
        Err("runtime_pair_not_allowed"),
        "placement refuses a host route not listed by project.runtime.matrix"
    );
}

#[test]
fn capacity_separates_delivery_seats_from_the_admission_ceiling() {
    for (relative, delivery_seats, ceiling, start_admission) in [
        ("config/examples/orchestration/orchestration.yml", 1, 2, 1),
        ("tests/fixtures/config/asma-orchestration.yml", 2, 7, 4),
    ] {
        let project = read(relative);
        let capacity = &project["project"]["capacity"];
        assert_eq!(
            capacity["max_delivery_seats"].as_i64(),
            Some(delivery_seats),
            "{relative}: max_delivery_seats"
        );
        assert_eq!(
            capacity["max_parallel_tasks"].as_i64(),
            Some(ceiling),
            "{relative}: max_parallel_tasks is the Paseo-direct placement ceiling"
        );
        assert_eq!(
            capacity["start_admission"].as_i64(),
            Some(start_admission),
            "{relative}: start_admission"
        );
        assert!(
            start_admission <= ceiling,
            "{relative}: start_admission must not exceed the ceiling"
        );
    }
}

#[test]
fn supervision_names_the_tpm_profile_not_the_paseo_watchdog() {
    for relative in [
        "config/examples/orchestration/orchestration.yml",
        "tests/fixtures/config/asma-orchestration.yml",
    ] {
        let document = read(relative);
        let project = &document["project"];
        assert_eq!(
            project["supervision"].as_str(),
            Some("orchestration/tpm-supervision.yml"),
            "{relative}: project.supervision names the TPM cadence profile"
        );
        assert!(
            project.get("paseo_supervision").is_none(),
            "{relative}: the Paseo watchdog is not a project key"
        );
    }

    let root = repo_root().join("config/examples");
    assert!(
        root.join("orchestration/tpm-supervision.yml").is_file(),
        "the neutral TPM cadence profile exists"
    );
    assert!(
        root.join("paseo-supervision.yml").is_file(),
        "the selected folder contains the Paseo watchdog policy"
    );

    let schema = read("config/examples/orchestration/orchestration.schema.yml");
    let contract = &schema["supervision_contract"];
    assert!(
        contract["project.supervision"]
            .as_str()
            .is_some_and(|text| text.contains("TPM cadence profile")),
        "the schema names project.supervision as the TPM profile"
    );
    assert!(
        contract["paseo_watchdog"]
            .as_str()
            .is_some_and(|text| text.contains("paseo-supervision.yml in the selected folder")),
        "the schema names the selected-folder Paseo watchdog path"
    );

    let docs = text("docs/CONFIGURATION.md");
    assert!(docs.contains("project.supervision"));
    assert!(docs.contains(
        "configured daemon startup reads\n`paseo-supervision.yml` from the selected folder"
    ));
}

#[test]
fn the_neutral_tpm_supervision_profile_has_the_consumer_shape() {
    let profile = read("config/examples/orchestration/tpm-supervision.yml");
    assert_eq!(profile["schema_version"].as_i64(), Some(1));
    assert_eq!(
        profile["profile_id"].as_str(),
        Some("autonomous_tpm_v1"),
        "the consumer requires the autonomous TPM profile id"
    );
    for key in [
        "cadence",
        "turn",
        "continuity",
        "wake",
        "recovery",
        "lifecycle",
        "surfaces",
    ] {
        assert!(
            profile.get(key).and_then(Value::as_mapping).is_some(),
            "tpm-supervision.yml.{key} is a mapping"
        );
    }
    for key in [
        "heartbeat_cron",
        "timezone",
        "liveness_cron",
        "stale_after_minutes",
        "lifetime_hours",
        "renew_before_hours",
    ] {
        assert!(
            profile["cadence"].get(key).is_some(),
            "tpm-supervision.yml.cadence.{key} is required"
        );
    }
    for key in [
        "context_max_tokens",
        "checkpoint_max_tokens",
        "recent_event_limit",
        "actionable_item_limit",
        "budget_minutes",
        "checkpoint_reserve_minutes",
    ] {
        assert!(
            profile["turn"].get(key).is_some(),
            "tpm-supervision.yml.turn.{key} is required"
        );
    }
    for key in [
        "same_tpm_native",
        "context_refresh",
        "transcript_authoritative",
        "checkpoint_required_before_turn_end",
        "checkpoint_backend",
        "optional_memory",
    ] {
        assert!(
            profile["continuity"].get(key).is_some(),
            "tpm-supervision.yml.continuity.{key} is required"
        );
    }
    for key in ["paseo-direct", "kontor-governed"] {
        assert!(
            profile["continuity"]["checkpoint_backend"]
                .get(key)
                .is_some(),
            "tpm-supervision.yml.continuity.checkpoint_backend.{key} is required"
        );
    }
    for key in [
        "primary",
        "safety",
        "overlap",
        "unchanged",
        "zero_inference_when_unchanged",
    ] {
        assert!(
            profile["wake"].get(key).is_some(),
            "tpm-supervision.yml.wake.{key} is required"
        );
    }
    for key in [
        "preferred",
        "replacement",
        "retry_limit_per_unchanged_blocker",
        "blocker_requires",
    ] {
        assert!(
            profile["recovery"].get(key).is_some(),
            "tpm-supervision.yml.recovery.{key} is required"
        );
    }
    for key in ["complete", "paused", "blocked", "ownership_transfer"] {
        assert!(
            profile["lifecycle"].get(key).is_some(),
            "tpm-supervision.yml.lifecycle.{key} is required"
        );
    }
    for key in ["authority", "scheduling", "kontor_required", "guarantees"] {
        assert!(
            profile["surfaces"]["paseo-direct"].get(key).is_some(),
            "tpm-supervision.yml.surfaces.paseo-direct.{key} is required"
        );
    }
    for key in [
        "authority",
        "scheduling",
        "guarantees",
        "duplicate_admission_writer",
    ] {
        assert!(
            profile["surfaces"]["kontor-governed"].get(key).is_some(),
            "tpm-supervision.yml.surfaces.kontor-governed.{key} is required"
        );
    }
}

#[test]
fn the_asma_fixture_uses_the_declared_folder_layout() {
    let document = read("tests/fixtures/config/asma-orchestration.yml");
    assert_eq!(document["fleet"].as_str(), Some("orchestration/fleet.yml"));
    assert_eq!(
        document["core_team"].as_str(),
        Some("orchestration/teams/core-team.yml")
    );
    let project = &document["project"];
    assert_eq!(project["fleet"].as_str(), Some("orchestration/fleet.yml"));
    assert_eq!(
        project["supervision"].as_str(),
        Some("orchestration/tpm-supervision.yml")
    );
    assert_eq!(
        project["tracker"]["spec"].as_str(),
        Some("tracker/jira.yml"),
        "the ASMA tracker spec is folder-root relative, not config/tracker"
    );
    assert_eq!(project["team_pack"].as_str(), Some("packs/team.yml"));
    assert_eq!(
        project["pr_desk"]["settings"].as_str(),
        Some("orchestration/pr-desk.yml")
    );
    for reference in [
        project["fleet"].as_str().expect("fleet"),
        project["supervision"].as_str().expect("supervision"),
        project["tracker"]["spec"].as_str().expect("tracker spec"),
        project["pr_desk"]["settings"]
            .as_str()
            .expect("pr desk settings"),
    ] {
        assert!(
            is_safe_relative_path(reference),
            "ASMA config reference {reference} is safe"
        );
        assert!(
            !reference.starts_with("config/"),
            "ASMA config reference {reference} does not repeat the folder name"
        );
    }
    let roots = map(&project["resource_roots"], "project.resource_roots");
    assert_eq!(
        roots.get("modules").and_then(Value::as_str),
        Some("Carasent-ASMA/asma-modules")
    );
    assert_eq!(
        roots.get("kontor").and_then(Value::as_str),
        Some("Carasent-ASMA/asma-rs-kontor")
    );
}

#[test]
fn path_classes_are_distinct() {
    let root = repo_root().join("config/examples");
    let orchestration = read("config/examples/orchestration/orchestration.yml");
    let project = &orchestration["project"];

    // Project resource paths belong to the project root, not the config folder.
    for path in [
        project["documentation_roots"][0]
            .as_str()
            .expect("docs root"),
        project["gap_ledger"].as_str().expect("gap ledger"),
        project["glossary"].as_str().expect("glossary"),
        project["decision_authorities"]["decision_records_root"]
            .as_str()
            .expect("decision root"),
    ] {
        assert!(
            !root.join(path).exists(),
            "project resource {path} is not captured inside the config folder"
        );
    }
    assert!(
        project["worktree_layout"]
            .as_str()
            .expect("worktree layout")
            .contains("<key-slug>"),
        "worktree_layout is a convention, not a resolved path"
    );

    // Host paths are absolute and validated as host state, not as config refs.
    let host = read("config/examples/host.example.yml");
    for (name, path) in map(&host["paths"], "paths") {
        let name = name.as_str().expect("a host path name");
        let path = path.as_str().expect("a host path");
        assert!(
            Path::new(path).is_absolute(),
            "host.paths.{name} is an absolute host path"
        );
        assert!(
            !root.join(path).exists(),
            "host.paths.{name} is not resolved from the config folder"
        );
    }
    assert!(
        Path::new(&required_string(
            &host["project_cli"],
            "example",
            "project_cli"
        ))
        .is_absolute(),
        "the project CLI binding is a host path"
    );
}

#[test]
fn the_host_file_is_ignored_beside_the_example() {
    let ignore = text("config/examples/.gitignore");
    assert!(
        ignore.lines().any(|line| line.trim() == "host.yml"),
        "host.yml is git-ignored in the example folder"
    );
}
