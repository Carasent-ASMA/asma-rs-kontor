//! Tracker authoring is selected once; installation remains an explicit command.

use std::collections::BTreeSet;

use kontor_jira::jira::SpecCatalog;
use serde::Deserialize;

use crate::config_folder::{ConfigError, ConfigFolder};

#[derive(Deserialize)]
struct Tracker {
    schema_version: u32,
    kind: String,
    project_key: String,
    specs: Vec<Reference>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    kind: Kind,
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    TicketFields,
    ExternalWorkflow,
}

pub(crate) fn read(
    folder: &ConfigFolder,
    source: Option<&str>,
) -> Result<SpecCatalog, ConfigError> {
    let mut catalog = SpecCatalog::empty();
    let Some(source) = source else {
        return Ok(catalog);
    };
    let path = folder.reference(source)?;
    let invalid = |problem| ConfigError {
        path: path.clone(),
        problem,
    };
    let manifest: Tracker = folder
        .optional(source)?
        .ok_or_else(|| invalid("missing project.tracker.spec"))?;
    if manifest.schema_version != 1 || manifest.kind != "jira" {
        return Err(invalid("invalid tracker schema_version or kind"));
    }
    if manifest.project_key != folder.project.key {
        return Err(invalid("tracker project_key conflicts with project.key"));
    }
    let mut selectors = BTreeSet::new();
    let mut projects = BTreeSet::new();
    for reference in manifest.specs {
        let path = folder.reference(&reference.path)?;
        let invalid = |problem| ConfigError {
            path: path.clone(),
            problem,
        };
        let value: serde_json::Value = folder
            .optional(&reference.path)?
            .ok_or_else(|| invalid("missing tracker specification"))?;
        let json =
            serde_json::to_string(&value).map_err(|_| invalid("invalid tracker specification"))?;
        let (kind, connector, project, issue_type, version) = match reference.kind {
            Kind::TicketFields => {
                catalog
                    .load_field_spec(&json)
                    .map_err(|_| invalid("invalid TicketFieldSpec"))?;
                let spec = catalog.field_specs().last().expect("just loaded").spec();
                (
                    "fields",
                    spec.connector.as_str(),
                    spec.project.as_str(),
                    spec.issue_type.as_str(),
                    spec.version,
                )
            }
            Kind::ExternalWorkflow => {
                catalog
                    .load_workflow_spec(&json)
                    .map_err(|_| invalid("invalid ExternalWorkflowSpec"))?;
                let spec = catalog.workflow_specs().last().expect("just loaded").spec();
                (
                    "workflow",
                    spec.connector.as_str(),
                    spec.project.as_str(),
                    spec.issue_type.as_str(),
                    spec.version,
                )
            }
        };
        if connector != "connector.jira" {
            return Err(invalid(
                "tracker specification connector conflicts with tracker.kind",
            ));
        }
        projects.insert(project.to_owned());
        if projects.len() != 1 {
            return Err(invalid(
                "tracker specifications declare conflicting external projects",
            ));
        }
        if !selectors.insert((
            kind,
            connector.to_owned(),
            project.to_owned(),
            issue_type.to_owned(),
            version,
        )) {
            return Err(invalid("duplicate tracker specification selector"));
        }
    }
    Ok(catalog)
}
