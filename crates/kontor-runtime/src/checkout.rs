//! Explicit task checkout sources and the Git readback frozen before creation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use kontor_core::branch::{BranchName, TrackerKey, valid_base_branch};
use kontor_core::id::ModuleKey;
use kontor_core::publication::CommitSha;
use kontor_core::pull_request::ForgeRepository;
use serde::{Deserialize, Serialize};

use crate::workspace::WorkspaceRoot;

/// A sanitized checkout refusal, never Git output or a configured value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CheckoutError(pub &'static str);

type Result<T> = std::result::Result<T, CheckoutError>;

/// One operator-declared Git source checkout, independent of destination layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckoutSource {
    /// Exact entry in the selected project's forge repositories.
    pub repository: ForgeRepository,
    /// Absolute canonical existing working-checkout root.
    pub path: String,
    /// Explicit full local or configured-remote base branch ref.
    pub base_ref: String,
}

/// The sole host input for task Git source selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutInputs {
    /// Source for root-only tasks and proven monorepo places.
    pub root: CheckoutSource,
    /// Exact durable Task.module overrides, including explicitly new modules.
    pub modules: BTreeMap<ModuleKey, CheckoutSource>,
}

/// Source identity and exact commit resolved before the placement intent is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckoutSourceSnapshot {
    /// The complete declared source record.
    pub declaration: CheckoutSource,
    /// Canonical Git common directory, proved from this exact working root.
    pub common_dir: String,
    /// Direct commit object held by the explicit base ref at selection time.
    pub base_commit: CommitSha,
}

/// Immutable task checkout intent. Persist before calling [`prepare`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCheckout {
    /// Record generation.
    pub schema_version: u32,
    /// Frozen declared source and Git identity.
    pub source: CheckoutSourceSnapshot,
    /// Canonical durable project root, independently checked against host.root.
    pub project_root: String,
    /// Exact declared task claim; retained even if its filesystem spelling differs.
    pub claim: String,
    /// Canonical destination, never inferred by the runtime.
    pub destination: String,
    /// Exact confirmed publication branch to create or adopt.
    pub branch: String,
    /// Confirmed keys this branch is permitted to carry.
    pub confirmed_keys: Vec<String>,
    /// Exact stored task module, absent for an explicitly placed root-only task.
    pub module: Option<ModuleKey>,
    /// Containment boundary for a rendered destination; explicit claims have none.
    pub managed_root: Option<String>,
}

impl CheckoutInputs {
    /// Validate every declaration without creating a Project, branch or worktree.
    ///
    /// # Errors
    /// Named source, repository or explicit-ref refusal.
    pub fn validate(&self, repositories: &[String], base: &str) -> Result<()> {
        for source in std::iter::once(&self.root).chain(self.modules.values()) {
            if !repositories
                .iter()
                .any(|repo| repo == source.repository.as_str())
            {
                return Err(CheckoutError("host.checkouts.repository_not_in_project"));
            }
            resolve_source(source, base)?;
        }
        Ok(())
    }

    /// Select an exact module record, otherwise prove monorepo membership at base.
    ///
    /// # Errors
    /// Refuses a foreign root, missing source or ambiguous repository boundary.
    pub fn select(
        &self,
        project_root: &str,
        module: Option<&ModuleKey>,
        base: &str,
    ) -> Result<CheckoutSourceSnapshot> {
        if canonical_root(&self.root.path)? != project_root {
            return Err(CheckoutError("host.checkouts.root.project_root_mismatch"));
        }
        let explicit = module.and_then(|module| self.modules.get(module));
        let snapshot = resolve_source(explicit.unwrap_or(&self.root), base)?;
        if let Some(module) = module.filter(|_| explicit.is_none()) {
            prove_monorepo(&snapshot, module)?;
        }
        Ok(snapshot)
    }
}

