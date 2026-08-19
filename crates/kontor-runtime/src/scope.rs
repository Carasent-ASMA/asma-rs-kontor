//! The epic and task identity one runtime operation is performed under.
//!
//! # Why this travels on the request
//!
//! A runtime plane used to *be* an epic. Every fact an adapter needed in order to
//! name, place or label a seat — the Jira epic key, the epic's short title, the
//! ticket's issue key and short code, the canonical task worktree — was static
//! configuration, keyed to one `mini_project_id` and read from a file the daemon
//! parses at startup. That shape has two consequences, and both of them are
//! defects rather than trade-offs:
//!
//! 1. **A second epic in one project cannot run.** Its native root is registered
//!    from the same directory as the first epic's, so the runtime hands back the
//!    project that already exists there and the store correctly refuses to
//!    repoint a container that belongs to another node.
//! 2. **A task imported after startup cannot run.** It has no entry in the static
//!    map, so every naming and placement lookup refuses — and the only way to
//!    give it one is to edit the settings file and restart the daemon, which
//!    makes a backlog change into an operations change.
//!
//! So identity moves here. Kontor already holds all of it durably: the epic's
//! tracker key and short title, the task's worktree, the task's external issue
//! link. The daemon reads that state and states it on the request; the adapter
//! consumes what it is told. Static configuration keeps only what is genuinely a
//! property of the *host* — where to reach it, what credential, how many
//! sessions — plus a compatibility-only display override, and stops being
//! authority over the backlog.
//!
//! # Why identity is never derived from a display name
//!
//! An epic's persisted `name` reads `ASMA-7869 · Kontor Operational MVP`, which
//! looks like it carries both fields this module needs. Splitting it would make
//! a mutable human string into an identity, and identity read off a display name
//! is how one epic comes to be registered under another's key the first time
//! somebody renames something. Both fields are stored separately and read
//! separately, and there is deliberately no parser here.

use kontor_core::id::{ExternalId, ExternalName, MiniProjectId, TaskId};

use crate::workspace::WorkspaceRoot;

/// The epic one runtime operation belongs to.
///
/// Every field is read from durable Kontor state. `mini_project_id` is the
/// identity; the other two are how that identity is *rendered* to a runtime that
/// shows names to people, and neither is ever matched on to find anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicScope {
    /// The Kontor epic. The only identity in this value.
    pub mini_project_id: MiniProjectId,
    /// The tracker key the epic is followed as, e.g. `ASMA-7869`.
    pub external_epic_key: ExternalId,
    /// The compact epic title, e.g. `Kontor Operational MVP`.
    pub short_title: ExternalName,
}

/// The ticket one runtime operation is performed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskScope {
    /// The Kontor task. The only identity in this value.
    pub task_id: TaskId,
    /// The tracker key the ticket is followed as, e.g. `ASMA-7676`.
    pub external_issue_key: ExternalId,
    /// The short code seats and workspaces are titled by, e.g. `OP-01`.
    ///
    /// Defaults to [`Self::external_issue_key`], because no durable Kontor state
    /// carries a second, shorter code and inventing one from a task title would
    /// be exactly the display-name parsing this module refuses. A plane may
    /// override it for compatibility with titles that already exist.
    pub short_code: ExternalId,
    /// The filesystem-canonical worktree this ticket's work happens in.
    ///
    /// Authority, never display data, and read from the task's own declared
    /// worktree rather than re-derived from configuration. An adapter comparing
    /// this against the root it was asked to prepare would be comparing the
    /// scope against itself.
    pub worktree: WorkspaceRoot,
}

/// The whole identity one runtime operation is performed under.
///
/// The epic is always present: every container, workspace and seat Kontor drives
/// belongs to one. The task is absent exactly when the operation is not about a
/// ticket — an epic root, an epic control plane, or a consultation raised at the
/// epic rather than at a ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionScope {
    /// The epic.
    pub epic: EpicScope,
    /// The ticket, when the operation is about one.
    pub task: Option<TaskScope>,
}

impl ExecutionScope {
    /// An epic-level scope, for a node or a consultation that serves no ticket.
    #[must_use]
    pub const fn for_epic(epic: EpicScope) -> Self {
        Self { epic, task: None }
    }

    /// An epic-level scope narrowed to one ticket.
    #[must_use]
    pub const fn for_task(epic: EpicScope, task: TaskScope) -> Self {
        Self {
            epic,
            task: Some(task),
        }
    }

    /// The ticket this operation serves, or a refusal that names what is missing.
    ///
    /// Refusing is the whole point. An operation that needs a ticket's worktree
    /// or title and is handed an epic-level scope has been routed wrongly, and
    /// falling back to anything — the epic's own root, the first ticket the plane
    /// knows about — is how a role ends up editing a tree nobody chose.
    ///
    /// # Errors
    /// Returns [`crate::adapter::RuntimeError::WorkspaceMismatch`] when this
    /// scope names no ticket.
    pub fn require_task(&self) -> crate::adapter::RuntimeResult<&TaskScope> {
        self.task
            .as_ref()
            .ok_or(crate::adapter::RuntimeError::WorkspaceMismatch {
                rule: "this operation needs a ticket scope and was given an epic-level one",
            })
    }

    /// Whether this scope names the given epic.
    ///
    /// Compared on `mini_project_id` alone: the tracker key and the title are
    /// display data and may both be corrected without the epic changing.
    #[must_use]
    pub fn is_epic(&self, mini_project_id: MiniProjectId) -> bool {
        self.epic.mini_project_id == mini_project_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn epic() -> EpicScope {
        EpicScope {
            mini_project_id: MiniProjectId::generate(),
            external_epic_key: ExternalId::parse("ASMA-7869").expect("epic key"),
            short_title: ExternalName::parse("Kontor Operational MVP").expect("title"),
        }
    }

    #[test]
    fn an_epic_level_scope_refuses_to_answer_a_ticket_question() {
        let scope = ExecutionScope::for_epic(epic());
        assert!(scope.require_task().is_err());
    }

    #[test]
    fn a_ticket_scope_answers_with_the_ticket_it_was_given() {
        let worktree = WorkspaceRoot::parse("/w/ticket").expect("root");
        let task_id = TaskId::generate();
        let scope = ExecutionScope::for_task(
            epic(),
            TaskScope {
                task_id,
                external_issue_key: ExternalId::parse("ASMA-7676").expect("issue"),
                short_code: ExternalId::parse("ASMA-7676").expect("code"),
                worktree: worktree.clone(),
            },
        );
        let task = scope.require_task().expect("a ticket scope");
        assert_eq!(task.task_id, task_id);
        assert_eq!(task.worktree, worktree);
    }

    #[test]
    fn an_epic_is_recognized_by_id_and_not_by_its_rendered_name() {
        let mut renamed = epic();
        let id = renamed.mini_project_id;
        renamed.external_epic_key = ExternalId::parse("ASMA-9999").expect("epic key");
        renamed.short_title = ExternalName::parse("Renamed").expect("title");
        let scope = ExecutionScope::for_epic(renamed);
        assert!(scope.is_epic(id));
        assert!(!scope.is_epic(MiniProjectId::generate()));
    }
}
