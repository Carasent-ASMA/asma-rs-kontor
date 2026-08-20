//! Local Git checkout preparation for declared task worktrees.
//!
//! Paseo's `workspace create --isolation local` registers a directory; it does
//! not create one. Kontor therefore prepares only the repository convention
//! that carries enough identity to do so without a guess:
//! `<project root>/.worktrees/<branch>`. Other roots stay runtime-owned.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use kontor_runtime::workspace::WorkspaceRoot;

/// Ensure a managed canonical task checkout exists before Paseo registers it.
///
/// An already-present managed checkout is verified against both the branch
/// encoded by its path and the project repository's common Git directory. An
/// absent managed checkout is created from an existing local/remote task branch
/// or, for a new branch, from the repository's default branch.
pub(crate) async fn prepare_managed_worktree(
    project_root: &WorkspaceRoot,
    task_root: &WorkspaceRoot,
) -> RuntimeResult<()> {
    let project_root = project_root.clone();
    let task_root = task_root.clone();
    tokio::task::spawn_blocking(move || {
        prepare_managed_worktree_blocking(&project_root, &task_root)
    })
    .await
    .map_err(|_| RuntimeError::WorkspacePreparationFailed {
        rule: "the managed worktree preparation worker did not complete",
    })?
}

fn prepare_managed_worktree_blocking(
    project_root: &WorkspaceRoot,
    task_root: &WorkspaceRoot,
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
    let branch = branch_from(relative)?;

    if task.exists() {
        return verify_checkout(project, task, &branch);
    }

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
            rule: "the declared worktree belongs to another Git repository",
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
