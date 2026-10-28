//! Project-authored placement patterns, resolved without touching a filesystem.

use serde::{Deserialize, Serialize};

use crate::branch::{BranchName, BranchType, ProjectBranchRules, TrackerKey, valid_base_branch};
use crate::id::TopologyNodeId;

fn relative(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['\\', ':', '{', '}', '*', '?', '[', ']'])
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|part| !matches!(part, "" | "." | "..") && part.trim() == part)
}

fn placeholders(value: &str, expected: &[&str]) -> bool {
    let parts: Vec<_> = value.split('/').collect();
    expected
        .iter()
        .all(|expected| parts.iter().filter(|part| *part == expected).count() == 1)
        && parts
            .iter()
            .all(|part| !part.contains(['<', '>']) || expected.contains(part))
}

/// The literal managed-root prefix of the one task layout contract.
///
/// # Errors
/// Refuses absolute/traversing paths and missing, repeated or partial placeholders.
pub fn managed_root(worktree_layout: &str) -> Result<&str, &'static str> {
    if !relative(worktree_layout) {
        return Err("invalid project.worktree_layout");
    }
    if !placeholders(worktree_layout, &["<key-slug>", "<module>"])
        || worktree_layout.starts_with('<')
    {
        return Err("invalid project.worktree_layout placeholders");
    }
    let end = worktree_layout
        .find('<')
        .ok_or("invalid project.worktree_layout placeholders")?;
    Ok(&worktree_layout[..end - 1])
}

/// Resolved consultation placement policy held by an immutable orchestration bundle.
/// It grants no project, repository or worktree ownership by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsultationLayout {
    /// The task pattern whose literal prefix owns the managed placement root.
    pub worktree_layout: String,
    /// Relative to that root; exactly one whole key-slug and node segment each.
    pub template: String,
    /// Selected shared branch grammar, frozen with the template.
    pub branch_rules: ProjectBranchRules,
    /// Explicit configured base branch for the declared root source.
    pub base_branch: String,
}

impl ConsultationLayout {
    /// Validate syntax and the selected project grammar without creating anything.
    ///
    /// # Errors
    /// Names the invalid selected project field, without echoing its value.
    pub fn validate(&self) -> Result<(), &'static str> {
        managed_root(&self.worktree_layout)?;
        if !relative(&self.template) || !placeholders(&self.template, &["<key-slug>", "<node>"]) {
            return Err("invalid project.consultation_layout");
        }
        self.branch_rules.validate()?;
        if !valid_base_branch(&self.base_branch) {
            return Err("invalid project.forge.base_branch");
        }
        Ok(())
    }

    /// Render relative destination, managed-root prefix and canonical branch.
    /// The caller must first prove the epic key and node's durable ownership,
    /// then prove canonical containment and the Git source before any effect.
    ///
    /// # Errors
    /// Refuses malformed policy or a branch outside the selected project grammar.
    pub fn render(
        &self,
        key: &TrackerKey,
        node: TopologyNodeId,
    ) -> Result<(String, String, BranchName), &'static str> {
        self.validate()?;
        let branch = BranchName::derive(BranchType::Chore, key, &format!("consultation {node}"));
        self.branch_rules
            .ensure_branch(&branch)
            .map_err(|reason| reason.rule())?;
        let prefix = managed_root(&self.worktree_layout)?;
        let target = self
            .template
            .replace("<key-slug>", &key.as_str().to_ascii_lowercase())
            .replace("<node>", &node.to_string());
        Ok((format!("{prefix}/{target}"), prefix.to_owned(), branch))
    }
}
