//! Same-subject evidence without independent consultation findings or outcomes.
use super::*;
use kontor_api::committee_evidence::{
    CommitteeCompletionEvidenceDto, CommitteeSubjectEvidenceBodyDto, CommitteeSubjectEvidenceDto,
    CommitteeTaskEvidenceDto,
};

impl Services {
    /// Existing MCP readers receive small report bodies without acquiring a new tool.
    pub(super) async fn hydrate_subject_reports(
        &self,
        run: &mut CommitteeRunDto,
    ) -> Result<(), ApiError> {
        let Some(subject) = &mut run.subject_evidence else {
            return Ok(());
        };
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut remaining = 256 * 1024;
        for record in subject
            .body
            .tasks
            .iter_mut()
            .flat_map(|task| &mut task.producer_artifacts)
        {
            let locator = &record["locator"];
            let canonical = CanonicalDocument::from_value(locator)
                .map_err(|error| self.refuse_domain(&error))?;
            let content = if record["locator_hash"].as_str() != Some(canonical.hash().as_str()) {
                serde_json::json!({"status":"unavailable", "reason":"stored locator digest mismatch"})
            } else if remaining == 0 || tokio::time::Instant::now() >= deadline {
                serde_json::json!({"status":"deferred", "reason":"bounded inline read budget exhausted; use kontor_committee_artifact_get"})
            } else {
                match tokio::time::timeout_at(deadline, read_registered_git_blob(locator)).await {
                    Ok(Ok(bytes)) => match String::from_utf8(bytes) {
                        Ok(text) => {
                            let mut end = text.len().min(64 * 1024).min(remaining);
                            while !text.is_char_boundary(end) {
                                end -= 1;
                            }
                            remaining -= end;
                            serde_json::json!({"status":"verified", "sha256":ContentHash::of(text.as_bytes()), "text": &text[..end], "total_bytes":text.len(), "next_offset":(end < text.len()).then_some(end)})
                        }
                        Err(_) => {
                            serde_json::json!({"status":"unavailable", "reason":"artifact is not UTF-8 text"})
                        }
                    },
                    Ok(Err(reason)) => serde_json::json!({"status":"unavailable", "reason":reason}),
                    Err(_) => {
                        serde_json::json!({"status":"deferred", "reason":"bounded inline read timeout; use kontor_committee_artifact_get"})
                    }
                }
            };
            record["content"] = content;
        }
        let canonical = CanonicalDocument::from_serializable(&subject.body)
            .map_err(|error| self.refuse_domain(&error))?;
        subject.content_hash = canonical.hash().as_str().to_owned();
        Ok(())
    }

