//! Explicit completion-only disposition of unavailable historical producer provenance.
use super::*;
use kontor_api::applications::{HistoricalEvidencePacketDto, HistoricalTaskEvidenceDto};
use kontor_scheduler::{HistoricalCompletionEvidence, HistoricalTaskEvidence};

impl Services {
    fn validate_historical_task(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        pin: &HistoricalTaskEvidence,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let task = self.task_row(project_id, pin.task_id)?;
        if task.mini_project_id != Some(epic_id)
            || task.state != TaskState::Done
            || task.imported_state.is_some()
            || task.revision != pin.expected_task_revision
        {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "historical completion evidence requires the exact native Done task in this epic",
            ));
        }
        let inspection = state
            .with_store(|store| store.snapshot_task_inspection(project_id, pin.task_id))
            .map_err(|error| self.refuse(&error))?
            .value
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the historical task is missing"))?;
        let workflow = inspection.workflow.ok_or_else(|| {
            self.deny(
                ApiErrorCode::RevisionConflict,
                "historical completion evidence requires an active workflow",
            )
        })?;
        if !workflow.active
            || workflow.id != pin.workflow_id
            || workflow.revision != pin.expected_workflow_revision
            || workflow.snapshot.definition_hash != pin.workflow_hash
            || workflow.snapshot.definition.gates.iter().any(|gate| {
                inspection.gates.get(&gate.id) != Some(&kontor_core::state::GateState::Passed)
            })
        {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the historical task workflow drifted or a required gate is not passed",
            ));
        }
        Ok(())
    }

    /// Keep every record for history, but only unchanged current-era pins may be consumed.
    pub(super) fn validated_history_state(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        current: &CompletionState,
    ) -> CompletionState {
        let mut judged = current.clone();
        judged.historical_evidence.retain(|record| {
            record.generation == current.generation
                && record.tasks.iter().all(|pin| {
                    self.validate_historical_task(project_id, epic_id, pin)
                        .is_ok()
                })
        });
        judged
    }

    pub(super) fn require_current_historical_evidence(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        current: &CompletionState,
    ) -> Result<(), ApiError> {
        for record in current
            .historical_evidence
            .iter()
            .filter(|record| record.generation == current.generation)
        {
            for pin in &record.tasks {
                self.validate_historical_task(project_id, epic_id, pin)?;
            }
        }
        Ok(())
    }

    pub(super) async fn historical_completion_observation(
        &self,
        stored: &StoredEpicCompletion,
        current: &CompletionState,
        evidence_hash: &ContentHash,
        reason: &str,
        packet: &HistoricalEvidencePacketDto,
        tasks: &[HistoricalTaskEvidenceDto],
    ) -> Result<CompletionObservation, ApiError> {
        if current.phase != CompletionPhase::Tickets
            || reason.trim().is_empty()
            || reason.len() > 2048
            || tasks.is_empty()
            || tasks.len() > 128
        {
            return Err(self.deny(ApiErrorCode::InvalidRequest, "historical evidence requires the ticket gate, a bounded explicit provenance reason and exact tasks"));
        }
        self.require_current_historical_evidence(
            stored.project_id,
            stored.mini_project_id,
            current,
        )?;
        let actual = self.epic_ticket_evidence(stored.project_id, &current.ticket_requirements)?;
        let effective = kontor_scheduler::completion_ticket_evidence(current, &actual);
        let mut seen_tasks = BTreeSet::new();
        let mut pins = Vec::new();
        for offered in tasks {
            if !seen_tasks.insert(offered.task_id) {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "historical evidence repeats a task",
                ));
            }
            let artifact_keys = offered
                .artifact_keys
                .iter()
                .map(|key| ExternalName::parse(key).map_err(|error| self.refuse_domain(&error)))
                .collect::<Result<BTreeSet<_>, _>>()?;
            let requirement = current
                .ticket_requirements
                .iter()
                .find(|task| task.task_id == offered.task_id)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::InvalidRequest,
                        "historical evidence names a task outside the frozen completion contract",
                    )
                })?;
            let present = effective
                .iter()
                .find(|task| task.task_id == offered.task_id)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::InvalidRequest,
                        "the historical task has no current evidence projection",
                    )
                })?;
            if artifact_keys.is_empty()
                || artifact_keys.len() != offered.artifact_keys.len()
                || !artifact_keys.is_subset(&requirement.evidence)
                || !artifact_keys.is_disjoint(&present.evidence)
            {
                return Err(self.deny(ApiErrorCode::InvalidRequest, "historical evidence must name distinct, declared, currently missing artifact keys"));
            }
            let pin = HistoricalTaskEvidence {
                task_id: offered.task_id,
                expected_task_revision: offered.expected_task_revision,
                workflow_id: offered.workflow_id,
                expected_workflow_revision: offered.expected_workflow_revision,
                workflow_hash: offered.workflow_hash.clone(),
                artifact_keys,
            };
            self.validate_historical_task(stored.project_id, stored.mini_project_id, &pin)?;
            pins.push(pin);
        }
        let root = self.project_row(stored.project_id)?.root_path;
        let git_dir = artifact_submission::verified_git_blob(
            root.as_str(),
            &packet.commit,
            &packet.path,
            &packet.sha256,
        )
        .await
        .map_err(|rule| self.deny(ApiErrorCode::RevisionConflict, rule))?;
        let locator = serde_json::json!({"kind":"git_blob","git_dir":git_dir,"commit":packet.commit,"path":packet.path,"sha256":packet.sha256});
        let bytes = committee_evidence::read_registered_git_blob(&locator)
            .await
            .map_err(|rule| self.deny(ApiErrorCode::RevisionConflict, rule))?;
        if bytes.is_empty() || bytes.len() > 64 * 1024 {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "historical verification packet must contain at most 64 KiB of UTF-8 text",
            ));
        }
        let body = String::from_utf8(bytes).map_err(|_| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "historical verification packet is not UTF-8",
            )
        })?;
        if body.trim().is_empty() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "historical verification packet is blank",
            ));
        }
        // The filesystem read awaited external work. Recheck all mutable pins before commit.
        for pin in &pins {
            self.validate_historical_task(stored.project_id, stored.mini_project_id, pin)?;
        }
        Ok(CompletionObservation::HistoricalEvidenceRecorded {
            record: HistoricalCompletionEvidence {
                disposition: kontor_scheduler::HistoricalEvidenceDisposition::ProvenanceUnavailable,
                evidence_hash: evidence_hash.clone(),
                generation: current.generation,
                reason: reason.to_owned(),
                recorded_by: kontor_scheduler::HistoricalEvidenceAuthority::Admin,
                git_dir,
                commit: packet.commit.clone(),
                path: packet.path.clone(),
                sha256: packet.sha256.clone(),
                packet: body,
                tasks: pins,
            },
            evidence: actual,
        })
    }
}
