//! Compose the question domain with current seat authority and atomic receipts.
use super::*;
use kontor_api::open_questions::{QuestionAction, QuestionActor, RecordQuestionRequest};
use kontor_core::open_question::{CloserPolicy, OpenQuestion};

impl Services {
    fn question_author(
        &self,
        project: ProjectId,
        epic: MiniProjectId,
        actor: QuestionActor,
    ) -> Result<kontor_core::state::SeatBinding, ApiError> {
        let state = self.state()?;
        let seat = state
            .with_store(|store| store.get_seat_binding(project, actor.seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Forbidden,
                    "the author is not a seat in this project",
                )
            })?;
        let node = state
            .with_store(|store| store.get_topology_node(project, seat.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(ApiErrorCode::Forbidden, "the author has no owning topology")
            })?;
        if seat.lifecycle != TopologyLifecycle::Active || node.mini_project_id != Some(epic) {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "the author must be an active seat of this exact epic",
            ));
        }
        if let Some(generation) = actor.occupancy_generation {
            let current = state
                .with_store(|store| {
                    store.hosted_topology_seat_occupancy_generation(project, seat.id)
                })
                .map_err(|error| self.refuse(&error))?;
            if current != Some(generation) {
                return Err(self.deny(
                    ApiErrorCode::StaleBinding,
                    "the question author credential has no matching current hosted occupancy",
                ));
            }
        }
        Ok(seat)
    }

    pub(super) fn read_open_questions(
        &self,
        project: ProjectId,
        epic: MiniProjectId,
        actor: Option<QuestionActor>,
    ) -> Result<serde_json::Value, ApiError> {
        self.epic_row(project, epic)?;
        if let Some(actor) = actor {
            self.question_author(project, epic, actor)?;
        }
        let questions = self
            .state()?
            .with_store(|store| store.list_questions_for_epic(project, epic))
            .map_err(|error| self.refuse(&error))?;
        let summaries: Vec<_> = questions.iter().map(OpenQuestion::summary).collect();
        Ok(
            serde_json::json!({"realm_id":self.state()?.realm_id(),"project_id":project,"epic_id":epic,"questions":questions,"summaries":summaries}),
        )
    }

    pub(super) fn write_open_question(
        &self,
        key: &IdempotencyKey,
        project: ProjectId,
        epic: MiniProjectId,
        actor: QuestionActor,
        request: &RecordQuestionRequest,
    ) -> Result<serde_json::Value, ApiError> {
        self.epic_row(project, epic)?;
        let seat = self.question_author(project, epic, actor)?;
        let intent = self.intent(&serde_json::json!({"schema_version":1,"operation":"record_open_question","project_id":project,"epic_id":epic,"actor":actor,"request":request}))?;
        if let Some(result) = self
            .state()?
            .with_store(|store| store.replay_open_question_command(key, &intent))
            .map_err(|error| self.refuse(&error))?
        {
            return Ok(
                serde_json::json!({"realm_id":self.state()?.realm_id(),"result":result,"replayed":true}),
            );
        }
        let now = kontor_api::now();
        let mut question = match &request.action {
            QuestionAction::Raise {
                subject,
                scope,
                attachment,
                why_ambiguous,
                options,
            } => {
                if request.expected_revision != 0 {
                    return Err(self.deny(
                        ApiErrorCode::RevisionConflict,
                        "raise requires expected_revision zero",
                    ));
                }
                OpenQuestion::raise(
                    request.question_id,
                    project,
                    epic,
                    subject.clone(),
                    *scope,
                    attachment.clone(),
                    seat.id,
                    why_ambiguous.clone(),
                    options.clone(),
                    now,
                )
                .map_err(|error| self.refuse_domain(&error))?
            }
            _ => {
                let question = self
                    .state()?
                    .with_store(|store| store.get_question(project, request.question_id))
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::NotFound,
                            "the question does not exist in this project",
                        )
                    })?;
                if question.mini_project_id != epic {
                    return Err(self.deny(
                        ApiErrorCode::Forbidden,
                        "the question belongs to another epic",
                    ));
                }
                if question.revision.get() != request.expected_revision {
                    return Err(self
                        .deny(
                            ApiErrorCode::RevisionConflict,
                            "the question moved since it was read",
                        )
                        .with_revision(Some(question.revision)));
                }
                question
            }
        };
        match &request.action {
            QuestionAction::Raise { .. } => {}
            QuestionAction::Correct {
                why_ambiguous,
                options,
                supersedes,
            } => {
                question
                    .append_round(
                        seat.id,
                        why_ambiguous.clone(),
                        options.clone(),
                        *supersedes,
                        now,
                    )
                    .map_err(|error| self.refuse_domain(&error))?;
            }
            QuestionAction::Dispose {
                outcome,
                supersedes,
            } => {
                let generation = actor.occupancy_generation.ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::Forbidden,
                        "a disposition needs the exact leadership seat credential",
                    )
                })?;
                let required = if question.scope.needs_architecture_closer() {
                    MANDATORY_LEAD_ROLE
                } else {
                    MANDATORY_PROGRAM_ROLE
                };
                self.require_completion_authority(project, epic, required, seat.id, generation)?;
                let policy = CloserPolicy {
                    architecture_closer: RoleKey::parse(&MANDATORY_LEAD_ROLE.to_ascii_lowercase())
                        .map_err(|error| self.refuse_domain(&error))?,
                    process_closer: RoleKey::parse(&MANDATORY_PROGRAM_ROLE.to_ascii_lowercase())
                        .map_err(|error| self.refuse_domain(&error))?,
                };
                question
                    .dispose(
                        seat.id,
                        &RoleKey::parse(&seat.role.role_code.as_str().to_ascii_lowercase())
                            .map_err(|error| self.refuse_domain(&error))?,
                        &policy,
                        outcome.clone(),
                        *supersedes,
                        now,
                    )
                    .map_err(|error| self.refuse_domain(&error))?;
            }
            QuestionAction::FireTrigger { trigger } => {
                question
                    .fire_trigger(trigger, seat.id, now)
                    .map_err(|error| self.refuse_domain(&error))?;
            }
        }
        let result = self
            .state()?
            .with_store(|store| {
                store.commit_open_question_command(
                    key,
                    &intent,
                    &question,
                    request.expected_revision,
                    seat.id,
                    seat.revision,
                    actor.occupancy_generation,
                )
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(
            serde_json::json!({"realm_id":self.state()?.realm_id(),"result":result,"replayed":false}),
        )
    }
}