/// Resolve an explicit source and base ref, with no default-branch search.
///
/// # Errors
/// Refuses invalid roots, refs, missing refs, tags and non-commit objects.
pub fn resolve_source(source: &CheckoutSource, base: &str) -> Result<CheckoutSourceSnapshot> {
    let path = canonical_root(&source.path)?;
    let root = Path::new(&path);
    let local = format!("refs/heads/{base}");
    let remote = source
        .base_ref
        .strip_prefix("refs/remotes/")
        .and_then(|rest| rest.strip_suffix(&format!("/{base}")));
    let declared_remote = remote.is_some_and(|name| {
        !name.is_empty()
            && git_text(root, &["remote"])
                .is_ok_and(|names| names.lines().any(|remote| remote == name))
    });
    if !valid_base_branch(base)
        || (source.base_ref != local && !declared_remote)
        || !git_status(root, &["check-ref-format", &source.base_ref])?
    {
        return Err(CheckoutError("host.checkouts.base_ref_invalid"));
    }
    let commit = git_text(root, &["show-ref", "--verify", "--hash", &source.base_ref])
        .map_err(|_| CheckoutError("host.checkouts.base_ref_unresolved"))?;
    let base_commit = CommitSha::parse(&commit)
        .map_err(|_| CheckoutError("host.checkouts.base_ref_noncommit"))?;
    ensure_commit(root, &base_commit)?;
    Ok(CheckoutSourceSnapshot {
        declaration: source.clone(),
        common_dir: common_dir(root)?,
        base_commit,
    })
}

fn prove_monorepo(snapshot: &CheckoutSourceSnapshot, module: &ModuleKey) -> Result<()> {
    let root = Path::new(&snapshot.declaration.path);
    let mut prefix = String::new();
    for segment in module.as_str().split('/') {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(segment);
        let output = git_text(
            root,
            &[
                "ls-tree",
                "-z",
                snapshot.base_commit.as_str(),
                "--",
                &prefix,
            ],
        )?;
        let mut entries = output.split('\0').filter(|entry| !entry.is_empty());
        let entry = entries
            .next()
            .filter(|_| entries.next().is_none())
            .ok_or(CheckoutError("host.checkouts.module_record_required"))?;
        let (mode, path) = entry.split_once('\t').ok_or(CheckoutError(
            "host.checkouts.monorepo_membership_inconclusive",
        ))?;
        if path != prefix || mode.starts_with("160000 ") || mode.starts_with("120000 ") {
            return Err(CheckoutError("host.checkouts.module_repository_boundary"));
        }
        let physical = root.join(&prefix);
        if let Ok(metadata) = std::fs::symlink_metadata(&physical) {
            if metadata.file_type().is_symlink() {
                return Err(CheckoutError("host.checkouts.module_symlink_boundary"));
            }
            let directory = if metadata.is_dir() {
                physical.as_path()
            } else {
                physical.parent().expect("a module has a parent")
            };
            let observed = git_text(directory, &["rev-parse", "--show-toplevel"])?;
            if canonical(&observed)? != snapshot.declaration.path {
                return Err(CheckoutError("host.checkouts.module_repository_boundary"));
            }
        }
    }
    Ok(())
}

/// Canonical spelling of a destination whose final components may not exist yet.
///
/// # Errors
/// Refuses non-normal paths and unreadable/symlink-altered ancestors.
pub fn destination(path: &str) -> Result<String> {
    WorkspaceRoot::parse(path).map_err(|_| CheckoutError("checkout.destination_invalid"))?;
    let mut ancestor = PathBuf::from(path);
    let mut missing = Vec::new();
    while !ancestor
        .try_exists()
        .map_err(|_| CheckoutError("checkout.destination_unreadable"))?
    {
        missing.push(
            ancestor
                .file_name()
                .ok_or(CheckoutError("checkout.destination_invalid"))?
                .to_owned(),
        );
        ancestor.pop();
    }
    let mut resolved = std::fs::canonicalize(&ancestor)
        .map_err(|_| CheckoutError("checkout.destination_unreadable"))?;
    for part in missing.iter().rev() {
        resolved.push(part);
    }
    resolved
        .to_str()
        .map(str::to_owned)
        .ok_or(CheckoutError("checkout.destination_invalid"))
}

