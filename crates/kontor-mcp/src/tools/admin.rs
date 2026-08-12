//! Operations that decide who may act at all, and the account reads that go with
//! them.
//!
//! # What makes an operation admin rather than operator
//!
//! Not how disruptive it looks. Cancelling every run in a project is operator work
//! and granting one bounded execution authorization is admin work, because the
//! split is by *what the operation is authority over*: an operator drives work that
//! is already permitted, and an admin decides what is permitted. The daemon draws
//! the same line in `kontor_api::control::command_authority`, and this module's
//! tiers agree with it — `tests/capability_matrix.rs` is what keeps them agreeing.
//!
//! # Arming, and why there is no automatic version of it
//!
//! [`AUTHORIZE`] grants a bounded execution authorization over one work scope: a
//! project, a goal or a task. There is deliberately no operand for an unbounded
//! grant, no "arm everything" and no schedule that re-arms on its own. An
//! authorization that renewed itself would be a standing permission wearing the
//! costume of a bounded one, and the whole reason a scope and a revision are named
//! here is so that somebody decided this scope, once, on purpose.
//!
//! Disarming is its own command kind in the domain — a revocation receipt must not
//! be replayable as its own grant — and it targets a schedule override rather than
//! an authorization. That command is staged with the rest of the calendar surface
//! (KON-MVP-21), so it is absent here rather than approximated.

use kontor_core::id::{MiniProjectId, ProjectId, TaskId};
use kontor_core::receipt::{AggregateRef, CommandKind};

use crate::capability::Denied;
use crate::client::{CallerTier, Request};
use crate::tools::{
    DRY_RUN, Effect, IDEMPOTENCY_KEY, Operands, Plan, Property, ToolSpec, command_request, intent,
};

/// The three aggregates a work scope can be.
///
/// Mirrors `kontor_core::calendar::WorkScope`, which is what an execution
/// authorization is granted over.
const SCOPES: &[&str] = &["project", "mini_project", "task"];

/// Every coding-account profile in a project.
const ACCOUNTS: ToolSpec = ToolSpec {
    name: "account_list",
    tier: CallerTier::Admin,
    effect: Effect::Query,
    description: "List the coding-account profiles in one project: the runtime family each \
                  authenticates against, the opaque approved alias its credential resolves under, \
                  and whether launches may select it. No credential value, environment value or \
                  endpoint is ever returned.",
    properties: &[Property::required(
        "project_id",
        "The project whose account profiles to list.",
    )],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/accounts",
            operands.project_id()?
        ))))
    },
};

/// One coding-account profile.
const ACCOUNT: ToolSpec = ToolSpec {
    name: "account_show",
    tier: CallerTier::Admin,
    effect: Effect::Query,
    description: "Read one coding-account profile and the control-plane position it is consistent \
                  with. No credential value, environment value or endpoint is ever returned.",
    properties: &[
        Property::required("project_id", "The project that owns the account profile."),
        Property::required(
            "account_profile_id",
            "The account profile to read, as a canonical identifier.",
        ),
    ],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/accounts/{}",
            operands.project_id()?,
            operands.account_profile_id()?
        ))))
    },
};

/// Grant a bounded execution authorization over one work scope.
const AUTHORIZE: ToolSpec = ToolSpec {
    name: "authorize_execution",
    tier: CallerTier::Admin,
    effect: Effect::Mutation,
    description: "Grant a bounded execution authorization over one work scope — a project, a goal \
                  or a single task — so scheduling may admit work inside it. The scope and the \
                  revision are both named: this is a deliberate, bounded grant and there is no \
                  unbounded or self-renewing form of it.",
    properties: &[
        Property::required("project_id", "The project the grant is recorded in."),
        Property::choice(
            "target_kind",
            SCOPES,
            "Which kind of aggregate the scope is.",
        ),
        Property::required(
            "target_id",
            "The scope's own identifier, of the kind named by target_kind.",
        ),
        Property::number(
            "expected_revision",
            "The scope aggregate's current revision, as a read returned it.",
        ),
        Property::optional(
            "reason",
            "Why this scope is being armed, recorded in the command's intent document.",
        ),
        IDEMPOTENCY_KEY,
        DRY_RUN,
    ],
    build: |operands| {
        let project_id = operands.project_id()?;
        let target = operands.work_scope(project_id)?;
        command_request(
            operands,
            CommandKind::AuthorizeExecution,
            project_id,
            target,
            operands.expected_revision()?,
            intent(operands.optional_text("reason"), &[]),
        )
    },
};

