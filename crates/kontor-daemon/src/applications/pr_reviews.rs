//! PR reviews on a PR desk (ASMA-8453).
//!
//! A Team Definition that declares `pr_review` places one review per pull
//! request on its PR desk: an Adam subject directly below the desk's
//! workspace, which owns no native container and runs its one seat in the
//! workspace's, and a review room directly below the desk, a native workspace
//! of the desk's project hosting the declared reviewer seats. The pull request
//! is the repository and the number, and nothing else; the confirmed Jira key
//! a review is opened under must authorize that repository, and is a fact
//! about the review, never a second identity. Closing a review fences and
//! releases its own seats and touches nothing else on the desk.
use super::*;
use kontor_api::applications::TeamDefinitionRefDto;
use kontor_api::pr_reviews::{
    ClosePrReviewRequest, OpenPrReviewRequest, PrReviewContainerDto, PrReviewDto, PrReviewNodeDto,
    PrReviewOutcomeDto, PrReviewReportDto, PrReviewSeatDto, PullRequestRequest,
};
use kontor_core::pull_request::PullRequestRef;
use kontor_core::repository::StoredPrReviewSubject;
use kontor_core::spec::TeamPrReview;
use kontor_store::JiraBindingSubject;

/// Re-state a refusal that happened after part of a PR review's close landed.
///
/// A retirement whose acknowledgement was lost has already taken the session,
/// and every session fenced before it is gone too. Keeping `unavailable`
/// would tell the caller nothing changed and invite a fresh key; this close is
/// idempotent and converges, so say that.
fn partial_close(error: ApiError) -> ApiError {
    ApiError::new(
        error.realm_id,
        ApiErrorCode::DeliveryUnconfirmed,
        "the PR review close was partially applied: a session was retired or a seat released before the runtime refused",
    )
    .advising(
        "retry this same close with the same idempotency key; it converges on the remaining seats and reopens nothing",
    )
}

/// One seat a review places: its node's kind, its slot and its role,
/// resolved in the role catalog before anything is written.
struct ReviewSeat {
    kind: TopologyKindKey,
    slot_id: RoleSlotId,
    role: CatalogRoleRef,
}

impl Services {
    /// The subject whose Adam or room is this node. A node scoped to an epic
    /// is never one, so it is not even asked.
    fn pr_review_subject_of(
        &self,
        node: &SessionTopologyNode,
    ) -> Result<Option<StoredPrReviewSubject>, ApiError> {
        if node.mini_project_id.is_some() {
            return Ok(None);
        }
        self.state()?
            .with_store(|store| store.get_pr_review_subject_by_node(node.project_id, node.id))
            .map_err(|error| self.refuse(&error))
    }