impl TaskCheckout {
    /// Re-prove the frozen source, branch binding and destination containment.
    /// Never re-resolve a moving base ref in place of the frozen commit.
    ///
    /// # Errors
    /// Refuses a changed source, malformed intent, escape or branch binding.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || canonical_root(&self.source.declaration.path)? != self.source.declaration.path
            || common_dir(Path::new(&self.source.declaration.path))? != self.source.common_dir
        {
            return Err(CheckoutError("checkout.source_identity_changed"));
        }
        ensure_commit(
            Path::new(&self.source.declaration.path),
            &self.source.base_commit,
        )?;
        let branch = BranchName::parse(&self.branch)
            .map_err(|_| CheckoutError("checkout.branch_invalid"))?;
        let keys = self
            .confirmed_keys
            .iter()
            .map(|key| TrackerKey::parse(key))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| CheckoutError("checkout.binding_unconfirmed"))?;
        branch
            .ensure_bound_to(&keys)
            .map_err(|_| CheckoutError("checkout.branch_binding_mismatch"))?;
        if destination(&self.claim)? != self.destination
            || canonical_root(&self.project_root)? != self.project_root
            || self.destination == self.project_root
            || self.destination == self.source.declaration.path
            || Path::new(&self.destination).starts_with(&self.source.common_dir)
            || Path::new(&self.destination).starts_with(common_dir(Path::new(&self.project_root))?)
        {
            return Err(CheckoutError("checkout.destination_changed_or_protected"));
        }
        if let Some(root) = &self.managed_root
            && (destination(root)? != *root
                || !Path::new(root).starts_with(&self.project_root)
                || !Path::new(&self.destination).starts_with(root)
                || self.destination == *root)
        {
            return Err(CheckoutError("checkout.destination_escapes_managed_root"));
        }
        Ok(())
    }
}

/// Verify an existing destination against the frozen source and exact branch.
///
/// # Errors
/// Refuses foreign common directories, parent-discovered roots and moved branches.
pub fn verify(snapshot: &TaskCheckout) -> Result<()> {
    snapshot.validate()?;
    if canonical_root(&snapshot.destination)? != snapshot.destination
        || common_dir(Path::new(&snapshot.destination))? != snapshot.source.common_dir
    {
        return Err(CheckoutError("checkout.destination_foreign_common_dir"));
    }
    if observed_branch(&snapshot.destination)? != snapshot.branch {
        return Err(CheckoutError("checkout.destination_branch_changed"));
    }
    Ok(())
}

/// Read a destination's actual branch; never repair or infer it from a path.
///
/// # Errors
/// Refuses a missing/non-root checkout or detached head.
pub fn observed_branch(path: &str) -> Result<String> {
    canonical_root(path)?;
    git_text(
        Path::new(path),
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )
    .map_err(|_| CheckoutError("checkout.destination_branch_unconfirmed"))
}

/// Correlated Git readback for a successful immutable placement intent.
///
/// # Errors
/// The same source, root, common-directory and branch checks as [`verify`].
pub fn readback(snapshot: &TaskCheckout) -> Result<serde_json::Value> {
    verify(snapshot)?;
    let root = Path::new(&snapshot.destination);
    let head = git_text(root, &["rev-parse", "--verify", "HEAD"])?;
    let head = CommitSha::parse(&head).map_err(|_| CheckoutError("checkout.head_invalid"))?;
    ensure_commit(root, &head)?;
    Ok(serde_json::json!({
        "destination": canonical_root(&snapshot.destination)?,
        "common_dir": common_dir(root)?,
        "branch": observed_branch(&snapshot.destination)?,
        "head_commit": head,
    }))
}

