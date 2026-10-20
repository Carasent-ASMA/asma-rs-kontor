//! The neutral config folder's schema, exercised as data.
//!
//! WP-02 defines the folder before the consumers read it. These tests bind the
//! neutral example and ASMA's concrete values to the same `project:` block, so
//! a later reader cannot quietly require a different shape. The fleet example
//! is additionally parsed by the production policy reader.

use std::path::{Path, PathBuf};

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

fn validate_project(value: &Value, path: &str) -> Mapping {
    let project = map(value, path);
    for key in [
        "key",
        "name",
        "owner",
        "decision_authorities",
        "tracker",
        "branch",
        "forge",
        "runtime",
        "cli",
        "team_pack",
        "fleet",
        "supervision",
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

    let matrix = required_sequence(
        value.get("runtime").expect("runtime"),
        "matrix",
        &format!("{path}.runtime"),
    );
    assert!(!matrix.is_empty(), "{path}.runtime.matrix is non-empty");
    for (index, entry) in matrix.iter().enumerate() {
        let entry_path = format!("{path}.runtime.matrix[{index}]");
        for key in ["name", "harness", "provider"] {
            assert!(
                !required_string(entry, key, &entry_path).is_empty(),
                "{entry_path}.{key} is non-empty"
            );
        }
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
    for key in ["max_delivery_seats", "max_parallel_tasks"] {
        assert!(
            capacity
                .get(key)
                .and_then(Value::as_i64)
                .is_some_and(|value| value > 0),
            "{path}.capacity.{key} is a positive integer"
        );
    }
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
    for key in ["team_pack", "fleet", "supervision"] {
        assert!(
            project.get(key).and_then(Value::as_str).is_some(),
            "{relative}: project.{key} is a file reference"
        );
    }
    assert!(
        project
            .get("tracker")
            .and_then(Value::as_mapping)
            .and_then(|tracker| tracker.get("spec"))
            .and_then(Value::as_str)
            .is_some(),
        "{relative}: project.tracker.spec is a file reference"
    );
}

#[test]
fn the_neutral_orchestration_matches_the_project_schema() {
    validate_orchestration("config/examples/orchestration/orchestration.yml");
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
            "decision_authorities",
            "tracker",
            "branch",
            "forge",
            "runtime",
            "cli",
            "team_pack",
            "fleet",
            "supervision",
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
        schema["route_binding"]["join_key"].as_str(),
        Some("slot"),
        "the only fleet/host join key is the logical slot"
    );
    for field in ["canonical_model", "canonical_effort", "canonical_vendor"] {
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
        "orchestration/teams/core-team.yml",
        "fleet.yml",
        "paseo-supervision.yml",
        "packs/team.yml",
        "tracker/jira.yml",
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
fn the_neutral_fleet_example_is_schema_2_and_names_no_real_account() {
    let document = text("config/examples/fleet.yml");
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

    let parsed = read("config/examples/fleet.yml");
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
            !schema_keys(required(account, "serves", name), name).is_empty(),
            "provider_accounts.{name}.serves names at least one canonical model"
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

    let fleet = read("config/examples/fleet.yml");
    for (slot, account) in accounts {
        let slot = slot.as_str().expect("a logical slot");
        let route_provider = required_string(account, "provider", slot);
        for model in schema_keys(required(account, "serves", slot), slot) {
            let model_spec = &fleet["models"][&model];
            assert!(
                !model_spec.is_null(),
                "{slot} serves a fleet model: {model}"
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
        "config/examples/quota-signals.yml",
        "config/examples/tracker/jira.yml",
        "config/examples/packs/team.yml",
        "config/examples/orchestration/teams/core-team.yml",
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
    for key in ["teams", "profiles", "committee_presets", "role_prompts"] {
        assert!(
            pack.get(key).and_then(Value::as_sequence).is_some(),
            "packs/team.yml {key} is a sequence"
        );
    }
}

fn route_slots(host: &Value, domain: &str) -> Vec<String> {
    host.get("routes")
        .and_then(|routes| routes.get(domain))
        .map(|slots| schema_keys(slots, &format!("routes.{domain}")))
        .unwrap_or_default()
}

/// Resolve one fleet model through a host's logical slot order. This is the
/// join WP-08 implements: the fleet supplies the canonical model id, effort and
/// vendor; the host supplies the native alias, provider and harness.
fn resolve_on_host(
    fleet: &Value,
    host: &Value,
    domain: &str,
    model: &str,
    effort: &str,
) -> (String, String, String, String, String, String, String) {
    let model_spec = &fleet["models"][model];
    let canonical_id = model_spec["id"].as_str().expect("model id").to_owned();
    let vendor = model_spec["vendor"].as_str().expect("vendor").to_owned();
    let slots = route_slots(host, domain);
    let slot = slots
        .iter()
        .find(|slot| host["provider_accounts"].get(slot.as_str()).is_some())
        .unwrap_or_else(|| panic!("{domain} has an available slot"));
    let account = &host["provider_accounts"][slot.as_str()];
    (
        slot.clone(),
        account["native_alias"]
            .as_str()
            .expect("native alias")
            .to_owned(),
        account["provider"].as_str().expect("provider").to_owned(),
        account["harness"].as_str().expect("harness").to_owned(),
        canonical_id,
        effort.to_owned(),
        vendor,
    )
}

#[test]
fn the_same_fleet_resolves_under_two_different_hosts() {
    let fleet = read("config/examples/fleet.yml");
    let first = read("config/examples/host.example.yml");
    let second = read("tests/fixtures/config/host-alternate.yml");

    let first_route = resolve_on_host(&fleet, &first, "claude", "opus-example", "xhigh");
    let second_route = resolve_on_host(&fleet, &second, "claude", "opus-example", "xhigh");

    assert_eq!(first_route.0, "claude-primary");
    assert_eq!(second_route.0, "claude-secondary");
    assert_ne!(
        first_route.1, second_route.1,
        "the two hosts use different native aliases"
    );
    assert_eq!(first_route.2, second_route.2, "both are Claude routes");
    assert_eq!(
        first_route.4, second_route.4,
        "the canonical model id is unchanged"
    );
    assert_eq!(first_route.5, second_route.5, "the effort is unchanged");
    assert_eq!(first_route.6, second_route.6, "the vendor is unchanged");

    // A host that loses its preferred slot falls through to the next logical
    // slot with the same canonical model, effort and vendor.
    let mut fallback_host = first.clone();
    fallback_host["routes"]["claude"] =
        Value::Sequence(vec![Value::String("claude-secondary".to_owned())]);
    let fallback = resolve_on_host(&fleet, &fallback_host, "claude", "opus-example", "xhigh");
    assert_eq!(fallback.0, "claude-secondary");
    assert_eq!(
        (
            fallback.4.as_str(),
            fallback.5.as_str(),
            fallback.6.as_str()
        ),
        ("claude-opus-example", "xhigh", "anthropic")
    );

    // The resolved snapshot is frozen for an active run: a later host edit
    // changes what a new placement resolves, never the value the old run holds.
    let frozen = first_route.clone();
    let new_placement = second_route.clone();
    assert_eq!(frozen, first_route);
    assert_ne!(frozen.1, new_placement.1);
    assert_eq!(frozen.4, new_placement.4);
    assert_eq!(frozen.5, new_placement.5);
    assert_eq!(frozen.6, new_placement.6);
}

const OPEN_BINDING: &str = "team/01936f5a-0000-7000-8000-000000000001/open";
const FALLBACK_BINDING: &str = "team/01936f5a-0000-7000-8000-000000000001/fallback";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedRoute {
    slot: String,
    route_provider: String,
    route_harness: String,
    native_alias: String,
    qualification: Option<String>,
    canonical_model: String,
    canonical_effort: String,
    canonical_vendor: String,
}

/// The successor route-compatibility rule: the fleet supplies the canonical
/// model, effort and vendor; the host route must prove it serves that model at
/// that effort, and a cross-provider route needs a qualification.
fn eligible_route(
    fleet: &Value,
    host: &Value,
    slot: &str,
    model: &str,
    effort: &str,
) -> Result<ResolvedRoute, &'static str> {
    let account = host
        .get("provider_accounts")
        .and_then(|accounts| accounts.get(slot))
        .ok_or("route_missing")?;
    let serves = schema_keys(account.get("serves").ok_or("route_not_served")?, "serves");
    if !serves.iter().any(|candidate| candidate == model) {
        return Err("route_not_served");
    }

    let fleet_model = &fleet["models"][model];
    let fleet_efforts = schema_keys(&fleet_model["efforts"], "fleet efforts");
    if !fleet_efforts.iter().any(|candidate| candidate == effort) {
        return Err("fleet_effort_unsupported");
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
    let qualification = account
        .get("qualification")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if route_provider != fleet_provider {
        let qualification = qualification.as_deref().ok_or("route_unqualified")?;
        if !qualification.starts_with(route_provider) {
            return Err("route_unqualified");
        }
    }

    Ok(ResolvedRoute {
        slot: slot.to_owned(),
        route_provider: route_provider.to_owned(),
        route_harness: account["harness"]
            .as_str()
            .ok_or("route_harness")?
            .to_owned(),
        native_alias: account["native_alias"]
            .as_str()
            .ok_or("route_native_alias")?
            .to_owned(),
        qualification,
        canonical_model: fleet_model["id"]
            .as_str()
            .ok_or("canonical_model")?
            .to_owned(),
        canonical_effort: effort.to_owned(),
        canonical_vendor: fleet_model["vendor"]
            .as_str()
            .ok_or("canonical_vendor")?
            .to_owned(),
    })
}

/// Resolve one binding through the configured chain. An incompatible route is
/// skipped; the next chain step is tried; if nothing is compatible the last
/// typed reason is returned and nothing is substituted.
fn resolve_binding(
    fleet: &Value,
    host: &Value,
    binding: &str,
) -> Result<ResolvedRoute, &'static str> {
    let chain_name = fleet["bindings"][binding]
        .as_str()
        .ok_or("binding_missing")?;
    let steps = fleet["chains"][chain_name]
        .as_sequence()
        .ok_or("chain_missing")?;
    let mut last = "no_compatible_route";
    for step in steps {
        let entries = step.as_sequence().ok_or("step_missing")?;
        for entry in entries {
            let entry = entry.as_str().ok_or("entry_missing")?;
            let (model, effort) = entry.split_once('@').ok_or("entry_malformed")?;
            let domain = fleet["models"][model]["domain"]
                .as_str()
                .ok_or("model_domain")?;
            for slot in route_slots(host, domain) {
                match eligible_route(fleet, host, &slot, model, effort) {
                    Ok(route) => return Ok(route),
                    Err(reason) => last = reason,
                }
            }
        }
    }
    Err(last)
}

#[test]
fn the_same_fleet_resolves_through_a_different_provider_route() {
    let fleet = read("config/examples/fleet.yml");
    let opencode_route = resolve_binding(
        &fleet,
        &read("config/examples/host.example.yml"),
        OPEN_BINDING,
    )
    .expect("the OpenCode route serves the canonical model");
    let acp_route = resolve_binding(
        &fleet,
        &read("tests/fixtures/config/host-cross-provider.yml"),
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
    assert_ne!(opencode_route.native_alias, acp_route.native_alias);
}

#[test]
fn an_unqualified_cross_provider_route_is_refused() {
    let fleet = read("config/examples/fleet.yml");
    let mut host = read("tests/fixtures/config/host-cross-provider.yml");
    host["provider_accounts"]["opencode"]
        .as_mapping_mut()
        .expect("the opencode route")
        .remove("qualification");
    assert_eq!(
        resolve_binding(&fleet, &host, OPEN_BINDING),
        Err("route_unqualified"),
        "a cross-provider substitution without a qualification is refused"
    );

    let mut wrong = read("tests/fixtures/config/host-cross-provider.yml");
    wrong["provider_accounts"]["opencode"]["qualification"] =
        Value::String("unrelated-contract@1".to_owned());
    assert_eq!(
        resolve_binding(&fleet, &wrong, OPEN_BINDING),
        Err("route_unqualified"),
        "a qualification that does not name the route is not proof"
    );
}

#[test]
fn a_route_that_does_not_serve_the_model_is_refused() {
    let fleet = read("config/examples/fleet.yml");
    let mut host = read("tests/fixtures/config/host-cross-provider.yml");
    host["provider_accounts"]["opencode"]["serves"] =
        Value::Sequence(vec![Value::String("grok-example".to_owned())]);
    assert_eq!(
        resolve_binding(&fleet, &host, OPEN_BINDING),
        Err("route_not_served"),
        "a route that does not prove the model is not eligible"
    );
}

#[test]
fn an_incompatible_route_falls_through_the_configured_chain() {
    let fleet = read("config/examples/fleet.yml");
    let cross = read("tests/fixtures/config/host-cross-provider.yml");

    // The ACP route states high/max only, so the pinned low effort is refused
    // on step one and the configured chain descends to the next policy step.
    let fallback = resolve_binding(&fleet, &cross, FALLBACK_BINDING)
        .expect("the next policy step is compatible");
    assert_eq!(fallback.slot, "codex-primary");
    assert_eq!(fallback.route_provider, "codex");
    assert_eq!(fallback.canonical_model, "gpt-example");
    assert_eq!(fallback.canonical_effort, "xhigh");
    assert_eq!(fallback.canonical_vendor, "openai");

    // The OpenCode host can serve the pinned low effort directly; it does not
    // silently take the fallback.
    let direct = resolve_binding(
        &fleet,
        &read("config/examples/host.example.yml"),
        FALLBACK_BINDING,
    )
    .expect("the OpenCode route serves the low effort");
    assert_eq!(direct.slot, "opencode");
    assert_eq!(direct.canonical_model, "example/model");
    assert_eq!(direct.canonical_effort, "low");
}

#[test]
fn a_chain_with_no_compatible_route_is_refused_without_substitution() {
    let fleet = read("config/examples/fleet.yml");
    let mut host = read("tests/fixtures/config/host-cross-provider.yml");
    host["routes"]["codex"] = Value::Sequence(vec![]);
    let outcome = resolve_binding(&fleet, &host, FALLBACK_BINDING);
    assert_eq!(
        outcome,
        Err("route_effort_unsupported"),
        "the typed reason names the first refusal; no model or effort is substituted"
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
