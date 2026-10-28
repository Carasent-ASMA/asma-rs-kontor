//! Selected project branch templates, resolved without a second grammar.

use kontor_core::branch::{BranchType, ProjectBranchRules};
use serde_yaml_ng::Value;

pub(crate) fn parse(text: &str) -> Result<Option<ProjectBranchRules>, &'static str> {
    let value: Value = serde_yaml_ng::from_str(text).map_err(|_| "invalid project.branch YAML")?;
    let Some(branch) = value
        .get("project")
        .and_then(|project| project.get("branch"))
    else {
        // Old published bundles without a project block keep their exact bytes.
        return Ok(None);
    };
    let branch = branch.as_mapping().ok_or("invalid project.branch")?;
    if branch
        .keys()
        .any(|key| !matches!(key.as_str(), Some("types" | "pattern" | "commit_pattern")))
    {
        return Err("unknown project.branch field");
    }
    let rules = ProjectBranchRules {
        project_key: value["project"]["key"]
            .as_str()
            .ok_or("invalid project.key")?
            .to_owned(),
        types: branch
            .get("types")
            .and_then(Value::as_sequence)
            .ok_or("invalid or missing project.branch.types")?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .and_then(|text| BranchType::parse(text).ok())
                    .ok_or("invalid project.branch.types")
            })
            .collect::<Result<_, _>>()?,
        pattern: branch
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or("invalid or missing project.branch.pattern")?
            .to_owned(),
        commit_pattern: branch
            .get("commit_pattern")
            .and_then(Value::as_str)
            .ok_or("invalid or missing project.branch.commit_pattern")?
            .to_owned(),
    };
    rules.validate()?;
    Ok(Some(rules))
}