    /// The desk one subject was placed on.
    fn pr_review_desk(&self, subject: &StoredPrReviewSubject) -> Result<StoredDesk, ApiError> {
        self.state()?
            .with_store(|store| store.get_desk(subject.project_id, &subject.desk_key))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the desk a PR review names is missing",
                )
            })
    }

    /// The desk workspace whose native container holds a PR review Adam's
    /// session, with its desk; `None` for any other node.
    pub(super) fn pr_review_adam_host(
        &self,
        node: &SessionTopologyNode,
    ) -> Result<Option<(SessionTopologyNode, StoredDesk)>, ApiError> {
        let Some(subject) = self
            .pr_review_subject_of(node)?
            .filter(|subject| subject.adam_node_id == node.id)
        else {
            return Ok(None);
        };
        let desk = self.pr_review_desk(&subject)?;
        let host = self
            .state()?
            .with_store(|store| store.get_topology_node(node.project_id, desk.workspace_node_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|host| node.parent_id == Some(host.id))
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a PR review Adam is not below the workspace its desk registered",
                )
            })?;
        Ok(Some((host, desk)))
    }

    /// The exact definition revision a PR review's Adam and room render
    /// from: the one that placed the review.
    pub(super) fn pr_review_definition_of(
        &self,
        node: &SessionTopologyNode,
    ) -> Result<Option<TeamDefinitionSpec>, ApiError> {
        let Some(subject) = self.pr_review_subject_of(node)? else {
            return Ok(None);
        };
        self.published_definition(node.project_id, &subject.team_definition)
            .map(Some)
    }

    /// What a review room's native container belongs to: its desk, named
    /// from the revision that placed the review. `None` for any other node.
    ///
    /// That revision must place the desk exactly as the desk's own pin does,
    /// so preparing the room's lineage can never retitle the desk's project.
    pub(super) fn pr_review_room_subject(
        &self,
        node: &SessionTopologyNode,
    ) -> Result<Option<ContainerSubject>, ApiError> {
        let Some(subject) = self
            .pr_review_subject_of(node)?
            .filter(|subject| subject.room_node_id == node.id)
        else {
            return Ok(None);
        };
        let desk = self.pr_review_desk(&subject)?;
        if node.parent_id != Some(desk.topology_node_id) {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "a PR review room is not directly below the desk its row registered",
            ));
        }
        let definition = self.published_definition(node.project_id, &subject.team_definition)?;
        let declaration = self.refuse_moved_desk(&desk, &definition)?;
        Ok(Some(ContainerSubject::Desk {
            desk,
            definition,
            declaration,
        }))
    }

    /// The Jira key and pull request a PR review's titles render.
    ///
    /// Both come from the subject row, never from a caller or a title, and
    /// the key is read again from its confirmed binding: a review whose key
    /// no longer resolves to that binding renders nothing.
    pub(super) fn pr_review_title_values(
        &self,
        node: &SessionTopologyNode,
        template: &NativeNameTemplate,
        mut values: NativeNameValues,
    ) -> Result<NativeNameValues, ApiError> {
        let subject = self.pr_review_subject_of(node)?.ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "only a PR review's Adam or room renders a pull request in a title",
            )
        })?;
        if template_uses_token(template, NativeNameToken::ScopeJiraKey) {
            let key = self.jira_key_for_subject(
                node.project_id,
                subject.mini_project_id,
                subject.task_id,
            )?;
            if key.as_str() != subject.jira_key.as_str() {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the PR review's Jira key no longer matches its confirmed binding",
                ));
            }
            values = values.with_scope_jira_key(&key);
        }
        Ok(values.with_pull_request(&subject.pull_request))
    }

    /// Refuse a definition that places `desk` other than the desk's own pin
    /// does, and return the desk's declaration.
    fn refuse_moved_desk(
        &self,
        desk: &StoredDesk,
        definition: &TeamDefinitionSpec,
    ) -> Result<TeamDeskDeclaration, ApiError> {
        let (pinned, declaration) = self.desk_definition(desk)?;
        let same = definition.desk(&desk.desk_key) == Some(&declaration)
            && definition.separator == pinned.separator
            && definition.topology.spec_id == pinned.topology.spec_id
            && [&declaration.kind, &declaration.workspace_kind]
                .into_iter()
                .all(|kind| definition.container(kind) == pinned.container(kind));
        if !same {
            return Err(self
                .deny(
                    ApiErrorCode::PlacementBlocked,
                    "the reviewing Team Definition places the desk differently than the desk's pin",
                )
                .about("PR review")
                .advising(
                    "review with a definition that places the desk exactly as its pin does; nothing was placed",
                ));
        }
        Ok(declaration)
    }

    /// One pull request as a request names it.
    fn pull_request(&self, request: &PullRequestRequest) -> Result<PullRequestRef, ApiError> {
        PullRequestRef::new(request.repository.clone(), request.number).map_err(|_| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "a pull request number is positive and fits the stored integer",
            )
            .about("PR review")
        })
    }

    /// The epic and task whose confirmed Jira binding is exactly `jira_key`,
    /// refused unless that binding may publish to the pull request's
    /// repository.
    ///
    /// The repositories are the publication binding's (ASMA-8101): the
    /// governed root, plus the modules the epic's own tasks own, or the one
    /// module a task owns. A display title is never read.
    fn authorized_pr_review_identity(
        &self,
        project_id: ProjectId,
        jira_key: &ExternalId,
        pull_request: &PullRequestRef,
    ) -> Result<(MiniProjectId, Option<TaskId>), ApiError> {
        let state = self.state()?;
        let unconfirmed = || {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the Jira key names no confirmed epic or task binding in this project",
            )
            .about("PR review")
            .advising("open the review under the exact confirmed key; nothing was placed")
        };
        let binding = match state
            .with_store(|store| store.resolve_confirmed_jira_key(project_id, jira_key.as_str()))
        {
            Ok(binding) => binding,
            Err(
                RepositoryError::NotFound { .. }
                | RepositoryError::Conflict { .. }
                | RepositoryError::Domain(_),
            ) => return Err(unconfirmed()),
            Err(error) => return Err(self.refuse(&error)),
        };
        if &binding.jira_key != jira_key {
            return Err(unconfirmed());
        }
        let (epic_id, task_id) = match binding.subject {
            JiraBindingSubject::Epic(epic_id) => (epic_id, None),
            JiraBindingSubject::Task(task_id) => {
                let task = state
                    .with_store(|store| store.get_task(project_id, task_id))
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(unconfirmed)?;
                // A task outside every epic has no publication binding.
                (task.mini_project_id.ok_or_else(unconfirmed)?, Some(task_id))
            }
        };
        let tracker = TrackerKey::from_external(&binding.jira_key).map_err(|_| unconfirmed())?;
        let authorized = self
            .resolve_publication_binding(project_id, &tracker)?
            .into_iter()
            .filter(|resolved| resolved.epic_id == epic_id && resolved.task_id == task_id)
            .any(|resolved| {
                resolved
                    .binding
                    .repositories
                    .iter()
                    .any(|name| name.as_str() == pull_request.repository().as_str())
            });
        if !authorized {
            return Err(self
                .deny(
                    ApiErrorCode::Forbidden,
                    "the Jira key does not authorize the pull request's repository",
                )
                .about("PR review")
                .advising(
                    "open the review under the key whose epic or task owns that repository; nothing was placed",
                ));
        }
        Ok((epic_id, task_id))
    }

    /// The seats one review places: the Adam's one seat, then every room
    /// seat, each with the role the definition declares for it.
    fn pr_review_seats(
        &self,
        definition: &TeamDefinitionSpec,
        review: &TeamPrReview,
    ) -> Result<Vec<ReviewSeat>, ApiError> {
        let undeclared = || {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the reviewing Team Definition does not declare a PR review seat",
            )
        };
        let mut seats = Vec::new();
        for slot in &definition
            .container(&review.adam_kind)
            .ok_or_else(undeclared)?
            .slots
        {
            let role_code = slot.role_code.as_ref().ok_or_else(undeclared)?;
            seats.push(ReviewSeat {
                kind: review.adam_kind.clone(),
                slot_id: slot.slot_id.clone(),
                role: self.pr_review_catalog_role(role_code)?,
            });
        }
        for slot in &definition
            .container(&review.room_kind)
            .ok_or_else(undeclared)?
            .slots
        {
            let role_code = definition
                .pr_review_role(&slot.slot_id)
                .ok_or_else(undeclared)?;
            seats.push(ReviewSeat {
                kind: review.room_kind.clone(),
                slot_id: slot.slot_id.clone(),
                role: self.pr_review_catalog_role(role_code)?,
            });
        }
        Ok(seats)
    }

    /// The catalog role one review seat is opened under.
    ///
    /// Publication checks a role code's spelling, not that the catalog
    /// declares it, so an undeclared one is refused here, before the desk or
    /// the review is written.
    fn pr_review_catalog_role(&self, code: &RoleCode) -> Result<CatalogRoleRef, ApiError> {
        self.catalog_role_for_code(code).map_err(|error| {
            if error.code == ApiErrorCode::PlacementBlocked {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the reviewing Team Definition names a role the catalog does not declare",
                )
                .about("PR review")
                .advising(
                    "select a revision whose review roles the catalog declares; nothing was placed",
                )
            } else {
                error
            }
        })
    }

    /// One review's node, created if it is not there yet.
    ///
    /// A node already there must be exactly the one the row planned: the
    /// declared kind, directly below its planned parent, belonging to no epic
    /// or task. Anything else is refused rather than adopted.
    fn ensure_pr_review_node(
        &self,
        project_id: ProjectId,
        node_id: TopologyNodeId,
        kind: &TopologyKindKey,
        parent_id: TopologyNodeId,
        topology: &TopologySnapshot,
    ) -> Result<SessionTopologyNode, ApiError> {
        let state = self.state()?;
        if let Some(node) = state
            .with_store(|store| store.get_topology_node(project_id, node_id))
            .map_err(|error| self.refuse(&error))?
        {
            if &node.kind != kind
                || node.parent_id != Some(parent_id)
                || node.mini_project_id.is_some()
                || node.task_id.is_some()
                || &node.topology != topology
            {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a PR review node no longer matches the placement its row planned",
                ));
            }
            return Ok(node);
        }
        state
            .with_store(|store| {
                store.create_topology_node(&NewSessionTopologyNode {
                    id: node_id,
                    project_id,
                    mini_project_id: None,
                    topology: topology.clone(),
                    kind: kind.clone(),
                    parent_id: Some(parent_id),
                    task_id: None,
                    created_at: kontor_api::now(),
                })
            })
            .map_err(|error| self.refuse(&error))
    }

    /// The current seat of one slot on one node.
    fn current_pr_review_seat(
        &self,
        project_id: ProjectId,
        node_id: TopologyNodeId,
        slot_id: &RoleSlotId,
    ) -> Result<Option<SeatBinding>, ApiError> {
        Ok(self
            .state()?
            .with_store(|store| store.list_seat_bindings(project_id, node_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|seat| &seat.role_slot_id == slot_id && seat.is_non_terminal()))
    }

    /// One review seat, created if its slot holds none.
    fn ensure_pr_review_seat(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        seat: &ReviewSeat,
    ) -> Result<SeatBinding, ApiError> {
        if let Some(held) = self.current_pr_review_seat(project_id, node.id, &seat.slot_id)? {
            return Ok(held);
        }
        let role = seat.role.clone();
        let now = kontor_api::now();
        let created = self.state()?.with_store(|store| {
            store.create_seat_binding(&NewSeatBinding {
                id: SeatBindingId::generate(),
                project_id,
                topology_node_id: node.id,
                role_slot_id: seat.slot_id.clone(),
                role,
                // Review presence, not delivery work: no task, no TeamRun.
                task_id: None,
                team_run_id: None,
                attach_deadline: now
                    .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
                    .unwrap_or(now),
                parent_seat_binding_id: None,
                created_at: now,
            })
        });
        match created {
            Ok(created) => Ok(created),
            Err(RepositoryError::Conflict { .. }) => self
                .current_pr_review_seat(project_id, node.id, &seat.slot_id)?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "a PR review seat could not be created or found",
                    )
                }),
            Err(error) => Err(self.refuse(&error)),
        }
    }

    /// The stable directory one review room works in: beside its desk's own,
    /// keyed by the room node, so no two rooms ever share one.
    fn pr_review_room_root(
        &self,
        desk: &StoredDesk,
        room_node_id: TopologyNodeId,
    ) -> Result<WorkspaceRoot, ApiError> {
        let mut root = self.runtime_roots.join(desk.project_id.to_string());
        root.push(format!(
            "desk-{}-room-{room_node_id}",
            desk.topology_node_id
        ));
        std::fs::create_dir_all(&root).map_err(|_| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the review room directory could not be created",
            )
        })?;
        let root = root.to_str().ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the review room directory is not valid UTF-8",
            )
        })?;
        WorkspaceRoot::parse(root).map_err(|error| self.refuse_domain(&error))
    }

    /// The seat launch, made once per review seat, that has not been
    /// reconciled: it may have created its native and lost the answer.
    fn unconfirmed_pr_review_launch(
        &self,
        project_id: ProjectId,
        seat: &SeatBinding,
    ) -> Result<bool, ApiError> {
        let state = self.state()?;
        if state
            .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
            .map_err(|error| self.refuse(&error))?
            .is_some()
        {
            return Ok(false);
        }
        let generation = state
            .with_store(|store| {
                store.list_hosted_topology_seat_history_native_ids(project_id, seat.id)
            })
            .map_err(|error| self.refuse(&error))?
            .len();
        let generation = u64::try_from(generation)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        Ok(state
            .with_store(|store| {
                store.get_hosted_seat_launch_intent(project_id, seat.id, generation)
            })
            .map_err(|error| self.refuse(&error))?
            .is_some_and(|intent| intent.state == HostedSeatLaunchIntentState::Prepared))
    }

    /// Open one pull request's review on `desk_key`, or return the review
    /// that pull request already has.
    ///
    /// Everything a refusal can depend on is checked before anything is
    /// written: the revision, the confirmed key and the repository it
    /// authorizes, the declared placement, the routes and the runtime. A
    /// replay of the same key returns the review as it is now.
    pub(super) async fn open_pr_review_subject(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        desk_key: &DeskKey,
        request: &OpenPrReviewRequest,
    ) -> Result<PrReviewOutcomeDto, ApiError> {
        let state = self.state()?;
        let pull_request = self.pull_request(&request.pull_request)?;
        let fleet = self.fleet.current();
        let routes = request
            .model_routes
            .as_ref()
            .map(|routes| {
                routes
                    .iter()
                    .map(|(slot, route)| {
                        Ok((
                            RoleSlotId::parse(slot).map_err(|error| self.refuse_domain(&error))?,
                            parse_runtime_model_route(route, fleet.as_deref())
                                .map_err(|error| self.refuse_domain(&error))?,
                        ))
                    })
                    .collect::<Result<BTreeMap<_, _>, ApiError>>()
            })
            .transpose()?;
        let project = self.project_at(project_id, request.expected_revision)?;
        let mut intent_document = serde_json::json!({
            "schema_version": 1,
            "operation": "open_pr_review",
            "project": project_id.to_string(),
            "desk": desk_key.as_str(),
            "repository": pull_request.repository().as_str(),
            "number": pull_request.number(),
            "jira_key": request.external_key.as_str(),
        });
        if let Some(routes) = &request.model_routes {
            intent_document["model_routes"] = serde_json::json!(routes);
        }
        let intent = self.intent(&intent_document)?;
        let target = AggregateRef::Project { project_id };
        let _held = self.pr_review_guard.lock().await;
        let replayed = self.replayed(key, &intent, Some(&target))?.is_some();
        if !replayed {
            self.place_pr_review(
                project_id,
                desk_key,
                &pull_request,
                &request.external_key,
                routes.as_ref(),
            )
            .await?;
        }
        let subject = self.stored_pr_review(project_id, desk_key, &pull_request)?;
        let (receipt_id, created) = self.record_classified(
            key,
            project_id,
            CommandKind::OpenPrReview,
            target,
            project.revision,
            &intent,
        )?;
        Ok(PrReviewOutcomeDto {
            review: self.pr_review_dto(&subject)?,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if created {
                    AppliedDto::Created
                } else {
                    AppliedDto::Unchanged
                },
                revision: project.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    /// Check one open, then place its subject, nodes and seats, and with
    /// routes launch every seat. Runs under the PR review guard.
    async fn place_pr_review(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
        pull_request: &PullRequestRef,
        jira_key: &ExternalId,
        routes: Option<&BTreeMap<RoleSlotId, ModelRung>>,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let (epic_id, task_id) =
            self.authorized_pr_review_identity(project_id, jira_key, pull_request)?;
        let existing = state
            .with_store(|store| store.get_pr_review_subject(project_id, desk_key, pull_request))
            .map_err(|error| self.refuse(&error))?;
        if let Some(existing) = &existing
            && &existing.jira_key != jira_key
        {
            return Err(self
                .deny(
                    ApiErrorCode::EnsureMismatch,
                    "this pull request's review was opened under another Jira key",
                )
                .about("PR review")
                .advising("open it under the key it was first opened under; nothing was placed"));
        }
        // An existing review keeps the revision that placed it, whatever the
        // project selects today; a new one is placed by today's selection.
        let snapshot = match &existing {
            Some(existing) => existing.team_definition.clone(),
            None => self.project_team_definition(project_id)?.0,
        };
        let definition = self.published_definition(project_id, &snapshot)?;
        let review = definition
            .pr_review
            .as_ref()
            .filter(|review| &review.desk_key == desk_key)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the Team Definition declares no PR review on this desk",
                )
                .about("PR review")
                .advising(
                    "select a Team Definition whose pr_review names this desk; nothing was placed",
                )
            })?;
        let seats = self.pr_review_seats(&definition, review)?;
        if let Some(routes) = routes {
            let named: BTreeSet<_> = routes.keys().collect();
            let declared: BTreeSet<_> = seats.iter().map(|seat| &seat.slot_id).collect();
            if named != declared {
                return Err(self
                    .deny(
                        ApiErrorCode::InvalidRequest,
                        "model_routes must name the Adam's slot and every room slot exactly once",
                    )
                    .about("PR review"));
            }
        }
        let desk = state
            .with_store(|store| store.get_desk(project_id, desk_key))
            .map_err(|error| self.refuse(&error))?;
        if let Some(desk) = &desk {
            self.refuse_moved_desk(desk, &definition)?;
        }
        // Titles render the repository's name alone, so two owners' same-named
        // repositories must never share a number on one desk.
        let collides = state
            .with_store(|store| store.list_pr_review_subjects(project_id, desk_key))
            .map_err(|error| self.refuse(&error))?
            .iter()
            .any(|other| {
                &other.pull_request != pull_request
                    && other.pull_request.title_value() == pull_request.title_value()
            });
        if collides {
            return Err(self
                .deny(
                    ApiErrorCode::PlacementBlocked,
                    "another pull request on this desk renders the same title",
                )
                .about("PR review"));
        }
        let launch = match routes {
            Some(routes) => {
                let runtime_kind = self.node_runtime_kind()?;
                let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "the runtime selected for PR review placement is not configured",
                    )
                })?;
                for route in routes.values() {
                    if !adapter.provider_available(route.provider.0.as_str()) {
                        return Err(ApiError::from_runtime(
                            state.realm_id(),
                            &RuntimeError::ProviderUnavailable {
                                provider: route.provider.0.clone(),
                            },
                        ));
                    }
                }
                Some((routes, adapter))
            }
            None => None,
        };
        if let (Some(existing), Some((routes, _))) = (&existing, &launch) {
            self.refuse_rerouted_pr_review(project_id, existing, routes)?;
        }

        self.ensure_desk(project_id, desk_key)?;
        let desk = state
            .with_store(|store| store.get_desk(project_id, desk_key))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "a desk was ensured and then vanished",
                )
            })?;
        let reread = || {
            state
                .with_store(|store| store.get_pr_review_subject(project_id, desk_key, pull_request))
                .map_err(|error| self.refuse(&error))
        };
        let subject = match existing {
            Some(existing) => existing,
            None => {
                let planned = StoredPrReviewSubject {
                    project_id,
                    desk_key: desk_key.clone(),
                    pull_request: pull_request.clone(),
                    jira_key: jira_key.clone(),
                    mini_project_id: epic_id,
                    task_id,
                    adam_node_id: TopologyNodeId::generate(),
                    room_node_id: TopologyNodeId::generate(),
                    team_definition: snapshot,
                    created_at: kontor_api::now(),
                };
                match state.with_store(|store| store.create_pr_review_subject(&planned)) {
                    Ok(()) => planned,
                    Err(RepositoryError::Conflict { .. }) => reread()?
                        .filter(|winner| &winner.jira_key == jira_key)
                        .ok_or_else(|| {
                            self.deny(
                                ApiErrorCode::PlacementBlocked,
                                "a PR review subject conflicts with another review's nodes or key",
                            )
                        })?,
                    Err(error) => return Err(self.refuse(&error)),
                }
            }
        };
        let adam = self.ensure_pr_review_node(
            project_id,
            subject.adam_node_id,
            &review.adam_kind,
            desk.workspace_node_id,
            &definition.topology,
        )?;
        let room = self.ensure_pr_review_node(
            project_id,
            subject.room_node_id,
            &review.room_kind,
            desk.topology_node_id,
            &definition.topology,
        )?;
        let mut placed = Vec::with_capacity(seats.len());
        for seat in &seats {
            let node = if seat.kind == review.adam_kind {
                &adam
            } else {
                &room
            };
            placed.push((node, self.ensure_pr_review_seat(project_id, node, seat)?));
        }
        let Some((routes, adapter)) = launch else {
            return Ok(());
        };
        let room_root = self.pr_review_room_root(&desk, room.id)?;
        let room_container = self
            .ensure_container(project_id, &room, &room_root, adapter.as_ref())
            .await?;
        let desk_root = self.desk_root(&desk)?;
        let adam_container = self
            .ensure_container(project_id, &adam, &desk_root, adapter.as_ref())
            .await?;
        let scope = HostedSeatScope::Desk {
            project: state
                .with_store(|store| {
                    store.get_topology_node_container(project_id, desk.topology_node_id)
                })
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the desk has no native project to place the review's seats in",
                    )
                })?
                .identity,
        };
        for (node, seat) in placed {
            let (container, cwd) = if node.id == room.id {
                (&room_container, &room_root)
            } else {
                (&adam_container, &desk_root)
            };
            let model_rung = routes.get(&seat.role_slot_id).ok_or_else(|| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "model_routes must name the Adam's slot and every room slot exactly once",
                )
            })?;
            let title = self.seat_name_from_definition(&definition, node, &seat.role_slot_id)?;
            self.launch_pr_review_seat(
                &subject,
                &seat,
                &title,
                model_rung,
                container,
                cwd,
                &scope,
                adapter.as_ref(),
            )
            .await?;
        }
        Ok(())
    }

    /// Refuse, before anything is placed, a route that differs from the one
    /// a review seat already runs under. A different route is a replacement,
    /// and a review is never rerouted in place.
    fn refuse_rerouted_pr_review(
        &self,
        project_id: ProjectId,
        subject: &StoredPrReviewSubject,
        routes: &BTreeMap<RoleSlotId, ModelRung>,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        for node_id in [subject.adam_node_id, subject.room_node_id] {
            let seats = state
                .with_store(|store| store.list_seat_bindings(project_id, node_id))
                .map_err(|error| self.refuse(&error))?;
            for seat in seats.into_iter().filter(SeatBinding::is_non_terminal) {
                let Some(hosted) = state
                    .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
                    .map_err(|error| self.refuse(&error))?
                else {
                    continue;
                };
                if routes.get(&seat.role_slot_id) != Some(&hosted.model_rung) {
                    return Err(self
                        .deny(
                            ApiErrorCode::PlacementBlocked,
                            "a seat of this review is already live under a different route",
                        )
                        .about("PR review")
                        .advising(
                            "open it with the routes it runs under, or close it and open it again; nothing was placed",
                        ));
                }
            }
        }
        Ok(())
    }

    /// Launch one review seat's native, or prove the one it already has.
    ///
    /// The launch of a desk seat (ASMA-8451), in the review's own container:
    /// the desk workspace for the Adam, the room for a reviewer. No epic
    /// label, nothing filed under an epic; the review it serves is its
    /// subject row's.
    #[allow(clippy::too_many_arguments)]
    async fn launch_pr_review_seat(
        &self,
        subject: &StoredPrReviewSubject,
        seat: &SeatBinding,
        title: &ExternalName,
        model_rung: &ModelRung,
        container: &ContainerBindingSnapshot,
        cwd: &WorkspaceRoot,
        scope: &HostedSeatScope,
        adapter: &dyn RuntimeAdapter,
    ) -> Result<(), ApiError> {
        let _native_activity = self.native_activity()?;
        let state = self.state()?;
        let project_id = subject.project_id;
        if let Some(existing) = state
            .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
            .map_err(|error| self.refuse(&error))?
        {
            // A review seat has no Core Team route to recover through: a
            // session that is no longer live is fenced by closing the review,
            // and opening it again launches afresh.
            return self
                .reuse_bound_core_team_seat(project_id, seat.id, &existing, model_rung, adapter)
                .await
                .map_err(|error| {
                    if error.code == ApiErrorCode::StaleBinding {
                        self.deny(
                            ApiErrorCode::StaleBinding,
                            "a session of this review is no longer live",
                        )
                        .about("PR review")
                        .advising("close this review and open it again; nothing was relaunched")
                    } else {
                        error.about("PR review")
                    }
                });
        }
        let history = state
            .with_store(|store| {
                store.list_hosted_topology_seat_history_native_ids(project_id, seat.id)
            })
            .map_err(|error| self.refuse(&error))?;
        let generation = u64::try_from(history.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        // A prepared intent means an earlier attempt resolved this
        // generation's authority and may have created the native before its
        // answer was lost: the replay asks for what that attempt did.
        let autonomy = match state
            .with_store(|store| {
                store.get_hosted_seat_launch_intent(project_id, seat.id, generation)
            })
            .map_err(|error| self.refuse(&error))?
        {
            Some(intent) => intent.autonomy,
            None => freeze_hosted_seat_autonomy(adapter.declared_autonomy()),
        };
        state
            .with_store(|store| {
                store.prepare_hosted_seat_launch_intent(&StoredHostedSeatLaunchIntent {
                    project_id,
                    seat_binding_id: seat.id,
                    occupancy_generation: generation,
                    autonomy,
                    model_rung: (model_rung.clone()).into(),
                    state: HostedSeatLaunchIntentState::Prepared,
                    observed_native_id: None,
                    prepared_at: kontor_api::now(),
                    installed_at: None,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        let role_persona = self.freeze_role_persona(&seat.role.role_code)?;
        if let Some(persona) = role_persona.as_ref() {
            state
                .with_store(|store| {
                    store.record_hosted_seat_role_persona(project_id, seat.id, generation, persona)
                })
                .map_err(|error| self.refuse(&error))?;
        }
        let capabilities = adapter
            .discover_capabilities()
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let context_policy = ContextPolicySnapshot::standard(
            &capabilities.limits.context_window,
            capabilities.supports(RuntimeCapability::ContextPolicy),
            SCHEMA_VERSION,
            kontor_api::now(),
        )
        .map_err(|error| self.refuse_domain(&error))?;
        let prompt = BoundedText::parse(&format!(
            "{} seat of the review of {} #{} on desk {}, under {}. It belongs to no epic and serves this pull request only. Continue only work authorized through Kontor, then remain reusable under SeatBinding {}. Use the generation-fenced KONTOR_AUTH value only for this seat's supported identity-bound routes and never disclose it.",
            seat.role.role_code.as_str(),
            subject.pull_request.repository().as_str(),
            subject.pull_request.number(),
            subject.desk_key.as_str(),
            subject.jira_key.as_str(),
            seat.id,
        ))
        .map_err(|error| self.refuse_domain(&error))?;
        let outcome = adapter
            .launch_hosted_seat(&HostedSeatLaunchRequest {
                seat_binding_id: seat.id,
                role_slot_id: seat.role_slot_id.clone(),
                display_name: title.clone(),
                container: container.clone(),
                cwd: cwd.clone(),
                scope: scope.clone(),
                prompt,
                role_prompt: role_persona.as_ref().map(|persona| persona.prompt.clone()),
                credential: ConsultationCredential::new(
                    state
                        .credentials()
                        .seat_credential_for_generation(seat.id, generation),
                ),
                fenced_predecessor_native_ids: history,
                model_rung: model_rung.clone(),
                fleet_provenance: None,
                autonomy,
                context_policy,
                requested_at: kontor_api::now(),
            })
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let hosted = StoredHostedTopologySeat {
            project_id,
            seat_binding_id: seat.id,
            model_rung: model_rung.clone(),
            native_identity: outcome.identity,
            autonomy,
            provider_session_id: outcome.provider_session_id,
            observed_at: outcome.observed_at,
        };
        state
            .with_store(|store| store.bind_hosted_topology_seat(&hosted))
            .map_err(|error| self.refuse(&error))?;
        state
            .with_store(|store| {
                store.install_hosted_seat_launch_intent(
                    project_id,
                    seat.id,
                    generation,
                    &hosted.native_identity.native_id,
                    hosted.observed_at,
                )
            })
            .map_err(|error| self.refuse(&error))?;
        state
            .with_store(|store| {
                store.observe_seat_binding(
                    project_id,
                    seat.id,
                    &SeatLivenessObservation {
                        attached_at: Some(hosted.observed_at),
                        runtime_reported: Some(kontor_core::state::ObservedRunState::Running),
                        ..SeatLivenessObservation::default()
                    },
                    hosted.observed_at,
                )
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(())
    }

    /// Close one pull request's review: fence each live session of its Adam
    /// and room, then release their seats.
    ///
    /// Only this review's two nodes are read, so nothing else on the desk can
    /// be touched: not the desk, not its workspace or durable seats, not
    /// another pull request's review. The subject row, its nodes and the
    /// room's native workspace stay, so the same pull request reopens onto
    /// them. A seat whose launch may have lost its answer, or a workspace that
    /// no longer proves, refuses before anything is fenced.
    pub(super) async fn close_pr_review_subject(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        desk_key: &DeskKey,
        request: &ClosePrReviewRequest,
    ) -> Result<PrReviewOutcomeDto, ApiError> {
        let state = self.state()?;
        let pull_request = self.pull_request(&request.pull_request)?;
        let project = self.project_at(project_id, request.expected_revision)?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "close_pr_review",
            "project": project_id.to_string(),
            "desk": desk_key.as_str(),
            "repository": pull_request.repository().as_str(),
            "number": pull_request.number(),
        }))?;
        let target = AggregateRef::Project { project_id };
        let _held = self.pr_review_guard.lock().await;
        let replayed = self.replayed(key, &intent, Some(&target))?.is_some();
        let subject = self.stored_pr_review(project_id, desk_key, &pull_request)?;
        let mut partial = false;
        if !replayed {
            let mut seats = Vec::new();
            for node_id in [subject.adam_node_id, subject.room_node_id] {
                let Some(node) = state
                    .with_store(|store| store.get_topology_node(project_id, node_id))
                    .map_err(|error| self.refuse(&error))?
                else {
                    continue;
                };
                let held = state
                    .with_store(|store| store.list_seat_bindings(project_id, node.id))
                    .map_err(|error| self.refuse(&error))?;
                for seat in held.into_iter().filter(SeatBinding::is_non_terminal) {
                    seats.push((node.clone(), seat));
                }
            }
            for (_, seat) in &seats {
                if self.unconfirmed_pr_review_launch(project_id, seat)? {
                    return Err(self
                        .deny(
                            ApiErrorCode::PlacementBlocked,
                            "a launch of this review's seat may have created its session and lost the answer",
                        )
                        .about("PR review")
                        .advising(
                            "retry the open that launched it with the same idempotency key, then close; nothing was released",
                        ));
                }
            }
            let mut live = Vec::new();
            for (node, seat) in &seats {
                if let Some(hosted) = state
                    .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
                    .map_err(|error| self.refuse(&error))?
                {
                    live.push((node, hosted));
                }
            }
            // Every workspace a session is fenced in is proved before the
            // first is fenced, so a misplaced room refuses the whole close
            // rather than after the Adam's session was already retired.
            {
                let _native_activity = self.native_activity()?;
                let mut proved = BTreeSet::new();
                for (node, _) in &live {
                    if proved.insert(node.id) {
                        self.prove_existing_bound_container(project_id, node)
                            .await?
                            .ok_or_else(|| {
                                self.deny(
                                    ApiErrorCode::StaleBinding,
                                    "a PR review session's workspace has no proved native container",
                                )
                            })?;
                    }
                }
            }
            for (node, hosted) in &live {
                match self
                    .fence_pr_review_node_session(
                        project_id,
                        node,
                        hosted,
                        (
                            "PR review closed",
                            "PR review closed; the session was already gone",
                        ),
                    )
                    .await
                {
                    Ok(()) => partial = true,
                    Err(error) => {
                        let unconfirmed = error.code == ApiErrorCode::DeliveryUnconfirmed;
                        return Err(if partial || unconfirmed {
                            partial_close(error)
                        } else {
                            error
                        });
                    }
                }
            }
            let now = kontor_api::now();
            for (_, seat) in &seats {
                match state.with_store(|store| {
                    store.observe_seat_binding(
                        project_id,
                        seat.id,
                        &SeatLivenessObservation {
                            released_at: Some(now),
                            ..SeatLivenessObservation::default()
                        },
                        now,
                    )
                }) {
                    Ok(_) => partial = true,
                    Err(error) => {
                        let error = self.refuse(&error);
                        return Err(if partial { partial_close(error) } else { error });
                    }
                }
            }
        }
        let (receipt_id, created) = match self.record_classified(
            key,
            project_id,
            CommandKind::ClosePrReview,
            target,
            project.revision,
            &intent,
        ) {
            Ok(recorded) => recorded,
            Err(error) if partial => return Err(partial_close(error)),
            Err(error) => return Err(error),
        };
        Ok(PrReviewOutcomeDto {
            review: self.pr_review_dto(&subject)?,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if created {
                    AppliedDto::Created
                } else {
                    AppliedDto::Unchanged
                },
                revision: project.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    /// Every review one desk holds, oldest first. Read-only.
    pub(super) fn read_pr_reviews(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
    ) -> Result<PrReviewReportDto, ApiError> {
        let state = self.state()?;
        state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such project exists in this realm",
                )
            })?;
        let reviews = state
            .with_store(|store| store.list_pr_review_subjects(project_id, desk_key))
            .map_err(|error| self.refuse(&error))?
            .iter()
            .map(|subject| self.pr_review_dto(subject))
            .collect::<Result<_, _>>()?;
        Ok(PrReviewReportDto {
            desk_key: desk_key.as_str().to_owned(),
            reviews,
        })
    }

    /// The subject one pull request names on one desk.
    fn stored_pr_review(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
        pull_request: &PullRequestRef,
    ) -> Result<StoredPrReviewSubject, ApiError> {
        self.state()?
            .with_store(|store| store.get_pr_review_subject(project_id, desk_key, pull_request))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "this pull request has no review on this desk",
                )
                .about("PR review")
            })
    }

    /// One review as Kontor records it now.
    fn pr_review_dto(&self, subject: &StoredPrReviewSubject) -> Result<PrReviewDto, ApiError> {
        let definition = self.published_definition(subject.project_id, &subject.team_definition)?;
        let adam =
            self.pr_review_node_dto(&definition, subject.project_id, subject.adam_node_id)?;
        let room =
            self.pr_review_node_dto(&definition, subject.project_id, subject.room_node_id)?;
        let open = [&adam, &room]
            .into_iter()
            .flatten()
            .any(|node| !node.seats.is_empty());
        Ok(PrReviewDto {
            desk_key: subject.desk_key.as_str().to_owned(),
            repository: subject.pull_request.repository().as_str().to_owned(),
            number: subject.pull_request.number(),
            external_key: subject.jira_key.as_str().to_owned(),
            epic_id: subject.mini_project_id.to_string(),
            task_id: subject.task_id.map(|id| id.to_string()),
            team_definition: TeamDefinitionRefDto {
                id: subject.team_definition.definition_id,
                version: subject.team_definition.version,
            },
            open,
            adam,
            room,
            created_at: subject.created_at.to_string(),
        })
    }

    /// One review node, its container and its current seats.
    fn pr_review_node_dto(
        &self,
        definition: &TeamDefinitionSpec,
        project_id: ProjectId,
        node_id: TopologyNodeId,
    ) -> Result<Option<PrReviewNodeDto>, ApiError> {
        let state = self.state()?;
        let Some(node) = state
            .with_store(|store| store.get_topology_node(project_id, node_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(None);
        };
        let host = self.session_container_node(&node)?;
        let container = state
            .with_store(|store| store.get_topology_node_container(project_id, host.id))
            .map_err(|error| self.refuse(&error))?
            .map(|binding| PrReviewContainerDto {
                topology_node_id: host.id.to_string(),
                native_id: binding.identity.native_id.as_str().to_owned(),
                observed_kind: binding.observed_kind.as_str().to_owned(),
                title: binding
                    .readback
                    .as_ref()
                    .map(|readback| readback.visible_title.as_str().to_owned()),
                cwd: binding
                    .canonical_cwd
                    .as_ref()
                    .map(|cwd| cwd.as_str().to_owned()),
                parent_native_id: binding
                    .readback
                    .as_ref()
                    .and_then(|readback| readback.native_parent.as_ref())
                    .map(|parent| parent.native_id.as_str().to_owned()),
            });
        let mut seats = Vec::new();
        for seat in state
            .with_store(|store| store.list_seat_bindings(project_id, node.id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .filter(SeatBinding::is_non_terminal)
        {
            let title = self.seat_name_from_definition(definition, &node, &seat.role_slot_id)?;
            let native_id = state
                .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
                .map_err(|error| self.refuse(&error))?
                .map(|hosted| hosted.native_identity.native_id.as_str().to_owned());
            let occupancy_generation = match native_id {
                Some(_) => state
                    .with_store(|store| {
                        store.hosted_topology_seat_occupancy_generation(project_id, seat.id)
                    })
                    .map_err(|error| self.refuse(&error))?,
                None => None,
            };
            seats.push(PrReviewSeatDto {
                slot_id: seat.role_slot_id.as_str().to_owned(),
                role_code: seat.role.role_code.as_str().to_owned(),
                title: title.as_str().to_owned(),
                seat_binding_id: seat.id.to_string(),
                native_id,
                occupancy_generation,
            });
        }
        Ok(Some(PrReviewNodeDto {
            topology_node_id: node.id.to_string(),
            kind: node.kind.as_str().to_owned(),
            parent_id: node
                .parent_id
                .map(|parent| parent.to_string())
                .unwrap_or_default(),
            container,
            seats,
        }))
    }
}
