//! Validate the selected project's path classes without resolving them from config.

use kontor_core::pull_request::ForgeRepository;
use serde_yaml_ng::Value;

pub(crate) fn validate(project: &Value) -> Result<(), &'static str> {
    let repositories = project["forge"]["repositories"]
        .as_sequence()
        .filter(|values| !values.is_empty())
        .ok_or("invalid or missing project.forge.repositories")?;
    let mut parsed = Vec::new();
    for value in repositories {
        let repository = value
            .as_str()
            .and_then(|value| ForgeRepository::parse(value).ok())
            .ok_or("invalid project.forge.repositories")?;
        if parsed.contains(&repository) {
            return Err("duplicate project.forge.repositories");
        }
        parsed.push(repository);
    }
    let roots = project["resource_roots"]
        .as_mapping()
        .filter(|roots| !roots.is_empty())
        .ok_or("invalid or missing project.resource_roots")?;
    for (key, value) in roots {
        if !key.as_str().is_some_and(|key| {
            relative(key, false) && !key.contains('/') && !key.contains(['<', '>'])
        }) || !value.as_str().is_some_and(|value| {
            repositories
                .iter()
                .any(|entry| entry.as_str() == Some(value))
        }) {
            return Err("invalid or unlisted project.resource_roots");
        }
    }
    for (value, field) in [
        (
            &project["gap_ledger"],
            "invalid or missing project.gap_ledger",
        ),
        (&project["glossary"], "invalid or missing project.glossary"),
        (
            &project["decision_authorities"]["decision_records_root"],
            "invalid or missing project.decision_authorities.decision_records_root",
        ),
    ] {
        resource(value, roots).ok_or(field)?;
    }
    let documents = project["documentation_roots"]
        .as_sequence()
        .filter(|paths| !paths.is_empty())
        .ok_or("invalid or missing project.documentation_roots")?;
    for value in documents {
        resource(value, roots).ok_or("invalid project.documentation_roots")?;
    }
    let layout = project["worktree_layout"]
        .as_str()
        .filter(|layout| relative(layout, false))
        .ok_or("invalid or missing project.worktree_layout")?;
    let parts: Vec<_> = layout.split('/').collect();
    if parts.first().is_none_or(|part| part.contains(['<', '>']))
        || parts.iter().filter(|part| **part == "<key-slug>").count() != 1
        || parts.iter().filter(|part| **part == "<module>").count() != 1
        || parts
            .iter()
            .any(|part| part.contains(['<', '>']) && !matches!(*part, "<key-slug>" | "<module>"))
    {
        return Err("invalid project.worktree_layout placeholders");
    }
    Ok(())
}

fn resource(value: &Value, roots: &serde_yaml_ng::Mapping) -> Option<()> {
    let path = value
        .as_str()
        .filter(|path| relative(path, true) && !path.contains(['<', '>']))?;
    let (root, rest) = path.split_once('/')?;
    (roots.contains_key(root) && !rest.is_empty()).then_some(())
}

fn relative(path: &str, directory: bool) -> bool {
    let body = if directory {
        path.strip_suffix('/').unwrap_or(path)
    } else {
        path
    };
    !body.is_empty()
        && !body.contains(['\\', ':', '{', '}', '*', '?', '[', ']'])
        && !body.chars().any(char::is_control)
        && body
            .split('/')
            .all(|part| !matches!(part, "" | "." | "..") && part.trim() == part)
}
