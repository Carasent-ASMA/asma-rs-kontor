//! Local Git checkout preparation for declared task worktrees.
//!
//! Paseo's `workspace create --isolation local` registers a directory; it does
//! not create one. Kontor therefore prepares only the repository convention
//! that carries enough identity to do so without a guess:
//! `<project root>/.worktrees/<branch>`. Other roots stay runtime-owned.
//!
//! A branch Kontor *creates* must be the branch the ASMA CLI would have created:
//! [`BranchName`] grammar, carrying the confirmed tracker key of the epic or
//! task the worktree serves ([`ManagedBranchBinding`]). Until ASMA-8101 the
//! path `.worktrees/cat-11` produced a branch `cat-11` and was published as
//! such. A branch that already exists — locally, on the remote, or as a live
//! checkout — is history and is adopted as-is; the grammar governs creation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use kontor_core::branch::{BranchName, BranchRefusal, TrackerKey};
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use kontor_runtime::scope::ExecutionScope;
use kontor_runtime::workspace::WorkspaceRoot;

/// The confirmed tracker keys a newly created managed branch may carry, and
/// the confirmed task keys an ASMA CLI catalog checkout must bind to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedBranchBinding {
    confirmed: Vec<TrackerKey>,
    /// The confirmed task keys the scopes name. A catalog checkout must bind
    /// its slug and its actual branch to one shared key from this set.
    tasks: Vec<TrackerKey>,
    /// Whether any scope names a ticket at all, even one whose key is absent or
    /// not canonical. A task-bearing scope may never fall back to the
    /// epic-level binding: a missing task key is a refusal, not an epic.
    task_present: bool,
}

impl ManagedBranchBinding {
    /// Every canonical tracker key the given scopes carry: each epic key and,
    /// where a scope names a ticket, each task key. Durable state and the
    /// plane's compatibility rendering are both Kontor-held identities, so the
    /// caller passes both spellings and either binds. A legacy scope whose "key"
    /// is an internal id contributes nothing, so creation under it alone is
    /// refused rather than named after a UUID.
    pub(crate) fn from_scopes<'a>(scopes: impl IntoIterator<Item = &'a ExecutionScope>) -> Self {
        let mut confirmed: Vec<TrackerKey> = Vec::with_capacity(4);
        let mut tasks: Vec<TrackerKey> = Vec::with_capacity(2);
        let mut task_present = false;
        for scope in scopes {
            if let Ok(key) = TrackerKey::from_external(&scope.epic.external_epic_key)
                && !confirmed.contains(&key)
            {
                confirmed.push(key);
            }
            if let Some(task) = scope.task.as_ref() {
                task_present = true;
                if let Ok(key) = TrackerKey::from_external(&task.external_issue_key) {
                    if !confirmed.contains(&key) {
                        confirmed.push(key.clone());
                    }
                    if !tasks.contains(&key) {
                        tasks.push(key);
                    }
                }
            }
        }
        Self {
            confirmed,
            tasks,
            task_present,
        }
    }

    /// Refuse to create `branch` unless it is canonical and bound to this work.
    fn ensure_creatable(&self, branch: &str) -> RuntimeResult<()> {
        let refusal = match BranchName::parse(branch) {
            Ok(parsed) => match parsed.ensure_bound_to(&self.confirmed) {
                Ok(()) => return Ok(()),
                Err(refusal) => refusal,
            },
            Err(refusal) => refusal,
        };
        Err(RuntimeError::WorkspacePreparationFailed {
            rule: refusal.rule(),
        })
    }

    /// Whether `slug` is the exact lowercase worktree slug of one confirmed
    /// tracker key. The ASMA CLI uses this slug as the parent directory for a
    /// catalog module checkout; it is placement identity, not a branch name.
    fn contains_asma_worktree_slug(&self, slug: &str) -> bool {
        self.confirmed
            .iter()
            .any(|key| key.as_str().to_ascii_lowercase() == slug)
    }

    /// Attest the two identity fields of an ASMA CLI catalog checkout: the
    /// worktree slug and the actual canonical branch.
    ///
    /// When the scope names a task, the slug selects exactly one confirmed task
    /// key and the actual branch must carry that same key: a task checkout may
    /// not mix two compatibility spellings, may not be named or branched after
    /// the containing epic, and may not be adopted when the task key is absent
    /// or non-canonical. An epic-level scope keeps its epic-or-task binding,
    /// which is what lets legitimately approved epic and milestone branches be
    /// adopted in their own integration contexts.
    fn ensure_catalog_identity(&self, slug: &str, branch: &str) -> RuntimeResult<()> {
        if self.task_present {
            let Some(selected) = self
                .tasks
                .iter()
                .find(|key| key.as_str().to_ascii_lowercase() == slug)
            else {
                return Err(RuntimeError::WorkspacePreparationFailed {
                    rule: if self.tasks.is_empty() {
                        BranchRefusal::BindingUnconfirmed.rule()
                    } else {
                        "the managed catalog worktree slug is not the confirmed task key"
                    },
                });
            };
            let parsed = BranchName::parse(branch).map_err(|refusal| {
                RuntimeError::WorkspacePreparationFailed {
                    rule: refusal.rule(),
                }
            })?;
            return parsed
                .ensure_bound_to(std::iter::once(selected))
                .map_err(|refusal| RuntimeError::WorkspacePreparationFailed {
                    rule: refusal.rule(),
                });
        }
        if !self.contains_asma_worktree_slug(slug) {
            return Err(RuntimeError::WorkspacePreparationFailed {
                rule: "the managed catalog worktree slug is not the confirmed task key",
            });
        }
        let parsed = BranchName::parse(branch).map_err(|refusal| {
            RuntimeError::WorkspacePreparationFailed {
                rule: refusal.rule(),
            }
        })?;
        parsed.ensure_bound_to(&self.confirmed).map_err(|refusal| {
            RuntimeError::WorkspacePreparationFailed {
                rule: refusal.rule(),
            }
        })
    }
}

