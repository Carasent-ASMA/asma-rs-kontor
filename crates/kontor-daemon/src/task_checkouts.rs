//! Selected host checkout inputs; runtime Git code consumes only frozen snapshots.

use std::collections::BTreeMap;

use kontor_core::id::ModuleKey;
use kontor_core::pull_request::ForgeRepository;
use kontor_runtime::checkout::{CheckoutInputs, CheckoutSource};
use serde_yaml_ng::Value;

use crate::config_folder::{ConfigError, ConfigFolder};

pub(crate) fn read(folder: &ConfigFolder) -> Result<Option<CheckoutInputs>, ConfigError> {
    let refuse = |problem| ConfigError {
        path: folder.root().join("host.yml"),
        problem,
    };
    let Some(host) = folder.optional::<Value>("host.yml")? else {
        return Ok(None);
    };
    let Some(value) = host.get("checkouts") else {
        return Ok(None);
    };
    let block = value
        .as_mapping()
        .ok_or_else(|| refuse("invalid host.checkouts"))?;
    if block
        .keys()
        .any(|key| !matches!(key.as_str(), Some("root" | "modules")))
    {
        return Err(refuse("unknown host.checkouts field"));
    }
    let root = source(&value["root"]).map_err(refuse)?;
    let modules = value["modules"]
        .as_mapping()
        .ok_or_else(|| refuse("invalid or missing host.checkouts.modules"))?;
    let mut selected = BTreeMap::new();
    for (key, value) in modules {
        let key = key
            .as_str()
            .and_then(|key| ModuleKey::parse(key).ok())
            .ok_or_else(|| refuse("invalid host.checkouts.modules key"))?;
        selected.insert(key, source(value).map_err(refuse)?);
    }
    let inputs = CheckoutInputs {
        root,
        modules: selected,
    };
    inputs
        .validate(
            &folder.project.forge.repositories,
            &folder.project.forge.base_branch,
        )
        .map_err(|error| refuse(error.0))?;
    Ok(Some(inputs))
}

fn source(value: &Value) -> Result<CheckoutSource, &'static str> {
    let fields = value
        .as_mapping()
        .ok_or("invalid or missing host.checkouts source record")?;
    if fields
        .keys()
        .any(|key| !matches!(key.as_str(), Some("repository" | "path" | "base_ref")))
    {
        return Err("unknown host.checkouts source field");
    }
    Ok(CheckoutSource {
        repository: value["repository"]
            .as_str()
            .and_then(|repo| ForgeRepository::parse(repo).ok())
            .ok_or("invalid or missing host.checkouts.repository")?,
        path: value["path"]
            .as_str()
            .ok_or("invalid or missing host.checkouts.path")?
            .to_owned(),
        base_ref: value["base_ref"]
            .as_str()
            .ok_or("invalid or missing host.checkouts.base_ref")?
            .to_owned(),
    })
}

pub(crate) fn derived_claim(
    folder: &ConfigFolder,
    project_root: &str,
    module: &ModuleKey,
    confirmed: &kontor_core::branch::TrackerKey,
) -> Result<kontor_core::id::ExternalName, kontor_runtime::checkout::CheckoutError> {
    use kontor_runtime::checkout::{CheckoutError, destination};
    let (rendered, managed) = crate::project_paths::render_layout(
        &folder.project.worktree_layout,
        project_root,
        confirmed,
        module,
    )
    .map_err(CheckoutError)?;
    let target = destination(&rendered)?;
    let managed = destination(&managed)?;
    if target != rendered
        || !std::path::Path::new(&managed).starts_with(project_root)
        || !std::path::Path::new(&target).starts_with(&managed)
    {
        return Err(CheckoutError("checkout.destination_escapes_managed_root"));
    }
    kontor_core::id::ExternalName::parse(&target)
        .map_err(|_| CheckoutError("checkout.destination_invalid"))
}

pub(crate) fn plan(
    folder: &ConfigFolder,
    project: &kontor_core::repository::Project,
    task: &kontor_core::repository::Task,
    claim: &kontor_core::id::ExternalName,
    confirmed: &[kontor_core::branch::TrackerKey],
) -> Result<kontor_runtime::checkout::TaskCheckout, kontor_runtime::checkout::CheckoutError> {
    use kontor_core::branch::{BranchName, BranchType};
    use kontor_runtime::checkout::{self, CheckoutError};
    let inputs = folder
        .checkouts
        .as_ref()
        .ok_or(CheckoutError("host.checkouts.not_configured"))?;
    let key = confirmed
        .first()
        .ok_or(CheckoutError("checkout.binding_unconfirmed"))?;
    let source = inputs.select(
        project.root_path.as_str(),
        task.module.as_ref(),
        &folder.project.forge.base_branch,
    )?;
    let destination = checkout::destination(claim.as_str())?;
    let branch = if std::path::Path::new(&destination)
        .try_exists()
        .map_err(|_| CheckoutError("checkout.destination_unreadable"))?
    {
        BranchName::parse(&checkout::observed_branch(&destination)?)
            .map_err(|_| CheckoutError("checkout.branch_invalid"))?
    } else {
        BranchName::derive(BranchType::Feat, key, task.title.as_str())
    };
    branch
        .ensure_bound_to(confirmed)
        .map_err(|_| CheckoutError("checkout.branch_binding_mismatch"))?;
    folder
        .project
        .branch_rules
        .as_ref()
        .ok_or(CheckoutError("missing project.branch"))?
        .ensure_branch(&branch)
        .map_err(|reason| CheckoutError(reason.rule()))?;
    let managed_root = task
        .module
        .as_ref()
        .map(|module| {
            crate::project_paths::render_layout(
                &folder.project.worktree_layout,
                project.root_path.as_str(),
                key,
                module,
            )
            .map_err(CheckoutError)
        })
        .transpose()?
        .and_then(|(rendered, root)| (rendered == claim.as_str()).then_some(root));
    let confirmed_keys = if managed_root.is_some() {
        vec![key.to_string()]
    } else {
        confirmed.iter().map(ToString::to_string).collect()
    };
    let snapshot = checkout::TaskCheckout {
        schema_version: 1,
        source,
        project_root: project.root_path.as_str().to_owned(),
        claim: claim.as_str().to_owned(),
        destination,
        branch: branch.to_string(),
        confirmed_keys,
        module: task.module.clone(),
        managed_root,
    };
    snapshot.validate()?;
    if std::path::Path::new(&snapshot.destination).exists() {
        checkout::verify(&snapshot)?;
    }
    Ok(snapshot)
}