impl Operands<'_> {
    /// The account profile a read names.
    ///
    /// # Errors
    /// Refuses an identifier that is not a canonical UUID v7.
    pub fn account_profile_id(&self) -> Result<kontor_core::id::AccountProfileId, Denied> {
        kontor_core::id::AccountProfileId::parse(self.opaque("account_profile_id")).map_err(
            |error| Denied::WrongTypeDetail {
                tool: self.tool_name().to_owned(),
                property: "account_profile_id".to_owned(),
                rule: error.to_string(),
            },
        )
    }

    /// The work scope an authorization is granted over.
    ///
    /// The scope kind and the identifier are read together, so a caller naming a
    /// task id under `project` is refused rather than having its id silently
    /// reinterpreted as the wrong aggregate.
    ///
    /// # Errors
    /// Refuses an identifier that is not canonical for the named kind.
    pub fn work_scope(&self, project_id: ProjectId) -> Result<AggregateRef, Denied> {
        let refuse = |error: &kontor_core::DomainError| Denied::WrongTypeDetail {
            tool: self.tool_name().to_owned(),
            property: "target_id".to_owned(),
            rule: error.to_string(),
        };
        let target_id = self.opaque("target_id");
        match self.opaque("target_kind") {
            "project" => {
                let named = ProjectId::parse(target_id).map_err(|error| refuse(&error))?;
                if named != project_id {
                    return Err(Denied::WrongTypeDetail {
                        tool: self.tool_name().to_owned(),
                        property: "target_id".to_owned(),
                        rule: "a project-scoped grant must name the project it is recorded in"
                            .to_owned(),
                    });
                }
                Ok(AggregateRef::Project { project_id: named })
            }
            "mini_project" => Ok(AggregateRef::MiniProject {
                mini_project_id: MiniProjectId::parse(target_id).map_err(|error| refuse(&error))?,
            }),
            "task" => Ok(AggregateRef::Task {
                task_id: TaskId::parse(target_id).map_err(|error| refuse(&error))?,
            }),
            // Unreachable through `plan`, which validates the choice first. Refused
            // rather than defaulted anyway: a scope this code did not understand is
            // not a scope to pick a fallback for.
            other => Err(Denied::WrongType {
                tool: self.tool_name().to_owned(),
                property: "target_kind".to_owned(),
                expected: match other {
                    "" => "a work scope kind",
                    _ => "one of project, mini_project or task",
                },
            }),
        }
    }
}

/// Every operation that requires admin authority.
#[must_use]
pub const fn tools() -> &'static [ToolSpec] {
    &[ACCOUNT, ACCOUNTS, AUTHORIZE]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::find;

    fn arguments(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        json.as_object().expect("an arguments object").clone()
    }

    #[test]
    fn every_admin_tool_requires_admin_authority() {
        for tool in tools() {
            assert_eq!(
                tool.tier,
                CallerTier::Admin,
                "{} is in the admin module and must require admin authority",
                tool.name
            );
        }
    }

    #[test]
    fn a_grant_names_one_of_the_three_work_scopes_and_builds_the_matching_target() {
        let project = "0192f0c0-0000-7000-8000-000000000001";
        let task = "0192f0c0-0000-7000-8000-000000000002";
        let goal = "0192f0c0-0000-7000-8000-000000000003";

        let scoped_to_task = find("authorize_execution")
            .expect("the authorize_execution tool")
            .plan(&arguments(serde_json::json!({
                "project_id": project,
                "target_kind": "task",
                "target_id": task,
                "expected_revision": 1
            })))
            .expect("a task-scoped grant plans");
        let body = scoped_to_task.request.body.as_ref().expect("a body");
        assert_eq!(body["target"]["kind"], serde_json::json!("task"));
        assert_eq!(body["target"]["task_id"], serde_json::json!(task));
        assert_eq!(
            scoped_to_task.request.path,
            "/v1/commands/authorize_execution"
        );

        let scoped_to_goal = find("authorize_execution")
            .expect("the authorize_execution tool")
            .plan(&arguments(serde_json::json!({
                "project_id": project,
                "target_kind": "mini_project",
                "target_id": goal,
                "expected_revision": 1
            })))
            .expect("a goal-scoped grant plans");
        assert_eq!(
            scoped_to_goal.request.body.as_ref().expect("a body")["target"]["mini_project_id"],
            serde_json::json!(goal)
        );
    }

    #[test]
    fn a_project_scoped_grant_may_not_name_a_different_project() {
        // Otherwise a caller holding one project's revision could arm another
        // project by pasting the wrong id into the target.
        let refusal = find("authorize_execution")
            .expect("the authorize_execution tool")
            .plan(&arguments(serde_json::json!({
                "project_id": "0192f0c0-0000-7000-8000-000000000001",
                "target_kind": "project",
                "target_id": "0192f0c0-0000-7000-8000-0000000000ff",
                "expected_revision": 1
            })))
            .expect_err("a mismatched project scope is refused");
        assert!(matches!(refusal, Denied::WrongTypeDetail { .. }));
    }

    #[test]
    fn a_scope_kind_and_an_identifier_must_agree() {
        // A task id under `project` is a caller mistake, and reinterpreting it as a
        // project id is how one aggregate's revision gets spent on another.
        assert!(
            find("authorize_execution")
                .expect("the authorize_execution tool")
                .plan(&arguments(serde_json::json!({
                    "project_id": "0192f0c0-0000-7000-8000-000000000001",
                    "target_kind": "mini_project",
                    "target_id": "not-a-uuid",
                    "expected_revision": 1
                })))
                .is_err(),
            "an identifier that is not canonical for the named kind is refused"
        );
    }

    #[test]
    fn there_is_no_operand_for_an_unbounded_or_self_renewing_grant() {
        let authorize = find("authorize_execution").expect("the authorize_execution tool");
        let names: Vec<&str> = authorize
            .properties
            .iter()
            .map(|property| property.name)
            .collect();
        for forbidden in [
            "auto_arm",
            "always",
            "renew",
            "recurring",
            "forever",
            "unbounded",
            "all_projects",
        ] {
            assert!(
                !names.contains(&forbidden),
                "an execution authorization must stay a deliberate bounded grant, and `{forbidden}` \
                 would make it something else"
            );
        }
    }

    #[test]
    fn an_account_view_declares_no_credential_operand() {
        for tool in [ACCOUNT, ACCOUNTS] {
            for property in tool.properties {
                assert!(
                    !property.name.contains("credential")
                        && !property.name.contains("token")
                        && !property.name.contains("secret"),
                    "{} must not take {} as an operand",
                    tool.name,
                    property.name
                );
            }
        }
    }
}