/// Ensure a managed canonical task checkout exists before Paseo registers it.
///
/// An already-present managed checkout is verified against both the branch
/// encoded by its path and the project repository's common Git directory. An
/// absent managed checkout is created from an existing local/remote task branch
/// or, for a new branch, from the repository's default branch — and only when
/// the new branch is the deterministic one `binding` allows.
pub(crate) async fn prepare_managed_worktree(
    project_root: &WorkspaceRoot,
    task_root: &WorkspaceRoot,
    binding: &ManagedBranchBinding,
) -> RuntimeResult<()> {
    let project_root = project_root.clone();
    let task_root = task_root.clone();
    let binding = binding.clone();
    tokio::task::spawn_blocking(move || {
        prepare_managed_worktree_blocking(&project_root, &task_root, &binding)
    })
    .await
    .map_err(|_| RuntimeError::WorkspacePreparationFailed {
        rule: "the managed worktree preparation worker did not complete",
    })?
}

fn prepare_managed_worktree_blocking(
    project_root: &WorkspaceRoot,
    task_root: &WorkspaceRoot,
    binding: &ManagedBranchBinding,
) -> RuntimeResult<()> {
    let project = Path::new(project_root.as_str());
    let task = Path::new(task_root.as_str());
    if task == project {
        return Ok(());
    }

    let managed = project.join(".worktrees");
    let Ok(relative) = task.strip_prefix(&managed) else {
        // An external root may be provisioned by another runtime or operator.
        // Preserve that contract instead of interpreting an arbitrary path as
        // a branch name.
        return Ok(());
    };
    let catalog_module = catalog_module_relative(relative, binding);

    if task.exists() {
        return if catalog_module {
            verify_catalog_module_checkout(project, task, relative, binding)
        } else {
            let branch = branch_from(relative)?;
            verify_checkout(project, task, &branch)
        };
    }
    if catalog_module {
        return Err(RuntimeError::WorkspacePreparationFailed {
            rule: "the ASMA CLI catalog worktree must exist before Kontor can attach it",
        });
    }
    let branch = branch_from(relative)?;

    let parent = task
        .parent()
        .ok_or(RuntimeError::WorkspacePreparationFailed {
            rule: "the managed worktree has no parent directory",
        })?;
    fs::create_dir_all(parent).map_err(|_| RuntimeError::WorkspacePreparationFailed {
        rule: "the managed worktree parent directory could not be created",
    })?;

    let local = format!("refs/heads/{branch}");
    let remote = format!("refs/remotes/origin/{branch}");
    let mut command = git(project);
    command.args(["worktree", "add"]);
    if ref_exists(project, &local)? {
        command.arg(task).arg(&branch);
    } else if ref_exists(project, &remote)? {
        command
            .args(["--track", "-b"])
            .arg(&branch)
            .arg(task)
            .arg(format!("origin/{branch}"));
    } else {
        // Only here does Kontor mint a branch, so only here is the grammar the
        // gate: an existing ref is history, this would be a new publication.
        binding.ensure_creatable(&branch)?;
        let base = default_branch_ref(project)?;
        command.arg("-b").arg(&branch).arg(task).arg(base);
    }

    let output = command
        .output()
        .map_err(|_| RuntimeError::WorkspacePreparationFailed {
            rule: "git could not be invoked to create the managed worktree",
        })?;
    if !output.status.success() {
        // Git serializes worktree administration. A concurrent exact attempt
        // may have won while this one waited, so read back before refusing.
        if verify_checkout(project, task, &branch).is_ok() {
            return Ok(());
        }
        return Err(RuntimeError::WorkspacePreparationFailed {
            rule: "git refused to create the declared branch at its canonical worktree",
        });
    }

    verify_checkout(project, task, &branch)
}