/// Execute one already-persisted intent, then prove its actual Git result.
///
/// # Errors
/// Refuses source/claim drift, foreign destinations, remote task guesses and Git failures.
pub fn prepare(snapshot: &TaskCheckout) -> Result<()> {
    snapshot.validate()?;
    let task = Path::new(&snapshot.destination);
    if task
        .try_exists()
        .map_err(|_| CheckoutError("checkout.destination_unreadable"))?
    {
        return verify(snapshot);
    }
    let source = Path::new(&snapshot.source.declaration.path);
    let local = format!("refs/heads/{}", snapshot.branch);
    let local_exists = git_status(source, &["show-ref", "--verify", "--quiet", &local])?;
    if !local_exists {
        let remotes = git_text(
            source,
            &["for-each-ref", "--format=%(refname)", "refs/remotes"],
        )?;
        if remotes
            .lines()
            .any(|remote| remote.ends_with(&format!("/{}", snapshot.branch)))
        {
            return Err(CheckoutError("checkout.remote_task_branch_input_required"));
        }
    }
    std::fs::create_dir_all(
        task.parent()
            .ok_or(CheckoutError("checkout.destination_invalid"))?,
    )
    .map_err(|_| CheckoutError("checkout.destination_parent_unwritable"))?;
    snapshot.validate()?;
    let mut command = git(source);
    command.args(["worktree", "add"]);
    if local_exists {
        command.arg(task).arg(&snapshot.branch);
    } else {
        command
            .arg("-b")
            .arg(&snapshot.branch)
            .arg(task)
            .arg(snapshot.source.base_commit.as_str());
    }
    let success = command
        .output()
        .map_err(|_| CheckoutError("checkout.git_unavailable"))?
        .status
        .success();
    if !success && verify(snapshot).is_err() {
        return Err(CheckoutError("checkout.git_worktree_refused"));
    }
    verify(snapshot)
}

fn canonical_root(path: &str) -> Result<String> {
    let normalized =
        WorkspaceRoot::parse(path).map_err(|_| CheckoutError("host.checkouts.path_invalid"))?;
    let canonical_path = canonical(normalized.as_str())?;
    if canonical_path != path || !Path::new(path).is_dir() {
        return Err(CheckoutError("host.checkouts.path_not_canonical_root"));
    }
    let top = git_text(Path::new(path), &["rev-parse", "--show-toplevel"])
        .map_err(|_| CheckoutError("host.checkouts.path_is_not_checkout_root"))?;
    if canonical(&top)? != canonical_path {
        return Err(CheckoutError("host.checkouts.path_is_not_checkout_root"));
    }
    Ok(canonical_path)
}

fn common_dir(root: &Path) -> Result<String> {
    canonical(&git_text(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?)
}

fn canonical(path: &str) -> Result<String> {
    std::fs::canonicalize(path)
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned))
        .ok_or(CheckoutError("host.checkouts.path_unreadable"))
}

fn ensure_commit(root: &Path, commit: &CommitSha) -> Result<()> {
    if git_text(root, &["cat-file", "-t", commit.as_str()])? != "commit" {
        return Err(CheckoutError("host.checkouts.base_ref_noncommit"));
    }
    Ok(())
}

fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    // A frozen commit names its original objects, independent of mutable refs/replace.
    command
        .arg("--no-replace-objects")
        .current_dir(root)
        .stdin(Stdio::null());
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ] {
        command.env_remove(key);
    }
    command
}

fn git_text(root: &Path, args: &[&str]) -> Result<String> {
    let output = git(root)
        .args(args)
        .output()
        .map_err(|_| CheckoutError("checkout.git_unavailable"))?;
    if !output.status.success() {
        return Err(CheckoutError("checkout.git_read_refused"));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim_end_matches('\n').to_owned())
        .map_err(|_| CheckoutError("checkout.git_read_invalid"))
}

fn git_status(root: &Path, args: &[&str]) -> Result<bool> {
    let output = git(root)
        .args(args)
        .output()
        .map_err(|_| CheckoutError("checkout.git_unavailable"))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(CheckoutError("checkout.git_read_refused")),
    }
}
