//! Explicit operator recovery, never inferred agent authorship of bytes.
use super::*;
use kontor_api::artifacts::{ArtifactSubmissionDto, RecordArtifactRequest};
use kontor_api::auth::CallerCapability;
use kontor_core::id::RoleTurnId;
use kontor_policy::model::ArtifactEvidenceId;
use kontor_store::{EvaluationBinding, NewArtifactEvidence, NewArtifactSubmission};
use std::path::{Component, Path};

impl Services {
    pub(super) async fn recover_artifact(
        &self,
        key: &IdempotencyKey,
        authority: CallerCapability,
        project_id: ProjectId,
        task_id: TaskId,
        request: &RecordArtifactRequest,
    ) -> Result<ArtifactSubmissionDto, ApiError> {
        let state = self.state()?;
        let intent = CanonicalDocument::from_value(&serde_json::json!({"schema_version":1, "project_id":project_id, "task_id":task_id, "authority":authority.as_str(), "request": request})).map_err(|e| self.refuse_domain(&e))?;
        let decode = |json: &str| {
            serde_json::from_str::<ArtifactSubmissionDto>(json).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the artifact receipt cannot be decoded",
                )
            })
        };
        if let Some(prior) = state
            .with_store(|s| s.artifact_submission_replay(project_id, key, intent.hash()))
            .map_err(|e| self.refuse(&e))?
        {
            let receipt = decode(&prior)?;
            self.resume_artifact_derivations(project_id, &receipt)
                .await?;
            return Ok(receipt);
        }
        let turn_id =
            RoleTurnId::parse(&request.role_turn_id).map_err(|e| self.refuse_domain(&e))?;
        let artifact =
            ArtifactKey::parse(&request.artifact_key).map_err(|e| self.refuse_domain(&e))?;
        let hash = ContentHash::parse(&request.sha256).map_err(|e| self.refuse_domain(&e))?;
        let task = self.task_row(project_id, task_id)?;
        if task.revision != request.expected_task_revision {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the task moved before artifact recovery",
            ));
        }
        let turn = state
            .with_store(|s| s.list_settled_turns(project_id, task_id))
            .map_err(|e| self.refuse(&e))?
            .into_iter()
            .find(|t| t.id == turn_id && t.artifacts.contains(&artifact))
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the exact settled turn does not claim this artifact for this task",
                )
            })?;
        let inspection = state
            .with_store(|s| s.snapshot_task_inspection(project_id, task_id))
            .map_err(|e| self.refuse(&e))?
            .value
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "no task inspection exists"))?;
        let workflow = inspection.workflow.ok_or_else(|| {
            self.deny(
                ApiErrorCode::RevisionConflict,
                "artifact recovery requires an active pinned workflow",
            )
        })?;
        self.stored_workflow_policy(project_id, &workflow)?;
        if turn.settled_at < workflow.created_at {
            return Err(self.deny(ApiErrorCode::RevisionConflict, "the settled claim predates the active workflow and cannot evidence its artifact contract"));
        }
        let contract = workflow
            .snapshot
            .definition
            .artifacts
            .iter()
            .find(|c| c.key == artifact)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the artifact is not declared by the active pinned workflow",
                )
            })?;
        let team = state
            .with_store(|s| s.get_team_run(project_id, turn.team_run_id))
            .map_err(|e| self.refuse(&e))?
            .ok_or_else(|| {
                self.deny(ApiErrorCode::NotFound, "the settled turn's team is missing")
            })?;
        let run = state
            .with_store(|s| s.get_agent_run(project_id, turn.agent_run_id))
            .map_err(|e| self.refuse(&e))?
            .ok_or_else(|| {
                self.deny(ApiErrorCode::NotFound, "the settled turn's run is missing")
            })?;
        let template = kontor_teams::spec::TeamTemplateSpec::from_snapshot(&team.snapshot)
            .map_err(|e| self.refuse_domain(&e))?;
        if team.task_id != task_id
            || run.team_run_id != team.id
            || run.role != turn.role_slot_id.clone().into_role_key()
            || template.slot(&turn.role_slot_id).is_none()
            || workflow
                .snapshot
                .definition
                .team_template
                .is_some_and(|pin| {
                    pin.template_id != team.snapshot.template_id
                        || pin.version != team.snapshot.template_version
                })
        {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the settled producer does not match the workflow's pinned team",
            ));
        }
        // Only restrict producer ownership when the frozen template actually
        // declares it. Older profiles have phases but no exclusive role owner.
        let producers: BTreeSet<_> = template
            .handoffs
            .iter()
            .filter(|h| h.required_artifacts.contains(&artifact))
            .map(|h| &h.from_slot)
            .collect();
        if !producers.is_empty() && !producers.contains(&turn.role_slot_id) {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "the pinned handoff assigns this artifact to another producer slot",
            ));
        }
        // An incoming profile edge names the role receiving work in this phase.
        // Outgoing edges name the next phase's role and cannot prove authorship here.
        let phase_roles: BTreeSet<_> = workflow
            .snapshot
            .definition
            .edges
            .iter()
            .filter(|edge| edge.to == contract.producer_phase)
            .filter_map(|edge| edge.handoff_role.as_ref())
            .collect();
        let roles = team
            .snapshot
            .slot_role_assignments()
            .map_err(|e| self.refuse_domain(&e))?;
        if !phase_roles.is_empty()
            && !roles
                .get(&turn.role_slot_id)
                .is_some_and(|role| phase_roles.contains(role))
        {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "the pinned producing phase is assigned to another logical role",
            ));
        }
        let account = run.account_profile_id.ok_or_else(|| {
            self.deny(
                ApiErrorCode::RevisionConflict,
                "the source producer has no attributable provider account",
            )
        })?;
        let root = match request.repository.as_str() {
            "project" => state
                .with_store(|s| s.get_project(project_id))
                .map_err(|e| self.refuse(&e))?
                .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the project is missing"))?
                .root_path
                .as_str()
                .to_owned(),
            "task" => state
                .with_store(|s| s.task_worktree(project_id, task_id))
                .map_err(|e| self.refuse(&e))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::RevisionConflict,
                        "the task has no declared worktree",
                    )
                })?
                .as_str()
                .to_owned(),
            _ => {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "repository must select the registered project or task root",
                ));
            }
        };
        let git_dir = verified_git_blob(&root, &request.commit, &request.path, &hash)
            .await
            .map_err(|reason| self.deny(ApiErrorCode::RevisionConflict, reason))?;
        let class = if turn.runtime_proof.is_some() {
            "runtime_proved"
        } else {
            "legacy_or_attested"
        };
        let now = kontor_api::now();
        let locator = CanonicalDocument::from_value(&serde_json::json!({"schema_version":1, "kind":"git_blob", "git_dir":git_dir, "commit":request.commit, "path":request.path, "sha256":hash.as_str(), "provenance":"operator_recovered_git_blob", "role_turn_id":turn.id, "turn_proof_class":class, "producer_phase":contract.producer_phase.as_str()})).map_err(|e| self.refuse_domain(&e))?;
        let id = ArtifactEvidenceId::generate();
        let response = ArtifactSubmissionDto {
            schema_version: 1,
            realm_id: state.realm_id().to_string(),
            evidence_id: id.to_string(),
            task_id: task_id.to_string(),
            workflow_id: workflow.id.to_string(),
            role_turn_id: turn.id.to_string(),
            agent_run_id: run.id.to_string(),
            producer_role: run.role.as_str().to_owned(),
            producer_account: account.to_string(),
            artifact_key: artifact.as_str().to_owned(),
            producer_phase: contract.producer_phase.as_str().to_owned(),
            provenance: "operator_recovered_git_blob".to_owned(),
            turn_proof_class: class.to_owned(),
            recorded_by: authority.as_str().to_owned(),
            git_dir,
            commit: request.commit.clone(),
            path: request.path.clone(),
            sha256: hash.as_str().to_owned(),
            locator_hash: locator.hash().as_str().to_owned(),
            recorded_at: now.to_string(),
        };
        let recorded = state
            .with_store(|s| {
                s.record_artifact_submission(&NewArtifactSubmission {
                    evidence: NewArtifactEvidence {
                        id,
                        binding: EvaluationBinding {
                            project_id,
                            task_id,
                            workflow_id: workflow.id,
                            team_run_id: Some(team.id),
                            agent_run_id: Some(run.id),
                        },
                        key: artifact,
                        locator,
                        producer_role: run.role,
                        producer_account: account,
                        recorded_at: now,
                    },
                    role_turn_id: turn.id,
                    task_revision: task.revision,
                    idempotency_key: key.clone(),
                    request_hash: intent.hash().clone(),
                    authority_tier: authority.as_str().to_owned(),
                    turn_proof_class: class.to_owned(),
                    response: CanonicalDocument::from_serializable(&response)?,
                })
            })
            .map_err(|e| self.refuse(&e))?;
        state.signals().appended();
        let receipt = decode(&recorded)?;
        self.resume_artifact_derivations(project_id, &receipt)
            .await?;
        Ok(receipt)
    }

    /// A receipt commits before its projections/native effects. Replaying it
    /// must finish that debt, without applying old claims to a replacement pin.
    async fn resume_artifact_derivations(
        &self,
        project_id: ProjectId,
        receipt: &ArtifactSubmissionDto,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let task_id = TaskId::parse(&receipt.task_id).map_err(|e| self.refuse_domain(&e))?;
        if self.task_row(project_id, task_id)?.state.is_terminal() {
            return Ok(());
        }
        let workflow = state
            .with_store(|s| s.get_active_task_workflow(project_id, task_id))
            .map_err(|e| self.refuse(&e))?;
        if workflow.is_none_or(|workflow| workflow.id.to_string() != receipt.workflow_id) {
            return Ok(());
        }
        let turn = state
            .with_store(|s| s.list_settled_turns(project_id, task_id))
            .map_err(|e| self.refuse(&e))?
            .into_iter()
            .find(|turn| turn.id.to_string() == receipt.role_turn_id)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the artifact receipt's source turn is missing",
                )
            })?;
        self.advance_workflow_from_evidence(project_id, task_id, PhaseRoute::Unambiguous)?;
        let now = kontor_api::now();
        self.derive_follow_ups(project_id, &turn, now).await?;
        // A crash can also occur after deriving a dispatch but before its native
        // acknowledgement. Retry only this receipt's owed rows, with their
        // original message identities, just like ordinary reconciliation.
        let pending = state
            .with_store(|s| s.list_turn_dispatches(project_id))
            .map_err(|e| self.refuse(&e))?;
        for row in pending
            .into_iter()
            .filter(|row| row.settled_turn_id == turn.id && !row.dispatched)
        {
            if self.slot_is_waived(project_id, row.team_run_id, &row.to_role_slot_id)? {
                continue;
            }
            let Some(handoff) = self.handoff_for(project_id, &turn, &row.to_role_slot_id)? else {
                continue;
            };
            let message_id = kontor_runtime::request::MessageId::parse(&row.message_id)
                .map_err(|e| self.refuse_domain(&e))?;
            let target = self.seat_for_slot(project_id, row.team_run_id, &row.to_role_slot_id)?;
            self.deliver_follow_up(project_id, &turn, &handoff, target, message_id, now)
                .await?;
        }
        Ok(())
    }
}

