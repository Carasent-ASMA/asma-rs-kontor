//! Validate the authoring projection used by startup and bundle preview.

use std::collections::BTreeMap;

use kontor_core::id::{ExternalName, ModuleKey, ProjectId};
use kontor_core::publication::PublicationPolicy;
use kontor_core::pull_request::ForgeRepository;
use serde_yaml_ng::Value;

/// No configured value or parser detail enters a refusal.
pub(crate) fn parse(text: &str) -> Result<Option<PublicationPolicy>, &'static str> {
    let value: Value =
        serde_yaml_ng::from_str(text).map_err(|_| "invalid project.publication YAML")?;
    let Some(project) = value.get("project") else {
        return Ok(None);
    };
    let project = project.as_mapping().ok_or("invalid project")?;
    let id = project.get("project_id");
    let publication = project.get("publication");
    if id.is_none() && publication.is_none() {
        return Ok(None);
    }
    let project_id = id
        .and_then(Value::as_str)
        .and_then(|id| ProjectId::parse(id).ok())
        .ok_or("invalid or missing project.project_id")?;
    let publication = publication
        .and_then(Value::as_mapping)
        .ok_or("invalid or missing project.publication")?;
    if publication.keys().any(|key| {
        !matches!(
            key.as_str(),
            Some("root_repository" | "module_repositories")
        )
    }) {
        return Err("unknown project.publication field");
    }
    let repositories = project
        .get("forge")
        .and_then(|forge| forge.get("repositories"))
        .and_then(Value::as_sequence)
        .ok_or("invalid project.forge.repositories")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .and_then(|text| ForgeRepository::parse(text).ok())
                .ok_or("invalid project.forge.repositories")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let root_repository = publication
        .get("root_repository")
        .and_then(Value::as_str)
        .and_then(|text| ForgeRepository::parse(text).ok())
        .filter(|repository| repositories.contains(repository))
        .ok_or("invalid or unlisted project.publication.root_repository")?;
    let modules = publication
        .get("module_repositories")
        .and_then(Value::as_mapping)
        .ok_or("invalid or missing project.publication.module_repositories")?;
    let mut module_repositories = BTreeMap::new();
    for (module, repository) in modules {
        let module = module
            .as_str()
            .and_then(|text| ModuleKey::parse(text).ok())
            .ok_or("invalid project.publication.module_repositories key")?;
        let repository = repository
            .as_str()
            .and_then(|text| ForgeRepository::parse(text).ok())
            .filter(|repository| repositories.contains(repository))
            .ok_or("invalid or unlisted project.publication.module_repositories value")?;
        module_repositories.insert(module, repository);
    }
    let base_branch = project
        .get("forge")
        .and_then(|forge| forge.get("base_branch"))
        .and_then(Value::as_str)
        .and_then(|branch| ExternalName::parse(branch).ok())
        .ok_or("invalid project.forge.base_branch")?;
    Ok(Some(PublicationPolicy {
        project_id,
        repositories,
        root_repository,
        module_repositories,
        base_branch,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: ProjectId) -> String {
        serde_yaml_ng::to_string(&serde_json::json!({
            "project": {
                "project_id": id,
                "publication": {
                    "root_repository": "example-org/root",
                    "module_repositories": {
                        "components/worker": "example-org/worker",
                        "components/other": "example-org/other",
                    },
                },
                "forge": {
                    "repositories": ["example-org/root", "example-org/worker", "example-org/other"],
                    "base_branch": "trunk",
                },
            },
        }))
        .unwrap()
    }

    #[test]
    fn explicit_project_policy_narrows_to_exact_stored_modules() {
        let id = ProjectId::generate();
        let policy = parse(&source(id)).unwrap().unwrap();
        assert_eq!(policy.project_id, id);
        assert_eq!(policy.base_branch.as_str(), "trunk");
        policy.validate().unwrap();
        let worker = ModuleKey::parse("components/worker").unwrap();
        let other = ModuleKey::parse("components/other").unwrap();
        let unmapped = ModuleKey::parse("components/worker-extra").unwrap();
        let names = |modules: Vec<&ModuleKey>| {
            policy
                .authorized_repositories(modules)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(vec![]), ["example-org/root"]);
        assert_eq!(
            names(vec![&worker]),
            ["example-org/root", "example-org/worker"]
        );
        assert_eq!(names(vec![&unmapped]), ["example-org/root"]);
        assert_eq!(
            names(vec![&worker, &other, &worker]),
            [
                "example-org/root",
                "example-org/worker",
                "example-org/other"
            ]
        );
        let mut root_only: Value = serde_yaml_ng::from_str(&source(id)).unwrap();
        root_only["project"]["publication"]["module_repositories"] =
            Value::Mapping(Default::default());
        let root_only = parse(&serde_yaml_ng::to_string(&root_only).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(root_only.authorized_repositories([&worker]).len(), 1);
        assert!(parse("schema_version: 1\n").unwrap().is_none());
        assert!(parse("project: {key: EXM}\n").unwrap().is_none());
    }

    #[test]
    fn duplicate_module_keys_and_unknown_policy_fields_refuse_without_values() {
        let id = ProjectId::generate();
        let yaml = source(id);
        let duplicate = format!(
            "project:\n  project_id: {id}\n  publication:\n    root_repository: example-org/root\n    module_repositories:\n      components/worker: example-org/worker\n      components/worker: example-org/other\n  forge:\n    repositories: [example-org/root, example-org/worker, example-org/other]\n    base_branch: trunk\n"
        );
        assert!(
            parse(&duplicate.replace("      components/worker: example-org/other\n", "")).is_ok()
        );
        assert!(
            parse(&duplicate)
                .unwrap_err()
                .contains("project.publication")
        );
        let mut value: Value = serde_yaml_ng::from_str(&yaml).unwrap();
        value["project"]["publication"]["extra-grant-canary"] = true.into();
        let error = parse(&serde_yaml_ng::to_string(&value).unwrap()).unwrap_err();
        assert!(error.contains("project.publication"));
        assert!(!error.contains("extra-grant-canary"));
    }
}