    pub(super) async fn read_committee_artifact(
        &self,
        project: ProjectId,
        committee: CommitteeRunId,
        evidence_id: &str,
        offset: u32,
    ) -> Result<kontor_api::committee_evidence::CommitteeArtifactContentDto, ApiError> {
        let run = self.consultation_run(project, ConsultationRunId::Committee(committee))?;
        let subject = self.committee_subject_evidence(&run)?;
        let record = subject.body.tasks.iter().flat_map(|task| &task.producer_artifacts)
            .find(|record| record["evidence_id"].as_str() == Some(evidence_id))
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the artifact is not registered under this Committee's exact subject and active workflow"))?;
        let locator = &record["locator"];
        let document =
            CanonicalDocument::from_value(locator).map_err(|error| self.refuse_domain(&error))?;
        if record["locator_hash"].as_str() != Some(document.hash().as_str()) {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the stored artifact locator does not match its immutable digest",
            ));
        }
        let bytes = read_registered_git_blob(locator)
            .await
            .map_err(|rule| self.deny(ApiErrorCode::RevisionConflict, rule))?;
        let text = String::from_utf8(bytes).map_err(|_| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "the artifact is not UTF-8 text; no report body can be rendered",
            )
        })?;
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if start > text.len() || !text.is_char_boundary(start) {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "artifact offset must be a character boundary within the verified text",
            ));
        }
        let mut end = start.saturating_add(64 * 1024).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let total_bytes = u32::try_from(text.len()).map_err(|_| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "artifact length exceeds the bounded read",
            )
        })?;
        Ok(
            kontor_api::committee_evidence::CommitteeArtifactContentDto {
                realm_id: self.realm_id.to_string(),
                committee_run_id: committee.to_string(),
                evidence_id: evidence_id.to_owned(),
                sha256: ContentHash::of(text.as_bytes()).as_str().to_owned(),
                offset,
                total_bytes,
                next_offset: (end < text.len())
                    .then(|| u32::try_from(end).expect("bounded artifact offset fits u32")),
                text: text[start..end].to_owned(),
            },
        )
    }

    pub(super) fn committee_subject_evidence(
        &self,
        run: &StoredConsultationRun,
    ) -> Result<CommitteeSubjectEvidenceDto, ApiError> {
        let state = self.state()?;
        let task_id = run.subject.ok_or_else(|| self.deny(ApiErrorCode::StaleBinding, "this legacy Committee has no durably recorded subject; evidence scope cannot be inferred"))?.task_id();
        let cursor_before = self.cursor()?.get();
        let epic = self.read_epic(run.project_id, run.mini_project_id)?;
        let mut tasks = Vec::new();
        for task in epic
            .tasks
            .into_iter()
            .filter(|task| task_id.is_none_or(|subject| subject == task.task_id))
        {
            let inspection = state
                .with_store(|store| store.snapshot_task_inspection(run.project_id, task.task_id))
                .map_err(|error| self.refuse(&error))?
                .value;
            let workflow = inspection.and_then(|inspection| inspection.workflow);
            let evaluations = match &workflow {
                Some(workflow) => state
                    .with_store(|store| store.list_gate_evaluations(run.project_id, workflow.id))
                    .map_err(|error| self.refuse(&error))?,
                None => Vec::new(),
            };
            let producer_artifacts = state
                .with_store(|store| {
                    store.task_artifact_evidence_documents(run.project_id, task.task_id)
                })
                .map_err(|error| self.refuse(&error))?;
            let closure_keys = state
                .with_store(|store| {
                    store.list_current_task_closure_artifact_keys(run.project_id, task.task_id)
                })
                .map_err(|error| self.refuse(&error))?;
            tasks.push(CommitteeTaskEvidenceDto {
                task,
                work_profile: workflow
                    .map(|workflow| serde_json::json!(workflow.snapshot.definition)),
                gate_evaluations: evaluations
                    .iter()
                    .map(|record| serde_json::json!(record))
                    .collect(),
                producer_artifacts,
                native_closure_artifact_keys: closure_keys
                    .into_iter()
                    .map(|key| key.as_str().to_owned())
                    .collect(),
            });
        }
        if task_id.is_some() && tasks.len() != 1 {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the Committee task no longer belongs to its frozen epic",
            ));
        }
        let (completion, open_questions) = if task_id.is_none() {
            let stored = state
                .with_store(|store| store.get_epic_completion(run.project_id, run.mini_project_id))
                .map_err(|error| self.refuse(&error))?;
            let completion = stored
                .as_ref()
                .map(|stored| {
                    let compiled = self.pinned_completion(stored)?;
                    let completion = self.completion_state(stored)?;
                    let projected = self.completion_dto(stored, &compiled)?;
                    // Integration is review input. Never copy rounds, findings, results,
                    // remediation authorizations or needs-human deliberation into this view.
                    let mut integrations = projected.integrations;
                    integrations.extend(
                        projected
                            .remediations
                            .into_iter()
                            .map(|record| record.integration),
                    );
                    Ok::<_, ApiError>(CommitteeCompletionEvidenceDto {
                        profile: projected.profile,
                        definition: serde_json::json!(compiled.profile),
                        ticket_requirements: completion
                            .ticket_requirements
                            .iter()
                            .map(|record| serde_json::json!(record))
                            .collect(),
                        phase: projected.phase,
                        blockers: projected.blockers,
                        integrations,
                        closeout: projected.closeout,
                        generation: projected.generation,
                        revision: projected.revision.get(),
                    })
                })
                .transpose()?;
            let questions = state
                .with_store(|store| {
                    store.list_questions_for_epic(run.project_id, run.mini_project_id)
                })
                .map_err(|error| self.refuse(&error))?;
            (
                completion,
                questions
                    .iter()
                    .map(|question| serde_json::json!(question))
                    .collect(),
            )
        } else {
            (None, Vec::new())
        };
        let body = CommitteeSubjectEvidenceBodyDto {
            schema_version: 1,
            project_id: run.project_id.to_string(),
            epic_id: run.mini_project_id.to_string(),
            task_id: task_id.map(|task| task.to_string()),
            question: run.question.as_str().to_owned(),
            jira_binding: epic.jira_binding,
            tasks,
            completion,
            open_questions,
            cursor_before,
            cursor_after: self.cursor()?.get(),
        };
        let document = CanonicalDocument::from_serializable(&body)
            .map_err(|error| self.refuse_domain(&error))?;
        Ok(CommitteeSubjectEvidenceDto {
            content_hash: document.hash().as_str().to_owned(),
            body,
        })
    }
}

/// Resolve only a server-recorded immutable locator, never a caller's arbitrary path or ref.
async fn read_registered_git_blob(locator: &serde_json::Value) -> Result<Vec<u8>, &'static str> {
    use std::path::{Component, Path};
    let field = |key| {
        locator[key]
            .as_str()
            .ok_or("the artifact has no supported immutable Git locator")
    };
    if field("kind")? != "git_blob" {
        return Err("the artifact locator is not a supported Git blob");
    }
    let git_dir = field("git_dir")?;
    let commit = field("commit")?;
    let path = field("path")?;
    let expected = ContentHash::parse(field("sha256")?)
        .map_err(|_| "the stored artifact digest is invalid")?;
    if !Path::new(git_dir).is_absolute()
        || !matches!(commit.len(), 40 | 64)
        || !commit
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || path.is_empty()
        || !Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err("the artifact locator is not an immutable commit and repository-relative path");
    }
    let git = |args: Vec<String>| async move {
        let mut command = tokio::process::Command::new("git");
        command
            .arg(format!("--git-dir={git_dir}"))
            .args(args)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
            .await
            .map_err(|_| "artifact content read exceeded its bounded Git timeout")?
            .map_err(|_| "the artifact's persistent Git repository is unavailable")?;
        if !output.status.success() {
            return Err("the registered artifact object is unavailable");
        }
        Ok(output.stdout)
    };
    if git(vec!["cat-file".into(), "-t".into(), commit.into()]).await? != b"commit\n" {
        return Err("the registered artifact does not name a commit object");
    }
    let object = format!("{commit}:{path}");
    let size = git(vec!["cat-file".into(), "-s".into(), object.clone()]).await?;
    if String::from_utf8_lossy(&size)
        .trim()
        .parse::<usize>()
        .ok()
        .is_none_or(|size| size > 16 * 1024 * 1024)
    {
        return Err("the artifact exceeds the 16 MiB verified read limit");
    }
    let bytes = git(vec!["cat-file".into(), "blob".into(), object]).await?;
    if ContentHash::of(&bytes) != expected {
        return Err("the committed artifact does not match its registered SHA-256");
    }
    Ok(bytes)
}