fn catalog_module_relative(relative: &Path, binding: &ManagedBranchBinding) -> bool {
    let components = relative.iter().collect::<Vec<_>>();
    let [slug, module] = components.as_slice() else {
        return false;
    };
    let Some(slug) = slug.to_str() else {
        return false;
    };
    !module.is_empty() && binding.contains_asma_worktree_slug(slug)
}

fn branch_from(relative: &Path) -> RuntimeResult<String> {
    let branch = relative
        .to_str()
        .filter(|branch| !branch.is_empty())
        .ok_or(RuntimeError::WorkspacePreparationFailed {
            rule: "the managed worktree path does not encode a UTF-8 branch name",
        })?
        .replace(std::path::MAIN_SEPARATOR, "/");
    let output = Command::new("git")
        .args(["check-ref-format", "--branch", &branch])
        .output()
        .map_err(|_| RuntimeError::WorkspacePreparationFailed {
            rule: "git could not validate the branch encoded by the worktree path",
        })?;
    if !output.status.success() {
        return Err(RuntimeError::WorkspacePreparationFailed {
            rule: "the managed worktree path does not encode a valid Git branch",
        });
    }
    Ok(branch)
}

fn verify_checkout(project: &Path, task: &Path, branch: &str) -> RuntimeResult<()> {
    let project_common = git_text(
        project,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let task_common = git_text(
        task,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let project_common = canonical(&project_common)?;
    let task_common = canonical(&task_common)?;
    if project_common != task_common {
        return Err(RuntimeError::WorkspacePreparationFailed {
            rule: "the branch-encoded worktree belongs to another Git repository",
        });
    }
    let observed = git_text(task, &["branch", "--show-current"])?;
    if observed != branch {
        return Err(RuntimeError::WorkspacePreparationFailed {
            rule: "the declared worktree is checked out on a different branch",
        });
    }
    Ok(())
}

/// Attest the child-module checkout created by `asma worktree add --mod`.
///
/// Its Git common directory is deliberately not the catalog root's: it belongs
/// to one registered submodule under `<catalog .git>/modules`. The checkout is
/// nevertheless safe only when every identity agrees — managed two-segment
/// path, submodule name, Git top-level, and a slug and actual canonical branch
/// that both bind to the same confirmed key. When the scope names a task, that
/// key is the task's: the containing epic's key may not name the checkout or
/// its branch. An epic-level scope keeps the epic-or-task binding its
/// integration contexts already rely on.
fn verify_catalog_module_checkout(
    project: &Path,
    task: &Path,
    relative: &Path,
    binding: &ManagedBranchBinding,
) -> RuntimeResult<()> {
    let project_common = canonical(&git_text(
        project,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?)?;
    let task_common = canonical(&git_text(
        task,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?)?;
    let modules = project_common.join("modules");
    if !task_common.starts_with(&modules) {
        return Err(RuntimeError::WorkspacePreparationFailed {
            rule: "the declared worktree belongs to another Git repository",
        });
    }

    let components = relative.iter().collect::<Vec<_>>();
    let Some([slug, declared_module]) = components.as_slice().first_chunk::<2>() else {
        return Err(RuntimeError::WorkspacePreparationFailed {
            rule: "the managed catalog worktree must name one slug and one module",
        });
    };
    if components.len() != 2
        || task.file_name() != Some(*declared_module)
        || task_common.file_name() != Some(*declared_module)
    {
        return Err(RuntimeError::WorkspacePreparationFailed {
            rule: "the managed catalog worktree does not match its registered module",
        });
    }

    let observed_root = canonical(&git_text(task, &["rev-parse", "--show-toplevel"])?)?;
    let declared_root =
        fs::canonicalize(task).map_err(|_| RuntimeError::WorkspacePreparationFailed {
            rule: "the declared task worktree has no canonical Git identity",
        })?;
    if observed_root != declared_root {
        return Err(RuntimeError::WorkspacePreparationFailed {
            rule: "the managed catalog worktree is not the module checkout root",
        });
    }

    let slug = slug.to_str().filter(|slug| !slug.is_empty()).ok_or(
        RuntimeError::WorkspacePreparationFailed {
            rule: "the managed catalog worktree slug is not valid UTF-8",
        },
    )?;
    let observed_branch = git_text(task, &["branch", "--show-current"])?;
    binding.ensure_catalog_identity(slug, &observed_branch)
}

fn default_branch_ref(project: &Path) -> RuntimeResult<String> {
    if let Ok(reference) = git_text(
        project,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    ) && ref_exists(project, &reference)?
    {
        return Ok(reference);
    }
    for candidate in ["refs/heads/master", "refs/heads/main"] {
        if ref_exists(project, candidate)? {
            return Ok(candidate.to_owned());
        }
    }
    Err(RuntimeError::WorkspacePreparationFailed {
        rule: "the repository has no resolvable default branch for a new task worktree",
    })
}

fn ref_exists(project: &Path, reference: &str) -> RuntimeResult<bool> {
    let status = git(project)
        .args(["show-ref", "--verify", "--quiet", reference])
        .status()
        .map_err(|_| RuntimeError::WorkspacePreparationFailed {
            rule: "git could not inspect the task branch",
        })?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(RuntimeError::WorkspacePreparationFailed {
            rule: "git could not inspect the task branch",
        }),
    }
}

fn git_text(cwd: &Path, arguments: &[&str]) -> RuntimeResult<String> {
    let output = git(cwd).args(arguments).output().map_err(|_| {
        RuntimeError::WorkspacePreparationFailed {
            rule: "git could not inspect the declared task worktree",
        }
    })?;
    output_text(output)
}

fn output_text(output: Output) -> RuntimeResult<String> {
    if !output.status.success() {
        return Err(RuntimeError::WorkspacePreparationFailed {
            rule: "git could not inspect the declared task worktree",
        });
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|_| RuntimeError::WorkspacePreparationFailed {
            rule: "git returned a non-UTF-8 checkout identity",
        })
}

fn canonical(path: &str) -> RuntimeResult<PathBuf> {
    fs::canonicalize(path).map_err(|_| RuntimeError::WorkspacePreparationFailed {
        rule: "the declared task worktree has no canonical Git identity",
    })
}

fn git(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontor_core::id::{ExternalId, ExternalName, MiniProjectId, TaskId};
    use kontor_runtime::scope::{EpicScope, TaskScope};

    fn binding(key: &str) -> ManagedBranchBinding {
        ManagedBranchBinding {
            confirmed: vec![TrackerKey::parse(key).expect("a canonical key")],
            tasks: Vec::new(),
            task_present: false,
        }
    }

    fn task_binding(epic: &str, task: &str) -> ManagedBranchBinding {
        let epic = TrackerKey::parse(epic).expect("a canonical epic key");
        let task = TrackerKey::parse(task).expect("a canonical task key");
        ManagedBranchBinding {
            confirmed: vec![epic, task.clone()],
            tasks: vec![task],
            task_present: true,
        }
    }

    /// One real task scope as the plane receives it: the epic key and the
    /// ticket's own external spelling.
    fn task_scope(epic_key: &str, task_key: &str) -> ExecutionScope {
        ExecutionScope::for_task(
            EpicScope {
                mini_project_id: MiniProjectId::generate(),
                external_epic_key: ExternalId::parse(epic_key).expect("an epic key"),
                short_title: ExternalName::parse("Epic").expect("a title"),
            },
            TaskScope {
                task_id: TaskId::generate(),
                external_issue_key: ExternalId::parse(task_key).expect("a task key"),
                short_code: None,
                worktree: WorkspaceRoot::parse("/fixture/.worktrees/asma-8114/asma-rs-kontor")
                    .expect("an absolute worktree"),
            },
        )
    }

    #[test]
    fn asma_catalog_path_is_recognized_by_its_exact_confirmed_key_slug() {
        let binding = task_binding("ASMA-7744", "ASMA-8114");
        assert!(catalog_module_relative(
            Path::new("asma-8114/asma-rs-kontor"),
            &binding
        ));
        assert!(
            catalog_module_relative(Path::new("asma-7744/asma-rs-kontor"), &binding),
            "an epic-keyed catalog path is recognized so it can be refused as a task mismatch"
        );
        assert!(!catalog_module_relative(
            Path::new("asma-811/asma-rs-kontor"),
            &binding
        ));
        assert!(!catalog_module_relative(
            Path::new("asma-8114/asma-rs-kontor/extra"),
            &binding
        ));
    }

    #[test]
    fn actual_catalog_branch_must_be_canonical_and_bound() {
        let binding = binding("ASMA-8114");
        assert_eq!(
            binding.ensure_creatable("feat/ASMA-8114-consultation-identity"),
            Ok(())
        );
        assert!(
            binding
                .ensure_creatable("feat/ASMA-8115-consultation-identity")
                .is_err()
        );
        assert!(
            binding
                .ensure_creatable("asma-8114/asma-rs-kontor")
                .is_err()
        );
    }

    #[test]
    fn a_task_catalog_checkout_binds_both_its_slug_and_branch_to_the_task_key() {
        let binding = task_binding("ASMA-7744", "ASMA-8114");
        assert_eq!(
            binding.ensure_catalog_identity("asma-8114", "feat/ASMA-8114-consultation-identity"),
            Ok(())
        );
        assert!(matches!(
            binding.ensure_catalog_identity("asma-8114", "feat/ASMA-7744-epic-integration"),
            Err(RuntimeError::WorkspacePreparationFailed { rule })
                if rule.starts_with("branch_binding_mismatch")
        ));
        assert!(matches!(
            binding.ensure_catalog_identity("asma-7744", "feat/ASMA-8114-task-branch"),
            Err(RuntimeError::WorkspacePreparationFailed { rule })
                if rule == "the managed catalog worktree slug is not the confirmed task key"
        ));
        assert!(matches!(
            binding.ensure_catalog_identity("asma-8114", "asma-8114/asma-rs-kontor"),
            Err(RuntimeError::WorkspacePreparationFailed { rule })
                if rule.starts_with("branch_type_unknown")
        ));
    }

    /// The two compatibility spellings of one ticket must not be mixed: the
    /// slug selects one confirmed task key, and the actual branch must carry
    /// that same key.
    #[test]
    fn a_catalog_checkout_binds_its_slug_and_branch_to_one_shared_task_key() {
        let binding = ManagedBranchBinding::from_scopes([
            &task_scope("ASMA-7744", "ASMA-8062"),
            &task_scope("ASMA-7744", "ASMA-7755"),
        ]);
        assert_eq!(
            binding.ensure_catalog_identity("asma-8062", "feat/ASMA-8062-native-naming"),
            Ok(())
        );
        assert_eq!(
            binding.ensure_catalog_identity("asma-7755", "feat/ASMA-7755-other-task"),
            Ok(())
        );
        assert!(matches!(
            binding.ensure_catalog_identity("asma-8062", "feat/ASMA-7755-other-task"),
            Err(RuntimeError::WorkspacePreparationFailed { rule })
                if rule.starts_with("branch_binding_mismatch")
        ));
        assert!(matches!(
            binding.ensure_catalog_identity("asma-7755", "feat/ASMA-8062-native-naming"),
            Err(RuntimeError::WorkspacePreparationFailed { rule })
                if rule.starts_with("branch_binding_mismatch")
        ));
    }

    /// A scope that names a ticket but carries no canonical task key is not an
    /// epic-level scope: its catalog checkout must refuse, while the true
    /// epic-level and branch-encoded paths keep their existing binding.
    #[test]
    fn a_task_scope_without_a_confirmed_task_key_cannot_be_treated_as_epic_level() {
        let binding =
            ManagedBranchBinding::from_scopes([&task_scope("ASMA-7744", "legacy-ticket")]);
        assert!(matches!(
            binding.ensure_catalog_identity("asma-7744", "feat/ASMA-7744-epic-integration"),
            Err(RuntimeError::WorkspacePreparationFailed { rule })
                if rule.starts_with("binding_unconfirmed")
        ));
        assert_eq!(
            binding.ensure_creatable("feat/ASMA-7744-epic-integration"),
            Ok(()),
            "the branch-encoded creation path keeps its epic-or-task binding"
        );
        let epic_level =
            ManagedBranchBinding::from_scopes([&ExecutionScope::for_epic(EpicScope {
                mini_project_id: MiniProjectId::generate(),
                external_epic_key: ExternalId::parse("ASMA-7744").expect("an epic key"),
                short_title: ExternalName::parse("Epic").expect("a title"),
            })]);
        assert_eq!(
            epic_level.ensure_catalog_identity("asma-7744", "feat/ASMA-7744-epic-integration"),
            Ok(()),
            "a true epic-level scope keeps its integration binding"
        );
    }

    #[test]
    fn an_epic_level_catalog_checkout_keeps_its_epic_or_task_binding() {
        let binding = binding("ASMA-7744");
        assert_eq!(
            binding.ensure_catalog_identity("asma-7744", "feat/ASMA-7744-epic-integration"),
            Ok(())
        );
        assert!(
            binding
                .ensure_catalog_identity("asma-8114", "feat/ASMA-8114-task-branch")
                .is_err(),
            "an epic-level binding cannot adopt a task slug it never carried"
        );
    }
}