/// Read immutable committed bytes. No shell, network, hooks or mutable ref resolution.
async fn verified_git_blob(
    root: &str,
    commit: &str,
    path: &str,
    expected: &ContentHash,
) -> Result<String, &'static str> {
    if !matches!(commit.len(), 40 | 64)
        || !commit
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || path.is_empty()
        || !Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err("artifact locator requires a full commit id and a repository-relative path");
    }
    let git = |args: Vec<String>| {
        let root = root.to_owned();
        async move {
            let mut command = tokio::process::Command::new("git");
            command
                .arg("-C")
                .arg(root)
                .args(args)
                .env("GIT_NO_REPLACE_OBJECTS", "1")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_OBJECT_DIRECTORY")
                .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
                .kill_on_drop(true);
            let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
                .await
                .map_err(|_| "artifact verification exceeded its bounded Git read")?
                .map_err(|_| "the registered Git repository cannot be read")?;
            if output.status.success() {
                Ok(output.stdout)
            } else {
                Err("the committed artifact cannot be resolved in the registered repository")
            }
        }
    };
    if git(vec!["cat-file".into(), "-t".into(), commit.into()]).await? != b"commit\n" {
        return Err("artifact locator must name a commit object");
    }
    let object = format!("{commit}:{path}");
    let size = git(vec!["cat-file".into(), "-s".into(), object.clone()]).await?;
    if String::from_utf8_lossy(&size)
        .trim()
        .parse::<u64>()
        .ok()
        .is_none_or(|s| s > 16 * 1024 * 1024)
    {
        return Err("artifact blob exceeds the 16 MiB verification limit");
    }
    let bytes = git(vec!["cat-file".into(), "blob".into(), object]).await?;
    if &ContentHash::of(&bytes) != expected {
        return Err("the committed artifact content does not match its claimed SHA-256");
    }
    let common = git(vec![
        "rev-parse".into(),
        "--path-format=absolute".into(),
        "--git-common-dir".into(),
    ])
    .await?;
    let common = String::from_utf8(common).map_err(|_| "the repository locator is not UTF-8")?;
    std::fs::canonicalize(common.trim())
        .map_err(|_| "the persistent Git directory is missing")?
        .to_str()
        .map(str::to_owned)
        .ok_or("the persistent Git directory is not UTF-8")
}
